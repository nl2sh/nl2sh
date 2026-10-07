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
