//! Tencent iLink Bot API: QR login, long polling and context-bound text replies.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
const BASE: &str = "https://ilinkai.weixin.qq.com";
pub(super) struct Weixin {
    account: Account,
    client: reqwest::Client,
    contexts: Mutex<BTreeMap<String, String>>,
}
impl Weixin {
    pub fn new(account: Account) -> Self {
        Self {
            account,
            client: client(),
            contexts: Mutex::new(BTreeMap::new()),
        }
    }
}
fn headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let random = uuid::Uuid::new_v4();
    let bytes = random.as_bytes();
    let uin = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).to_string();
    request
        .header("AuthorizationType", "ilink_bot_token")
        .header("X-WECHAT-UIN", STANDARD.encode(uin))
        .header("iLink-App-Id", "bot")
        .header("iLink-App-ClientVersion", "131584")
}
async fn post(
    client: &reqwest::Client,
    account: &Account,
    endpoint: &str,
    mut body: Value,
) -> Result<Value> {
    body["base_info"] = json!({"channel_version":"2.2.0"});
    response_json(
        headers(client.post(format!(
            "{}/ilink/bot/{endpoint}",
            account.base_url.trim_end_matches('/')
        )))
        .bearer_auth(&account.secret)
        .json(&body)
        .send()
        .await?,
    )
    .await
}
fn approved_base(base: &str) -> Result<String> {
    let url = url::Url::parse(base)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url
            .host_str()
            .is_some_and(|h| h == "ilinkai.weixin.qq.com" || h.ends_with(".weixin.qq.com"))
    {
        bail!("invalid iLink API host");
    }
    Ok(url.origin().ascii_serialization())
}
pub(super) async fn bind(
    manager: Arc<ChannelManager>,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let client = client();
    let response = tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),r=client.get(format!("{BASE}/ilink/bot/get_bot_qrcode")).query(&[("bot_type","3")]).header("iLink-App-Id","bot").send()=>response_json(r?).await?};
    let code = response["qrcode"]
        .as_str()
        .context("QR response missing code")?
        .to_owned();
    let qr = response["qrcode_img_content"]
        .as_str()
        .context("QR response missing URL")?
        .to_owned();
    let url = url::Url::parse(&qr)?;
    if qr.len() > 4096 || code.len() > 4096 || !matches!(url.scheme(), "https" | "http") {
        bail!("invalid QR URL");
    }
    if let Ok(mut statuses) = manager.statuses.lock() {
        if let Some(status) = statuses.get_mut("weixin") {
            status.qr_url = Some(qr);
        }
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(480);
    let mut base = BASE.to_owned();
    loop {
        let response = tokio::select! {biased;_=cancelled(&mut cancel)=>return Ok(()),_=tokio::time::sleep_until(deadline)=>{manager.status("weixin","expired",Some("二维码已过期，请重新绑定"));return Ok(());},r=async { response_json(client.get(format!("{base}/ilink/bot/get_qrcode_status")).query(&[("qrcode",&code)]).header("iLink-App-Id","bot").send().await?).await }=>r};
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                tokio::select! { _=cancelled(&mut cancel)=>return Ok(()), _=tokio::time::sleep(Duration::from_secs(2))=>{} }
                continue;
            }
        };
        match response["status"].as_str().unwrap_or("wait") {
            "scaned" => manager.status("weixin", "scanned", None),
            "scaned_but_redirect" => {
                let host = response["redirect_host"]
                    .as_str()
                    .context("missing redirect host")?;
                base = approved_base(&format!("https://{host}"))?;
            }
            "expired" => {
                manager.status("weixin", "expired", Some("二维码已过期，请重新绑定"));
                return Ok(());
            }
            "confirmed" => {
                let id = response["ilink_bot_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512)
                    .context("missing or oversized bot ID")?
                    .to_owned();
                let secret = response["bot_token"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                    .context("missing or oversized bot token")?
                    .to_owned();
                let account = Account {
                    id,
                    secret,
                    base_url: approved_base(response["baseurl"].as_str().unwrap_or(BASE))?,
                    enabled: true,
                };
                {
                    let mut saved = manager.saved.lock().await;
                    saved.weixin = Some(account.clone());
                    saved.cursor.clear();
                    manager.persist(&saved).await?;
                }
                run_adapter(manager, account, "weixin", cancel).await;
                return Ok(());
            }
            _ => {}
        }
        tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(1))=>{}}
    }
}
#[async_trait]
impl MessageChannel for Weixin {
    async fn receive(
        &self,
        manager: Arc<ChannelManager>,
        queue: &mpsc::Sender<Incoming>,
        cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        receive(manager, self.account.clone(), queue, self, cancel).await
    }

    fn observe(&self, message: &Incoming) {
        if !message.context.is_empty() && message.context.len() <= 4096 {
            if let Ok(mut contexts) = self.contexts.lock() {
                if contexts.len() >= 512 && !contexts.contains_key(&message.route.key()) {
                    return;
                }
                contexts.insert(message.route.key(), message.context.clone());
            }
        }
    }
    fn secrets(&self) -> Vec<String> {
        vec![self.account.secret.clone()]
    }
    async fn send_formatted(
        &self,
        message: &Incoming,
        text: &str,
        format: &str,
        buttons: &[outbound::MessageButton],
    ) -> Result<Value> {
        if !buttons.is_empty() {
            bail!("WeChat does not support native buttons");
        }
        let text = if format == "markdown" {
            markdown::compatible(text)
        } else {
            text.to_owned()
        };
        if text.trim().is_empty() {
            bail!("WeChat Markdown contains no supported text; send images as media");
        }
        let parts = if format == "markdown" {
            markdown::split(&text, 3500)
        } else {
            chunks(&text, 3500)
        };
        let count = parts.len();
        for (index, part) in parts.into_iter().enumerate() {
            if let Err(error) = self.send(message, &part).await {
                bail!("WeChat delivery failed after {index} submitted segments; do not automatically retry: {error}");
            }
        }
        Ok(
            json!({"status":"submitted","segments":count,"format":format,"rendering":if format == "markdown" {"compatible"} else {"text"}}),
        )
    }
    async fn send_media(
        &self,
        message: &Incoming,
        name: &str,
        kind: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()> {
        upload_and_send(self, message, name, kind, bytes, caption).await
    }
    async fn send(&self, message: &Incoming, text: &str) -> Result<()> {
        let context = self
            .contexts
            .lock()
            .ok()
            .and_then(|c| c.get(&message.route.key()).cloned())
            .unwrap_or_else(|| message.context.clone());
        for text in chunks(&crate::limits::truncate_text(text, 12000), 3500) {
            post(&self.client,&self.account,"sendmessage",json!({"msg":{"from_user_id":"","to_user_id":message.route.peer,"client_id":uuid::Uuid::new_v4().to_string(),"message_type":2,"message_state":2,"context_token":context,"item_list":[{"type":1,"text_item":{"text":text}}]}})).await?;
        }
        Ok(())
    }
}
fn parse(account: &Account, message: &Value) -> Option<Incoming> {
    if message["message_type"].as_i64() != Some(1) {
        return None;
    }
    let sender = message["from_user_id"].as_str()?.to_owned();
    if sender.is_empty() || sender == account.id {
        return None;
    }
    let text = message["item_list"]
        .as_array()?
        .iter()
        .filter(|item| item["type"] == 1)
        .filter_map(|item| item["text_item"]["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let id = message["message_id"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| message["message_id"].as_u64().map(|n| n.to_string()))?;
    Some(Incoming {
        route: Route {
            kind: "weixin".into(),
            account: account.id.clone(),
            peer: sender.clone(),
            sender,
            group: false,
        },
        id,
        text,
        context: message["context_token"].as_str().unwrap_or_default().into(),
        media: media::weixin(message),
    })
}
pub(super) async fn receive(
    manager: Arc<ChannelManager>,
    account: Account,
    queue: &mpsc::Sender<Incoming>,
    channel: &dyn MessageChannel,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let client = client();
    let mut cursor = manager.saved.lock().await.cursor.clone();
    loop {
        let response = tokio::select! {biased;_=cancelled(&mut cancel)=>return Ok(()),r=post(&client,&account,"getupdates",json!({"get_updates_buf":cursor}))=>match r{Ok(v)=>v,Err(e) if e.downcast_ref::<reqwest::Error>().is_some_and(|e|e.is_timeout())=>continue,Err(e)=>return Err(e)}};
        manager.status("weixin", "online", None);
        if let Some(messages) = response["msgs"].as_array() {
            for value in messages {
                if let Some(message) = parse(&account, value) {
                    manager.intake(message, queue, channel).await?;
                }
            }
        }
        if let Some(next) = response["get_updates_buf"].as_str() {
            if next.len() > 65536 {
                bail!("iLink cursor exceeds limit");
            }
            if next != cursor {
                cursor = next.into();
                let mut saved = manager.saved.lock().await;
                saved.cursor = cursor.clone();
                manager.persist(&saved).await?;
            }
        }
        // Protect against empty immediate responses while retaining normal long polling.
        tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(200))=>{}}
    }
}

async fn upload_and_send(
    wx: &Weixin,
    message: &Incoming,
    name: &str,
    kind: &str,
    bytes: Vec<u8>,
    caption: &str,
) -> Result<()> {
    use md5::Md5;
    if bytes.is_empty() || bytes.len() > media::MAX_FILE_BYTES {
        bail!("invalid attachment size");
    }
    let raw_size = bytes.len();
    let (encrypted, key, md5) = tokio::task::spawn_blocking(move || {
        let digest =
            Sha256::digest(format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()).as_bytes());
        let mut key = [0; 16];
        key.copy_from_slice(&digest[..16]);
        let md5 = format!("{:x}", Md5::digest(&bytes));
        (media::encrypt(bytes, &key), key, md5)
    })
    .await?;
    // Ordinary audio is sent as an intact file; iLink native voice requires SILK metadata.
    let (media_type, item_type, item_key) = match kind {
        "image" => (1, 2, "image_item"),
        "video" if name.to_ascii_lowercase().ends_with(".mp4") => (2, 5, "video_item"),
        _ => (3, 4, "file_item"),
    };
    let hex = key.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let filekey = uuid::Uuid::new_v4().simple().to_string();
    let uploaded=post(&wx.client,&wx.account,"getuploadurl",json!({"filekey":filekey,"media_type":media_type,"to_user_id":message.route.peer,"rawsize":raw_size,"rawfilemd5":md5,"filesize":encrypted.len(),"no_need_thumb":true,"aeskey":hex})).await?;
    let url = if let Some(full) = uploaded["upload_full_url"].as_str() {
        full.to_owned()
    } else {
        let param = uploaded["upload_param"]
            .as_str()
            .context("iLink upload URL missing")?;
        let mut url = url::Url::parse("https://novac2c.cdn.weixin.qq.com/c2c/upload")?;
        url.query_pairs_mut()
            .append_pair("encrypted_query_param", param)
            .append_pair("filekey", &filekey);
        url.into()
    };
    #[cfg(test)]
    let validated =
        if url::Url::parse(&url)?.origin() == url::Url::parse(&wx.account.base_url)?.origin() {
            url::Url::parse(&url)?
        } else {
            media::validate_url("weixin", &url)?
        };
    #[cfg(not(test))]
    let validated = media::validate_url("weixin", &url)?;
    let cipher_size = encrypted.len();
    let response = wx
        .client
        .post(validated)
        .timeout(Duration::from_secs(120))
        .header("Content-Type", "application/octet-stream")
        .body(encrypted)
        .send()
        .await
        .map_err(reqwest::Error::without_url)?;
    if !response.status().is_success() {
        bail!("iLink CDN upload failed ({})", response.status().as_u16());
    }
    let param = response
        .headers()
        .get("x-encrypted-param")
        .and_then(|s| s.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 16384)
        .context("iLink CDN download token missing")?
        .to_owned();
    let mut payload = json!({"media":{"encrypt_query_param":param,"aes_key":STANDARD.encode(hex.as_bytes()),"encrypt_type":1}});
    match item_type {
        2 => payload["mid_size"] = json!(cipher_size),
        5 => {
            payload["video_size"] = json!(cipher_size);
            payload["play_length"] = json!(0);
            payload["video_md5"] = json!(md5);
        }
        _ => {
            payload["file_name"] = json!(name);
            payload["len"] = json!(raw_size.to_string());
        }
    }
    let mut items = Vec::new();
    if !caption.is_empty() {
        items.push(json!({"type":1,"text_item":{"text":caption}}));
    }
    items.push(json!({"type":item_type,item_key:payload}));
    // iLink accepts one MessageItem per request; captions are separate text messages.
    for (index, item) in items.into_iter().enumerate() {
        let context = wx
            .contexts
            .lock()
            .map_err(|_| anyhow!("iLink context unavailable"))?
            .get(&message.route.key())
            .cloned()
            .unwrap_or_else(|| message.context.clone());
        post(&wx.client, &wx.account, "sendmessage", json!({"msg":{
            "from_user_id":"", "to_user_id":message.route.peer,
            "client_id":format!("nl2sh-{}",uuid::Uuid::new_v4()),
            "message_type":2, "message_state":2, "context_token":context,
            "item_list":[item]
        }})).await.with_context(|| if index > 0 {
            "iLink caption delivered, but attachment delivery failed; do not automatically retry"
        } else { "iLink delivery failed; do not automatically retry" })?;
    }
    Ok(())
}

#[cfg(test)]
mod media_protocol_tests {
    use super::*;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[tokio::test]
    async fn encrypted_uploads_use_download_header_and_hex_encoded_key_for_every_kind() -> Result<()>
    {
        for (kind, name, media_type, item_type, item_key) in [
            ("image", "photo.png", 1, 2, "image_item"),
            ("audio", "voice.wav", 3, 4, "file_item"),
            ("video", "clip.mp4", 2, 5, "video_item"),
            ("file", "report.zip", 3, 4, "file_item"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST")).and(path("/ilink/bot/getuploadurl")).and(header("Authorization","Bearer secret")).and(body_partial_json(json!({"media_type":media_type,"to_user_id":"peer","rawsize":5,"filesize":16,"no_need_thumb":true}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ret":0,"upload_full_url":format!("{}/upload?encrypted_query_param=upload-auth",server.uri())}))).expect(1).mount(&server).await;
            Mock::given(method("POST"))
                .and(path("/upload"))
                .respond_with(
                    ResponseTemplate::new(200).insert_header("x-encrypted-param", "download-key"),
                )
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST")).and(path("/ilink/bot/sendmessage")).and(body_partial_json(json!({"msg":{"message_type":2,"message_state":2,"to_user_id":"peer","context_token":"latest-context"}}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"ret":0}))).expect(2).mount(&server).await;
            let wx = Weixin::new(Account {
                id: "bot".into(),
                secret: "secret".into(),
                base_url: server.uri(),
                enabled: true,
            });
            let message = Incoming {
                route: Route {
                    kind: "weixin".into(),
                    account: "bot".into(),
                    peer: "peer".into(),
                    sender: "peer".into(),
                    group: false,
                },
                id: "m1".into(),
                text: String::new(),
                context: "latest-context".into(),
                media: Vec::new(),
            };
            wx.send_media(&message, name, kind, b"bytes".to_vec(), "caption")
                .await?;
            let requests = server
                .received_requests()
                .await
                .context("requests missing")?;
            let upload: Value = serde_json::from_slice(
                &requests
                    .iter()
                    .find(|r| r.url.path().ends_with("getuploadurl"))
                    .context("prepare missing")?
                    .body,
            )?;
            let key = upload["aeskey"].as_str().context("AES key missing")?;
            let encrypted = &requests
                .iter()
                .find(|r| r.url.path() == "/upload")
                .context("upload missing")?
                .body;
            assert_eq!(media::decrypt(encrypted.clone(), key, true)?, b"bytes");
            let messages: Vec<Value> = requests
                .iter()
                .filter(|r| r.url.path().ends_with("sendmessage"))
                .map(|r| serde_json::from_slice(&r.body))
                .collect::<std::result::Result<_, _>>()?;
            assert_eq!(messages.len(), 2);
            assert!(messages.iter().all(|msg| msg["msg"]["item_list"]
                .as_array()
                .is_some_and(|items| items.len() == 1)));
            assert_eq!(
                messages[0]["msg"]["item_list"][0]["text_item"]["text"],
                "caption"
            );
            assert_ne!(
                messages[0]["msg"]["client_id"],
                messages[1]["msg"]["client_id"]
            );
            let item = &messages[1]["msg"]["item_list"][0];
            assert_eq!(item["type"], item_type);
            assert_eq!(
                item[item_key]["media"]["encrypt_query_param"],
                "download-key"
            );
            assert_eq!(
                item[item_key]["media"]["aes_key"],
                STANDARD.encode(key.as_bytes())
            );
        }
        Ok(())
    }
    #[tokio::test]
    async fn caption_delivery_then_media_failure_is_partial_and_never_retried() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ilink/bot/getuploadurl"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"ret":0,"upload_full_url":format!("{}/upload",server.uri())}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/upload"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("x-encrypted-param", "download-token"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/ilink/bot/sendmessage"))
            .respond_with(|request: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let items = body["msg"]["item_list"].as_array().unwrap();
                assert_eq!(items.len(), 1);
                ResponseTemplate::new(200)
                    .set_body_json(json!({"ret":if items[0]["type"]==1 {0} else {-2}}))
            })
            .expect(2)
            .mount(&server)
            .await;
        let wx = Weixin::new(Account {
            id: "bot".into(),
            secret: "private".into(),
            base_url: server.uri(),
            enabled: true,
        });
        let message = Incoming {
            route: Route {
                kind: "weixin".into(),
                account: "bot".into(),
                peer: "peer".into(),
                sender: "peer".into(),
                group: false,
            },
            id: "message".into(),
            text: String::new(),
            context: "context".into(),
            media: vec![],
        };
        let error = wx
            .send_media(&message, "report.zip", "file", b"bytes".to_vec(), "caption")
            .await
            .err()
            .context("failure expected")?;
        let text = format!("{error:#}");
        assert!(
            text.contains("caption delivered")
                && text.contains("attachment delivery failed")
                && text.contains("ret=-2")
        );
        assert!(!text.contains("download-token") && !text.contains("private"));
        Ok(())
    }
}
