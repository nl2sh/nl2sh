//! Message channels share routing, session isolation and the assessed Agent boundary.
//! Adapters own protocol I/O; TUI and Web use the same management API.
mod approval;
pub(crate) mod attachments;
mod control;
mod markdown;
pub(crate) mod media;
pub mod outbound;
mod qq;
pub(crate) mod qr;
mod store;
mod weixin;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};
use store::Store;
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    weixin: Option<Account>,
    qq: Option<Account>,
    qq_uin: String,
    command_allowlist: Vec<approval::ApprovalPattern>,
    seen: VecDeque<String>,
    cursor: String,
    generations: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    id: String,
    secret: String,
    base_url: String,
    enabled: bool,
}

/// Public channel state. Credentials and protocol context tokens are never exposed.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ChannelStatus {
    /// Protocol identifier: weixin or qq.
    pub kind: String,
    /// Bound bot identity.
    pub account: Option<String>,
    /// Whether this channel starts with the UI service.
    pub enabled: bool,
    /// stopped, connecting, binding, scanned, online, expired, or error.
    pub state: String,
    /// Safe, actionable error summary.
    pub error: Option<String>,
    /// Temporary WeChat QR URL, only during binding.
    pub qr_url: Option<String>,
    /// Official QQ bot number, used only to generate its invitation QR.
    #[serde(default)]
    pub robot_uin: Option<String>,
    /// QQ profile/add-to-group URL, distinct from WeChat login QR.
    #[serde(default)]
    pub invite_url: Option<String>,
    /// Number of received conversation messages in this runtime.
    pub received: u64,
    /// Number of completed replies in this runtime.
    pub sent: u64,
    /// Number of active one-shot approval/question requests.
    pub pending: usize,
}
/// Mutations available to owner interfaces; blank QQ secret preserves the saved secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChannelAction {
    /// Begin WeChat QR pairing.
    BindWeixin,
    /// Revoke one persisted approval by its displayed SHA-256 ID, without touching credentials.
    RevokePermanentApproval { id: String },
    /// Save official QQ Bot credentials and start the channel.
    ConfigureQq {
        app_id: String,
        secret: String,
        #[serde(default)]
        robot_uin: String,
    },
    /// Enable and connect a bound channel.
    Start { kind: String },
    /// Disable, cancel active work, and disconnect.
    Stop { kind: String },
    /// Disconnect and delete channel credentials.
    Forget { kind: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct Route {
    kind: String,
    account: String,
    peer: String,
    sender: String,
    group: bool,
}
impl Route {
    fn key(&self) -> String {
        let bytes = serde_json::to_vec(&(
            &self.kind,
            &self.account,
            &self.peer,
            &self.sender,
            self.group,
        ))
        .unwrap_or_default();
        format!("{:x}", Sha256::digest(bytes))
    }
}
#[derive(Clone)]
pub(super) struct Incoming {
    route: Route,
    id: String,
    text: String,
    context: String,
    media: Vec<media::RemoteMedia>,
}
#[async_trait]
pub(super) trait MessageChannel: Send + Sync {
    async fn send(&self, message: &Incoming, text: &str) -> Result<()>;
    async fn send_media(
        &self,
        _message: &Incoming,
        _name: &str,
        _kind: &str,
        _bytes: Vec<u8>,
        _caption: &str,
    ) -> Result<()> {
        bail!("this channel cannot send attachments");
    }
    async fn send_formatted(
        &self,
        message: &Incoming,
        text: &str,
        _format: &str,
        buttons: &[outbound::MessageButton],
    ) -> Result<Value> {
        if !buttons.is_empty() {
            bail!("this channel does not support buttons");
        }
        self.send(message, text).await?;
        Ok(json!({"status":"submitted"}))
    }
    async fn send_approval(
        &self,
        message: &Incoming,
        text: &str,
        _id: &str,
        _remember: bool,
        _language: crate::config::UiLanguage,
    ) -> Result<()> {
        self.send_formatted(message, text, "markdown", &[]).await?;
        Ok(())
    }
    fn observe(&self, _message: &Incoming) {}
    async fn receive(
        &self,
        manager: Arc<ChannelManager>,
        queue: &mpsc::Sender<Incoming>,
        cancel: watch::Receiver<bool>,
    ) -> Result<()>;
    fn secrets(&self) -> Vec<String>;
}
struct Job {
    cancel: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}
static MANAGERS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<ChannelManager>>>> = OnceLock::new();

/// One process owns each configuration's gateway; no extra daemon or public callback is needed.
pub struct ChannelManager {
    path: PathBuf,
    store: Option<Arc<Store>>,
    controller: AsyncMutex<Option<Job>>,
    saved: AsyncMutex<Saved>,
    media_cache: AsyncMutex<()>,
    statuses: Mutex<BTreeMap<String, ChannelStatus>>,
    jobs: AsyncMutex<BTreeMap<String, Job>>,
    actions: AsyncMutex<()>,
    pending: Mutex<BTreeMap<String, approval::Pending>>,
    targets: Mutex<BTreeMap<String, outbound::KnownTarget>>,
    adapters: Mutex<BTreeMap<String, Arc<dyn MessageChannel>>>,
    delivery_cancels: Mutex<BTreeMap<String, watch::Receiver<bool>>>,
    session_grants: Mutex<BTreeMap<String, ()>>,
    active: Mutex<BTreeMap<String, watch::Sender<bool>>>,
    closing: std::sync::atomic::AtomicBool,
}
impl ChannelManager {
    /// Start configured channels, claiming a private cross-process state lock.
    pub async fn start(path: PathBuf) -> Result<Arc<Self>> {
        let key = if path.exists() {
            std::fs::canonicalize(&path)?
        } else {
            path.clone()
        };
        if let Some(manager) = Self::lookup(&key) {
            return Ok(manager);
        }
        let store_path = path.clone();
        let (store, saved) = tokio::task::spawn_blocking(move || -> Result<_> {
            match Store::open(&store_path) {
                Ok(store) => {
                    let saved = store.load()?;
                    Ok((Some(store), saved))
                }
                Err(error) if error.is::<store::AlreadyManaged>() => Ok((None, Saved::default())),
                Err(error) => Err(error),
            }
        })
        .await??;
        let manager = Arc::new(Self {
            path,
            store: store.map(Arc::new),
            controller: AsyncMutex::new(None),
            saved: AsyncMutex::new(saved),
            media_cache: AsyncMutex::new(()),
            statuses: Mutex::new(BTreeMap::new()),
            jobs: AsyncMutex::new(BTreeMap::new()),
            actions: AsyncMutex::new(()),
            pending: Mutex::new(BTreeMap::new()),
            targets: Mutex::new(BTreeMap::new()),
            adapters: Mutex::new(BTreeMap::new()),
            delivery_cancels: Mutex::new(BTreeMap::new()),
            session_grants: Mutex::new(BTreeMap::new()),
            active: Mutex::new(BTreeMap::new()),
            closing: std::sync::atomic::AtomicBool::new(false),
        });
        MANAGERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| anyhow!("channel registry unavailable"))?
            .insert(key, Arc::downgrade(&manager));
        if manager.store.is_none() {
            manager.snapshot().await?;
            return Ok(manager);
        }
        control::start(&manager).await?;
        for kind in ["weixin", "qq"] {
            manager.status(kind, "stopped", None);
            if manager.account(kind).await.is_some_and(|a| a.enabled) {
                manager.launch(kind).await?;
            }
        }
        Ok(manager)
    }
    /// Find the manager attached to this process's UI service.
    pub fn lookup(path: &Path) -> Option<Arc<Self>> {
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        MANAGERS.get()?.lock().ok()?.get(&key)?.upgrade()
    }
    /// Read safe channel summaries including a temporary QR URL.
    pub async fn snapshot(&self) -> Result<Vec<ChannelStatus>> {
        if self.store.is_none() {
            return control::snapshot(&self.path).await;
        }
        let saved = self.saved.lock().await;
        let mut statuses = self
            .statuses
            .lock()
            .map_err(|_| anyhow!("channel status unavailable"))?
            .clone();
        for kind in ["weixin", "qq"] {
            let status = statuses.entry(kind.into()).or_default();
            status.kind = kind.into();
            let account = if kind == "weixin" {
                &saved.weixin
            } else {
                &saved.qq
            };
            status.account = account.as_ref().map(|a| a.id.clone());
            status.enabled = account.as_ref().is_some_and(|a| a.enabled);
            if kind == "qq" {
                status.robot_uin = (!saved.qq_uin.is_empty()).then(|| saved.qq_uin.clone());
                status.invite_url = account
                    .as_ref()
                    .and_then(|a| qr::invitation(&a.id, &saved.qq_uin));
            }
            status.pending = self
                .pending
                .lock()
                .map_err(|_| anyhow!("channel approval unavailable"))?
                .values()
                .filter(|p| p.route.kind == kind)
                .count();
        }
        Ok(statuses.into_values().collect())
    }
    pub(super) fn status(&self, kind: &str, state: &str, error: Option<&str>) {
        if let Ok(mut statuses) = self.statuses.lock() {
            let status = statuses.entry(kind.into()).or_default();
            status.kind = kind.into();
            status.state = state.into();
            status.error = error.map(str::to_owned);
            if !matches!(state, "binding" | "scanned") {
                status.qr_url = None;
            }
        }
    }
    async fn persist(&self, saved: &Saved) -> Result<()> {
        let store = self
            .store
            .clone()
            .context("channel state is owned by another process")?;
        let saved = saved.clone();
        tokio::task::spawn_blocking(move || store.save(&saved)).await??;
        Ok(())
    }
    async fn account(&self, kind: &str) -> Option<Account> {
        let saved = self.saved.lock().await;
        if kind == "weixin" {
            saved.weixin.clone()
        } else {
            saved.qq.clone()
        }
    }
    async fn language(&self) -> crate::config::UiLanguage {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::load_or_default_unvalidated(&path).map(|cfg| cfg.ui_language)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default()
    }
    async fn secrets(&self) -> Vec<String> {
        let saved = self.saved.lock().await;
        [saved.weixin.as_ref(), saved.qq.as_ref()]
            .into_iter()
            .flatten()
            .map(|a| a.secret.clone())
            .collect()
    }
    async fn redact(&self, text: &str) -> String {
        let mut secrets = self.secrets().await;
        let path = self.path.clone();
        if let Ok(Ok(cfg)) =
            tokio::task::spawn_blocking(move || crate::config::load_or_default_unvalidated(&path))
                .await
        {
            secrets.extend([
                cfg.api_key,
                cfg.protocol_token,
                cfg.ima_api_key,
                cfg.jev_api_key,
                cfg.proxy_password,
            ]);
        }
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets
            .iter()
            .filter(|s| !s.is_empty())
            .fold(text.to_owned(), |text, secret| {
                text.replace(secret, "[REDACTED]")
            })
    }
    /// Apply a channel management action immediately and persist it privately.
    pub async fn apply(self: &Arc<Self>, action: ChannelAction) -> Result<()> {
        if self.store.is_none() {
            return control::apply(&self.path, &action).await;
        }
        let manager = self.clone();
        tokio::spawn(async move { manager.apply_owned(action).await })
            .await
            .context("channel action task failed")?
    }
    async fn apply_owned(self: &Arc<Self>, action: ChannelAction) -> Result<()> {
        let _action = self.actions.lock().await;
        if self.closing.load(std::sync::atomic::Ordering::Acquire) {
            bail!("channel manager is stopping");
        }
        if let ChannelAction::RevokePermanentApproval { id } = &action {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("invalid permanent approval ID");
            }
            let mut saved = self.saved.lock().await;
            let mut next = saved.clone();
            next.command_allowlist.retain(|pattern| pattern.id() != *id);
            if next.command_allowlist.len() == saved.command_allowlist.len() {
                bail!("permanent approval not found");
            }
            self.persist(&next).await?;
            *saved = next;
            return Ok(());
        }
        let kind = match &action {
            ChannelAction::BindWeixin => "weixin",
            ChannelAction::RevokePermanentApproval { .. } => bail!("invalid channel action"),
            ChannelAction::ConfigureQq { .. } => "qq",
            ChannelAction::Start { kind }
            | ChannelAction::Stop { kind }
            | ChannelAction::Forget { kind } => kind.as_str(),
        }
        .to_owned();
        if !matches!(kind.as_str(), "weixin" | "qq") {
            bail!("unknown channel");
        }
        if let ChannelAction::ConfigureQq {
            app_id,
            secret,
            robot_uin,
        } = &action
        {
            if app_id.is_empty() || app_id.len() > 64 || !app_id.bytes().all(|b| b.is_ascii_digit())
            {
                bail!("QQ AppID must contain digits");
            }
            if !robot_uin.is_empty()
                && (robot_uin.len() > 20 || !robot_uin.bytes().all(|b| b.is_ascii_digit()))
            {
                bail!("QQ bot number must contain digits (maximum 20)");
            }
            let actual = if secret.is_empty() {
                self.account("qq")
                    .await
                    .filter(|a| a.id == *app_id)
                    .map(|a| a.secret)
                    .unwrap_or_default()
            } else {
                secret.clone()
            };
            if actual.is_empty() || actual.len() > 512 || actual.chars().any(char::is_control) {
                bail!("QQ AppSecret is required (maximum 512 characters)");
            }
        }
        if matches!(action, ChannelAction::Start { .. }) && self.account(&kind).await.is_none() {
            bail!("bind this channel first");
        }
        self.stop_job(&kind).await;
        {
            let mut saved = self.saved.lock().await;
            if let ChannelAction::ConfigureQq {
                app_id, robot_uin, ..
            } = &action
            {
                if !robot_uin.is_empty() {
                    saved.qq_uin = robot_uin.clone();
                } else if saved.qq.as_ref().is_none_or(|a| a.id != *app_id) {
                    saved.qq_uin.clear();
                }
            }
            if matches!(&action,ChannelAction::Forget {kind} if kind=="qq") {
                saved.qq_uin.clear();
            }
            let account = if kind == "weixin" {
                &mut saved.weixin
            } else {
                &mut saved.qq
            };
            match &action {
                ChannelAction::RevokePermanentApproval { .. } => bail!("invalid channel action"),
                ChannelAction::ConfigureQq { app_id, secret, .. } => {
                    let secret = if secret.is_empty() {
                        account
                            .as_ref()
                            .map(|a| a.secret.clone())
                            .unwrap_or_default()
                    } else {
                        secret.clone()
                    };
                    *account = Some(Account {
                        id: app_id.clone(),
                        secret,
                        base_url: "https://api.bot.qq.com".into(),
                        enabled: true,
                    });
                }
                ChannelAction::Start { .. } => {
                    if let Some(a) = account {
                        a.enabled = true;
                    }
                }
                ChannelAction::Stop { .. } | ChannelAction::BindWeixin => {
                    if let Some(a) = account {
                        a.enabled = false;
                    }
                }
                ChannelAction::Forget { .. } => {
                    *account = None;
                    if kind == "weixin" {
                        saved.cursor.clear();
                    }
                }
            }
            self.persist(&saved).await?;
        }
        self.status(&kind, "stopped", None);
        if matches!(action, ChannelAction::BindWeixin) {
            let (cancel, rx) = watch::channel(false);
            let manager = self.clone();
            self.status("weixin", "binding", None);
            let task = tokio::spawn(async move {
                if weixin::bind(manager.clone(), rx).await.is_err() {
                    manager.status("weixin", "error", Some("微信绑定失败，请重新获取二维码"));
                }
            });
            self.jobs.lock().await.insert(kind, Job { cancel, task });
        } else if matches!(
            action,
            ChannelAction::Start { .. } | ChannelAction::ConfigureQq { .. }
        ) {
            self.launch(&kind).await?;
        }
        Ok(())
    }
    async fn launch(self: &Arc<Self>, kind: &str) -> Result<()> {
        let account = self.account(kind).await.context("channel is not bound")?;
        let (cancel, rx) = watch::channel(false);
        let manager = self.clone();
        let key = kind.to_owned();
        self.status(kind, "connecting", None);
        let task = tokio::spawn(async move {
            run_adapter(manager, account, &key, rx).await;
        });
        self.jobs
            .lock()
            .await
            .insert(kind.into(), Job { cancel, task });
        Ok(())
    }
    async fn stop_job(&self, kind: &str) {
        if let Some(job) = self.jobs.lock().await.remove(kind) {
            job.cancel.send_replace(true);
            let _ = job.task.await;
        }
        if let Ok(mut targets) = self.targets.lock() {
            targets.retain(|_, t| t.info.kind != kind);
        }
        if let Ok(mut adapters) = self.adapters.lock() {
            adapters.remove(kind);
        }
        if let Ok(mut cancels) = self.delivery_cancels.lock() {
            cancels.remove(kind);
        }
        self.status(kind, "stopped", None);
    }
    /// Cancel diagnostics and approvals and wait for the adapters to drain.
    pub async fn shutdown(&self) {
        if self.store.is_none() {
            return;
        }
        self.closing
            .store(true, std::sync::atomic::Ordering::Release);
        let _action = self.actions.lock().await;
        for kind in ["weixin", "qq"] {
            self.stop_job(kind).await;
        }
        if let Some(job) = self.controller.lock().await.take() {
            job.cancel.send_replace(true);
            let _ = job.task.await;
        }
    }
    async fn admit(&self, message: &Incoming) -> Result<bool> {
        if message.id.is_empty()
            || message.id.len() > 512
            || message.text.len() > 8192
            || (message.text.trim().is_empty() && message.media.is_empty())
            || message.media.len() > 8
        {
            return Ok(false);
        }
        let id = hex_digest(&format!("{}:{}", message.route.key(), message.id));
        let mut saved = self.saved.lock().await;
        if saved.seen.contains(&id) {
            return Ok(false);
        }
        saved.seen.push_back(id);
        while saved.seen.len() > 2048 {
            saved.seen.pop_front();
        }
        self.persist(&saved).await?;
        Ok(true)
    }
    async fn intake(
        &self,
        message: Incoming,
        queue: &mpsc::Sender<Incoming>,
        channel: &dyn MessageChannel,
    ) -> Result<()> {
        if message.media.len() > 8 || message.text.len() > 8192 {
            channel.send(&message,"消息过大：文字最多 8192 字节、附件最多 8 个。 / Message too large: maximum 8192 text bytes and 8 attachments.").await?;
            return Ok(());
        }
        if !self.admit(&message).await? {
            return Ok(());
        }
        channel.observe(&message);
        self.remember_target(&message)?;
        if message.media.is_empty() && approval::control(self, &message, channel).await? {
            return Ok(());
        }
        if message.media.is_empty() && message.text.trim() == "/cancel" {
            if let Some(tx) = self
                .active
                .lock()
                .map_err(|_| anyhow!("channel task unavailable"))?
                .get(&message.route.key())
            {
                tx.send_replace(true);
            }
            channel
                .send(&message, "已请求取消当前任务。 / Cancel requested.")
                .await?;
            return Ok(());
        }
        if message.media.is_empty()
            && message.text.trim().starts_with('/')
            && message.text.trim() != "/new"
        {
            let zh = self.language().await == crate::config::UiLanguage::ZhCn;
            channel.send_formatted(&message, if zh {
                "**渠道命令**：`/new`、`/cancel`、`/approve`、`/approve session`、`/approve always`、`/deny`、`/answer ID 内容`。审批命令需在待审批消息出现后单独回复；多个待处理请求需指定编号。"
            } else {
                "**Channel commands**: `/new`, `/cancel`, `/approve`, `/approve session`, `/approve always`, `/deny`, `/answer ID text`. Reply with an approval command after a pending prompt; multiple requests require an ID."
            }, "markdown", &[]).await?;
            return Ok(());
        }
        if let Ok(mut statuses) = self.statuses.lock() {
            if let Some(status) = statuses.get_mut(&message.route.kind) {
                status.received += 1;
            }
        }
        if let Err(error) = queue.try_send(message) {
            channel
                .send(
                    &error.into_inner(),
                    "渠道忙碌，请稍后重试。 / Channel busy, try again later.",
                )
                .await?;
        }
        Ok(())
    }
}
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    while !*cancel.borrow() {
        if cancel.changed().await.is_err() {
            break;
        }
    }
}
async fn run_adapter(
    manager: Arc<ChannelManager>,
    account: Account,
    kind: &str,
    mut cancel: watch::Receiver<bool>,
) {
    let (tx, rx) = mpsc::channel(32);
    let channel: Arc<dyn MessageChannel> = if kind == "weixin" {
        Arc::new(weixin::Weixin::new(account.clone()))
    } else {
        Arc::new(qq::Qq::new(account.clone()))
    };
    if let Ok(mut adapters) = manager.adapters.lock() {
        adapters.insert(kind.into(), channel.clone());
    }
    if let Ok(mut cancels) = manager.delivery_cancels.lock() {
        cancels.insert(kind.into(), cancel.clone());
    }
    let worker = tokio::spawn(worker(manager.clone(), channel.clone(), rx, cancel.clone()));
    let mut delay = 2;
    loop {
        let result = channel.receive(manager.clone(), &tx, cancel.clone()).await;
        if *cancel.borrow() {
            break;
        }
        manager.status(
            kind,
            "error",
            Some(if result.is_err() {
                "连接中断或凭据失效；正在重连，请检查账号与网络"
            } else {
                "连接已关闭；正在重连"
            }),
        );
        tokio::select! {_=cancelled(&mut cancel)=>break,_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
        delay = (delay * 2).min(60);
    }
    drop(tx);
    let _ = worker.await;
}
async fn worker(
    manager: Arc<ChannelManager>,
    channel: Arc<dyn MessageChannel>,
    mut queue: mpsc::Receiver<Incoming>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        let message = tokio::select! {biased;_=cancelled(&mut cancel)=>break,message=queue.recv()=>match message{Some(m)=>m,None=>break}};
        let key = message.route.key();
        if message.media.is_empty() && message.text.trim() == "/new" {
            let result = async {
                let mut saved = manager.saved.lock().await;
                if saved.generations.len() >= 512 && !saved.generations.contains_key(&key) {
                    bail!("channel session limit reached");
                }
                saved
                    .generations
                    .insert(key.clone(), uuid::Uuid::new_v4().to_string());
                manager.persist(&saved).await
            }
            .await;
            if result.is_ok() {
                if let Ok(mut allowlist) = manager.session_grants.lock() {
                    allowlist.remove(&key);
                }
            }
            let _ = channel
                .send(
                    &message,
                    if result.is_ok() {
                        "已开始新会话。 / New conversation."
                    } else {
                        "无法创建新会话。 / Session limit reached."
                    },
                )
                .await;
            continue;
        }
        let generation = manager
            .saved
            .lock()
            .await
            .generations
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let session = session_id(&key, &generation);
        let (task_cancel, task_rx) = watch::channel(false);
        if let Ok(mut active) = manager.active.lock() {
            active.insert(key.clone(), task_cancel.clone());
        }
        let mut secrets = manager.secrets().await;
        secrets.extend(channel.secrets());
        secrets.push(message.context.clone());
        for media in &message.media {
            secrets.push(media.url.clone());
            if let Some(key) = &media.key {
                secrets.push(key.clone());
            }
        }
        let task = async {
            let staged = media::stage(&manager, &message, &mut task_rx.clone()).await?;
            let confirmer = approval::ChannelConfirmer::new(
                manager.clone(),
                channel.clone(),
                &message,
                task_rx.clone(),
            )
            .with_images(staged.images);
            crate::protocol::execution::channel_ask(
                &manager.path,
                &session,
                &staged.text,
                task_rx,
                &confirmer,
                &secrets,
            )
            .await
        };
        tokio::pin!(task);
        let outcome = tokio::select! {result=&mut task=>result,_=cancelled(&mut cancel)=>{task_cancel.send_replace(true);task.await}};
        if let Ok(mut active) = manager.active.lock() {
            active.remove(&key);
        }
        let answer = match outcome {
            Ok(value) => value["answer"]
                .as_str()
                .unwrap_or("诊断结束，未返回文本。")
                .to_owned(),
            Err(error) => format!(
                "诊断未完成 / Diagnosis failed: {}",
                crate::limits::truncate_text(&format!("{error:#}"), 2048)
            ),
        };
        let mut answer = manager
            .redact(&crate::limits::truncate_text(&answer, 12000))
            .await;
        for secret in &secrets {
            if !secret.is_empty() {
                answer = answer.replace(secret, "[REDACTED]");
            }
        }
        if !*cancel.borrow() {
            if channel
                .send_formatted(&message, &answer, "markdown", &[])
                .await
                .is_ok()
            {
                if let Ok(mut statuses) = manager.statuses.lock() {
                    if let Some(status) = statuses.get_mut(&message.route.kind) {
                        status.sent += 1;
                    }
                }
            } else {
                manager.status(
                    &message.route.kind,
                    "error",
                    Some("诊断结果发送失败；任务不会自动重放"),
                );
            }
        }
    }
}
fn session_id(key: &str, generation: &str) -> String {
    let mut digest = hex_digest(&format!("{key}:{generation}"));
    digest.truncate(56);
    format!("channel-{digest}")
}
fn hex_digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.len() + ch.len_utf8() > max {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}
async fn response_json(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > 1024 * 1024 {
            bail!("channel response exceeds size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        bail!("channel HTTP status {}", status.as_u16());
    }
    if bytes.is_empty() {
        return Ok(serde_json::json!({}));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    for field in ["code", "ret", "errcode", "err_code", "biz_code"] {
        if let Some(code) = value.get(field).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        }) {
            if code != 0 {
                bail!("channel API rejected request ({field}={code})");
            }
        }
    }
    Ok(value)
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
