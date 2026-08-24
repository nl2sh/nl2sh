//! Reusable nl2sh core. UI and CLI are deliberately thin adapters.
/// Agent loop, conversation policy, confirmation, and built-in tools.
pub mod agent;
/// Validated TOML configuration and initialization wizard.
pub mod config;
/// Persistent structured interaction history for diagnostics.
pub mod history;
/// Shared bounded-text utilities for execution, UI, logs, and model context.
pub mod limits;
/// Provider-neutral LLM types and OpenAI-compatible HTTP clients.
pub mod llm;
pub mod network;
/// Read-only provider account data for documented balance endpoints.
pub mod provider_account;
/// Provider-specific model discovery and normalized metadata.
pub mod provider_metadata;
/// Local shell command classification and confirmation requirements.
pub mod security;
/// PTY/pipeline execution, process cleanup, and Android root selection.
pub mod shell;
/// Ratatui/crossterm terminal input interface.
pub mod tui;
/// Signed-by-checksum GitHub Release discovery and self-update support.
pub mod update;
