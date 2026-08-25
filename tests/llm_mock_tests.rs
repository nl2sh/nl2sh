use nl2sh::{
    config::{ApiType, Config},
    llm::{build_client, ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role},
};
use serde_json::json;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[derive(Default)]
struct CapturingSink(std::sync::Mutex<String>);

impl nl2sh::llm::TextDeltaSink for CapturingSink {
    fn delta(&self, text: &str) {
        if let Ok(mut output) = self.0.lock() {
            output.push_str(text);
        }
    }
}
fn request() -> LlmRequest {
    LlmRequest {
        model: "test".into(),
        items: vec![ConversationItem::Message(ConversationMessage::new(
            Role::User,
            "hi",
        ))],
        tools: vec![],
    }
}
#[tokio::test]
async fn chat_text_and_tool_call() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":{}}),
        ))
        .mount(&server)
        .await;
    let c = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::ChatCompletions,
        ..Config::default()
    })?;
    assert_eq!(c.complete(request()).await?.text.as_deref(), Some("ok"));
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"execute_shell_command","arguments":"{\"command\":\"id\"}"}}]},"finish_reason":"tool_calls"}],"usage":{}
        })))
        .mount(&server).await;
    assert_eq!(c.complete(request()).await?.tool_calls.len(), 1);
    Ok(())
}
#[tokio::test]
async fn responses_text_and_tool_call() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/v1/responses")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"output":[{"type":"function_call","call_id":"c1","name":"execute_shell_command","arguments":"{\"command\":\"id\"}"}],"usage":{}}))).mount(&server).await;
    let c = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::Responses,
        ..Config::default()
    })?;
    assert_eq!(
        c.complete(request()).await?.tool_calls[0].name,
        "execute_shell_command"
    );
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output":[{"type":"message","content":[{"type":"output_text","text":"done"}]}],"usage":{}
        })))
        .mount(&server).await;
    assert_eq!(c.complete(request()).await?.text.as_deref(), Some("done"));
    Ok(())
}
#[tokio::test]
async fn does_not_retry_401() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let c = build_client(&Config {
        endpoint: server.uri(),
        api_type: ApiType::Responses,
        llm_retry_count: 3,
        llm_retry_base_delay_ms: 1,
        ..Config::default()
    })?;
    assert!(c.complete(request()).await.is_err());
    Ok(())
}
#[tokio::test]
async fn retries_429_then_succeeds() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .with_priority(1)
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"output":[{"type":"message","content":[{"type":"output_text","text":"retried"}]}]})))
        .with_priority(2).expect(1).mount(&server).await;
    let c = build_client(&Config {
        endpoint: server.uri(),
        api_type: ApiType::Responses,
        llm_retry_count: 1,
        llm_retry_base_delay_ms: 1,
        ..Config::default()
    })?;
    assert_eq!(
        c.complete(request()).await?.text.as_deref(),
        Some("retried")
    );
    Ok(())
}
#[tokio::test]
async fn request_timeout_is_reported() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)))
        .mount(&server)
        .await;
    let c = build_client(&Config {
        endpoint: server.uri(),
        api_type: ApiType::Responses,
        llm_request_timeout_secs: 1,
        llm_retry_count: 0,
        ..Config::default()
    })?;
    assert!(c.complete(request()).await.is_err());
    Ok(())
}
#[tokio::test]
async fn malformed_and_empty_responses_fail() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;
    let c = build_client(&Config {
        endpoint: server.uri(),
        api_type: ApiType::Responses,
        ..Config::default()
    })?;
    assert!(c.complete(request()).await.is_err());
    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"output":[]})))
        .mount(&server)
        .await;
    assert!(c.complete(request()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn auto_falls_back_on_responses_shape_mismatch_and_caches_chat() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"output":[]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices":[{"message":{"content":"chat-ok"},"finish_reason":"stop"}],"usage":{}
        })))
        .expect(2)
        .mount(&server)
        .await;
    let client = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::Auto,
        ..Config::default()
    })?;

    assert_eq!(
        client.complete(request()).await?.text.as_deref(),
        Some("chat-ok")
    );
    assert_eq!(
        client.complete(request()).await?.text.as_deref(),
        Some("chat-ok")
    );
    Ok(())
}

#[tokio::test]
async fn auto_does_not_treat_service_failure_as_protocol_mismatch() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(503).set_body_string("no available channel"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let client = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::Auto,
        llm_retry_count: 0,
        ..Config::default()
    })?;
    assert!(client.complete(request()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn auto_stream_falls_back_before_emitting_and_caches_chat() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    let chat_stream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"stream-ok\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(chat_stream),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(chat_stream),
        )
        .expect(2)
        .mount(&server)
        .await;
    let client = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::Auto,
        ..Config::default()
    })?;
    let sink = CapturingSink::default();

    assert_eq!(
        client
            .complete_stream(request(), &sink)
            .await?
            .text
            .as_deref(),
        Some("stream-ok")
    );
    assert_eq!(
        client
            .complete_stream(request(), &sink)
            .await?
            .text
            .as_deref(),
        Some("stream-ok")
    );
    assert_eq!(
        sink.0
            .lock()
            .map_err(|_| anyhow::anyhow!("sink poisoned"))?
            .as_str(),
        "stream-okstream-ok"
    );
    Ok(())
}

#[tokio::test]
async fn auto_stream_never_replays_after_text_was_emitted() -> anyhow::Result<()> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let client = build_client(&Config {
        endpoint: format!("{}/v1", server.uri()),
        api_type: ApiType::Auto,
        ..Config::default()
    })?;
    let sink = CapturingSink::default();

    assert!(client.complete_stream(request(), &sink).await.is_err());
    assert_eq!(
        sink.0
            .lock()
            .map_err(|_| anyhow::anyhow!("sink poisoned"))?
            .as_str(),
        "partial"
    );
    Ok(())
}
