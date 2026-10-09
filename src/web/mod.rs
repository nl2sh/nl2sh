//! Embedded Web API, shared session runtime, and browser transports.

use crate::{
    agent::{AgentRunner, ConfirmationDecision, Confirmer, QuestionAnswers, UserQuestion},
    config::{self, Config, ConfirmPolicy, ExecuteUserMode},
    file_references::{augment_file_references, file_suggestions},
    history::HistoryLog,
    llm::{
        build_client, ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role,
        TextDeltaSink, ToolRound,
    },
    provider_metadata::build_metadata_client,
    security::{PrivilegeBroker, SecurityAssessment},
    session_title::generate_title,
    sessions::{SessionStore, WebCheckpoint, WebCheckpointEvent, WebPresentation, WebTaskMetrics},
    shell::{
        CommandExecutor, ExecutionBroker, ExecutionResult, OutputSink, ShellExecutor,
        SystemRootProbe,
    },
    tools::{
        android::{
            diagnostics::{list_android_apps, ListAndroidAppsArgs},
            environment::inspect_environment,
        },
        memory::domain::{AgentMemory, AgentMemoryAction, AgentMemoryArgs, MemoryEntry},
        Capability, ToolRegistry,
    },
};

use anyhow::{anyhow, bail, Context, Result};

use async_trait::async_trait;

use axum::{
    body::Body,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path as AxumPath, Query, State,
    },
    http::{header, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};

use rust_embed::RustEmbed;

use serde::{Deserialize, Serialize};

use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Instant, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncReadExt, AsyncSeekExt},
    net::TcpListener,
    sync::{broadcast, oneshot, watch, Mutex as AsyncMutex},
};

use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use tokio_util::io::ReaderStream;

use tower_http::set_header::SetResponseHeaderLayer;

const PORT: u16 = 9999;

const MAX_REQUEST: usize = 256 * 1024;

const MAX_HISTORY: usize = 400;

const MAX_WEB_SESSIONS: usize = 64;

const MAX_EXPORT_LOG_BYTES: u64 = 20 * 1024 * 1024;

const MAX_FILE_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

const MAX_IMAGE_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;

const MAX_VIDEO_PREVIEW_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const MAX_AUDIO_PREVIEW_BYTES: u64 = 256 * 1024 * 1024;

const MAX_RAW_PCM_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;

const MAX_DIRECTORY_ENTRIES: usize = 1000;

const LIVE_TOOL_CALL_PREFIX: &str = "\u{1e}TOOL_CALL:";

const LIVE_TOOL_PENDING_PREFIX: &str = "\u{1e}TOOL_PENDING:";

static WELCOME_URL: OnceLock<String> = OnceLock::new();

static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

static PENDING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

static MODEL_LIST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

mod agent;
mod auth;
mod error;
mod interaction;
mod routes;
mod server;
mod state;
mod websocket;

use agent::*;
use auth::*;
use error::*;
use interaction::*;
use routes::*;
#[cfg(test)]
use server::{bind_web_listener, start_with_listener};
pub use server::{set_welcome_url, start, start_on_port, welcome_url, WebServer};
use state::*;
use websocket::*;
#[cfg(test)]
mod tests;
