# Audio analysis

`analyze_audio` performs deterministic pure-Rust DSP on local WAV / raw PCM files, extracting duration, peaks, energy, clipping, silence, and related features. WAV uses actual headers rather than model guesses.

Headerless raw PCM requires reliable sample rate, channel count, and format (`s16le`, `s24le`, `s32le`, `f32le`). Missing metadata yields `needs_input`; the TUI gathers common or custom values in a structured question and retries locally. Cancellation never guesses.

`judge_audio_quality` judges Feature JSON with Jev when `jev_api_key` is configured, otherwise the main LLM. Actual analyses are cached within the task to avoid rewritten numbers. Raw WAV is not uploaded to the judge, but features are sent to the chosen model service. Quality judgments remain model conclusions and should be explained against the source features.

```toml
jev_api_key = ""
jev_endpoint = "https://api.typesafe.ai/v1/systemone"
jev_model = "jev-latest"
```

Override with `NL2SH_JEV_API_KEY` / `NL2SH_JEV_ENDPOINT` / `NL2SH_JEV_MODEL`. Browser audio previews do not mean a tool analyzed quality.

Try “Analyze clipping and silence in `@/data/local/tmp/test.wav` and explain the evidence.” Supply actual recording metadata for raw PCM. See [the catalog](../reference/tool-catalog.md) for arguments.
