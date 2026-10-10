use super::*;
use super::{
    store::Store,
    tasks::{result, Operation},
};
use crate::config::{ApiType, Config};
use serde_json::{json, Value};
use std::time::Duration;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

fn configuration(root: &Path, extra: &str) -> Result<PathBuf> {
    let path = root.join("config.toml");
    std::fs::write(&path, extra)?;
    Ok(path)
}

#[tokio::test]
async fn protocol_listener_prefers_requested_port_and_falls_back_only_when_occupied() -> Result<()>
{
    let host = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let occupied = bind_http_listener(host, 0).await?;
    let preferred = occupied.local_addr()?.port();
    let fallback = bind_http_listener(host, preferred).await?;
    assert_ne!(fallback.local_addr()?.port(), preferred);
    assert_eq!(fallback.local_addr()?.ip(), host);
    assert!(occupied.local_addr().is_ok());
    drop(occupied);
    let available = bind_http_listener(host, preferred).await?;
    assert_eq!(available.local_addr()?.port(), preferred);
    let unavailable = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
    assert!(bind_http_listener(unavailable, 0).await.is_err());
    Ok(())
}
async fn server(tasks: Arc<Tasks>) -> Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let app = router(
        tasks,
        settings("127.0.0.1".parse()?, port, None, false, &"t".repeat(32))?,
        CancellationToken::new(),
    );
    let job = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://127.0.0.1:{port}"), job))
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()?)
}
async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Result<Value> {
    Ok(client
        .post(format!("{url}/a2a"))
        .bearer_auth("t".repeat(32))
        .header("a2a-version", "1.0")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?
        .json()
        .await?)
}
#[test]
fn listener_settings_validate_overrides_and_restricted_http() -> Result<()> {
    let token = "t".repeat(32);
    assert!(settings(
        "0.0.0.0".parse()?,
        8765,
        Some("http://device:8765"),
        false,
        &token
    )
    .is_err());
    let automatic = settings("0.0.0.0".parse()?, 8765, None, true, &token)?;
    assert!(!automatic.origin.contains("0.0.0.0"));
    assert!(settings(
        "127.0.0.1".parse()?,
        8765,
        Some("http://device:8765"),
        false,
        &token
    )
    .is_err());
    for origin in [
        "https://user:secret@device",
        "https://device/path",
        "https://device?key=secret",
        "https://device#fragment",
        "http://0.0.0.0:8765",
        "http://[::]:8765",
    ] {
        assert!(settings("127.0.0.1".parse()?, 8765, Some(origin), false, &token).is_err());
    }
    assert!(settings("127.0.0.1".parse()?, 8765, None, false, "short").is_err());
    assert!(settings(
        "0.0.0.0".parse()?,
        8765,
        Some("https://device"),
        true,
        &token
    )
    .is_ok());
    Ok(())
}
#[test]
fn auto_origin_uses_device_ip_actual_port_or_loopback_fallback() -> Result<()> {
    let wildcard = "0.0.0.0".parse()?;
    let ip = "192.168.1.23".parse()?;
    assert_eq!(
        default_origin(wildcard, 9123, Some(ip)),
        "http://192.168.1.23:9123"
    );
    assert_eq!(
        default_origin(wildcard, 8765, None),
        "http://127.0.0.1:8765"
    );
    assert_eq!(
        default_origin(wildcard, 8765, Some(Ipv4Addr::UNSPECIFIED)),
        "http://127.0.0.1:8765"
    );
    assert_eq!(
        default_origin("127.0.0.1".parse()?, 8765, Some(ip)),
        "http://127.0.0.1:8765"
    );
    assert_eq!(
        default_origin("::".parse()?, 8765, Some(ip)),
        "http://[::1]:8765"
    );
    Ok(())
}
#[test]
fn generated_tokens_are_unique_and_only_generated_values_appear_in_startup() -> Result<()> {
    let (first, generated) = protocol_token(Err(std::env::VarError::NotPresent), "")?;
    let (second, _) = protocol_token(Err(std::env::VarError::NotPresent), "")?;
    assert_eq!(
        protocol_token(Err(std::env::VarError::NotPresent), &first)?,
        (first.clone(), false)
    );
    assert_eq!(
        protocol_token(Ok(second.clone()), &first)?,
        (second.clone(), false)
    );
    assert!(generated);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(first, second);
    let host = "127.0.0.1".parse()?;
    let access = settings(host, 8765, Some("https://agent.example"), true, &first)?;
    let generated = startup_info(&access, host, 8765, &first, true)?;
    assert!(generated.contains(&first));
    assert!(generated.contains("https://agent.example/mcp"));
    assert!(generated.contains("/.well-known/agent-card.json"));
    assert!(generated.contains("[mcp_servers.nl2sh]"));
    let configured = startup_info(&access, host, 8765, &first, false)?;
    assert!(!configured.contains(&first));
    assert_eq!(protocol_token(Ok(first.clone()), "")?, (first, false));
    Ok(())
}
#[tokio::test]
async fn http_auth_origin_body_limit_and_a2a_errors() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = configuration(root.path(), "")?;
    let tasks = Tasks::open(path)?;
    let (url, job) = server(tasks.clone()).await?;
    let client = client()?;
    let card: Value = client
        .get(format!("{url}/.well-known/agent-card.json"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(card["supportedInterfaces"][0]["protocolVersion"], "1.0");
    for endpoint in ["/a2a", "/mcp"] {
        assert_eq!(
            client
                .post(format!("{url}{endpoint}"))
                .json(&json!({}))
                .send()
                .await?
                .status(),
            401
        );
    }
    assert_eq!(
        client
            .post(format!("{url}/mcp"))
            .bearer_auth("x".repeat(32))
            .json(&json!({}))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        client
            .post(format!("{url}/mcp"))
            .bearer_auth("t".repeat(32))
            .header("origin", "https://evil.example")
            .json(&json!({}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{url}/.well-known/agent-card.json"))
            .header("host", "evil.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        client
            .post(format!("{url}/a2a"))
            .bearer_auth("t".repeat(32))
            .body("x".repeat(32769))
            .send()
            .await?
            .status(),
        413
    );
    assert_eq!(
        rpc(&client, &url, "GetTask", json!({"id":"missing"})).await?["error"]["code"],
        -32001
    );
    assert_eq!(
        rpc(&client, &url, "SendStreamingMessage", json!({})).await?["error"]["code"],
        -32004
    );
    assert_eq!(
        rpc(&client, &url, "GetTask", json!({"id":""})).await?["error"]["code"],
        -32602
    );
    assert_eq!(
        rpc(&client, &url, "ListTasks", json!({"pageSize":101})).await?["error"]["code"],
        -32602
    );
    let bad = client
        .post(format!("{url}/a2a"))
        .bearer_auth("t".repeat(32))
        .header("a2a-version", "0.3")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"ListTasks"}))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(bad["error"]["code"], -32009);
    tasks.shutdown().await;
    job.abort();
    Ok(())
}
#[tokio::test]
async fn mcp_http_initialization_and_direct_tools_need_no_model() -> Result<()> {
    let root = tempfile::tempdir()?;
    let tasks = Tasks::open(configuration(root.path(), "")?)?;
    let (url, job) = server(tasks.clone()).await?;
    let client = client()?;
    let init=client.post(format!("{url}/mcp")).bearer_auth("t".repeat(32)).header("accept","application/json, text/event-stream").json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).send().await?.error_for_status()?.json::<Value>().await?;
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    let list = client
        .post(format!("{url}/mcp"))
        .bearer_auth("t".repeat(32))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    assert!(list["result"]["tools"]
        .as_array()
        .is_some_and(|items| items.len() == 7));
    let call=client.post(format!("{url}/mcp")).bearer_auth("t".repeat(32)).header("accept","application/json, text/event-stream").header("mcp-protocol-version","2025-11-25").json(&json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nl2sh_invoke","arguments":{"tool":"list_dir","arguments":{"path":root.path().to_string_lossy()}}}})).send().await?.error_for_status()?.json::<Value>().await?;
    assert_eq!(
        call["result"]["structuredContent"]["success"], true,
        "{call}"
    );
    assert_eq!(call["result"]["isError"], false);
    assert!(!root.path().join("sessions").exists());
    tasks.shutdown().await;
    job.abort();
    Ok(())
}
#[tokio::test]
async fn mcp_sdk_duplex_transport_discovers_and_invokes() -> Result<()> {
    use rmcp::{model::CallToolRequestParams, ServiceExt};
    let root = tempfile::tempdir()?;
    let tasks = Tasks::open(configuration(root.path(), "")?)?;
    let (input, output) = tokio::io::duplex(65536);
    let server_tasks = tasks.clone();
    let server = tokio::spawn(async move {
        super::mcp::Mcp {
            tasks: server_tasks,
        }
        .serve(input)
        .await
    });
    let client = ().serve(output).await?;
    let server = server.await??;
    assert_eq!(client.list_all_tools().await?.len(), 7);
    let response=client.call_tool(CallToolRequestParams::new("nl2sh_invoke").with_arguments(json!({"tool":"read_file","arguments":{"path":root.path().join("config.toml").to_string_lossy()}}).as_object().cloned().unwrap_or_default())).await?;
    let raw = serde_json::to_value(response)?;
    assert_eq!(raw["structuredContent"]["success"], true, "{raw}");
    client.cancel().await?;
    server.cancel().await?;
    tasks.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn pending_mutation_cancels_without_writing_and_shutdown_drains() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = configuration(
        root.path(),
        "security_level = 'unsafe'\nexecute_confirm_policy = 'never'\n",
    )?;
    let tasks = Tasks::open(path)?;
    let target = root.path().join("target");
    let ticket=tasks.submit(Operation::Invoke {tool:"apply_patch".into(), arguments:json!({"path":target.to_string_lossy(),"old_text":"","new_text":"unsafe"})},None,None).await?;
    let id = ticket.updates.borrow()["id"]
        .as_str()
        .context("missing task id")?
        .to_owned();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if root.path().join(".nl2sh-a").exists()
                && std::fs::read_dir(root.path().join(".nl2sh-a")).is_ok_and(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.path().join("live").exists())
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(!target.exists());
    let cancelled = tasks
        .cancel(&id)
        .await?
        .context("expected active task")?
        .wait()
        .await?;
    assert_eq!(cancelled["status"]["state"], "TASK_STATE_CANCELED");
    assert!(!target.exists());
    tasks.shutdown().await;
    assert!(tasks.submit(Operation::Tools, None, None).await.is_err());
    Ok(())
}
#[tokio::test]
async fn explicit_protocol_auto_approval_executes_and_persists() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = configuration(root.path(), "protocol_auto_approve=true\n")?;
    let tasks = Tasks::open(path.clone())?;
    let target = root.path().join("target");
    let task=tasks.submit(Operation::Invoke {tool:"apply_patch".into(),arguments:json!({"path":target.to_string_lossy(),"old_text":"","new_text":"approved"})},None,None).await?.wait().await?;
    assert_eq!(result(&task)?["success"], true);
    assert_eq!(std::fs::read_to_string(target)?, "approved");
    tasks.shutdown().await;
    drop(tasks);
    let reopened = Tasks::open(path)?;
    assert_eq!(
        reopened
            .get(task["id"].as_str().context("missing id")?)
            .await?,
        Some(task)
    );
    reopened.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn task_store_single_owner_restart_and_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let path = configuration(root.path(), "")?;
    let store = Store::open(&path)?;
    assert!(Store::open(&path).is_err());
    store.put(json!({"id":"interrupted","contextId":"ctx","status":{"state":"TASK_STATE_WORKING","timestamp":store::timestamp()},"history":[]})).await?;
    drop(store);
    let store = Store::open(&path)?;
    let task = store
        .get("interrupted")
        .await?
        .context("missing interrupted task")?;
    assert_eq!(task["status"]["state"], "TASK_STATE_FAILED");
    assert!(task["artifacts"].is_null());
    assert_eq!(
        std::fs::metadata(root.path().join("protocol/tasks.sqlite3"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(store);
    std::fs::set_permissions(
        root.path().join("protocol"),
        std::fs::Permissions::from_mode(0o755),
    )?;
    assert!(Store::open(&path).is_err());
    Ok(())
}
#[tokio::test]
async fn a2a_agent_contexts_serialize_history_and_list_filters() -> Result<()> {
    let provider = MockServer::start().await;
    Mock::given(method("POST")).respond_with(|request: &wiremock::Request| {
        let body: Value=serde_json::from_slice(&request.body).unwrap_or_default();
        let turns=body["messages"].as_array().map(|items|items.iter().filter(|message|message["role"]=="user").count()).unwrap_or_default();
        ResponseTemplate::new(200).insert_header("content-type","text/event-stream").set_body_string(format!("data: {}\n\ndata: [DONE]\n\n",json!({"choices":[{"delta":{"content":format!("turn-{turns}")},"finish_reason":"stop"}],"usage":{}})))
    }).mount(&provider).await;
    let root = tempfile::tempdir()?;
    let cfg = Config {
        endpoint: format!("{}/v1", provider.uri()),
        api_type: ApiType::ChatCompletions,
        api_key: "test-secret-key".into(),
        ..Config::default()
    };
    let path = configuration(root.path(), &toml::to_string(&cfg)?)?;
    let tasks = Tasks::open(path)?;
    let (url, job) = server(tasks.clone()).await?;
    let client = client()?;
    let send = |id: &str| json!({"message":{"messageId":id,"role":"ROLE_USER","contextId":"shared","parts":[{"text":"hello test-secret-key"}]}});
    let first = rpc(&client, &url, "SendMessage", send("m1")).await?;
    assert_eq!(
        first["result"]["task"]["status"]["state"], "TASK_STATE_COMPLETED",
        "{first}"
    );
    assert_eq!(result(&first["result"]["task"])?["answer"], "turn-1");
    let second = rpc(&client, &url, "SendMessage", send("m2")).await?;
    assert_eq!(result(&second["result"]["task"])?["answer"], "turn-2");
    assert!(!first.to_string().contains("test-secret-key"));
    let id = first["result"]["task"]["id"]
        .as_str()
        .context("missing id")?;
    let get = rpc(&client, &url, "GetTask", json!({"id":id,"historyLength":0})).await?;
    assert_eq!(get["result"]["history"], json!([]));
    let list = rpc(
        &client,
        &url,
        "ListTasks",
        json!({"contextId":"shared","pageSize":1}),
    )
    .await?;
    assert_eq!(list["result"]["totalSize"], 2);
    assert!(list["result"]["tasks"][0]["artifacts"].is_null());
    let next=rpc(&client,&url,"ListTasks",json!({"contextId":"shared","pageSize":1,"pageToken":list["result"]["nextPageToken"],"includeArtifacts":true})).await?;
    assert_eq!(next["result"]["tasks"].as_array().map(Vec::len), Some(1));
    assert_eq!(next["result"]["nextPageToken"], "");
    assert!(next["result"]["tasks"][0]["artifacts"].is_array());
    assert_eq!(
        rpc(&client, &url, "CancelTask", json!({"id":id})).await?["error"]["code"],
        -32002
    );
    tasks.shutdown().await;
    job.abort();
    Ok(())
}

#[tokio::test]
async fn a2a_async_cancel_settles_queued_and_model_tasks() -> Result<()> {
    let provider = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)).insert_header("content-type","text/event-stream").set_body_string("data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")).mount(&provider).await;
    let root = tempfile::tempdir()?;
    let cfg = Config {
        endpoint: format!("{}/v1", provider.uri()),
        api_type: ApiType::ChatCompletions,
        api_key: "dummy".into(),
        ..Config::default()
    };
    let tasks = Tasks::open(configuration(root.path(), &toml::to_string(&cfg)?)?)?;
    let (url, job) = server(tasks.clone()).await?;
    let client = client()?;
    let send = |id: &str| json!({"message":{"messageId":id,"role":"ROLE_USER","contextId":"queue","parts":[{"text":"hello"}]},"configuration":{"returnImmediately":true}});
    let first = rpc(&client, &url, "SendMessage", send("first")).await?;
    let first_id = first["result"]["task"]["id"]
        .as_str()
        .context("missing task id")?;
    assert!(matches!(
        first["result"]["task"]["status"]["state"].as_str(),
        Some("TASK_STATE_SUBMITTED" | "TASK_STATE_WORKING")
    ));
    let second = rpc(&client, &url, "SendMessage", send("second")).await?;
    let second_id = second["result"]["task"]["id"]
        .as_str()
        .context("missing queued id")?;
    let cancelled = rpc(&client, &url, "CancelTask", json!({"id":second_id})).await?;
    assert_eq!(
        cancelled["result"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );
    let cancelled = rpc(&client, &url, "CancelTask", json!({"id":first_id})).await?;
    assert_eq!(
        cancelled["result"]["status"]["state"],
        "TASK_STATE_CANCELED"
    );
    let calls = provider
        .received_requests()
        .await
        .context("missing captured requests")?;
    assert!(
        calls.len() <= 1,
        "queued context started a second model call"
    );
    tasks.shutdown().await;
    job.abort();
    Ok(())
}
#[tokio::test]
async fn task_store_bounds_results_and_prunes_terminal_tasks() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = configuration(root.path(), "")?;
    let store = Store::open(&path)?;
    for index in 0..201 {
        store.put(json!({"id":format!("task-{index:03}"),"contextId":"ctx","status":{"state":"TASK_STATE_COMPLETED","timestamp":format!("2026-10-09T00:00:{:02}Z",index%60)},"history":[]})).await?;
    }
    assert_eq!(store.all().await?.len(), 200);
    assert!(store.put(json!({"id":"oversize","contextId":"ctx","status":{"state":"TASK_STATE_COMPLETED","timestamp":store::timestamp()},"artifacts":[{"parts":[{"data":{"text":"x".repeat(4*1024*1024)}}]}]})).await.is_err());
    assert!(store.get("oversize").await?.is_none());
    Ok(())
}
#[test]
fn mcp_screenshot_uses_native_image_content_and_refuses_invalid_contract() -> Result<()> {
    let result = super::mcp::screen_result(
        json!({"tool":"android.read_screen","success":true,"output":"screen","attachments":[{"media_type":"image/png","base64_data":"aW1hZ2U="}]}),
    )?;
    let result = serde_json::to_value(result)?;
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    assert!(super::mcp::screen_result(json!({"success":true,"attachments":[]})).is_err());
    assert!(super::mcp::screen_result(
        json!({"success":true,"attachments":[{"media_type":"text/html","base64_data":"abc"}]})
    )
    .is_err());
    Ok(())
}
