use crate::{
    audio_tools::AudioAnalysisResult,
    config::Config,
    llm::{ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role},
};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JudgeAudioQualityArgs {
    #[serde(default)]
    pub features: Option<Value>,
    /// Exact path of a completed analysis cached by the current Agent task.
    #[serde(default)]
    pub analysis_path: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityScore {
    pub score: f64,
    pub confidence: f64,
}

impl QualityScore {
    fn clamped(mut self) -> Self {
        self.score = self.score.clamp(0.0, 5.0);
        self.confidence = self.confidence.clamp(0.0, 1.0);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioQualityJudgment {
    pub backend: String,
    pub clarity: QualityScore,
    pub background_noise: QualityScore,
    pub distortion: QualityScore,
    pub loudness: QualityScore,
    pub continuity: QualityScore,
    pub usability: QualityScore,
    pub overall: QualityScore,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAudioQualityJudgment {
    clarity: QualityScore,
    background_noise: QualityScore,
    distortion: QualityScore,
    loudness: QualityScore,
    continuity: QualityScore,
    usability: QualityScore,
    overall: QualityScore,
    #[serde(default)]
    summary: Option<String>,
}

impl RawAudioQualityJudgment {
    fn into_judgment(self, backend: &str) -> AudioQualityJudgment {
        AudioQualityJudgment {
            backend: backend.into(),
            clarity: self.clarity.clamped(),
            background_noise: self.background_noise.clamped(),
            distortion: self.distortion.clamped(),
            loudness: self.loudness.clamped(),
            continuity: self.continuity.clamped(),
            usability: self.usability.clamped(),
            overall: self.overall.clamped(),
            summary: self.summary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioJudgeBackend {
    Jev,
    GeneralLlm,
}

fn selected_backend(config: &Config) -> AudioJudgeBackend {
    if config.jev_is_configured() {
        AudioJudgeBackend::Jev
    } else {
        AudioJudgeBackend::GeneralLlm
    }
}

pub async fn judge_audio_quality(
    config: &Config,
    llm: &dyn LlmClient,
    args: &JudgeAudioQualityArgs,
) -> Result<AudioQualityJudgment> {
    validate_features(
        args.features
            .as_ref()
            .context("audio quality judgment requires resolved analyze_audio features")?,
    )?;
    match selected_backend(config) {
        AudioJudgeBackend::Jev => judge_with_jev(config, args).await,
        AudioJudgeBackend::GeneralLlm => judge_with_llm(config, llm, args).await,
    }
}

fn validate_features(features: &Value) -> Result<()> {
    let parsed: AudioAnalysisResult = serde_json::from_value(features.clone())
        .context("audio features must be a complete structured result returned by analyze_audio")?;
    match parsed {
        AudioAnalysisResult::Ok { .. } => Ok(()),
        AudioAnalysisResult::NeedsInput { missing, .. } => bail!(
            "audio analysis is incomplete; missing raw PCM format parameters: {}",
            missing.join(", ")
        ),
    }
}

async fn judge_with_llm(
    config: &Config,
    llm: &dyn LlmClient,
    args: &JudgeAudioQualityArgs,
) -> Result<AudioQualityJudgment> {
    let purpose = args
        .purpose
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("general speech recording");
    let system = r#"You are an audio quality scoring engine.

You do not receive the waveform itself. You only receive deterministic acoustic measurements produced from the audio file. Do not invent acoustic facts that are absent from the provided features.

Score these dimensions from 0.0 to 5.0: clarity, background_noise, distortion, loudness, continuity, usability, overall.
For every dimension return exactly {\"score\": number, \"confidence\": number} where score is 0.0..5.0 and confidence is 0.0..1.0.
Higher scores always mean better quality. In particular, a higher background_noise score means cleaner audio and a higher distortion score means less distortion.
Use the requested purpose when provided. Consider measurable evidence such as estimated_snr_db, noise_floor_dbfs, rms_dbfs, peak_dbfs, clipping_ratio, silence ratio, longest silence, spectral centroid, spectral rolloff, spectral flatness, speech-band ratio, and low/high-frequency ratios.
The overall score should be a holistic usability judgment for the stated purpose, not a mechanical arithmetic mean.
Return one valid JSON object only, with keys clarity, background_noise, distortion, loudness, continuity, usability, overall, and optional summary. Do not return Markdown or commentary outside JSON."#;
    let user = serde_json::to_string(&json!({
        "purpose": purpose,
        "features": args.features.clone().context("missing resolved audio features")?,
    }))?;
    let response = llm
        .complete(LlmRequest {
            model: config.model.clone(),
            items: vec![
                ConversationItem::Message(ConversationMessage::new(Role::System, system)),
                ConversationItem::Message(ConversationMessage::new(Role::User, user)),
            ],
            tools: vec![],
        })
        .await
        .context("general LLM audio quality judgment failed")?;
    if !response.tool_calls.is_empty() {
        bail!("general LLM returned tool calls even though no tools were exposed")
    }
    let text = response
        .text
        .context("general LLM returned no judgment text")?;
    parse_llm_json(&text).map(|raw| raw.into_judgment("llm"))
}

async fn judge_with_jev(
    config: &Config,
    args: &JudgeAudioQualityArgs,
) -> Result<AudioQualityJudgment> {
    let purpose = args
        .purpose
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("general speech recording");
    let body = json!({
        "state": {
            "purpose": purpose,
            "features": args.features.clone().context("missing resolved audio features")?,
        },
        "model": config.jev_model.clone(),
        "questions": jev_questions(),
    });
    let client = crate::network::build_http_client(config)?;
    let endpoint = config.jev_endpoint.trim();
    let mut last_error = None;

    for attempt in 0..=config.llm_retry_count {
        let request = client
            .post(endpoint)
            .bearer_auth(config.jev_api_key.trim())
            .json(&body);
        let sent = tokio::select! {
            response = request.send() => response,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                return Err(anyhow!("Jev audio quality request cancelled by user"));
            }
        };
        match sent {
            Ok(response) if response.status().is_success() => {
                let value: JevResponse = response
                    .json()
                    .await
                    .context("invalid Jev audio quality JSON response")?;
                return map_jev_response(value);
            }
            Ok(response) => {
                let status = response.status();
                let retryable = status == StatusCode::TOO_MANY_REQUESTS
                    || status.as_u16() == 529
                    || status.is_server_error();
                let mut text = response.text().await.unwrap_or_default();
                if !config.jev_api_key.is_empty() {
                    text = text.replace(&config.jev_api_key, "[REDACTED]");
                }
                last_error = Some(anyhow!(
                    "Jev audio quality HTTP {status}: {}",
                    text.chars().take(512).collect::<String>()
                ));
                if !retryable || attempt == config.llm_retry_count {
                    break;
                }
            }
            Err(error) => {
                last_error = Some(anyhow!("Jev audio quality request failed: {error}"));
                if attempt == config.llm_retry_count {
                    break;
                }
            }
        }
        let shift = attempt.min(16);
        let multiplier = 1u64 << shift;
        let delay_ms = config.llm_retry_base_delay_ms.saturating_mul(multiplier);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {},
            signal = tokio::signal::ctrl_c() => {
                signal?;
                return Err(anyhow!("Jev audio quality retry cancelled by user"));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("Jev audio quality request failed")))
}

fn jev_questions() -> Value {
    json!({
        "clarity": score_question(
            "How clear and intelligible is this audio for the stated `purpose`, based only on `features`?",
            [
                "Effectively unintelligible.",
                "Very unclear; understanding is difficult.",
                "Noticeably unclear but partially understandable.",
                "Acceptable clarity for ordinary use.",
                "Clear with only minor limitations.",
                "Exceptionally clear and easy to understand."
            ]
        ),
        "background_noise": score_question(
            "How clean is the audio from background noise for the stated `purpose`, based only on `features`? Higher means cleaner.",
            [
                "Noise dominates and seriously prevents use.",
                "Very noisy with major interference.",
                "Clearly noisy and distracting.",
                "Some noise is present but generally acceptable.",
                "Very little distracting background noise.",
                "Essentially free of perceptible background noise."
            ]
        ),
        "distortion": score_question(
            "How free is the audio from clipping and other measurable distortion, based only on `features`? Higher means less distortion.",
            [
                "Severe distortion or clipping makes the audio unusable.",
                "Heavy distortion is obvious.",
                "Noticeable distortion reduces quality.",
                "Minor distortion but acceptable.",
                "Little measurable or perceptible distortion is indicated.",
                "No meaningful distortion is indicated by the measurements."
            ]
        ),
        "loudness": score_question(
            "How suitable and stable is the measured signal level for the stated `purpose`, based only on `features`?",
            [
                "Level is severely unsuitable.",
                "Level is badly unsuitable.",
                "Level has significant problems.",
                "Level is usable with some limitations.",
                "Level is well suited to the purpose.",
                "Level is exceptionally well suited and stable."
            ]
        ),
        "continuity": score_question(
            "How continuous and stable is the recording, especially considering silence and interruptions in `features`?",
            [
                "Severely discontinuous or interrupted.",
                "Frequent major gaps or interruptions.",
                "Noticeable continuity problems.",
                "Mostly continuous with acceptable gaps.",
                "Continuous with only minor gaps.",
                "Highly continuous and stable."
            ]
        ),
        "usability": score_question(
            "How usable is this audio for the stated `purpose`, based only on the objective `features`?",
            [
                "Not usable for the purpose.",
                "Poor usability; likely to fail the purpose.",
                "Limited usability with significant compromises.",
                "Usable for the purpose with ordinary limitations.",
                "Well suited to the purpose.",
                "Excellent for the stated purpose."
            ]
        ),
        "overall": score_question(
            "What is the overall audio quality for the stated `purpose`, based only on `features`? Make a holistic judgment rather than mechanically averaging hypothetical sub-scores.",
            [
                "Unusable overall quality.",
                "Very poor overall quality.",
                "Below-average overall quality.",
                "Acceptable overall quality.",
                "Good overall quality.",
                "Excellent overall quality."
            ]
        )
    })
}

fn score_question(instructions: &str, criteria: [&str; 6]) -> Value {
    json!({
        "type": "score",
        "instructions": instructions,
        "criteria": criteria,
    })
}

#[derive(Debug, Deserialize)]
struct JevResponse {
    answers: BTreeMap<String, JevScoreAnswer>,
}

#[derive(Debug, Deserialize)]
struct JevScoreAnswer {
    #[serde(rename = "type")]
    answer_type: String,
    score: f64,
    confidence: f64,
}

fn map_jev_response(response: JevResponse) -> Result<AudioQualityJudgment> {
    fn take(answers: &BTreeMap<String, JevScoreAnswer>, key: &str) -> Result<QualityScore> {
        let answer = answers
            .get(key)
            .with_context(|| format!("Jev response is missing answer {key}"))?;
        if answer.answer_type != "score" {
            bail!(
                "Jev answer {key} has unexpected type {}",
                answer.answer_type
            )
        }
        if !answer.score.is_finite() || !answer.confidence.is_finite() {
            bail!("Jev answer {key} contains non-finite values")
        }
        Ok(QualityScore {
            score: answer.score,
            confidence: answer.confidence,
        }
        .clamped())
    }

    Ok(AudioQualityJudgment {
        backend: "jev".into(),
        clarity: take(&response.answers, "clarity")?,
        background_noise: take(&response.answers, "background_noise")?,
        distortion: take(&response.answers, "distortion")?,
        loudness: take(&response.answers, "loudness")?,
        continuity: take(&response.answers, "continuity")?,
        usability: take(&response.answers, "usability")?,
        overall: take(&response.answers, "overall")?,
        summary: None,
    })
}

fn parse_llm_json(text: &str) -> Result<RawAudioQualityJudgment> {
    let trimmed = text.trim();
    let json_text = if trimmed.starts_with("```") {
        let mut lines = trimmed.lines();
        let _ = lines.next();
        lines
            .take_while(|line| line.trim() != "```")
            .collect::<Vec<_>>()
            .join("\n")
    } else if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        trimmed[start..=end].to_owned()
    } else {
        trimmed.to_owned()
    };
    let raw: RawAudioQualityJudgment = serde_json::from_str(json_text.trim())
        .context("invalid audio quality JSON from general LLM")?;
    for (name, value) in [
        ("clarity", &raw.clarity),
        ("background_noise", &raw.background_noise),
        ("distortion", &raw.distortion),
        ("loudness", &raw.loudness),
        ("continuity", &raw.continuity),
        ("usability", &raw.usability),
        ("overall", &raw.overall),
    ] {
        if !value.score.is_finite() || !value.confidence.is_finite() {
            bail!("audio quality field {name} contains non-finite values")
        }
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{FinishReason, LlmResponse, ToolCall, Usage};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockLlm {
        calls: AtomicUsize,
        text: String,
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, request: LlmRequest) -> Result<LlmResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(request.tools.is_empty());
            Ok(LlmResponse {
                text: Some(self.text.clone()),
                tool_calls: Vec::<ToolCall>::new(),
                usage: Usage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                },
                finish_reason: FinishReason::Stop,
            })
        }
    }

    fn features() -> Value {
        json!({
            "status": "ok",
            "input": {"container":"wav","metadata_source":"header"},
            "format": {"sample_rate":16000,"channels":1,"bits_per_sample":16,"sample_format":"s16le","duration_sec":1.0},
            "level": {"rms_dbfs":-20.0,"peak_dbfs":-2.0,"crest_factor_db":18.0,"clipping_ratio":0.0,"dc_offset":0.0},
            "noise": {"noise_floor_dbfs":-50.0,"estimated_snr_db":30.0},
            "silence": {"ratio":0.05,"longest_ms":100},
            "spectrum": {"centroid_hz":1200.0,"rolloff_95_hz":3800.0,"flatness":0.1,"speech_band_ratio":0.7,"low_frequency_ratio":0.02,"high_frequency_ratio":0.08}
        })
    }

    #[tokio::test]
    async fn missing_jev_key_uses_general_llm_without_tools_and_clamps() -> Result<()> {
        let config = Config::default();
        let llm = MockLlm {
            calls: AtomicUsize::new(0),
            text: r#"{
                "clarity":{"score":6.2,"confidence":1.4},
                "background_noise":{"score":4.0,"confidence":0.8},
                "distortion":{"score":4.5,"confidence":0.9},
                "loudness":{"score":3.5,"confidence":0.7},
                "continuity":{"score":4.2,"confidence":0.8},
                "usability":{"score":4.1,"confidence":0.85},
                "overall":{"score":4.0,"confidence":0.82}
            }"#
            .into(),
        };
        let result = judge_audio_quality(
            &config,
            &llm,
            &JudgeAudioQualityArgs {
                features: Some(features()),
                analysis_path: None,
                purpose: Some("speech recognition".into()),
            },
        )
        .await?;
        assert_eq!(result.backend, "llm");
        assert_eq!(result.clarity.score, 5.0);
        assert_eq!(result.clarity.confidence, 1.0);
        assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn configured_jev_selects_jev_without_implicit_fallback() {
        let config = Config {
            jev_api_key: "test-key".into(),
            ..Config::default()
        };
        assert_eq!(selected_backend(&config), AudioJudgeBackend::Jev);

        let fallback = Config::default();
        assert_eq!(selected_backend(&fallback), AudioJudgeBackend::GeneralLlm);
    }

    #[test]
    fn incomplete_analysis_is_rejected_before_any_backend() {
        let result = validate_features(&json!({"status":"needs_input","missing":["sample_rate"]}));
        assert!(result.is_err());
    }

    #[test]
    fn jev_mapping_requires_every_score() {
        let response = JevResponse {
            answers: BTreeMap::new(),
        };
        assert!(map_jev_response(response).is_err());
    }
}
