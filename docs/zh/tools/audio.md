# 音频分析

`analyze_audio` 使用纯 Rust 对本地 WAV / Raw PCM 做确定性 DSP，提取时长、峰值、能量、削波、静音等特征。WAV 使用真实 header，不把模型猜测作为采样参数。

Raw PCM 没有头部；必须提供可靠采样率、声道和采样格式（`s16le`、`s24le`、`s32le`、`f32le`）。缺失时返回 `needs_input`，TUI 用多字段问答收集常见值或自定义值并本地重试；取消不猜测。

`judge_audio_quality` 基于 Feature JSON 做质量判断。配置 `jev_api_key` 后用 Jev，否则用当前通用 LLM；缓存当前任务的真实分析结果，避免模型改写数值。原始 WAV 不上传给判断模型，但特征会发送给所选模型服务。判断是模型结论，需结合原始特征解释局限。

```toml
jev_api_key = ""
jev_endpoint = "https://api.typesafe.ai/v1/systemone"
jev_model = "jev-latest"
```

可用 `NL2SH_JEV_API_KEY` / `NL2SH_JEV_ENDPOINT` / `NL2SH_JEV_MODEL` 覆盖。Web 的音频预览是浏览器播放功能，不等于工具已分析音质。

例子：“分析 `@/data/local/tmp/test.wav` 的削波与静音比例，再说明证据”；无头 PCM 请同时说明真实录制参数。参数见 [工具目录](../reference/tool-catalog.md)。
