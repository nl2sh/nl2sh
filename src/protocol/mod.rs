//! Device-native MCP and A2A servers. Network credentials never authorize local approvals.
mod a2a;
mod approval;
pub(crate) mod connections;
mod execution;
mod mcp;
mod store;
mod tasks;
pub use connections::{connection_info, set_welcome_connections, ConnectionInfo};
#[cfg(test)]
mod tests;

use anyhow::{bail, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rmcp::{
    transport::{
        stdio as io_transport,
        streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
    },
    ServiceExt,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
use tasks::Tasks;
use tokio_util::sync::CancellationToken;

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Clone)]
struct Access {
    token_hash: [u8; 32],
    authorities: Vec<String>,
    origin: String,
}
fn protocol_token(value: Result<String, std::env::VarError>) -> Result<(String, bool)> {
    match value {
        Ok(token) => Ok((token, false)),
        Err(std::env::VarError::NotPresent) => {
            let mut random = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .context("cannot open the system random source")?
                .read_exact(&mut random)
                .context("cannot generate protocol token")?;
            Ok((
                random.iter().map(|byte| format!("{byte:02x}")).collect(),
                true,
            ))
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            bail!("NL2SH_PROTOCOL_TOKEN must be printable ASCII")
        }
    }
}
fn startup_info(
    access: &Access,
    host: IpAddr,
    port: u16,
    token: &str,
    generated: bool,
) -> Result<String> {
    let token_line = if generated {
        format!("Token (generated for this run; changes after restart): {token}")
    } else {
        "Token: using NL2SH_PROTOCOL_TOKEN (configured value hidden)".into()
    };
    Ok(format!(
        "nl2sh device MCP / A2A\nListen: http://{}\nMCP (Streamable HTTP): {}/mcp\nA2A (JSON-RPC 1.0): {}/a2a\nAgent Card (public): {}/.well-known/agent-card.json\nAuthentication: Authorization: Bearer <token>\n{token_line}\n\nMCP client configuration (set NL2SH_PROTOCOL_TOKEN on the client to the token above):\n[mcp_servers.nl2sh]\nurl = {}\nbearer_token_env_var = \"NL2SH_PROTOCOL_TOKEN\"\ntool_timeout_sec = 210\n\nA2A requests: POST application/json, A2A-Version: 1.0, Authorization: Bearer <token>.\nDevice mutations require local protocol approvals/approve unless explicitly auto-approved.\nHTTP transmits the token in plaintext; protect this startup output and use a trusted network or HTTPS.\n",
        SocketAddr::new(host, port), access.origin, access.origin, access.origin,
        serde_json::to_string(&format!("{}/mcp", access.origin))?
    ))
}
fn default_origin(host: IpAddr, port: u16, detected: Option<Ipv4Addr>) -> String {
    let host = match host {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(
            detected
                .filter(|ip| {
                    !ip.is_unspecified()
                        && !ip.is_loopback()
                        && !ip.is_link_local()
                        && !ip.is_multicast()
                        && !ip.is_broadcast()
                })
                .unwrap_or(Ipv4Addr::LOCALHOST),
        ),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        other => other,
    };
    format!("http://{}", SocketAddr::new(host, port))
}
fn settings(
    host: IpAddr,
    port: u16,
    advertised: Option<&str>,
    insecure: bool,
    token: &str,
) -> Result<Access> {
    if token.len() < 32 || token.len() > 256 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        bail!("NL2SH_PROTOCOL_TOKEN must contain 32–256 printable ASCII characters");
    }
    if !host.is_loopback() && !insecure {
        bail!("non-loopback HTTP binding requires --allow-insecure-http; use a trusted network or HTTPS reverse proxy");
    }
    let origin = advertised.map(str::to_owned).unwrap_or_else(|| {
        let detected = if host.is_unspecified() && host.is_ipv4() {
            crate::network::local_ipv4()
        } else {
            None
        };
        default_origin(host, port, detected)
    });
    let url = url::Url::parse(&origin).context("invalid advertised origin")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || url.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_unspecified())
        })
    {
        bail!(
            "advertised URL must be an HTTP(S) origin without path, credentials, query or fragment"
        );
    }
    if url.scheme() == "http"
        && !insecure
        && !url.host_str().is_some_and(|host| {
            host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        })
    {
        bail!("remote plaintext advertised URL requires --allow-insecure-http");
    }
    let origin = url.origin().ascii_serialization();
    let authority = url[url::Position::BeforeHost..url::Position::AfterPort].to_owned();
    Ok(Access {
        token_hash: Sha256::digest(token.as_bytes()).into(),
        authorities: vec![
            authority,
            SocketAddr::new(host, port).to_string(),
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ],
        origin,
    })
}
async fn guard(State(access): State<Access>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok());
    if !host.is_some_and(|host| {
        access
            .authorities
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
    }) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request
        .headers()
        .get("origin")
        .is_some_and(|origin| origin.to_str().ok() != Some(access.origin.as_str()))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.uri().path() != "/.well-known/agent-card.json" {
        let tokens: Vec<_> = request.headers().get_all("authorization").iter().collect();
        let token = if tokens.len() == 1 {
            tokens[0]
                .to_str()
                .ok()
                .and_then(|value| value.strip_prefix("Bearer "))
        } else {
            None
        };
        let valid = token
            .filter(|token| token.len() <= 256)
            .is_some_and(|token| {
                let actual = Sha256::digest(token.as_bytes());
                actual
                    .iter()
                    .zip(access.token_hash)
                    .fold(0u8, |difference, (actual, expected)| {
                        difference | (actual ^ expected)
                    })
                    == 0
            });
        if !valid {
            return (
                StatusCode::UNAUTHORIZED,
                [("www-authenticate", "Bearer realm=\"nl2sh\"")],
            )
                .into_response();
        }
    }
    next.run(request).await
}
fn router(tasks: Arc<Tasks>, access: Access, cancellation: CancellationToken) -> Router {
    let mcp_tasks = tasks.clone();
    let mut config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(access.authorities.clone())
        .with_allowed_origins([access.origin.clone()])
        .enforce_origin_validation();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.max_request_body_bytes = 32 * 1024;
    config.cancellation_token = cancellation;
    let service = StreamableHttpService::new(
        move || {
            Ok(mcp::Mcp {
                tasks: mcp_tasks.clone(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let card = a2a::card(&access.origin);
    Router::new()
        .route(
            "/.well-known/agent-card.json",
            get(move || {
                let card = card.clone();
                async move { ([("cache-control", "public, max-age=60")], Json(card)) }
            }),
        )
        .route("/a2a", post(a2a::rpc))
        .route_service("/mcp", service)
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(tasks)
        .layer(middleware::from_fn_with_state(access, guard))
}
/// Serve Bearer-authenticated HTTP MCP and A2A on the device, with a separate listener from Web.
pub async fn serve(
    path: PathBuf,
    host: IpAddr,
    port: u16,
    advertised: Option<&str>,
    insecure: bool,
) -> Result<()> {
    let (token, generated) = protocol_token(std::env::var("NL2SH_PROTOCOL_TOKEN"))?;
    // Validate before opening the listener or modifying persistent state.
    settings(host, port, advertised, insecure, &token)?;
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(host, port))
        .await
        .context("cannot bind protocol listener")?;
    let port = listener.local_addr()?.port();
    let access = settings(host, port, advertised, insecure, &token)?;
    let tasks = Tasks::open_with_token(path, Some(token.clone()))?;
    let _connection = connections::Registration::publish(&tasks.path, Some(&access.origin))?;
    let cancellation = CancellationToken::new();
    let app = router(tasks.clone(), access.clone(), cancellation.clone());
    {
        let text = startup_info(&access, host, port, &token, generated)?;
        let mut output = std::io::stdout().lock();
        output
            .write_all(text.as_bytes())
            .context("cannot print protocol connection information")?;
        output
            .flush()
            .context("cannot flush protocol connection information")?;
    }
    let drain = tasks.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            drain.shutdown().await;
            cancellation.cancel();
        })
        .await;
    tasks.shutdown().await;
    result.context("protocol HTTP server failed")
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{}, _=term.recv()=>{}};
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
/// Serve MCP on stdin/stdout in the same process as device execution; stdout is protocol-only.
pub async fn stdio(path: PathBuf) -> Result<()> {
    let tasks = Tasks::open(path)?;
    let _connection = connections::Registration::publish(&tasks.path, None)?;
    let service = mcp::Mcp {
        tasks: tasks.clone(),
    }
    .serve(io_transport())
    .await
    .context("cannot initialize MCP stdio")?;
    let cancellation = service.cancellation_token();
    let wait = service.waiting();
    tokio::pin!(wait);
    let result = tokio::select! {
        result=&mut wait=>result,
        _=shutdown_signal()=>{
            tasks.shutdown().await;
            cancellation.cancel();
            wait.await
        }
    };
    tasks.shutdown().await;
    result.context("MCP stdio service failed")?;
    Ok(())
}
/// List pending device-local approvals without exposing a remote approval tool.
pub fn list_approvals(path: &Path) -> Result<()> {
    approval::list_pending(path)
}
/// Ask the local terminal user for a one-time decision, including strong confirmation when required.
pub async fn approve(path: &Path, id: &str) -> Result<()> {
    let path = path.to_path_buf();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || approval::approve_interactive(&path, &id))
        .await
        .context("local approval task failed")?
}
