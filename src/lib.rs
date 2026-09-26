//! Reusable nl2sh core. UI and CLI are deliberately thin adapters.
/// Agent loop, conversation policy, confirmation, and built-in tools.
pub mod agent;
/// Bounded machine-readable adapter for an external A2A gateway.
pub mod bridge;
/// Validated TOML configuration and initialization wizard.
pub mod config;
pub mod file_references;
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
/// Model-facing tool adapters and their explicit registry.
pub mod tools;
/// Ratatui/crossterm terminal input interface.
pub mod tui;
/// Signed-by-checksum GitHub Release discovery and self-update support.
pub mod update;
/// Embedded browser interface for configuration and Agent sessions.
pub mod web_ui;
