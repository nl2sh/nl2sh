use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    f64::consts::PI,
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const MAX_AUDIO_FILE_BYTES: u64 = 256 * 1024 * 1024;
const FFT_SIZE: usize = 1024;
const FFT_HOP: usize = FFT_SIZE / 2;
const MAX_SPECTRUM_WINDOWS: u64 = 4096;
const SILENCE_THRESHOLD_DBFS: f64 = -50.0;
const MIN_DBFS: f64 = -120.0;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum RawSampleFormat {
    #[serde(rename = "s16le")]
    S16Le,
    #[serde(rename = "s24le")]
    S24Le,
    #[serde(rename = "s32le")]
    S32Le,
    #[serde(rename = "f32le")]
    F32Le,
}

impl RawSampleFormat {
    fn bytes_per_sample(self) -> usize {
        match self {
            Self::S16Le => 2,
            Self::S24Le => 3,
            Self::S32Le | Self::F32Le => 4,
        }
    }

    fn bits_per_sample(self) -> u16 {
        (self.bytes_per_sample() * 8) as u16
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::S16Le => "s16le",
            Self::S24Le => "s24le",
            Self::S32Le => "s32le",
            Self::F32Le => "f32le",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeAudioArgs {
    pub path: String,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u16>,
    #[serde(default)]
    pub sample_format: Option<RawSampleFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AudioAnalysisResult {
    Ok {
        input: AudioInputInfo,
        format: AudioFormatFeatures,
        level: AudioLevelFeatures,
        noise: AudioNoiseFeatures,
        silence: AudioSilenceFeatures,
        spectrum: AudioSpectrumFeatures,
    },
    NeedsInput {
        detected_container: String,
        missing: Vec<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        detected: BTreeMap<String, DetectionValue>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        candidates: BTreeMap<String, Vec<DetectionValue>>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionValue {
    pub value: serde_json::Value,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInputInfo {
    pub container: String,
    pub metadata_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFormatFeatures {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub sample_format: String,
    pub duration_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioLevelFeatures {
    pub rms_dbfs: f64,
    pub peak_dbfs: f64,
    pub crest_factor_db: f64,
    pub clipping_ratio: f64,
    pub dc_offset: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioNoiseFeatures {
    pub noise_floor_dbfs: f64,
    pub estimated_snr_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSilenceFeatures {
    pub ratio: f64,
    pub longest_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSpectrumFeatures {
    pub centroid_hz: f64,
    pub rolloff_95_hz: f64,
    pub flatness: f64,
    pub speech_band_ratio: f64,
    pub low_frequency_ratio: f64,
    pub high_frequency_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct AudioToolExecutor {
    base: PathBuf,
}

impl AudioToolExecutor {
    pub fn new(base: &Path) -> Result<Self> {
        let base = fs::canonicalize(base)
            .with_context(|| format!("cannot resolve audio-tool base {}", base.display()))?;
        Ok(Self { base })
    }

    pub fn analyze(&self, args: &AnalyzeAudioArgs) -> Result<AudioAnalysisResult> {
        let path = self.resolve_existing(&args.path)?;
        let metadata = fs::metadata(&path)
            .with_context(|| format!("cannot inspect audio file {}", path.display()))?;
        if !metadata.is_file() {
            bail!("audio path is not a regular file")
        }
        if metadata.len() == 0 {
            bail!("audio file is empty")
        }
        if metadata.len() > MAX_AUDIO_FILE_BYTES {
            bail!("audio file exceeds {MAX_AUDIO_FILE_BYTES} byte limit")
        }

        let mut reader = BufReader::new(
            File::open(&path).with_context(|| format!("cannot open {}", path.display()))?,
        );
        let mut signature = [0u8; 12];
        let signature_len = reader
            .read(&mut signature)
            .context("cannot read audio header")?;
        reader.seek(SeekFrom::Start(0))?;
        let starts_riff = signature_len >= 4 && &signature[0..4] == b"RIFF";
        let starts_rifx = signature_len >= 4 && &signature[0..4] == b"RIFX";
        let starts_rf64 = signature_len >= 4 && &signature[0..4] == b"RF64";
        let has_wave_form = signature_len == signature.len() && &signature[8..12] == b"WAVE";
        if starts_riff && !has_wave_form {
            bail!("invalid or truncated RIFF/WAVE header")
        }
        if (starts_rifx || starts_rf64) && has_wave_form {
            bail!("recognized WAV container is unsupported; analyze_audio currently supports little-endian RIFF/WAVE only")
        }
        let is_wav = starts_riff && has_wave_form;
        if has_wave_form && !is_wav {
            bail!("unsupported WAV container; only little-endian RIFF/WAVE is supported")
        }
        if signature_len >= 4
            && (&signature[0..4] == b"fLaC"
                || &signature[0..4] == b"OggS"
                || &signature[0..4] == b"caff")
        {
            bail!("recognized compressed or non-PCM audio container is unsupported; analyze_audio currently accepts RIFF/WAVE or headerless raw PCM")
        }
        if signature_len >= 3 && &signature[0..3] == b"ID3" {
            bail!("recognized MP3/ID3 audio is unsupported; analyze_audio currently accepts RIFF/WAVE or headerless raw PCM")
        }
        if signature_len == signature.len()
            && &signature[0..4] == b"FORM"
            && (&signature[8..12] == b"AIFF" || &signature[8..12] == b"AIFC")
        {
            bail!("recognized AIFF audio is unsupported; analyze_audio currently accepts RIFF/WAVE or headerless raw PCM")
        }
        if signature_len >= 8 && &signature[4..8] == b"ftyp" {
            bail!("recognized ISO media audio container is unsupported; analyze_audio currently accepts RIFF/WAVE or headerless raw PCM")
        }

        if is_wav {
            let wav = parse_wav(&mut reader, metadata.len())?;
            validate_header_conflicts(args, &wav.spec)?;
            return analyze_pcm(
                &path,
                wav.data_offset,
                wav.data_len,
                wav.spec,
                "wav",
                "header",
            );
        }

        analyze_raw(&path, metadata.len(), args)
    }

    fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            bail!("path must not be empty")
        }
        let path = Path::new(raw);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        };
        let resolved = fs::canonicalize(&candidate)
            .with_context(|| format!("cannot resolve path {}", candidate.display()))?;
        Ok(resolved)
    }
}

#[derive(Debug, Clone, Copy)]
struct AudioSpec {
    sample_rate: u32,
    channels: u16,
    sample_format: RawSampleFormat,
}

impl AudioSpec {
    fn block_align(self) -> usize {
        self.channels as usize * self.sample_format.bytes_per_sample()
    }
}

struct WavLayout {
    spec: AudioSpec,
    data_offset: u64,
    data_len: u64,
}

fn parse_wav(reader: &mut BufReader<File>, file_len: u64) -> Result<WavLayout> {
    let mut riff = [0u8; 12];
    reader
        .read_exact(&mut riff)
        .context("truncated WAV header")?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        bail!("unsupported WAV container; expected RIFF/WAVE")
    }

    let mut spec = None;
    let mut data = None;
    loop {
        let pos = reader.stream_position()?;
        if pos.saturating_add(8) > file_len {
            break;
        }
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).is_err() {
            break;
        }
        let chunk_id = &header[0..4];
        let chunk_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
        let chunk_start = reader.stream_position()?;
        let chunk_end = chunk_start
            .checked_add(chunk_len)
            .context("WAV chunk length overflow")?;
        if chunk_end > file_len {
            bail!("truncated WAV chunk")
        }

        if chunk_id == b"fmt " {
            if chunk_len < 16 {
                bail!("invalid WAV fmt chunk")
            }
            let mut fmt = vec![0u8; chunk_len.min(64) as usize];
            reader.read_exact(&mut fmt)?;
            let format_tag = le_u16(&fmt, 0)?;
            let channels = le_u16(&fmt, 2)?;
            let sample_rate = le_u32(&fmt, 4)?;
            let block_align = le_u16(&fmt, 12)? as usize;
            let bits = le_u16(&fmt, 14)?;
            if !(1..=8).contains(&channels) {
                bail!("unsupported WAV channel count: {channels}")
            }
            if !(1_000..=384_000).contains(&sample_rate) {
                bail!("unsupported WAV sample rate: {sample_rate}")
            }

            let effective_tag = if format_tag == 0xfffe {
                if fmt.len() < 40 {
                    bail!("truncated WAVE_FORMAT_EXTENSIBLE fmt chunk")
                }
                le_u16(&fmt, 24)?
            } else {
                format_tag
            };
            let sample_format = match (effective_tag, bits) {
                (1, 16) => RawSampleFormat::S16Le,
                (1, 24) => RawSampleFormat::S24Le,
                (1, 32) => RawSampleFormat::S32Le,
                (3, 32) => RawSampleFormat::F32Le,
                _ => bail!(
                    "unsupported WAV encoding: format_tag={effective_tag} bits_per_sample={bits}"
                ),
            };
            let parsed = AudioSpec {
                sample_rate,
                channels,
                sample_format,
            };
            if block_align != parsed.block_align() {
                bail!("WAV block_align conflicts with sample format")
            }
            spec = Some(parsed);
        } else if chunk_id == b"data" {
            data = Some((chunk_start, chunk_len));
        }

        let padded = chunk_len + (chunk_len & 1);
        reader.seek(SeekFrom::Start(chunk_start + padded))?;
    }

    let spec = spec.context("WAV file has no fmt chunk")?;
    let (data_offset, data_len) = data.context("WAV file has no data chunk")?;
    if data_len == 0 {
        bail!("WAV data chunk is empty")
    }
    if data_len % spec.block_align() as u64 != 0 {
        bail!("WAV data length is not aligned to complete sample frames")
    }
    Ok(WavLayout {
        spec,
        data_offset,
        data_len,
    })
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .context("truncated WAV fmt field")?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .context("truncated WAV fmt field")?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn validate_header_conflicts(args: &AnalyzeAudioArgs, spec: &AudioSpec) -> Result<()> {
    if args.sample_rate.is_some_and(|v| v != spec.sample_rate) {
        bail!("provided sample_rate conflicts with WAV metadata")
    }
    if args.channels.is_some_and(|v| v != spec.channels) {
        bail!("provided channels conflicts with WAV metadata")
    }
    if args.sample_format.is_some_and(|v| v != spec.sample_format) {
        bail!("provided sample_format conflicts with WAV metadata")
    }
    Ok(())
}

fn analyze_raw(path: &Path, file_len: u64, args: &AnalyzeAudioArgs) -> Result<AudioAnalysisResult> {
    let detected = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    let sample_format = args.sample_format;

    if sample_format.is_none() {
        let format_candidates = raw_format_candidates(path, file_len)?;
        if !format_candidates.is_empty() {
            candidates.insert("sample_format".into(), format_candidates);
        }
    }

    if args.channels.is_none() {
        candidates.insert(
            "channels".into(),
            vec![
                DetectionValue {
                    value: json!(1),
                    confidence: 0.5,
                },
                DetectionValue {
                    value: json!(2),
                    confidence: 0.5,
                },
            ],
        );
    }

    let mut missing = Vec::new();
    if args.sample_rate.is_none() {
        missing.push("sample_rate".into());
    }
    if args.channels.is_none() {
        missing.push("channels".into());
    }
    if sample_format.is_none() {
        missing.push("sample_format".into());
    }

    if !missing.is_empty() {
        return Ok(AudioAnalysisResult::NeedsInput {
            detected_container: "raw_pcm".into(),
            missing: missing.clone(),
            detected,
            candidates,
            message: format!(
                "The file has no recognized WAV header. Raw PCM cannot be decoded reliably without {}. Provide only the listed missing fields; sample_rate is never guessed from waveform content.",
                missing.join(", ")
            ),
        });
    }

    let sample_rate = args
        .sample_rate
        .context("raw PCM sample_rate is required")?;
    let channels = args.channels.context("raw PCM channels is required")?;
    let sample_format = sample_format.context("raw PCM sample_format is required")?;
    if !(1_000..=384_000).contains(&sample_rate) {
        bail!("raw PCM sample_rate must be between 1000 and 384000 Hz")
    }
    if !(1..=8).contains(&channels) {
        bail!("raw PCM channels must be between 1 and 8")
    }
    let spec = AudioSpec {
        sample_rate,
        channels,
        sample_format,
    };
    if !file_len.is_multiple_of(spec.block_align() as u64) {
        bail!("raw PCM file length is not aligned to the supplied sample format and channel count")
    }
    analyze_pcm(path, 0, file_len, spec, "raw_pcm", "user")
}

fn raw_format_candidates(path: &Path, file_len: u64) -> Result<Vec<DetectionValue>> {
    let mut candidates = Vec::new();
    if file_len.is_multiple_of(2) {
        candidates.push(DetectionValue {
            value: json!("s16le"),
            confidence: 0.45,
        });
    }
    if file_len.is_multiple_of(3) {
        candidates.push(DetectionValue {
            value: json!("s24le"),
            confidence: 0.35,
        });
    }
    if file_len.is_multiple_of(4) {
        candidates.push(DetectionValue {
            value: json!("s32le"),
            confidence: 0.35,
        });
        let mut reader = BufReader::new(File::open(path)?);
        let mut sample = vec![0u8; file_len.min(64 * 1024) as usize];
        reader.read_exact(&mut sample)?;
        let mut total = 0usize;
        let mut finite = 0usize;
        let mut normalized = 0usize;
        let mut active = 0usize;
        for chunk in sample.as_chunks::<4>().0 {
            total += 1;
            let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
            if value.is_finite() {
                finite += 1;
                if value.abs() <= 1.2 {
                    normalized += 1;
                }
                if value.abs() >= 1e-6 && value.abs() <= 1.2 {
                    active += 1;
                }
            }
        }
        if total >= 128 {
            let finite_ratio = finite as f64 / total as f64;
            let normalized_ratio = normalized as f64 / total as f64;
            let active_ratio = active as f64 / total as f64;
            let confidence =
                if finite_ratio == 1.0 && normalized_ratio >= 0.995 && active_ratio >= 0.30 {
                    0.85
                } else if finite_ratio >= 0.98 && normalized_ratio >= 0.95 {
                    0.60
                } else {
                    0.10
                };
            candidates.push(DetectionValue {
                value: json!("f32le"),
                confidence,
            });
        }
    }
    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(candidates)
}

fn analyze_pcm(
    path: &Path,
    data_offset: u64,
    data_len: u64,
    spec: AudioSpec,
    container: &str,
    metadata_source: &str,
) -> Result<AudioAnalysisResult> {
    let block_align = spec.block_align();
    if block_align == 0 || !data_len.is_multiple_of(block_align as u64) {
        bail!("audio data is not aligned to complete sample frames")
    }
    let frame_count = data_len / block_align as u64;
    if frame_count == 0 {
        bail!("audio contains no sample frames")
    }

    let mut reader = BufReader::new(File::open(path)?);
    reader.seek(SeekFrom::Start(data_offset))?;
    let mut bytes_remaining = data_len;
    let mut mono_count = 0u64;
    let mut source_sample_count = 0u64;
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f64;
    let mut clipped = 0u64;

    let analysis_frame_samples = ((spec.sample_rate as usize * 20) / 1000).max(1);
    let mut analysis_frame = Vec::with_capacity(analysis_frame_samples);
    let mut frame_dbfs = Vec::new();
    let mut silent_samples = 0u64;
    let mut current_silent_samples = 0u64;
    let mut longest_silent_samples = 0u64;

    let mut spectrum_queue = VecDeque::with_capacity(FFT_SIZE * 2);
    let mut spectrum_power = vec![0.0f64; FFT_SIZE / 2 + 1];
    let mut spectrum_scratch = vec![Complex::default(); FFT_SIZE];
    let possible_spectrum_windows = if frame_count <= FFT_SIZE as u64 {
        1
    } else {
        1 + (frame_count - FFT_SIZE as u64) / FFT_HOP as u64
    };
    let spectrum_stride = possible_spectrum_windows
        .div_ceil(MAX_SPECTRUM_WINDOWS)
        .max(1) as usize;
    let spectrum_advance = FFT_HOP.saturating_mul(spectrum_stride);
    let mut spectrum_skip_remaining = 0usize;
    let mut spectrum_frames = 0usize;

    while bytes_remaining >= block_align as u64 {
        let mut mono = 0.0f64;
        for _ in 0..spec.channels {
            let sample = read_sample(&mut reader, spec.sample_format)?;
            if !sample.is_finite() {
                bail!("audio contains non-finite floating-point samples")
            }
            source_sample_count += 1;
            let abs = sample.abs();
            peak = peak.max(abs);
            if abs >= 0.999 {
                clipped += 1;
            }
            mono += sample;
        }
        mono /= spec.channels as f64;
        bytes_remaining -= block_align as u64;
        mono_count += 1;
        sum += mono;
        sum_sq += mono * mono;
        analysis_frame.push(mono);
        if spectrum_skip_remaining > 0 {
            spectrum_skip_remaining -= 1;
        } else {
            spectrum_queue.push_back(mono);
        }

        if analysis_frame.len() >= analysis_frame_samples {
            let db = rms_dbfs(&analysis_frame);
            frame_dbfs.push(db);
            let frame_len = analysis_frame.len() as u64;
            if db <= SILENCE_THRESHOLD_DBFS {
                silent_samples += frame_len;
                current_silent_samples += frame_len;
                longest_silent_samples = longest_silent_samples.max(current_silent_samples);
            } else {
                current_silent_samples = 0;
            }
            analysis_frame.clear();
        }

        while spectrum_queue.len() >= FFT_SIZE {
            {
                let contiguous = spectrum_queue.make_contiguous();
                accumulate_spectrum(
                    &contiguous[..FFT_SIZE],
                    &mut spectrum_power,
                    &mut spectrum_scratch,
                );
            }
            spectrum_frames += 1;
            if spectrum_advance < spectrum_queue.len() {
                for _ in 0..spectrum_advance {
                    spectrum_queue.pop_front();
                }
            } else {
                spectrum_skip_remaining = spectrum_advance.saturating_sub(spectrum_queue.len());
                spectrum_queue.clear();
            }
        }
    }

    if !analysis_frame.is_empty() {
        let db = rms_dbfs(&analysis_frame);
        frame_dbfs.push(db);
        let frame_len = analysis_frame.len() as u64;
        if db <= SILENCE_THRESHOLD_DBFS {
            silent_samples += frame_len;
            current_silent_samples += frame_len;
            longest_silent_samples = longest_silent_samples.max(current_silent_samples);
        }
    }
    if spectrum_frames == 0 && !spectrum_queue.is_empty() {
        let mut window = spectrum_queue.into_iter().collect::<Vec<_>>();
        window.resize(FFT_SIZE, 0.0);
        accumulate_spectrum(&window, &mut spectrum_power, &mut spectrum_scratch);
        spectrum_frames = 1;
    }

    if mono_count == 0 || source_sample_count == 0 {
        bail!("audio contains no complete samples")
    }
    let rms = (sum_sq / mono_count as f64).sqrt();
    let rms_dbfs = amplitude_dbfs(rms);
    let peak_dbfs = amplitude_dbfs(peak);
    let crest_factor_db = if rms <= 0.0 || peak <= 0.0 {
        0.0
    } else {
        peak_dbfs - rms_dbfs
    };
    let dc_offset = sum / mono_count as f64;
    let clipping_ratio = clipped as f64 / source_sample_count as f64;

    let mut sorted_frames = frame_dbfs.clone();
    sorted_frames.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor_dbfs = percentile(&sorted_frames, 0.10).unwrap_or(MIN_DBFS);
    let signal_dbfs = percentile(&sorted_frames, 0.80).unwrap_or(noise_floor_dbfs);
    let estimated_snr_db = (signal_dbfs - noise_floor_dbfs).clamp(-20.0, 80.0);
    let silence_ratio = silent_samples as f64 / mono_count as f64;
    let longest_ms =
        ((longest_silent_samples as f64 * 1000.0) / spec.sample_rate as f64).round() as u64;

    if spectrum_frames > 0 {
        for value in &mut spectrum_power {
            *value /= spectrum_frames as f64;
        }
    }
    let spectrum = summarize_spectrum(&spectrum_power, spec.sample_rate);

    Ok(AudioAnalysisResult::Ok {
        input: AudioInputInfo {
            container: container.into(),
            metadata_source: metadata_source.into(),
        },
        format: AudioFormatFeatures {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            bits_per_sample: spec.sample_format.bits_per_sample(),
            sample_format: spec.sample_format.as_str().into(),
            duration_sec: mono_count as f64 / spec.sample_rate as f64,
        },
        level: AudioLevelFeatures {
            rms_dbfs,
            peak_dbfs,
            crest_factor_db,
            clipping_ratio,
            dc_offset,
        },
        noise: AudioNoiseFeatures {
            noise_floor_dbfs,
            estimated_snr_db,
        },
        silence: AudioSilenceFeatures {
            ratio: silence_ratio,
            longest_ms,
        },
        spectrum,
    })
}

fn read_sample(reader: &mut BufReader<File>, format: RawSampleFormat) -> Result<f64> {
    match format {
        RawSampleFormat::S16Le => {
            let mut bytes = [0u8; 2];
            reader.read_exact(&mut bytes)?;
            Ok(i16::from_le_bytes(bytes) as f64 / 32768.0)
        }
        RawSampleFormat::S24Le => {
            let mut bytes = [0u8; 3];
            reader.read_exact(&mut bytes)?;
            let raw = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
            let signed = if raw & 0x0080_0000 != 0 {
                raw | !0x00ff_ffff
            } else {
                raw
            };
            Ok(signed as f64 / 8_388_608.0)
        }
        RawSampleFormat::S32Le => {
            let mut bytes = [0u8; 4];
            reader.read_exact(&mut bytes)?;
            Ok(i32::from_le_bytes(bytes) as f64 / 2_147_483_648.0)
        }
        RawSampleFormat::F32Le => {
            let mut bytes = [0u8; 4];
            reader.read_exact(&mut bytes)?;
            Ok(f32::from_le_bytes(bytes) as f64)
        }
    }
}

fn rms_dbfs(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return MIN_DBFS;
    }
    let sum_sq = samples.iter().map(|sample| sample * sample).sum::<f64>();
    amplitude_dbfs((sum_sq / samples.len() as f64).sqrt())
}

fn amplitude_dbfs(value: f64) -> f64 {
    if value <= 1e-12 {
        MIN_DBFS
    } else {
        (20.0 * value.log10()).max(MIN_DBFS)
    }
}

fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let index = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
    sorted.get(index).copied()
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn add(self, rhs: Self) -> Self {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }

    fn sub(self, rhs: Self) -> Self {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }

    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }

    fn power(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

fn accumulate_spectrum(samples: &[f64], accumulator: &mut [f64], scratch: &mut [Complex]) {
    debug_assert_eq!(scratch.len(), FFT_SIZE);
    for (index, value) in scratch.iter_mut().enumerate() {
        let sample = samples.get(index).copied().unwrap_or(0.0);
        let window = 0.5 - 0.5 * (2.0 * PI * index as f64 / (FFT_SIZE - 1) as f64).cos();
        value.re = sample * window;
        value.im = 0.0;
    }
    fft(scratch);
    for (index, power) in accumulator.iter_mut().enumerate() {
        *power += scratch[index].power();
    }
}

fn fft(values: &mut [Complex]) {
    let n = values.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            values.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        let w_len = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..n).step_by(len) {
            let mut w = Complex { re: 1.0, im: 0.0 };
            for offset in 0..len / 2 {
                let even = values[start + offset];
                let odd = values[start + offset + len / 2].mul(w);
                values[start + offset] = even.add(odd);
                values[start + offset + len / 2] = even.sub(odd);
                w = w.mul(w_len);
            }
        }
        len <<= 1;
    }
}

fn summarize_spectrum(power: &[f64], sample_rate: u32) -> AudioSpectrumFeatures {
    let total = power.iter().sum::<f64>();
    if total <= 1e-18 {
        return AudioSpectrumFeatures {
            centroid_hz: 0.0,
            rolloff_95_hz: 0.0,
            flatness: 0.0,
            speech_band_ratio: 0.0,
            low_frequency_ratio: 0.0,
            high_frequency_ratio: 0.0,
        };
    }
    let bin_hz = sample_rate as f64 / FFT_SIZE as f64;
    let centroid_hz = power
        .iter()
        .enumerate()
        .map(|(index, value)| index as f64 * bin_hz * value)
        .sum::<f64>()
        / total;

    let threshold = total * 0.95;
    let mut cumulative = 0.0;
    let mut rolloff_95_hz = 0.0;
    for (index, value) in power.iter().enumerate() {
        cumulative += value;
        if cumulative >= threshold {
            rolloff_95_hz = index as f64 * bin_hz;
            break;
        }
    }

    let non_dc = &power[1..];
    let arithmetic = non_dc.iter().sum::<f64>() / non_dc.len().max(1) as f64;
    let geometric = (non_dc
        .iter()
        .map(|value| value.max(1e-18).ln())
        .sum::<f64>()
        / non_dc.len().max(1) as f64)
        .exp();
    let flatness = if arithmetic <= 1e-18 {
        0.0
    } else {
        (geometric / arithmetic).clamp(0.0, 1.0)
    };

    let low = band_power(power, bin_hz, 0.0, 80.0);
    let speech = band_power(power, bin_hz, 300.0, 3400.0);
    let high = band_power(power, bin_hz, 3400.0, sample_rate as f64 / 2.0 + bin_hz);

    AudioSpectrumFeatures {
        centroid_hz,
        rolloff_95_hz,
        flatness,
        speech_band_ratio: (speech / total).clamp(0.0, 1.0),
        low_frequency_ratio: (low / total).clamp(0.0, 1.0),
        high_frequency_ratio: (high / total).clamp(0.0, 1.0),
    }
}

fn band_power(power: &[f64], bin_hz: f64, start_hz: f64, end_hz: f64) -> f64 {
    power
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let hz = index as f64 * bin_hz;
            (hz >= start_hz && hz < end_hz).then_some(*value)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, io::Write};

    fn write_pcm16_wav(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        samples: &[i16],
    ) -> Result<()> {
        let data_len = (samples.len() * 2) as u32;
        let byte_rate = sample_rate * channels as u32 * 2;
        let block_align = channels * 2;
        let mut file = File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(&(36u32 + data_len).to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&16u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&data_len.to_le_bytes())?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes())?;
        }
        Ok(())
    }

    fn executor(dir: &Path) -> Result<AudioToolExecutor> {
        AudioToolExecutor::new(dir)
    }

    #[test]
    fn wav_header_drives_format_and_sine_spectrum() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("tone.bin");
        let sample_rate = 16_000u32;
        let samples = (0..sample_rate)
            .map(|n| {
                let t = n as f64 / sample_rate as f64;
                (0.5 * (2.0 * PI * 1_000.0 * t).sin() * i16::MAX as f64) as i16
            })
            .collect::<Vec<_>>();
        write_pcm16_wav(&path, sample_rate, 1, &samples)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        let AudioAnalysisResult::Ok {
            format,
            level,
            spectrum,
            ..
        } = result
        else {
            panic!("expected successful WAV analysis")
        };
        assert_eq!(format.sample_rate, sample_rate);
        assert_eq!(format.channels, 1);
        assert_eq!(format.sample_format, "s16le");
        assert!((format.duration_sec - 1.0).abs() < 0.001);
        assert!(level.clipping_ratio < 0.001);
        assert!((spectrum.centroid_hz - 1_000.0).abs() < 80.0);
        Ok(())
    }

    #[test]
    fn clipping_and_silence_are_detected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("clip.wav");
        let sample_rate = 16_000u32;
        let mut samples = vec![i16::MAX; sample_rate as usize / 2];
        samples.extend(vec![0; sample_rate as usize / 2]);
        write_pcm16_wav(&path, sample_rate, 1, &samples)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        let AudioAnalysisResult::Ok { level, silence, .. } = result else {
            panic!("expected successful WAV analysis")
        };
        assert!(level.clipping_ratio > 0.45);
        assert!(silence.ratio > 0.45);
        assert!(silence.longest_ms >= 450);
        Ok(())
    }

    #[test]
    fn raw_pcm_without_metadata_requests_only_required_fields() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("record.wav");
        fs::write(&path, [0u8; 3200])?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: Some(16_000),
            channels: None,
            sample_format: Some(RawSampleFormat::S16Le),
        })?;
        let AudioAnalysisResult::NeedsInput { missing, .. } = result else {
            panic!("expected raw metadata request")
        };
        assert_eq!(missing, vec!["channels"]);
        Ok(())
    }

    #[test]
    fn raw_pcm_with_complete_metadata_is_analyzed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("record.pcm");
        let mut data = Vec::new();
        for n in 0..16_000u32 {
            let t = n as f64 / 16_000.0;
            let sample = (0.25 * (2.0 * PI * 440.0 * t).sin() * i16::MAX as f64) as i16;
            data.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(&path, data)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: Some(16_000),
            channels: Some(1),
            sample_format: Some(RawSampleFormat::S16Le),
        })?;
        let AudioAnalysisResult::Ok { input, format, .. } = result else {
            panic!("expected raw analysis")
        };
        assert_eq!(input.container, "raw_pcm");
        assert_eq!(format.sample_rate, 16_000);
        Ok(())
    }

    #[test]
    fn dc_offset_is_measured() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("dc.wav");
        let samples = vec![3277i16; 16_000];
        write_pcm16_wav(&path, 16_000, 1, &samples)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        let AudioAnalysisResult::Ok { level, .. } = result else {
            panic!("expected successful WAV analysis")
        };
        assert!((level.dc_offset - 0.1).abs() < 0.01);
        Ok(())
    }

    #[test]
    fn stereo_is_downmixed_for_dsp_but_preserves_channel_count() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("stereo.wav");
        let sample_rate = 16_000u32;
        let mut samples = Vec::with_capacity(sample_rate as usize * 2);
        for n in 0..sample_rate {
            let t = n as f64 / sample_rate as f64;
            let sample = (0.3 * (2.0 * PI * 440.0 * t).sin() * i16::MAX as f64) as i16;
            samples.push(sample);
            samples.push(sample);
        }
        write_pcm16_wav(&path, sample_rate, 2, &samples)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        let AudioAnalysisResult::Ok {
            format, spectrum, ..
        } = result
        else {
            panic!("expected successful stereo analysis")
        };
        assert_eq!(format.channels, 2);
        assert!((format.duration_sec - 1.0).abs() < 0.001);
        assert!((spectrum.centroid_hz - 440.0).abs() < 80.0);
        Ok(())
    }

    #[test]
    fn estimated_snr_tracks_quiet_and_active_frames() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("snr.wav");
        let sample_rate = 16_000u32;
        let mut samples = Vec::with_capacity(sample_rate as usize);
        for n in 0..sample_rate {
            let sample = if n < sample_rate / 4 {
                let deterministic_noise =
                    ((n.wrapping_mul(1103515245).wrapping_add(12345) >> 16) & 0x7fff) as i32;
                ((deterministic_noise - 16384) / 40) as i16
            } else {
                let t = n as f64 / sample_rate as f64;
                (0.3 * (2.0 * PI * 700.0 * t).sin() * i16::MAX as f64) as i16
            };
            samples.push(sample);
        }
        write_pcm16_wav(&path, sample_rate, 1, &samples)?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        let AudioAnalysisResult::Ok { noise, .. } = result else {
            panic!("expected successful WAV analysis")
        };
        assert!(noise.estimated_snr_db > 15.0);
        Ok(())
    }

    #[test]
    fn conflicting_wav_parameters_are_rejected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("conflict.wav");
        write_pcm16_wav(&path, 48_000, 2, &[0; 480])?;
        let error = executor(dir.path())?
            .analyze(&AnalyzeAudioArgs {
                path: path.display().to_string(),
                sample_rate: Some(16_000),
                channels: None,
                sample_format: None,
            })
            .expect_err("conflicting metadata must fail");
        assert!(format!("{error:#}").contains("conflicts with WAV metadata"));
        Ok(())
    }

    #[test]
    fn invalid_audio_file_fails_cleanly() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("bad.bin");
        fs::write(&path, b"not audio")?;
        let result = executor(dir.path())?.analyze(&AnalyzeAudioArgs {
            path: path.display().to_string(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })?;
        assert!(matches!(result, AudioAnalysisResult::NeedsInput { .. }));
        Ok(())
    }
}
