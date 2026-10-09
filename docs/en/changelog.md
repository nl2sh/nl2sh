# Changelog

## [Unreleased]

- Add a bounded task-owned Agent continuation queue: system traces trigger read-only analysis at their deadline before the final model response; TUI/Web display the wait, cancellation and budgets stop continuation, security checks and audits remain enforced, and restart does not replay jobs.

- Add `start_system_trace`, `stop_system_trace`, and `analyze_system_trace`: approved bounded device Perfetto capture/stop, with read-only Rust decoding of supported protobuf scheduling, rendering, frame, Binder and wakeup events and explicit evidence coverage/limitations.

- Make DEX helper acquisition explicitly on-demand: add read-only `jadx_check` and approval-gated `jadx_install`, and stop `decompile_apk_class` from reaching the network so it only runs an already installed, verified helper and points at the install step when none exists. Acquisition is model-driven: the advisory runtime line reports `jadx_helper=installed|absent|unprovisionable`, and when `absent` the model inspects the source, installs after approval, and retries in the same task with no restart. A source build that embeds no signed policy falls back to a compile-time pinned release URL and SHA-256, with the signed policy still taking precedence; a policy that fails verification now errors instead of downgrading. The Agent is instructed never to claim recovered source from static indexes alone.

## [1.1.0] - 2026-10-08

- Verify GPG signatures, sizes, and checksums for runtime manifests and assets using a pinned trust root and embedded extension policy; Helper management supports healthy reconnects, update ownership, rollback, and extension installation.
- Unify tool descriptors, environment filtering, resource locks, and execution audit; constrain diagnostic queries and preserve execution identity.
- Add eight bounded static APK/DEX tools for methods, strings, references, manifests, permissions, exported components, and native libraries without JADX.
- Separate Web API responsibilities and show installation ownership, the correct update entry point, and verified extension versions in device information.

- Tailcat shortcut confirmation/retry stops and reaps the old managed listener before sharing selected ports. Filter fragmented arrow-key Escape sequences in TUI to prevent accidental dismissal; keep success and failure dialogs open.
- Simplify Web/TUI Tailcat guidance; detect the ADB connection port with an editable 5555 fallback, using a Web input or TUI P key, and share selected existing services through the port-sharing tool.
- Added a Tailcat icon/label at the top right of Web and `/tailcat` in TUI. The model-free dialog selects Web/ADB by default, shows actual download and execution status, stays open with copyable peer commands, and executes directly after port selection without further safety prompts, retaining existing tool verification.

- Service directory ownership errors report the path, UIDs and permissions, with recovery steps for shell/root transitions; cross-UID takeover remains forbidden.

- Verify Helper installation ownership and checksum before native self-update; runtime information reports the correct update entry point.

- Add native `service start/stop/restart/status --json` with actual ports, health checks, private shutdown tokens, and verified process identity; launcher Web mode uses this interface.

- Added read-only runtime capability discovery and `/healthz` / `/api/info`; Agent and direct calls filter tools by environment and ready adapters. Bridge negotiates v2 JSON RPC, validates reply request IDs and never replays failed writes; legacy companions remain supported.

- Add a text-free copy icon to fenced code blocks in Web model replies. It copies the complete code, briefly changes to a check mark on success, retains an accessible state name, and uses a browser-compatible fallback when the Clipboard API is unavailable.

- Replace Agent memory persistence with transactional embedded SQLite. Native Android stores it in `memory/` beside the executable and Termux uses its state directory, without a system `sqlite3` dependency or old-JSON migration. Add a Web sidebar memory panel with search and CRUD controls.

- Improve Agent memory retrieval guidance: read memory before answering about the user's name, personal identity, preferences, standing instructions, or saved facts; distinguish persistent memory from Android system users, device Owners, and app accounts; ask when an unanswered identity question remains ambiguous, and avoid loading memory for unrelated device tasks.

- Make `tailcat_send_file` default to the raw stream when `mode` is omitted, avoiding an scp dependency. Keep `copy` as an explicit drop-box compatibility mode and document that stock Android shells normally lack its external scp executable.

- Add strongly confirmed `tailcat_adb_pair`: guide Android 11+ Wireless debugging, revalidate current pairing information, share pairing/connection and optional Web ports, and return the pairing code and peer commands; reject stale approvals and replacement of existing listeners.

- Document arguments for all eight Tailcat tools, peer-side `forward` access to port 9999 and port mappings, and raw-stream/drop-box transfers in both directions.

- Expand Tailcat documentation for the Android 8/9 DNS protocol incompatibility, diagnostics, and recovery. Record portrait emulator comparisons using the same v0.7.0 binary: failure on Android 8.1/API 27 and successful HTTP forwarding on Android 15/API 35, with explicit validation limits.

- Add model-facing `nl2sh_config` with list/get/set/reset, dotted tool switches, and persisted/current-task inspection. Credentials are redacted and user-managed; writes require approval and sensitive policies require strong approval. Preserve other fields/comments and reject stale-preview overwrites. Changes apply on reload, without altering the current task.

- Fix TUI Ctrl+C/Ctrl+Q getting stuck while awaiting a model response by connecting request-scoped cancellation while preserving execution cleanup and terminal restoration.

- Fix Tailcat installation on Android x86_64 using the pinned official static amd64 archive with full verification. Clarify that port sharing forwards to an existing service without rebinding its port, and provide approved-install recovery for missing executables.

- Add group and per-tool enable switches to TUI `/config`, with configuration reload on save, group override reset, and a selection-following list.

- Add nl2sh-helper as the second Quick Start installation path, with Android controller setup, target connection, Web configuration, and ABI limits.

- Fixed Linux/Windows build and run launchers failing to recognize wireless ADB mDNS serials containing spaces and incorrectly requesting a device IP.

- Updated launcher and one-click installer messaging for all three ABIs, with migration guidance when reusing older packages without x86_64.

- Validated native execution on Android 8.1/API 27 x86_64 emulators, including TUI/PTY, mutation and dangerous-action confirmation, timeouts, terminal restoration, resizing, and Web-only pages/APIs.

- Added native Android API 26+ x86_64 builds, launchers, self-update assets, combined release archives, and self-hosted Termux deb/APT packages. Launchers prefer native x86_64; signed APT merging remains compatible with older two-architecture snapshots.

- Document A2A/MCP wire formats, task/result semantics, limits, approval and troubleshooting; refresh client setup and explain independently maintained Android projects.

### Added

- Add a Chinese-default, fully translated English MkDocs Material site, canonical manuals and translated release history, bilingual/link/code-reference CI, and one Pages artifact combining docs with signature- and digest-verified Termux APT.

- Let Web Quick Start fetch model names using its unsaved Base URL and API key, then choose a returned model or enter a name manually.
- Add an opt-in `tailcat_install` tool that downloads the pinned official release for the device ABI after confirmation, validates its checksum and executable, and atomically installs it.
- Add opt-in APK/JADX and Tailcat tool groups with persistent group and per-tool switches in the Web catalog; disabled tools are absent from model and direct-call registries.
- Add structured Tailcat file transfer and local-port sharing with local confirmation, plus a host/ADB device transfer test script.

### Changed

- Move Android Bridge and JADX helper source into independent `nl2sh/android-bridge` and `nl2sh/jadx-helper` repositories with their own Android builds, CI, tagged releases, and bilingual docs. Main releases no longer build the helper from source; current releases select and bundle component assets through the signed compatibility manifest.

- Record credential-free diagnostics for Web Quick Start model discovery, including a request ID, provider host, duration and upstream error status.
- Replace the Web high-risk `CONFIRM` text field with two explicit approval clicks, enforced per pending request by the server; color the approval button with the warning palette.
- Classify Tailcat shell commands by their network and file effects, including strong confirmation for uploads and port sharing.

### Fixed

- Retry a transient HTTP 405 from an explicitly selected or already negotiated LLM API dialect, including post-tool streaming requests, while preserving immediate Responses-to-Chat fallback during initial automatic protocol discovery.
- Show every fetched model in the Web Quick Start picker even when the model field already contains a preset name.

## [1.0.6] - 2026-10-01

### Added

- Show the running program version below the Web sidebar logo.
- Add bounded file browsing with file type, size, and modified time, plus previews for common images, video, text, code, and audio files. WAV and Raw PCM can be decoded with selected format parameters.

### Changed

- Move Web sessions, files, installed apps, tools, terminal, and configuration into a persistent left activity bar with resizable panels.

## [1.0.5] - 2026-10-01

- Publish the Android Web-only launcher support introduced after v1.0.4, so release binaries accept `--web-only` for background startup.

### Added

- Add `--web-only` to run the embedded Web UI without initializing the TUI, allowing a redirected Android shell process to remain available after ADB disconnects.
- Pass optional Web-only mode through source-build, packaged Android, and bootstrap launchers; these start the device process with `nohup`, skip ADB PTY allocation, and record output in `nl2sh-web.log`.
- Add an optional `nl2sh Keyboard` input method to the Android companion: `android.input_text` commits Unicode text through the focused app's `InputConnection` when a field advertises no editable state and no text action, the case ADBKeyboard exists for. The backend is fixed before confirmation and never swapped after it — `ACTION_SET_TEXT`, then the clipboard path reported read-only by the new `can_write` probe, then the input method, which is also the only backend that can clear a field through `mode: "replace"`. Editor package and field ID are rechecked across confirmation, password editors are always refused, and an editor that accepts a commit without reporting its text is reported as a failure instead of a silent no-op. With the keyboard selected, shell and root can also type from an ADB session through `com.nl2sh.bridge.IME_TEXT`, `IME_TEXT_B64` and `IME_CLEAR`; that receiver is registered by the input method itself because Android skips manifest broadcasts to background apps, requires the shell/root-only DUMP sender permission, and carries the authority of `adb shell` without the nl2sh confirmation chain. Selecting the keyboard stays a user decision, and the Accessibility and keyboard paths work independently.
- Add opt-in `bridge_auto_approve` for unattended A2A/MCP `ask` and `invoke` operations at every risk level; the default remains local approval or rejection.
- Serve authenticated Streamable HTTP MCP at `/mcp` on the A2A gateway's existing port, reusing the stdio adapter's tools and device approval boundary.
- Support Docker Compose, wireless ADB device-IP connections, and cross-host Hermes access. Remote plain HTTP requires explicit opt-in; device confirmation is preserved.
- Add an optional Android Accessibility companion APK for Unicode text entry, live node inspection, semantic text/bounds node clicks, and bounded swipe/scroll gestures. Its Binder provider accepts shell/root callers only; nl2sh retains its existing approval boundary.
- Add a direct registered Tool Runtime path for A2A/MCP callers and semantic `android.*` UI tools, preserving device-side risk assessment and confirmation. Direct calls can wait for a one-time local terminal decision; the gateway cannot approve actions. Screen capture can return a bounded MCP image block without retaining a screenshot file.

### Changed

- Move Web sessions, files, installed apps, tool guide, safe terminal, and configuration into a left activity bar with one minimizable panel. Give each panel its own resizable width while reserving chat space, and add an arrow beside each file or folder to insert its path into the chat composer. The file and app lists use bounded read-only data; terminal commands retain their existing security and approval flow.
- Let the gateway image build from a Docker Hub mirror through the `NL2SH_GATEWAY_BASE_IMAGE` Compose variable or the `GATEWAY_BASE_IMAGE` build argument, keeping `python:3.12-slim-bookworm` as the default base and the file free of vendor-specific registries.
- Give each `android.*` tool a focused argument schema with its required fields, and reject unrelated arguments before preparing an action.
- Describe the A2A Agent Card and MCP tools as a direct Android Device Runtime, with built-in Agent consultation explicitly optional; document that direct calls require no device model provider.
- Make `brush-parser` AST effects the primary shell classifier, split shell and domain policy modules, and keep built-in regex only for the fork-bomb signature. A command-bound in-process privilege broker rechecks approval and root plans before user shell execution; unknown syntax or dynamic code requires strong confirmation.

### Fixed

- Paste Unicode text through the accessibility companion when a field hides `isEditable`: the companion writes the system clipboard and pastes into the identity-matched node or its editable ancestor, and `android.input_text` falls back to that path (with `focused_target` locating the input-focused node without the `isEditable` requirement) instead of failing on apps such as Toutiao/抖音 whose search boxes hide the flag. The node identity checks, package binding and the shell/root-only provider boundary are unchanged.
- Percent-encode `%` and `:` inside every Accessibility companion extra value and decode it in the companion. `content call --extra <name>:<type>:<value>` rejects a value containing a colon and prints its usage text **with exit code 0**, and `resource_id` always contains one (`package:id/name`), so every identity-bound companion action (`tap_node`, `tap_text`, `input_text`) failed as an "invalid reply" on vendor Android 12+. Report rejected call arguments as such instead of as an invalid reply.
- Resolve the launcher activity and start that component explicitly in `android.launch_app`: the package-scoped `am start -p <package>` filter returns exit code 1 on vendor Android 12+ builds (reproduced on Android 16) even when the app declares a MAIN/LAUNCHER activity; the resolved component is validated to stay shell-safe before it is used.
- Bind semantic Android node clicks and Unicode focused input to the target app package across confirmation and the companion's final action; reject missing or changed package identity.
- Launch Android packages through a package-scoped MAIN/LAUNCHER intent instead of Monkey's random event stream; treat `am start` errors printed with a zero exit status as failures.
- Reclaim stale one-time device approval requests after an abrupt bridge exit, and serialize pending-limit checks with request publication across concurrent bridge processes.
- Keep dense Accessibility UI trees within the Binder reply limit by returning a marked partial snapshot, and capture the companion's machine replies through quiet pipes even when ordinary shell commands use PTY.
- Bind Accessibility semantic clicks to hashes of complete node text and descriptions, allowing exact `android.tap_text` lookup and bounds selection for long text while rejecting a changed value that shares the same displayed prefix.
- Bound adb stdout/stderr and MCP A2A HTTP replies while streaming direct bridge results; terminate and reap the device subprocess as soon as either adb stream exceeds its limit.
- Decode XML character references in uiautomator fallback UI nodes so semantic text matching sees the displayed text; report UI snapshot failures from `android.wait_text` immediately instead of treating them as a missing node.
- Report a failed Android UI hierarchy read as a failed tool call instead of a successful empty screen result.
- Recheck semantic tap node identity after approval and inside the Accessibility companion, so a replacement control at the same bounds is rejected. Mark truncated UI trees as partial and refuse to infer a unique target from them.
- Bind companion Unicode input to the editable control focused at confirmation time and reject a changed target before writing text.
- Derive default Android scroll coordinates from the display and reject Unicode shell text input explicitly; preserve literal `%s` instead of letting Android decode it as a space.
- Give every Web approval or question request a distinct presentation identity so immediately consecutive prompts cannot inherit the previous dialog's submitted/disabled state.
- Keep quoted diagnostic patterns such as `grep -E "pid:|>>> |Cmdline|signal "` read-only by binding code-execution options to known interpreters; real redirections and reparsed writes still require confirmation.
- Update Web turn, model-step and tool-call counts during execution and restore task statistics after failures, cancellation and restart.
- Display provider-supplied reasoning separately, preserve intermediate tool-step commentary, and prevent protocol replay after reasoning is emitted.
- Keep per-task total, model, tool and user-wait timing in Web history, saved sessions and exports.
- Record Web conversation and tool events in the shared audit log with session IDs, credential redaction and shared file limits.
- Prefer bounded, targeted Android log queries and narrow timed-out scans before retrying.

## [1.0.4] - 2026-09-28

### Added

- Add bounded APK archive inspection, entry listing, and DEX class indexing tools. Single-class JADX decompilation requires strong confirmation and an Android DEX helper run with `app_process`; a local helper build project and hash-pinned on-demand download path are included.
- Add a Windows CMD bootstrap that accepts the same long options as the Linux installer, downloads the PowerShell installation core when needed, and preserves the interactive console for ADB and the TUI.
- Add explicit Gitee bootstrap commands for Linux, PowerShell, and CMD. Installers resolve the selected mirror's latest release and retain the same ZIP checksum verification without silently changing sources after a failure.

### Changed

- Make the Android JADX helper compatible with API 28 ARMv7 by pinning the Android-compatible JADX 1.5.1 line, restricting work to the requested class, and supplying a private writable ART temporary directory. The helper continues to require strong confirmation and now rejects unexpected XML parsing.
- Document the normal post-install workflow for release archives and one-command installers, including repeat launches, device selection, configuration persistence, intentional redeployment, and safe TUI exit.

### Fixed

- Keep the documented `curl | bash` Android bootstrap interactive by reconnecting the launcher input to the host controlling terminal after the installer has consumed the pipe, allowing ADB to allocate the TUI PTY.
- Resolve the selected PowerShell installer URL before entering the CMD parenthesized block, preventing `%PS_INSTALLER_URL%` from expanding to an empty curl argument in the downloaded batch bootstrap.
- Preserve the CMD bootstrap directory before argument `shift` operations and perform temporary PowerShell-installer path assignment outside parenthesized blocks, preventing endpoint text from corrupting the download destination or stale early-expanded paths from being used.
- Make the Windows bootstrap idempotent for complete existing installations: reuse the launcher, merge explicitly supplied provider fields into `config.toml` without discarding unrelated settings, preserve configuration on an option-free rerun, and continue to reject incomplete directories.
- Apply the same idempotent reuse and field-preserving configuration merge to the Linux bootstrap, including controlling-terminal restoration for repeated `curl | bash` launches.

## [1.0.3] - 2026-09-27

### Changed

- Compare actual host/device SHA-256 in Linux/Windows launchers, skip identical pushes, and verify changed deployments.
- Constrain `agent_memory` actions to a generated JSON Schema enum and reject unknown actions before risk routing. Failed Web tasks can now explicitly resubmit their original user input without replaying checkpoint tool output, approvals, or irrecoverably redacted text.
- Recover malformed Tool Calling JSON by rejecting it without execution, returning a diagnostic Tool Result to the model, and allowing at most two regeneration attempts before failing the task. Rejected calls still consume task budgets and repaired arguments traverse the full safety and confirmation chain.
- Highlight fenced code in Web model replies and TUI Markdown using the existing semantic palette; unknown languages remain plain text, with HTML escaped in Web.
- Generate short TUI session titles after the first completed Agent turn, and show Web session creation time as relative time for the first day or a local date and time thereafter.
- Web reconnects discard stale running state and recover saved sessions. In-progress requests now keep bounded, redacted diagnostic checkpoints; completed replies are saved before automatic titles are generated in the background.
- Web Quick Start now verifies an actual model reply after saving provider settings. The tool catalog groups capabilities by Chinese purpose and confirmation requirement, and completed turns summarize complete, partial, and failed tool results.
- Ask the Web storage example to display total, used, and remaining space in a chart while keeping the task read-only.
- Remove temporary `[OUT]` and `[ERR]` lines from Web conversations once the matching tool result is available, so completed output appears only in its collapsible card.
- Render Web safety terminal command results as separate stdout, stderr, and exit-status lines, with wrapped long text on narrow screens.
- Improve Web layouts across desktop, tablet, and narrow mobile widths: wrap the tablet header, move sessions into a horizontal strip on mobile, expand conversation and configuration space, and keep quick-start and approval actions visible on short screens.
- Put DeepSeek first in Web Quick Start with `deepseek-flash` prefilled and include all built-in provider choices plus Custom. OpenAI uses its official endpoint; Custom keeps the editable Base URL.
- Make the Web default view focus on conversation and task progress, with tools, terminal, quick runtime controls, and detailed metrics under Advanced. Web approval now presents the local risk assessment and complete operation before approval, while edited operations are reclassified.
- Ask the Agent to report completed work, evidence, failed or unverified steps, and device changes supported by tool results.
- Classify quoted output and read-only mount listings without false mutation prompts while preserving confirmation for real redirection and mount changes.
- Summarize Android connectivity evidence instead of returning the full connectivity dump; distinguish ICMP reachability from HTTPS access and report device ABI separately from process architecture.
- Highlight the browser URL in a dedicated TUI startup entry at the end of the welcome content, with narrow-screen wrapping.

### Added

- Add a standalone English README with reciprocal language links and current TUI/Web storage-analysis screenshots.
- Add Linux/Windows bootstrap scripts that verify the latest ZIP, generate optional provider settings, deploy, and launch with private 0600 configuration.
- Add a stdio MCP adapter that discovers the A2A gateway, authenticates requests, and exposes device inspection, tool listing, consultation, and task lookup to coding agents.
- Add an optional host-side A2A 1.0 gateway with Agent Card discovery, authenticated JSON-RPC tasks, persistent context, and a narrow Android bridge. Add explicit build/deploy checkpoints and reject unattended device modifications through the existing confirmation boundary.
- Added a Web task stop control, a model-free read-only device overview, example prompts in the tool catalog, and keyboard focus management for modal dialogs.
- Add a bounded read-only chart tool for statistical results. Web conversations render bar, line, and pie charts with a data table and restore them from saved sessions; TUI displays the values as text.
- Add a Web quick-start flow for provider setup and model-list connection checking, read-only first-task examples, automatic session creation on first send, actionable error hints, and a beginner guide. The Web server continues to listen on every IPv4 interface without login.
- Added the project logo to the Web sidebar and a GitHub Star link to the top bar.
- Added a bounded read-only Android environment tool for OS, device ABI, command availability, memory, and data storage.
- Added single-session and delete-all controls to the Web session sidebar, with confirmation for delete-all and active-session protection.
- Added grouped key/value editing and raw TOML editing to the Web configuration page, with live configuration validation and automatic use of saved settings by subsequent Web tasks.

## [1.0.2] - 2026-09-24

### Changed

- Stream each Web tool call into the conversation as it starts and fill in its bounded result by call ID as soon as it completes, while keeping the final transcript as the single persistence source without duplicating cards.
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
- Add UTF-8-safe Left/Right/Home/End editing, insertion, Backspace and Delete with masked passwords.
- Restore background model discovery in unified settings, filling model metadata while preserving manual input on failure.
- Filter SGR mouse reports with lost CSI prefixes from input and settings fields.
- Reserve all slash input for local commands, reject unknown commands locally, and keep /update out of Agent submission.
- Keep /config out of model requests; settings takes focus with a dedicated input boundary and cursor.
- Remove /provider, /model, /models and /proxy in favor of /config or /setting.
- Add update, /update and background checks with ABI selection, SHA-256 verification, atomic replacement and defer/skip options.
- Unify provider, model, Agent, safety, interface and proxy settings in tabs, with historical step/turn recommendations of 24/16.

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
