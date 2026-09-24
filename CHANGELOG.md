# Changelog

## [Unreleased]

### Changed

- Removed the old top-level Rust tool-module re-exports; library callers now use `nl2sh::tools::<domain>` paths.
- Reorganized all built-in tool implementations by domain and routed them through an explicit registry with derived schemas, local risk metadata, and prepare/confirm/execute handling. Existing Android, audio, network, memory, Web approval, and shell safety behavior remains in place; the procedural macro is build-time only.
- Add F2 to expand or collapse all Web tool results while preserving per-card controls.
- Render escaped newlines in Web tool output as visible line breaks without changing stored results.
- Show each Web tool call and its matching output in a separate collapsible item, including restored sessions.
- Aligned the Web dark theme with the TUI's semantic palette, including navigation, Markdown, focus, and status colors.
- Repaint only the TUI conversation area on scroll, including blank cells after shorter lines, to remove old characters without flashing the whole screen.
- Generate embedded Web assets from the npm lockfile during Cargo builds, and stop tracking `web/dist`; Android releases remain single binaries.
- Rebuilt the embedded Web UI with Axum 0.8, rust-embed, SSE, security-gated WebSocket terminal commands, and a Preact/TypeScript/Vite frontend while retaining single-binary Android distribution.
- Stabilized the live-TUI Agent-response regression by using a valid Responses SSE fixture, disabling startup decoration in that test, and avoiding assertions against fragmented raw ratatui ANSI output.
- Added a process-lifetime approval option and `/permission` controls for eligible mutations; root, strong-confirmation, Dangerous, and Critical operations remain individually confirmed.
- Made Android visual workflows bounded and evidence-driven: compact UI inspection, post-input UI state, PNG/JPEG/WebP viewing with in-process downscaling, provider-error detection, and corrected MediaStore projections and ordering.
- Moved the canonical source, release history, package, support, and self-update links to the `nl2sh` GitHub organization, and refreshed the project logo and Cargo package metadata.

### Added

- Added Web `@` path suggestions with keyboard completion and shared path reference resolution, plus a searchable toolbar catalog of available tools and descriptions.
- Added a Web toolbar action to download the selected conversation and shared audit log as a ZIP archive.
- Added Web Markdown rendering, collapsible tool output, joined streaming responses, quick provider/model/approval controls, and session usage statistics.
- Added concurrent Web Agent sessions with a persistent session sidebar, per-session approvals, background status, and LLM-generated titles.
- Added an embedded browser interface for configuration and Agent conversations, including live output, approvals, follow-up questions, and saved sessions.
- Added confirmed, bounds-validated Android input injection; bounded screenshot image attachments; confirmed public JSON POST; structured notification, crash/ANR, thermal/power, netstats, storage, Wi-Fi/Ethernet, Doze, permission, clipboard, media, connectivity, and persistent Agent-memory tools. Mutations remain behind local confirmation and image payloads are omitted from saved sessions.
- Added Codex-style `!command` execution in the Agent TUI. Commands bypass the Provider but retain local security classification, confirmation, root, PTY, timeout, and terminal-restoration boundaries; bounded output and exit status remain visible without entering model context.
- Added built-in `analyze_audio` and `judge_audio_quality` tools: deterministic WAV/raw-PCM DSP with explicit `needs_input` for unknown headerless metadata, plus Jev-backed quality scoring with general-LLM fallback only when Jev is not configured.
- Added a Codex-style structured question popup for missing tool input. Headerless raw PCM analysis now lets users select common metadata values or type custom answers, then retries locally without asking the model to guess.

## [1.0.1] - 2026-08-31

### Added

- Added a TUR-ready `tur/nl2sh/build.sh` recipe using a pinned release archive, SHA-256 verification, and the Termux Rust toolchain.
- Added centralized direct-Android-shell versus Termux runtime detection for prompts, runtime summaries, configuration paths, and shell selection.
- Added `android-build-tmux-run.sh` for ABI-aware Termux package builds, ADB deployment, and SSH/tmux installation and launch.
- Added a self-hosted signed Termux APT repository for `aarch64` and `arm`, with package-manager-owned updates and XDG config/state paths.
- Added a local `pack-termux-release.sh` workflow that directly emits separate `aarch64` and `arm` `.deb` packages, with a dedicated Termux installation guide kept in the repository.
- Added `pack-termux-release.ps1`, which builds with the Windows Android NDK and delegates only Debian packaging to WSL `dpkg-deb`.
- Added OpenRouter as a built-in OpenAI-compatible Provider and made `openrouter/free` at `https://openrouter.ai/api/v1` the default for new configurations.

### Changed

- Made stock Android shell the explicit first-class runtime and Termux a compatibility runtime with dynamic `$PREFIX/bin/sh`, XDG, package-manager, and optional-tool guidance; security and confirmation policy remain unchanged.

## [1.0.0] - 2026-08-25

### Changed

- The startup train now uses a multi-color theme treatment for its smoke, roof, body, `NL2SH` branding, and wheels while preserving viewport clipping and ANSI 256 fallback.
- Changed `@` path suggestions so Enter or Tab inserts the selected candidate, while Right keeps its normal cursor-movement behavior.
- Added an optional read-only Tencent ima knowledge-base connector with no-proxy direct networking, dynamically exposed list/search/read Agent tools, bounded original-content retrieval, strict temporary-URL origin policy, and credential/session/log redaction; no ima write operations are implemented.
- Added `@` file and directory references in the TUI with bounded path suggestions, Up/Down selection, Enter/Tab completion, relative/absolute/tilde paths, and longest-existing-prefix parsing for prompts such as `@test.txt写的是什么内容`; referenced content remains behind bounded structured file tools.
- Made the confirmation panel size itself from wrapped content; oversized commands and diffs scroll with the wheel or PageUp/PageDown while approval controls remain pinned.
- Added bounded `read_file`, `list_dir`, `search_text`, and `apply_patch` tools without a workspace path sandbox; edits show a diff and require confirmation before an atomic write.
- Added private session autosave plus `/sessions` list, resume, rename, and delete operations; credentials, balances, and temporary approvals are excluded.
- Fixed history scrolling through Windows ADB terminals with a launcher-enabled alternate-scroll mode that leaves remote mouse capture disabled and maps terminal-generated Up/Down events to conversation scrolling; Linux keeps native mouse capture.
- Completed the Android device validation matrix for root/non-root execution, mutation confirmation, command timeout cleanup, and fullscreen interactive programs with terminal/TUI restoration.
- Fixed incomplete TUI frames after leaving `/shell` by explicitly invalidating ratatui's retained buffer before redrawing the restored alternate screen.
- TUI Settings now keeps separate in-session Endpoint drafts for Ollama and Custom, restoring each value after switching through other Provider presets.
- Restored built-in Provider selection inside the unified TUI Settings panel, sharing the OpenAI, DeepSeek, Moonshot/Kimi, SiliconFlow, Ollama, and Custom presets with the legacy wizard while preserving API keys, models, and protocol choices.
- Added `/shell` to suspend the TUI and open a direct interactive system shell; `exit` or Ctrl+D restores and fully redraws the existing TUI without sending shell content to the model or audit log.
- Agent tasks now enforce independent step, tool-call, active-time, stalled-progress, repeated-action, and hard-step budgets. Fast/Normal/Deep presets are available; confirmation waits are excluded from active time, while safety classification and confirmation remain mandatory.
- Repeated normalized commands with unchanged results are blocked before a fourth execution, stalled rounds force replanning before termination, and 80%/90% step warnings ask the model to converge. Task summaries now expose steps, tool calls, active duration, replans, and the terminating limit.
- 设置面板文本字段新增 UTF-8 安全的 Left/Right/Home/End 光标移动，插入、Backspace 和 Delete 均围绕当前光标执行，密码字段保持掩码。
- 在统一设置面板的“模型与智能体”Tab 恢复后台模型发现；可在 TUI 内拉取、选择并回填模型及 Provider 元数据，失败时保留手工输入。
- 修复部分 adb 终端丢失 CSI 前缀后将 `<35;46;8M` 一类 SGR 鼠标报告写入主输入框或设置字段的问题。
- 所有以 `/` 开头的输入统一限定为本地命令，未知命令在本地提示，绝不提交给 LLM；修复 `/update` 落入 Agent 提交流程的问题。
- 修复 `/config` 打开设置后仍被提交给模型的问题；设置面板接管输入焦点并显示独立输入边界、背景和闪烁光标。
- 移除 `/provider`、`/model`、`/models`、`/proxy`，统一使用 `/config` 或其别名 `/setting`。
- 新增 `update`/`/update` 和启动后台版本检查；按 ABI 下载、SHA-256 校验并原子替换，支持暂不更新或跳过指定版本。
- 将 Provider、模型、Agent、安全、界面和代理配置整合为多 Tab TUI 设置面板，最大步骤/轮次显示推荐值 24/16。

### Added

- API protocol now defaults to automatic negotiation: Responses is preferred, safe protocol mismatches fall back to Chat Completions, and the successful dialect is cached without treating authentication, rate-limit, 5xx, timeout, or partial-stream failures as negotiation signals.
- The unified Settings UI now provides an audit-log clear action and independent, default-on switches for the Buddha and startup-train ASCII art.

- Agent tasks now accumulate provider-reported input/output token usage across every tool-calling step and show the task totals in the TUI status line.
- A `/models` flow now fetches the current provider's model list with a visible network-loading message and falls back to manual model entry without logging credentials or raw account responses.
- Provider metadata is normalized behind a dedicated client for OpenAI, DeepSeek, SiliconFlow, and Ollama; known or user-overridden context windows drive an estimated context-usage percentage in the TUI.
- A non-audited `/balance` command uses documented bearer-token endpoints for DeepSeek and SiliconFlow; unsupported providers fail visibly without attempting private console APIs.
- Supported provider balances refresh every 60 seconds and remain visible in the TUI title bar; failures retain the last successful in-memory value without adding account data to conversation, configuration, or audit history.
- Agent history now contracts by complete oldest turns when observed provider input tokens cross the known context-window safety watermark, preserving the system instruction, current interaction, and complete tool rounds.
- Added an in-TUI `/proxy` editor for HTTP CONNECT, SOCKS5/SOCKS5H, authentication, bypass rules, and a non-destructive master switch; all Provider clients now share the same credential-safe proxy policy.
- Fragmented CSI/SS3 left and right arrow sequences are now reconstructed inside the proxy editor instead of being mistaken for a standalone Escape and closing the popup.
- Agent and command prompts now explicitly target stock Android `/system/bin/sh` and toybox, requiring evidence before using desktop scripting runtimes, development tools, or package managers.
- Fragmented `ESC O Q` F2 sequences are now normalized on ordinary input paths, preventing stray `OQ` text and reliably toggling tool-result expansion.
- Chat Completions and Responses now stream model text into the Agent TUI over SSE, with an animated semantic gradient while generation is active and normal Markdown styling immediately after completion.
- Release archives now contain both ARM64 and ARMv7 binaries in ABI-specific directories; Linux and double-clickable Windows BAT launchers select or connect an ADB device, detect its ABI, and deploy the matching binary automatically.
- Source build/deploy launchers are now named `android-build-run.sh` and `android-build-run.ps1`; they use the same ADB device selection flow and automatically compile the Rust target matching the selected device ABI.
- Local `pack-release.sh` and `pack-release.ps1` helpers build both Android ABIs and create the same combined `nl2sh-android.zip` layout and SHA256 checksum used by the GitHub release workflow.
- README, user-guide, and release-package TUI media now use the animated `screenshots/nl2sh.gif` demonstration instead of the previous static screenshot.

## [0.2.0] - 2026-08-22

### Added

- Agent prompts now receive a once-per-task, low-sensitivity Android runtime summary containing API level, ABI, shell, UID, and root/su capability hints; probe failures are omitted and security policy remains authoritative.
- A local `/exit` command now safely quits the TUI without entering model context.
- A non-blocking, one-shot ASCII steam train now crosses beneath the Buddha illustration on the startup welcome screen, with animated smoke and `NL2SH` branding; it is clipped to the conversation viewport and excluded from session/model history.
- The startup train now snaps to the conversation viewport's right edge before exiting when its two-column animation step would otherwise skip the exact edge position.
- A README support section with project contribution copy and a linked remote WeChat donation code.
- Project support and donation links plus a terminal-safe text illustration in both the startup welcome page and `/help`, without embedded image or QR rendering.
- Local `/help` and `/clear` TUI commands; clearing removes the current conversation, model context, and input recall while preserving the audit log.
- Bounded live TUI output, captured tool results, model tool context, and JSONL history with explicit truncation markers.
- MIT license file.
- A project-wide TUI visual specification covering the dark palette, semantic colors, component styling, ANSI 256 fallback, safety boundaries, and acceptance criteria.
- Mouse-wheel conversation scrolling together with native Shift+drag highlighting and right-click context-menu copy.
- A blinking accent-colored input caret, Unicode-safe cursor editing, and Up/Down input-history recall.
- A filtered vertical slash-command menu with keyboard selection and completion.
- Initial Rust project structure and Android cross-build script.
- Configuration loading, validation, secure initialization wizard and environment API-key override.
- CLI endpoint, model, and API-type overrides applied before final validation.
- Persistent ratatui/crossterm conversation screen, ASCII fallback, scrolling, and terminal restoration guard.
- Chat Completions and Responses protocol adapters behind a unified LLM trait.
- Agent tool loop with ordered call/result rounds, complete-turn context bounding, editing, and execution feedback.
- Fail-closed built-in/configurable security rules, non-TTY refusal, confirmation and strong double confirmation.
- Real openpty executor, interactive bridge/resize, ANSI filtering, pipeline fallback, cancellation, timeout escalation and root/su policy.
- Incremental output sinks for console streaming and TUI history replay.
- Configuration, security, root, HTTP mock, Agent loop/history/error and PTY tests.
- Isolated-process SIGINT regression test proving Agent cancellation and PTY child reaping.
- Live single-frame Agent TUI with in-frame confirmations, execution-mode overrides, cancellation, and pseudo-terminal lifecycle coverage.
- Android NDK build-script support for cc-rs native dependencies and a verified r28c/API 26 AArch64 release build.
- Selectable ARMv7 cross-build and API 34 device smoke coverage for Agent networking, PTY execution, result feedback, and TUI restoration.
- TUI-triggered Base-URL-first configuration, secure `0600` config writes, and provider hot reload.
- Separate TUI rows for user input and runtime status/context information.
- Semantic conversation colors for user input, tool calls, Agent responses, commands, successes, and errors.
- Append-only `0600` JSON Lines history logging for user requests, commands, outputs, results, and errors.
- Simplified Chinese and English TUI localization with Chinese as the default, plus localized setup and confirmation prompts.
- Populated startup history with common Android task examples, `/config`, scrolling, cancellation, and exit guidance.
- Collapsed completed tool results with F2 expansion while retaining live output, full diagnostic logs, and complete model feedback.
- Terminal-native Markdown rendering for Agent replies, including styled inline content, code blocks, Unicode-width tables, wrapping cells, and narrow-screen list fallback.
- One-command Android build/deploy/run script with configurable target directory, Rust target, and adb serial.
- Native Windows PowerShell Android build/deploy/run script using the NDK Windows LLVM toolchain without Bash or WSL.
- Linux and Windows PowerShell deploy/run scripts that push a prebuilt adjacent `nl2sh` binary without compiling it.
- Arrow-key provider selection for common OpenAI-compatible API base URLs, custom endpoints, and visible API-key entry through `/config`, `/provider`, or explicit `--init` setup.
- GitHub Actions release workflow that builds `aarch64-linux-android` and `armv7-linux-androideabi` release binaries with NDK r28c on tag push, packages each with `android-run-linux.sh`, `android-run-windows.ps1` and `config.toml.example` as `.tar.gz`/`.zip` with SHA256 checksums, and publishes them to a GitHub Release.
- A plain-language Chinese user guide covering ADB setup, Linux and Windows launch steps, ABI package selection, first-run configuration, and common troubleshooting; every 32-bit and 64-bit release archive includes it.
- Screenshot of the memory-query TUI conversation embedded in the Chinese user guide and README, and included in release archives so the packaged guide keeps its image.

### Changed

- Restored the project logo at `assets/logo.png` and centered it above the README title; terminal image rendering remains disabled.
- Buddha illustration rays and linework now use a dedicated bold decorative-gold theme token while text and facial details remain in the normal foreground color; copied history remains free of ANSI bytes and warning colors retain their security meaning.
- Missing configuration now opens the TUI without an automatic startup wizard; model tasks remain locally blocked until `/config` or `/provider` completes setup, while `/model` can update the model independently.
- Raised default `max_agent_steps` from 8 to 24 and `max_context_turns` from 10 to 16 so multi-stage Android tasks (install-and-verify, multi-step diagnostics) can complete before hitting the step limit.
- Split TUI output/history lifecycle handling from the main session controller.
- Treat 0.1.0 as the published baseline and continue development toward 0.1.1.
- Reworked command approval into a numbered, keyboard-navigable action list with numeric and `y/n/a/e/i/t` aliases; exact-command task approvals are memory-only and unavailable to root or high-risk commands.
- Replaced scattered high-saturation TUI colors with a centralized GitHub-Dark-inspired semantic palette, including TrueColor/ANSI 256 selection and field-level styling for Markdown, tool results, tables, status, input, and confirmations.
- Unified project documentation around nl2sh's positioning as an Android shell-focused Hermes-like AI agent delivered as a single executable with a rich TUI; this describes product shape, not Hermes API or plugin compatibility.
- Restyled the input row as a Codex-like muted-gray editor strip while keeping the shortcut separator, status row, and bottom separator on the terminal background.
- Agent final-answer guidance now favors user-language summaries, Markdown tables for structured comparisons, and concise readable text over raw tool output.
- Documented Android's misleading `No such file or directory` error for ABI/ELF-interpreter mismatches, including ARMv7 rebuild and verification commands.

### Fixed

- Fragmented CSI/SS3 arrow sequences from ADB terminals are reconstructed while the slash-command menu is open, so wrapping past the first or last item no longer closes the menu or inserts `A`/`B` characters after `/`.
- Completing or cancelling a streamed LLM response now invalidates ratatui's retained frame and performs one full redraw, preventing stale gradient characters after the final Markdown layout replaces the streaming layout.
- `android-run.sh` now applies the host terminal's current rows and columns to the allocated Android PTY before starting nl2sh, preventing an adb default width from truncating full-width TUI animations and layouts.
- The startup train now advances by terminal columns across the actual conversation viewport, so its final visible engine reaches the right border before the animation ends on wide terminals.
- The Buddha terminal illustration now measures its Chinese blessing row at the same 65-column display width as the surrounding ASCII frame, preventing right-edge protrusion.
- Android launch cleanup now disables host mouse tracking after `adb shell -t` exits, including interrupted/error exits, and every Rust panic path uses the same complete terminal restoration routine.
- Approval panels are now anchored above the input at the lower left, and fragmented adb arrow-key sequences can no longer trigger rejection or task approval and dismiss the panel.
- Approval-stage transitions now clear a stable full-panel area, preventing old option characters from remaining behind; the panel also consistently fills its bordered area with the alternate background.
- Read-only Android package-version queries using command substitution no longer trigger repeated mutation confirmations; mutating substitutions remain protected.
- Conversation history now accepts mouse-wheel and PageUp/PageDown scrolling instead of being forced to the bottom every frame.
- Android deployment now runs `adb root`, waits for the restarted daemon, verifies UID 0, and only falls back to `su -c`; unreadable private configuration fails early without weakening permissions.
- Fragmented SGR mouse reports from adb terminals are filtered at the input boundary instead of appearing as `[<...M` text or clearing existing input.
- Redirection to `/dev/null` and file-descriptor duplication no longer misclassify read-only diagnostics as mutations or spuriously require root; real writes remain protected.
- Multiline Agent Markdown is rendered as real terminal rows, preserving headings, lists, blank lines, and table rows instead of concatenating them into one line.
- Expanded tool results now calculate wrapped screen rows for bottom alignment and are no longer limited to the number of logical history entries when scrolling.
- Returning from an interactive PTY command now restores mouse capture and forces a full ratatui repaint instead of leaving only command output on a blank screen.

## [0.1.0] - 2026-08-04

### Added

- Initial development baseline.
