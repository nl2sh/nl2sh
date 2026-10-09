//! Native service lifecycle, independent of model configuration and fixed Web ports.
use serde_json::Value;
use std::{path::PathBuf, process::Command, time::Duration};

struct Fixture {
    config: PathBuf,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            config: dir.path().join("config.toml"),
            _dir: dir,
        }
    }
    async fn call(&self, args: &[&str]) -> std::process::Output {
        tokio::time::timeout(
            Duration::from_secs(25),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"))
                .arg("--config")
                .arg(&self.config)
                .arg("service")
                .args(args)
                .output(),
        )
        .await
        .expect("lifecycle timed out")
        .expect("CLI runs")
    }
    async fn status(&self, args: &[&str]) -> Value {
        let result = self.call(args).await;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = Command::new(env!("CARGO_BIN_EXE_nl2sh"))
            .arg("--config")
            .arg(&self.config)
            .args(["service", "stop", "--json"])
            .output();
    }
}

#[tokio::test]
async fn status_and_web_discover_separate_protocol_listener_without_disclosing_token() {
    let fixture = Fixture::new();
    let token = "connection-discovery-secret-token-0123456789";
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_nl2sh"))
        .arg("--config")
        .arg(&fixture.config)
        .args(["protocol", "serve", "--port", "0"])
        .env("NL2SH_PROTOCOL_TOKEN", token)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("protocol starts");
    let status = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let status = fixture.status(&["status", "--json"]).await;
            if status["connections"]["state"] == "running" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("protocol announces itself");
    assert_eq!(status["state"], "stopped", "Web status is independent");
    let connections = &status["connections"];
    let mcp = connections["mcp_url"].as_str().expect("MCP URL");
    let address = reqwest::Url::parse(mcp).expect("valid advertised URL");
    let ip = address
        .host_str()
        .expect("device IPv4")
        .parse::<std::net::Ipv4Addr>()
        .expect("IPv4 origin");
    assert!(!ip.is_unspecified());
    assert!(!mcp.contains(":8765/"), "actual ephemeral port is shown");
    assert_eq!(connections["transport"], "http");
    assert!(!status.to_string().contains(token));
    let summary = fixture.call(&["status"]).await;
    let summary = String::from_utf8(summary.stdout).expect("status text");
    assert!(summary.contains(mcp));
    assert!(summary.contains("Authorization: Bearer <token>"));
    assert!(!summary.contains(token));
    let web = fixture.status(&["start", "--port", "0", "--json"]).await;
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client");
    let base = format!("http://127.0.0.1:{}", web["port"]);
    let response = client
        .get(format!("{base}/api/connections"))
        .send()
        .await
        .expect("connection API");
    assert!(response.status().is_success());
    let api: Value = response.json().await.expect("connection JSON");
    assert_eq!(&api, connections);
    assert!(!api.to_string().contains(token));
    let info: Value = client
        .get(format!("{base}/api/info"))
        .send()
        .await
        .expect("info API")
        .json()
        .await
        .expect("info JSON");
    assert_eq!(&info["connections"], connections);
    child.kill().await.expect("stop protocol process");
    let stopped: Value = client
        .get(format!("{base}/api/connections"))
        .send()
        .await
        .expect("connection API")
        .json()
        .await
        .expect("connection JSON");
    assert_eq!(stopped["state"], "stopped");
    assert!(stopped["mcp_url"].is_null());
}

#[tokio::test]
async fn native_service_is_idempotent_uses_actual_port_and_private_authorized_shutdown() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.status(&["status", "--json"]).await["state"],
        "stopped"
    );
    // An unrelated process owns the preferred port; service must select its own port.
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let preferred = occupied.local_addr().unwrap().port().to_string();
    let start_args = ["start", "--port", &preferred, "--json"];
    let (first, concurrent) =
        tokio::join!(fixture.status(&start_args), fixture.status(&start_args));
    assert_eq!(first["pid"], concurrent["pid"]);
    assert_eq!(first["state"], "ready");
    assert_ne!(first["port"].as_u64().unwrap().to_string(), preferred);
    assert!(first.get("token").is_none());
    let same = fixture.status(&["start", "--json"]).await;
    assert_eq!(first["pid"], same["pid"]);
    let info: Value = reqwest::get(format!("http://127.0.0.1:{}/api/info", first["port"]))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["pid"], first["pid"]);
    assert_eq!(info["version"], first["version"]);
    // A caller without the private token cannot stop the process.
    use tokio::io::AsyncWriteExt;
    let state: Value = serde_json::from_slice(
        &std::fs::read(fixture.config.with_extension("service").join("state.json")).unwrap(),
    )
    .unwrap();
    let mut socket = tokio::net::TcpStream::connect((
        std::net::Ipv4Addr::LOCALHOST,
        state["control_port"].as_u64().unwrap() as u16,
    ))
    .await
    .unwrap();
    socket
        .write_all(format!("{}\n", "0".repeat(64)).as_bytes())
        .await
        .unwrap();
    drop(socket);
    assert_eq!(
        fixture.status(&["status", "--json"]).await["state"],
        "ready"
    );
    let second = fixture.status(&["restart", "--port", "0", "--json"]).await;
    assert_eq!(second["state"], "ready");
    assert_ne!(second["pid"], first["pid"]);
    assert_eq!(
        fixture.status(&["stop", "--json"]).await["state"],
        "stopped"
    );
    assert_eq!(
        fixture.status(&["stop", "--json"]).await["state"],
        "stopped"
    );
    // Strict mode must report failure and leave the unrelated listener untouched.
    let failed = fixture
        .call(&["start", "--port", &preferred, "--port-strict", "--json"])
        .await;
    assert!(!failed.status.success());
    assert_eq!(
        fixture.status(&["status", "--json"]).await["state"],
        "stopped"
    );
    assert!(occupied.local_addr().is_ok());
}

#[tokio::test]
async fn managed_protocol_follows_config_start_restart_stop_and_startup_failure() {
    let fixture = Fixture::new();
    let occupied = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
        .await
        .unwrap();
    let protocol_port = occupied.local_addr().unwrap().port();
    std::fs::write(
        &fixture.config,
        format!("protocol_start_with_service=true\nprotocol_service_port={protocol_port}\n"),
    )
    .unwrap();
    assert!(!fixture
        .call(&["start", "--port", "0", "--json"])
        .await
        .status
        .success());
    assert_eq!(
        fixture.status(&["status", "--json"]).await["state"],
        "stopped"
    );
    drop(occupied);
    let first = fixture.status(&["start", "--port", "0", "--json"]).await;
    assert_eq!(first["state"], "ready");
    assert_eq!(first["connections"]["state"], "running");
    assert_eq!(
        fixture.status(&["start", "--json"]).await["pid"],
        first["pid"]
    );
    let log_path = fixture.config.with_extension("service").join("service.log");
    let token = || {
        let log = std::fs::read_to_string(&log_path).unwrap();
        log.lines()
            .filter_map(|line| {
                line.strip_prefix("Token (generated for this run; changes after restart): ")
            })
            .last()
            .unwrap()
            .to_owned()
    };
    let first_token = token();
    assert!(!first.to_string().contains(&first_token));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mcp_url = format!("http://127.0.0.1:{protocol_port}/mcp");
    assert_eq!(client.post(&mcp_url).send().await.unwrap().status(), 401);
    let initialize = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"managed-test","version":"1"}}});
    assert!(client
        .post(&mcp_url)
        .bearer_auth(&first_token)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let second = fixture.status(&["restart", "--port", "0", "--json"]).await;
    assert_eq!(second["connections"]["state"], "running");
    assert_ne!(first["pid"], second["pid"]);
    let second_token = token();
    assert_ne!(first_token, second_token);
    assert_eq!(
        client
            .post(&mcp_url)
            .bearer_auth(&first_token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(client
        .post(&mcp_url)
        .bearer_auth(&second_token)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let stopped = fixture.status(&["stop", "--json"]).await;
    assert_eq!(stopped["connections"]["state"], "stopped");
    assert!(client
        .get(format!(
            "http://127.0.0.1:{protocol_port}/.well-known/agent-card.json"
        ))
        .send()
        .await
        .is_err());
    // Failure after protocol initialization must release its listener and announcement.
    let web_port = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
        .await
        .unwrap();
    let port = web_port.local_addr().unwrap().port().to_string();
    assert!(!fixture
        .call(&["start", "--port", &port, "--port-strict", "--json"])
        .await
        .status
        .success());
    assert_eq!(
        fixture.status(&["status", "--json"]).await["connections"]["state"],
        "stopped"
    );
    assert!(
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, protocol_port))
            .await
            .is_ok()
    );
    std::fs::write(&fixture.config, "protocol_start_with_service=false\n").unwrap();
    let disabled = fixture.status(&["start", "--port", "0", "--json"]).await;
    assert_eq!(disabled["state"], "ready");
    assert_eq!(disabled["connections"]["state"], "stopped");
}
