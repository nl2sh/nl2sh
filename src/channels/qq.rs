//! Official QQ Bot API v2: token auth, WebSocket gateway and passive REST replies.
use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};
const TOKEN: &str = "https://api.bot.qq.com/app/getAppAccessToken";
pub(super) struct Qq {
    account: Account,
    client: reqwest::Client,
    token: AsyncMutex<Option<(String, std::time::Instant)>>,
    seq: Mutex<BTreeMap<String, u64>>,
    anchors: Mutex<BTreeMap<String, String>>,
    gateway: Mutex<GatewaySession>,
    interactions: Mutex<BTreeMap<String, ()>>,
}
#[derive(Default, Clone)]
struct GatewaySession {
    session: Option<String>,
    seq: Option<u64>,
}

impl Qq {
    pub fn new(account: Account) -> Self {
        Self {
            account,
            client: client(),
            token: AsyncMutex::new(None),
            seq: Mutex::new(BTreeMap::new()),
            anchors: Mutex::new(BTreeMap::new()),
            gateway: Mutex::new(GatewaySession::default()),
            interactions: Mutex::new(BTreeMap::new()),
        }
    }
    async fn formatted(
        &self,
        message: &Incoming,
        text: &str,
        markdown: bool,
        keyboard: Option<Value>,
    ) -> Result<Value> {
        if text.len() > 12000 {
            bail!("QQ message exceeds limit");
        }
        let anchor = self
            .anchors
            .lock()
            .map_err(|_| anyhow!("QQ anchors unavailable"))?
            .get(&message.route.key())
            .cloned()
            .unwrap_or_else(|| message.id.clone());
        let mut ids = Vec::new();
        let parts = if markdown {
            super::markdown::split(text, 3500)
        } else {
            chunks(text, 3500)
        };
        let count = parts.len();
        for (index, part) in parts.into_iter().enumerate() {
            let seq = {
                let mut seq = self
                    .seq
                    .lock()
                    .map_err(|_| anyhow!("QQ sequence unavailable"))?;
                if seq.len() > 2048 {
                    seq.clear();
                }
                let n = seq.entry(anchor.clone()).or_default();
                *n += 1;
                *n
            };
            let mut body = if markdown {
                json!({"msg_type":2,"markdown":{"content":part}})
            } else {
                json!({"msg_type":0,"content":part})
            };
            body["msg_id"] = json!(anchor);
            body["msg_seq"] = json!(seq);
            // Only the last segment owns the buttons so the complete preview precedes approval.
            if index + 1 == count {
                if let Some(keyboard) = &keyboard {
                    body["keyboard"] = keyboard.clone();
                }
            }
            match media_post(self,message,"messages",body).await {
                Ok(value) => ids.push(value["id"].as_str().map(str::to_owned)),
                Err(error) => bail!("QQ delivery failed after {} submitted segments; do not automatically retry: {}",ids.len(),error),
            }
        }
        Ok(
            json!({"message_ids":ids,"segments":ids.len(),"format":if markdown {"markdown"}else{"text"}}),
        )
    }
    async fn interaction(&self, manager: &ChannelManager, event: &Value) -> Result<()> {
        let d = &event["d"];
        if !matches!(d["type"].as_u64(), Some(11 | 12)) {
            return Ok(());
        }
        let id = d["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 512)
            .context("QQ interaction ID missing")?;
        {
            let mut seen = self
                .interactions
                .lock()
                .map_err(|_| anyhow!("QQ interaction ledger unavailable"))?;
            if seen.contains_key(id) {
                return Ok(());
            }
            if seen.len() >= 2048 {
                seen.clear();
            }
            seen.insert(id.into(), ());
        }
        let parsed = interaction_decision(&self.account, d);
        let code = if let Some((message, pending_id, mode)) = parsed {
            if approval::button(manager, &message.route, &pending_id, &mode)? {
                0
            } else {
                3
            }
        } else {
            4
        };
        let token = self.token().await?;
        let mut url = url::Url::parse(&self.account.base_url)?;
        url.path_segments_mut()
            .map_err(|_| anyhow!("invalid QQ URL"))?
            .extend(["interactions", id]);
        response_json(
            self.client
                .put(url)
                .header("Authorization", format!("QQBot {token}"))
                .header("X-Union-Appid", &self.account.id)
                .timeout(Duration::from_secs(2))
                .json(&json!({"code":code}))
                .send()
                .await?,
        )
        .await?;
        Ok(())
    }
    async fn token(&self) -> Result<String> {
        let mut cache = self.token.lock().await;
        if let Some((token, expires)) = &*cache {
            if *expires > std::time::Instant::now() {
                return Ok(token.clone());
            }
        }
        let response = response_json(
            self.client
                .post(TOKEN)
                .json(&json!({"appId":self.account.id,"clientSecret":self.account.secret}))
                .send()
                .await?,
        )
        .await?;
        let token = response["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("QQ token response missing access token")?
            .to_owned();
        let ttl = response["expires_in"]
            .as_u64()
            .or_else(|| response["expires_in"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(7200)
            .clamp(61, 86400);
        *cache = Some((
            token.clone(),
            std::time::Instant::now() + Duration::from_secs(ttl - 60),
        ));
        Ok(token)
    }
}
#[async_trait]
impl MessageChannel for Qq {
    async fn receive(
        &self,
        manager: Arc<ChannelManager>,
        queue: &mpsc::Sender<Incoming>,
        cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        receive(manager, self, queue, cancel).await
    }

    fn observe(&self, message: &Incoming) {
        if let Ok(mut anchors) = self.anchors.lock() {
            if anchors.len() >= 512 && !anchors.contains_key(&message.route.key()) {
                return;
            }
            anchors.insert(message.route.key(), message.id.clone());
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
        outbound::validate_buttons(buttons)?;
        let keyboard = if buttons.is_empty() {
            None
        } else {
            Some(
                json!({"content":{"rows":[{"buttons":buttons.iter().enumerate().map(|(i,b)|json!({"id":format!("command-{i}"),"render_data":{"label":b.label,"style":1},"action":{"type":2,"permission":{"type":2},"data":b.command,"enter":true,"reply":true}})).collect::<Vec<_>>()}]}}),
            )
        };
        self.formatted(message, text, format == "markdown", keyboard)
            .await
    }
    async fn send_approval(
        &self,
        message: &Incoming,
        text: &str,
        id: &str,
        remember: bool,
        language: crate::config::UiLanguage,
    ) -> Result<()> {
        let zh = language == crate::config::UiLanguage::ZhCn;
        let modes = if remember {
            vec!["once", "session", "always", "deny"]
        } else {
            vec!["once", "deny"]
        };
        // QQ v2 message openids are not keyboard UserIDs. Let the click reach
        // our callback; approval::button enforces the original full route atomically.
        let buttons:Vec<_> = modes.into_iter().map(|mode| {
            let label = match (zh,mode) {(true,"once")=>"允许一次",(true,"session")=>"允许本会话",(true,"always")=>"永久允许此模式",(true,_)=>"拒绝",(false,"once")=>"Allow once",(false,"session")=>"Session",(false,"always")=>"Always",(false,_)=>"Deny"};
            json!({"id":mode,"render_data":{"label":label,"style":1},"action":{"type":1,"permission":{"type":2},"data":format!("nl2sh:{id}:{mode}")}})
        }).collect();
        self.formatted(
            message,
            text,
            true,
            Some(json!({"content":{"rows":[{"buttons":buttons}]}})),
        )
        .await?;
        Ok(())
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
        let token = self.token().await?;
        let anchor = self
            .anchors
            .lock()
            .ok()
            .and_then(|a| a.get(&message.route.key()).cloned())
            .unwrap_or_else(|| message.id.clone());
        let mut url = url::Url::parse(&self.account.base_url)?;
        url.path_segments_mut()
            .map_err(|_| anyhow!("invalid QQ URL"))?
            .extend([
                "v2",
                if message.route.group {
                    "groups"
                } else {
                    "users"
                },
                &message.route.peer,
                "messages",
            ]);
        for text in chunks(&crate::limits::truncate_text(text, 12000), 3500) {
            let seq = {
                let mut seq = self
                    .seq
                    .lock()
                    .map_err(|_| anyhow!("QQ message sequence unavailable"))?;
                if seq.len() > 2048 {
                    seq.clear();
                }
                let n = seq.entry(anchor.clone()).or_default();
                *n += 1;
                *n
            };
            response_json(
                self.client
                    .post(url.clone())
                    .header("Authorization", format!("QQBot {token}"))
                    .header("X-Union-Appid", &self.account.id)
                    .json(&json!({"content":text,"msg_type":0,"msg_id":anchor,"msg_seq":seq}))
                    .send()
                    .await?,
            )
            .await?;
        }
        Ok(())
    }
}
fn interaction_decision(account: &Account, d: &Value) -> Option<(Incoming, String, String)> {
    if d["type"] != 11
        || d["application_id"]
            .as_str()
            .is_some_and(|app| app != account.id)
    {
        return None;
    }
    let data = d["data"]["resolved"]["button_data"].as_str()?;
    let mut parts = data.split(':');
    if parts.next()? != "nl2sh" {
        return None;
    }
    let id = parts.next()?;
    uuid::Uuid::parse_str(id).ok()?;
    let mode = parts.next()?;
    if parts.next().is_some() || !matches!(mode, "once" | "session" | "always" | "deny") {
        return None;
    }
    let group = match d["scene"].as_str()? {
        "c2c" => false,
        "group" => true,
        _ => return None,
    };
    let sender = d[if group {
        "group_member_openid"
    } else {
        "user_openid"
    }]
    .as_str()?;
    let peer = if group {
        d["group_openid"].as_str()?
    } else {
        sender
    };
    if sender.is_empty() || peer.is_empty() {
        return None;
    }
    let text = match mode {
        "deny" => format!("/deny {id}"),
        "once" => format!("/approve {id}"),
        _ => format!("/approve {id} {mode}"),
    };
    Some((
        Incoming {
            route: Route {
                kind: "qq".into(),
                account: account.id.clone(),
                peer: peer.into(),
                sender: sender.into(),
                group,
            },
            id: d["id"].as_str()?.into(),
            text,
            context: String::new(),
            media: Vec::new(),
        },
        id.into(),
        mode.into(),
    ))
}
fn parse(account: &Account, event: &Value) -> Option<Incoming> {
    let group = match event["t"].as_str()? {
        "C2C_MESSAGE_CREATE" => false,
        "GROUP_AT_MESSAGE_CREATE" => true,
        _ => return None,
    };
    let d = &event["d"];
    let sender = d["author"][if group {
        "member_openid"
    } else {
        "user_openid"
    }]
    .as_str()?
    .to_owned();
    let peer = if group {
        d["group_openid"].as_str()?.to_owned()
    } else {
        sender.clone()
    };
    if sender.is_empty() || peer.is_empty() {
        return None;
    }
    Some(Incoming {
        route: Route {
            kind: "qq".into(),
            account: account.id.clone(),
            peer,
            sender,
            group,
        },
        id: d["id"].as_str()?.into(),
        text: strip_mention(d["content"].as_str().unwrap_or_default()),
        context: String::new(),
        media: media::qq(d),
    })
}
fn strip_mention(text: &str) -> String {
    let text = text.trim();
    if text.starts_with("<@") {
        if let Some(end) = text.find('>') {
            let id = text[2..end].trim_start_matches('!');
            if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                return text[end + 1..].trim().into();
            }
        }
    }
    text.into()
}

pub(super) async fn receive(
    manager: Arc<ChannelManager>,
    qq: &Qq,
    queue: &mpsc::Sender<Incoming>,
    mut cancel: watch::Receiver<bool>,
) -> Result<()> {
    let account = &qq.account;
    let channel: &dyn MessageChannel = qq;
    let token = tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),r=qq.token()=>r?};
    let response = tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),r=qq.client.get(format!("{}/gateway",account.base_url)).header("Authorization",format!("QQBot {token}")).send()=>response_json(r?).await?};
    let gateway = response["url"].as_str().context("QQ gateway URL missing")?;
    let url = url::Url::parse(gateway)?;
    if url.scheme() != "wss"
        || !url.host_str().is_some_and(|h| h.ends_with(".qq.com"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("invalid QQ gateway URL");
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(1024 * 1024))
        .max_frame_size(Some(1024 * 1024));
    let (mut socket, _) = tokio::select! {_=cancelled(&mut cancel)=>return Ok(()),r=tokio::time::timeout(Duration::from_secs(30),tokio_tungstenite::connect_async_with_config(gateway,Some(config),false))=>r??};
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.tick().await;
    let resume = qq
        .gateway
        .lock()
        .map_err(|_| anyhow!("QQ gateway state unavailable"))?
        .clone();
    let mut seq = resume.seq;
    let mut awaiting_ack = false;
    let expires = qq
        .token
        .lock()
        .await
        .as_ref()
        .map(|(_, expires)| tokio::time::Instant::from_std(*expires))
        .unwrap_or_else(tokio::time::Instant::now);
    loop {
        tokio::select! {
            biased;
            _=cancelled(&mut cancel)=>{let _=socket.close(None).await;return Ok(());}
            _=tokio::time::sleep_until(expires)=>return Ok(()),
            _=heartbeat.tick()=>{
                if awaiting_ack{bail!("QQ heartbeat acknowledgement missing");}
                socket.send(Message::Text(json!({"op":1,"d":seq}).to_string().into())).await?;awaiting_ack=true;
            }
            incoming=socket.next()=>{
                let Some(incoming)=incoming else{return Ok(());};
                match incoming?{
                    Message::Text(text)=>{
                        let value:Value=serde_json::from_str(&text)?;if let Some(s)=value["s"].as_u64(){seq=Some(s);if let Ok(mut state)=qq.gateway.lock(){state.seq=seq;}}
                        match value["op"].as_u64(){
                            Some(10)=>{
                                let ms=value["d"]["heartbeat_interval"].as_u64().unwrap_or(30000).clamp(1000,120000);
                                heartbeat=tokio::time::interval_at(tokio::time::Instant::now()+Duration::from_millis(ms*8/10),Duration::from_millis(ms*8/10));
                                let auth=if let Some(session)=&resume.session {json!({"op":6,"d":{"token":format!("QQBot {token}"),"session_id":session,"seq":seq}})}else{json!({"op":2,"d":{"token":format!("QQBot {token}"),"intents":(1u64<<25)|(1u64<<26),"shard":[0,1],"properties":{"$os":std::env::consts::OS,"$browser":"nl2sh","$device":"nl2sh"}}})};
                                socket.send(Message::Text(auth.to_string().into())).await?;
                            }
                            Some(11)=>awaiting_ack=false,
                            Some(1)=>{socket.send(Message::Text(json!({"op":1,"d":seq}).to_string().into())).await?;},
                            Some(7)=>return Ok(()),
                            Some(9)=>{if value["d"]!=true{if let Ok(mut state)=qq.gateway.lock(){*state=GatewaySession::default();}}return Ok(());},
                            Some(0)=>{
                                if value["t"]=="READY"{if let Ok(mut state)=qq.gateway.lock(){state.session=value["d"]["session_id"].as_str().map(str::to_owned);}manager.status("qq","online",None);}
                                if value["t"]=="RESUMED"{manager.status("qq","online",None);}
                                if value["t"]=="INTERACTION_CREATE" {
                                    let _ = tokio::time::timeout(Duration::from_secs(2),qq.interaction(&manager,&value)).await;
                                } else if let Some(message)=parse(account,&value){manager.intake(message,queue,channel).await?;}
                            },_=>{}
                        }
                    }
                    Message::Ping(bytes)=>socket.send(Message::Pong(bytes)).await?,
                    Message::Close(frame)=>{if frame.is_some_and(|frame|matches!(u16::from(frame.code),4006|4007|4009)){if let Ok(mut state)=qq.gateway.lock(){*state=GatewaySession::default();}}return Ok(());},_=>{}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[test]
    fn group_mentions_preserve_commands_and_sender_scope() {
        let account = Account {
            id: "bot".into(),
            secret: "secret".into(),
            base_url: "https://api.bot.qq.com".into(),
            enabled: true,
        };
        let event = json!({"t":"GROUP_AT_MESSAGE_CREATE","d":{"id":"m1","group_openid":"g","author":{"member_openid":"alice"},"content":"<@!123> /approve request"}});
        let parsed = parse(&account, &event).expect("valid fixture");
        assert_eq!(parsed.text, "/approve request");
        assert_eq!(parsed.route.sender, "alice");
        assert_eq!(parsed.route.peer, "g");
        assert!(parsed.route.group);
        let mut other = event.clone();
        other["d"]["author"]["member_openid"] = json!("bob");
        assert_ne!(
            parsed.route.key(),
            parse(&account, &other).expect("valid fixture").route.key()
        );
        assert!(parse(&account, &json!({"t":"READY"})).is_none());
    }
    #[tokio::test]
    async fn qq_replies_use_latest_msg_id_and_unique_sequences() -> Result<()> {
        let server = MockServer::start().await;
        for seq in [1, 2] {
            Mock::given(method("POST"))
                .and(path("/v2/groups/group/messages"))
                .and(header("Authorization", "QQBot token"))
                .and(body_partial_json(
                    json!({"msg_type":0,"msg_id":"latest","msg_seq":seq,"content":"reply"}),
                ))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"sent"})))
                .expect(1)
                .mount(&server)
                .await;
        }
        let qq = Qq::new(Account {
            id: "bot".into(),
            secret: "secret".into(),
            base_url: server.uri(),
            enabled: true,
        });
        *qq.token.lock().await = Some((
            "token".into(),
            std::time::Instant::now() + Duration::from_secs(60),
        ));
        let message = Incoming {
            route: Route {
                kind: "qq".into(),
                account: "bot".into(),
                peer: "group".into(),
                sender: "alice".into(),
                group: true,
            },
            id: "old".into(),
            text: String::new(),
            context: String::new(),
            media: Vec::new(),
        };
        qq.observe(&Incoming {
            id: "latest".into(),
            route: message.route.clone(),
            text: String::new(),
            context: String::new(),
            media: Vec::new(),
        });
        qq.send(&message, "reply").await?;
        qq.send(&message, "reply").await?;
        Ok(())
    }
}

fn endpoint(qq: &Qq, message: &Incoming, endpoint: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(&qq.account.base_url)?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid QQ API URL"))?
        .extend([
            "v2",
            if message.route.group {
                "groups"
            } else {
                "users"
            },
            &message.route.peer,
            endpoint,
        ]);
    Ok(url)
}
async fn media_post(qq: &Qq, message: &Incoming, path: &str, body: Value) -> Result<Value> {
    let token = qq.token().await?;
    response_json(
        qq.client
            .post(endpoint(qq, message, path)?)
            .header("Authorization", format!("QQBot {token}"))
            .header("X-Union-Appid", &qq.account.id)
            .timeout(Duration::from_secs(120))
            .json(&body)
            .send()
            .await?,
    )
    .await
}
fn numeric(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn upload_url(qq: &Qq, raw: &str) -> Result<url::Url> {
    #[cfg(test)]
    if url::Url::parse(raw)?.origin() == url::Url::parse(&qq.account.base_url)?.origin() {
        return Ok(url::Url::parse(raw)?);
    }
    let _ = qq;
    let url = url::Url::parse(raw)?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !(host.ends_with(".myqcloud.com")
            || host.ends_with(".qcloud.com")
            || host.ends_with(".qq.com"))
    {
        bail!("invalid QQ upload CDN");
    }
    Ok(url)
}
async fn upload_and_send(
    qq: &Qq,
    message: &Incoming,
    name: &str,
    kind: &str,
    bytes: Vec<u8>,
    caption: &str,
) -> Result<()> {
    use md5::Md5;
    use sha1::Sha1;
    if bytes.is_empty() || bytes.len() > media::MAX_FILE_BYTES {
        bail!("invalid attachment size");
    }
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let file_type = match kind {
        "image" => 1,
        "video" if extension == "mp4" => 2,
        "audio" if matches!(extension.as_str(), "silk" | "mp3" | "wav" | "ogg") => 3,
        _ => 4,
    };
    let (bytes, md5, sha1, prefix) = tokio::task::spawn_blocking(move || {
        let md5 = format!("{:x}", Md5::digest(&bytes));
        let sha1 = format!("{:x}", Sha1::digest(&bytes));
        let prefix = format!("{:x}", Md5::digest(&bytes[..bytes.len().min(10002432)]));
        (bytes, md5, sha1, prefix)
    })
    .await?;
    let prepared = media_post(qq, message, "upload_prepare", json!({"file_type":file_type,"file_name":name,"file_size":bytes.len().to_string(),"md5":md5,"sha1":sha1,"md5_10m":prefix})).await?;
    let prepared = prepared
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(&prepared);
    let upload_id = prepared["upload_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .context("QQ upload ID missing")?;
    let block_size = numeric(&prepared["block_size"])
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0 && *n <= media::MAX_FILE_BYTES)
        .context("invalid QQ block size")?;
    let parts = prepared
        .get("parts")
        .or_else(|| prepared.get("part_list"))
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty() && p.len() <= 64)
        .context("QQ upload parts missing or excessive")?;
    let mut parts = parts
        .iter()
        .map(|p| {
            let index = numeric(
                p.get("index")
                    .or_else(|| p.get("part_index"))
                    .unwrap_or(&Value::Null),
            )
            .context("QQ part index missing")?;
            Ok((index, p))
        })
        .collect::<Result<Vec<_>>>()?;
    parts.sort_by_key(|(index, _)| *index);
    let first = parts[0].0;
    if first > 1 {
        bail!("invalid QQ first part index");
    }
    let mut offset = 0;
    for (position, (index, part)) in parts.iter().enumerate() {
        if *index != first + position as u64 || offset >= bytes.len() {
            bail!("invalid QQ part sequence");
        }
        let size = numeric(&part["block_size"])
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(block_size);
        if size == 0 || size > media::MAX_FILE_BYTES {
            bail!("invalid QQ part size");
        }
        let end = offset.saturating_add(size).min(bytes.len());
        let data = bytes[offset..end].to_vec();
        let raw_url = part["presigned_url"]
            .as_str()
            .or_else(|| part["url"].as_str())
            .context("QQ part URL missing")?;
        let url = upload_url(qq, raw_url)?;
        let size = data.len();
        let digest = format!("{:x}", Md5::digest(&data));
        let response = qq
            .client
            .put(url)
            .timeout(Duration::from_secs(120))
            .body(data)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        if !response.status().is_success() {
            bail!("QQ CDN part upload failed ({})", response.status().as_u16());
        }
        media_post(qq,message,"upload_part_finish",json!({"upload_id":upload_id,"part_index":index,"block_size":size.to_string(),"md5":digest})).await?;
        offset = end;
    }
    if offset != bytes.len() {
        bail!("QQ upload parts did not cover the file");
    }
    let uploaded = media_post(
        qq,
        message,
        "files",
        json!({"upload_id":upload_id,"srv_send_msg":false}),
    )
    .await?;
    let info = uploaded["file_info"]
        .as_str()
        .or_else(|| uploaded["data"]["file_info"].as_str())
        .filter(|s| !s.is_empty() && s.len() <= 65536)
        .context("QQ file_info missing")?;
    let anchor = qq
        .anchors
        .lock()
        .map_err(|_| anyhow!("QQ message anchor unavailable"))?
        .get(&message.route.key())
        .cloned()
        .unwrap_or_else(|| message.id.clone());
    let seq = {
        let mut seq = qq
            .seq
            .lock()
            .map_err(|_| anyhow!("QQ sequence unavailable"))?;
        if seq.len() > 2048 {
            seq.clear();
        }
        let n = seq.entry(anchor.clone()).or_default();
        *n += 1;
        *n
    };
    media_post(qq,message,"messages",json!({"msg_type":7,"media":{"file_info":info},"content":caption,"msg_id":anchor,"msg_seq":seq})).await?;
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
    async fn chunk_uploads_all_media_types_in_both_chat_scopes_with_zero_based_parts() -> Result<()>
    {
        for group in [false, true] {
            for (kind, name, file_type) in [
                ("image", "photo.png", 1),
                ("audio", "voice.wav", 3),
                ("video", "clip.mp4", 2),
                ("file", "report.zip", 4),
            ] {
                let server = MockServer::start().await;
                let scope = if group { "groups" } else { "users" };
                let prefix = format!("/v2/{scope}/peer");
                Mock::given(method("POST")).and(path(format!("{prefix}/upload_prepare"))).and(header("Authorization","QQBot token")).and(body_partial_json(json!({"file_type":file_type,"file_name":name,"file_size":"5"})))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"upload_id":"upload","block_size":"3","parts":[{"index":0,"block_size":"3","presigned_url":format!("{}/part0",server.uri())},{"index":1,"block_size":"3","presigned_url":format!("{}/part1",server.uri())}]}))).expect(1).mount(&server).await;
                for part in [0, 1] {
                    Mock::given(method("PUT"))
                        .and(path(format!("/part{part}")))
                        .respond_with(ResponseTemplate::new(200))
                        .expect(1)
                        .mount(&server)
                        .await;
                    Mock::given(method("POST")).and(path(format!("{prefix}/upload_part_finish"))).and(body_partial_json(json!({"upload_id":"upload","part_index":part,"block_size":if part==0{"3"}else{"2"}}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({}))).expect(1).mount(&server).await;
                }
                Mock::given(method("POST"))
                    .and(path(format!("{prefix}/files")))
                    .and(body_partial_json(
                        json!({"upload_id":"upload","srv_send_msg":false}),
                    ))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .set_body_json(json!({"file_info":"opaque-file"})),
                    )
                    .expect(1)
                    .mount(&server)
                    .await;
                Mock::given(method("POST")).and(path(format!("{prefix}/messages"))).and(body_partial_json(json!({"msg_type":7,"media":{"file_info":"opaque-file"},"content":"caption","msg_id":"latest","msg_seq":1}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"sent"}))).expect(1).mount(&server).await;
                let qq = Qq::new(Account {
                    id: "123".into(),
                    secret: "secret".into(),
                    base_url: server.uri(),
                    enabled: true,
                });
                *qq.token.lock().await = Some((
                    "token".into(),
                    std::time::Instant::now() + Duration::from_secs(3600),
                ));
                let message = Incoming {
                    route: Route {
                        kind: "qq".into(),
                        account: "123".into(),
                        peer: "peer".into(),
                        sender: "alice".into(),
                        group,
                    },
                    id: "latest".into(),
                    text: String::new(),
                    context: String::new(),
                    media: Vec::new(),
                };
                qq.send_media(&message, name, kind, b"bytes".to_vec(), "caption")
                    .await?;
                let requests = server
                    .received_requests()
                    .await
                    .context("requests unavailable")?;
                assert_eq!(
                    requests
                        .iter()
                        .find(|r| r.url.path() == "/part0")
                        .context("first part missing")?
                        .body,
                    b"byt"
                );
                assert_eq!(
                    requests
                        .iter()
                        .find(|r| r.url.path() == "/part1")
                        .context("second part missing")?
                        .body,
                    b"es"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn upload_urls_cannot_forward_bytes_to_untrusted_hosts() {
        let qq = Qq::new(Account {
            id: "123".into(),
            secret: "secret".into(),
            base_url: "https://api.bot.qq.com".into(),
            enabled: true,
        });
        assert!(upload_url(
            &qq,
            "https://bucket.cos.ap-shanghai.myqcloud.com/part?sign=secret"
        )
        .is_ok());
        for url in [
            "http://127.0.0.1/a",
            "https://bucket.myqcloud.com.evil.test/a",
            "https://user:pass@bucket.myqcloud.com/a",
        ] {
            assert!(upload_url(&qq, url).is_err());
        }
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::agent::{ConfirmationDecision, ConfirmationRequest, Confirmer};
    use wiremock::{
        matchers::{body_partial_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    async fn fixture(server: &MockServer) -> Qq {
        let qq = Qq::new(Account {
            id: "bot".into(),
            secret: "secret".into(),
            base_url: server.uri(),
            enabled: true,
        });
        *qq.token.lock().await = Some((
            "token".into(),
            std::time::Instant::now() + Duration::from_secs(60),
        ));
        qq
    }
    fn incoming(group: bool) -> Incoming {
        Incoming {
            route: Route {
                kind: "qq".into(),
                account: "bot".into(),
                peer: if group { "group" } else { "alice" }.into(),
                sender: "alice".into(),
                group,
            },
            id: "anchor".into(),
            text: String::new(),
            context: String::new(),
            media: Vec::new(),
        }
    }
    #[tokio::test]
    async fn native_markdown_and_command_buttons_use_correct_fields_in_both_scopes() -> Result<()> {
        for group in [false, true] {
            let server = MockServer::start().await;
            let prefix = if group { "groups/group" } else { "users/alice" };
            Mock::given(method("POST")).and(path(format!("/v2/{prefix}/messages"))).and(body_partial_json(json!({"msg_type":2,"markdown":{"content":"# report\n**ok**"},"keyboard":{"content":{"rows":[{"buttons":[{"action":{"type":2,"permission":{"type":2},"data":"inspect"}}]}]}},"msg_id":"anchor","msg_seq":1}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"sent"}))).expect(1).mount(&server).await;
            let qq = fixture(&server).await;
            let receipt = qq
                .send_formatted(
                    &incoming(group),
                    "# report\n**ok**",
                    "markdown",
                    &[outbound::MessageButton {
                        label: "Inspect".into(),
                        command: "inspect".into(),
                    }],
                )
                .await?;
            assert_eq!(receipt["message_ids"], json!(["sent"]));
            let requests = server
                .received_requests()
                .await
                .context("requests missing")?;
            let body: Value = serde_json::from_slice(&requests[0].body)?;
            assert!(body.get("content").is_none());
        }
        Ok(())
    }
    #[tokio::test]
    async fn approval_keyboard_and_callback_are_sender_scoped_once_only_and_acknowledged(
    ) -> Result<()> {
        for mode in ["once", "session", "always", "deny"] {
            let server = MockServer::start().await;
            let qq = fixture(&server).await;
            let (_dir, manager) = crate::channels::tests::manager().await?;
            let channel = Arc::new(crate::channels::tests::MockChannel::default());
            let (_cancel, rx) = watch::channel(false);
            let confirmer =
                approval::ChannelConfirmer::new(manager.clone(), channel, &incoming(true), rx);
            let task = tokio::spawn(async move {
                confirmer
                    .confirm(
                        &ConfirmationRequest::shell("touch callback"),
                        &crate::security::SecurityAssessment::from_policy(
                            crate::security::RiskLevel::Mutating,
                            vec![],
                            false,
                            "test".into(),
                            false,
                        ),
                    )
                    .await
            });
            let id = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if let Some(id) = manager
                        .pending
                        .lock()
                        .ok()
                        .and_then(|p| p.keys().next().cloned())
                    {
                        break id;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await?;
            let mut event = json!({"t":"INTERACTION_CREATE","d":{"type":11,"id":"event-wrong","application_id":"bot","scene":"group","group_openid":"group","group_member_openid":"bob","data":{"resolved":{"button_data":format!("nl2sh:{id}:{mode}")}}}});
            Mock::given(method("PUT"))
                .and(path("/interactions/event-wrong"))
                .and(body_partial_json(json!({"code":3})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
                .mount(&server)
                .await;
            qq.interaction(&manager, &event).await?;
            assert!(!task.is_finished());
            event["d"]["id"] = json!("event-valid");
            event["d"]["group_member_openid"] = json!("alice");
            Mock::given(method("PUT"))
                .and(path("/interactions/event-valid"))
                .and(body_partial_json(json!({"code":0})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
                .expect(1)
                .mount(&server)
                .await;
            qq.interaction(&manager, &event).await?;
            qq.interaction(&manager, &event).await?;
            let result = task.await??;
            assert_eq!(
                result,
                if mode == "deny" {
                    ConfirmationDecision::Reject
                } else {
                    ConfirmationDecision::Approve
                }
            );
            assert!(manager
                .pending
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .is_empty());
            assert_eq!(
                manager
                    .session_grants
                    .lock()
                    .map_err(|_| anyhow!("poisoned"))?
                    .len(),
                usize::from(mode == "session")
            );
            assert_eq!(
                manager.saved.lock().await.command_allowlist.len(),
                usize::from(mode == "always")
            );
            Mock::given(method("POST")).and(path("/v2/groups/group/messages")).and(body_partial_json(json!({"msg_type":2,"markdown":{"content":"审批"},"keyboard":{"content":{"rows":[{"buttons":[{"id":"once","render_data":{"label":"允许一次"},"action":{"type":1,"data":format!("nl2sh:{id}:once"),"permission":{"type":2}}},{"id":"session"},{"id":"always"},{"id":"deny"}]}]}}}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":"approval"}))).expect(1).mount(&server).await;
            qq.send_approval(
                &incoming(true),
                "审批",
                &id,
                true,
                crate::config::UiLanguage::ZhCn,
            )
            .await?;
            manager.shutdown().await;
        }
        Ok(())
    }
    #[test]
    fn callbacks_reject_other_app_scene_and_invalid_payloads() {
        let account = Account {
            id: "bot".into(),
            secret: String::new(),
            base_url: String::new(),
            enabled: true,
        };
        let mut event = json!({"type":11,"id":"event","application_id":"bot","scene":"c2c","user_openid":"alice","data":{"resolved":{"button_data":format!("nl2sh:{}:once",uuid::Uuid::new_v4())}}});
        assert!(interaction_decision(&account, &event).is_some());
        event["application_id"] = json!("other");
        assert!(interaction_decision(&account, &event).is_none());
        event["application_id"] = json!("bot");
        event["scene"] = json!("guild");
        assert!(interaction_decision(&account, &event).is_none());
        event["scene"] = json!("c2c");
        event["data"]["resolved"]["button_data"] = json!("nl2sh:invented:once");
        assert!(interaction_decision(&account, &event).is_none());
    }
}
