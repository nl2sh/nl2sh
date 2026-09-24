//! Reusable nl2sh core. UI and CLI are deliberately thin adapters.
/// Agent loop, conversation policy, confirmation, and built-in tools.
pub mod agent;
/// Private persistent key/value notes for Agent continuity.
pub mod agent_memory;
/// Bounded, read-only Android framework diagnostics used by structured tools.
pub mod android_diagnostics;
/// Structured Android automation, device, media, and clipboard tools.
pub mod android_tools;
/// Audio-quality judgment using Jev or the configured general LLM.
pub mod audio_quality;
/// Deterministic WAV/raw-PCM parsing and DSP feature extraction.
pub mod audio_tools;
/// Validated TOML configuration and initialization wizard.
pub mod config;
pub mod file_references;
/// Bounded structured file operations for the Agent.
pub mod file_tools;
/// Persistent structured interaction history for diagnostics.
pub mod history;
/// Optional read-only Tencent ima knowledge-base client.
pub mod ima;
/// Shared bounded-text utilities for execution, UI, logs, and model context.
pub mod limits;
/// Provider-neutral LLM types and OpenAI-compatible HTTP clients.
pub mod llm;
pub mod network;
/// Read-only provider account data for documented balance endpoints.
pub mod provider_account;
/// Provider-specific model discovery and normalized metadata.
pub mod provider_metadata;
/// Direct Android shell versus Termux compatibility runtime detection.
pub mod runtime;
/// Local shell command classification and confirmation requirements.
pub mod security;
/// Private, bounded conversation snapshots used by `/sessions`.
pub mod sessions;
/// PTY/pipeline execution, process cleanup, and Android root selection.
pub mod shell;
/// Direct, read-only TLS chain and certificate diagnostics.
pub mod tls_tools;
/// Model-facing tool adapters and their explicit registry.
pub mod tools;
/// Ratatui/crossterm terminal input interface.
pub mod tui;
/// Structured Android UI hierarchy inspection and confirmed screenshots.
pub mod ui_tools;
/// Signed-by-checksum GitHub Release discovery and self-update support.
pub mod update;
/// Bounded HTTP retrieval and confirmed atomic downloads for Agent tools.
pub mod web_tools;
/// Embedded browser interface for configuration and Agent sessions.
pub mod web_ui;
