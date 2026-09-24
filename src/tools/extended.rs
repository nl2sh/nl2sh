//! Adapters for the newer audio, Android, network, UI, and memory tools.

use super::{
    android::{
        diagnostics::{
            self as android_diagnostics, AndroidContentQueryArgs, AndroidDumpsysArgs,
            AndroidLogcatArgs, AndroidSettingsArgs, InspectAndroidAppArgs, ListAndroidAppsArgs,
            TopAndroidAppsArgs,
        },
        domain::{
            self as android_tools, AndroidInputArgs, ClipboardArgs, ConnectivityArgs,
            MediaControlArgs, MediaQueryArgs, PackageLimitArgs,
        },
    },
    audio::{
        domain::{AnalyzeAudioArgs, AudioAnalysisResult},
        quality::{judge_audio_quality, JudgeAudioQualityArgs},
    },
    memory::domain::{AgentMemory, AgentMemoryArgs},
    network::{
        domain::{
            self as web_tools, DownloadUrlArgs, HttpPostArgs, HttpRequestArgs, PreparedDownload,
        },
        tls::{self as tls_tools, TlsInspectArgs},
    },
    ui::domain::{
        self as ui_tools, CaptureAndroidScreenArgs, InspectAndroidUiArgs, ViewScreenshotArgs,
    },
};
use super::{
    definition, parse_args, PreparedExecution, PreparedToolCall, Tool, ToolCategory, ToolContext,
    ToolMetadata, ToolOutput, ToolRisk,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

macro_rules! meta {
    ($name:literal, $description:literal, $category:ident, $risk:ident) => {
        ToolMetadata {
            name: $name,
            description: $description,
            category: ToolCategory::$category,
            risk: ToolRisk::$risk,
            requires: &[],
            parallel_safe: false,
        }
    };
}

static METADATA: &[ToolMetadata] = &[
    meta!("analyze_audio", "Analyze a local WAV or raw PCM file using deterministic DSP. Missing raw PCM metadata is requested from the user, never guessed.", Audio, ReadOnly),
    meta!("judge_audio_quality", "Judge audio quality using the completed analysis cached in this task; cached features are authoritative.", Audio, ReadOnly),
    meta!("inspect_android_app", "Inspect bounded read-only evidence for an Android package or the foreground package.", Android, ReadOnly),
    meta!("list_android_apps", "List bounded installed Android applications with package, APK path, and UID.", Android, ReadOnly),
    meta!("top_android_apps", "Return a bounded Android process snapshot sorted by resident memory.", Android, ReadOnly),
    meta!("android_dumpsys", "Run one bounded, validated read-only Android dumpsys service query.", Android, ReadOnly),
    meta!("android_logcat", "Read a bounded Android logcat snapshot with an optional validated filter.", Android, ReadOnly),
    meta!("android_settings", "Read or list Android system, secure, or global settings; writes are unavailable.", Android, ReadOnly),
    meta!("android_content_query", "Run a bounded read-only query against a content URI; writes are unavailable.", Android, ReadOnly),
    meta!("http_request", "Perform a bounded GET or HEAD request to a public HTTP(S) URL without redirects or private targets.", Network, ReadOnly),
    meta!("download_url", "Download a bounded public HTTP(S) resource and atomically write it after confirmation.", Network, Mutating),
    meta!("inspect_android_ui", "Read the current Android UI hierarchy, focused window, display size, and density.", Android, ReadOnly),
    meta!("capture_android_screen", "Capture the current Android display as a PNG after local confirmation.", Android, Mutating),
    meta!("view_screenshot", "Attach an existing PNG, JPEG, or WebP image to the next model request with bounded in-process scaling.", Android, ReadOnly),
    meta!("inject_android_input", "Inject validated Android tap, swipe, long-press, or text after confirmation and bounds revalidation.", Android, Mutating),
    meta!("http_post", "Send a bounded JSON POST to a public HTTP(S) URL after confirmation.", Network, Mutating),
    meta!("android_notification", "Return a bounded structured Android notification snapshot.", Android, ReadOnly),
    meta!("android_crash_report", "Return bounded Android crash and ANR evidence.", Android, ReadOnly),
    meta!("android_thermal_power", "Aggregate bounded battery, thermal, power, and DeviceIdle state.", Android, ReadOnly),
    meta!("android_netstats", "Return bounded Android network accounting evidence.", Android, ReadOnly),
    meta!("android_storage", "Return filesystem usage and bounded per-app storage evidence.", Android, ReadOnly),
    meta!("android_wifi_eth", "Aggregate Wi-Fi, Ethernet, interface, IP, signal, and route evidence.", Android, ReadOnly),
    meta!("android_doze", "Return DeviceIdle state and whitelist evidence.", Android, ReadOnly),
    meta!("android_permission_audit", "Audit Android permissions and AppOps for a package or bounded app set.", Android, ReadOnly),
    meta!("android_clipboard", "Read clipboard text or, after confirmation, set bounded text.", Android, ReadOnly),
    meta!("android_media_control", "Read media status or, after confirmation, change playback or volume.", Android, ReadOnly),
    meta!("android_media_query", "Query bounded MediaStore image, video, or audio metadata.", Android, ReadOnly),
    meta!("agent_memory", "Read or update a small private task notebook; writes require confirmation.", Memory, ReadOnly),
    meta!("android_connectivity", "Aggregate bounded Android connectivity evidence for a validated public host.", Android, ReadOnly),
    meta!("inspect_tls", "Inspect and validate the TLS certificate chain of a public host.", Network, ReadOnly),
];

pub(super) fn builtin_tools() -> Vec<Box<dyn Tool>> {
    METADATA
        .iter()
        .map(|metadata| Box::new(ExtendedTool(metadata)) as Box<dyn Tool>)
        .collect()
}

struct ExtendedTool(&'static ToolMetadata);

#[async_trait]
impl Tool for ExtendedTool {
    fn metadata(&self) -> &'static ToolMetadata {
        self.0
    }

    fn definition(&self) -> crate::llm::ToolDefinition {
        let name = self.0.name;
        let description = self.0.description;
        match name {
            "analyze_audio" => definition::<AnalyzeAudioArgs>(name, description),
            "judge_audio_quality" => definition::<JudgeAudioQualityArgs>(name, description),
            "inspect_android_app" => definition::<InspectAndroidAppArgs>(name, description),
            "list_android_apps" => definition::<ListAndroidAppsArgs>(name, description),
            "top_android_apps" => definition::<TopAndroidAppsArgs>(name, description),
            "android_dumpsys" => definition::<AndroidDumpsysArgs>(name, description),
            "android_logcat" => definition::<AndroidLogcatArgs>(name, description),
            "android_settings" => definition::<AndroidSettingsArgs>(name, description),
            "android_content_query" => definition::<AndroidContentQueryArgs>(name, description),
            "http_request" => definition::<HttpRequestArgs>(name, description),
            "download_url" => definition::<DownloadUrlArgs>(name, description),
            "inspect_android_ui" => definition::<InspectAndroidUiArgs>(name, description),
            "capture_android_screen" => definition::<CaptureAndroidScreenArgs>(name, description),
            "view_screenshot" => definition::<ViewScreenshotArgs>(name, description),
            "inject_android_input" => definition::<AndroidInputArgs>(name, description),
            "http_post" => definition::<HttpPostArgs>(name, description),
            "android_notification"
            | "android_crash_report"
            | "android_netstats"
            | "android_storage"
            | "android_permission_audit" => definition::<PackageLimitArgs>(name, description),
            "android_thermal_power" | "android_wifi_eth" | "android_doze" => {
                definition::<EmptyArgs>(name, description)
            }
            "android_clipboard" => definition::<ClipboardArgs>(name, description),
            "android_media_control" => definition::<MediaControlArgs>(name, description),
            "android_media_query" => definition::<MediaQueryArgs>(name, description),
            "agent_memory" => definition::<AgentMemoryArgs>(name, description),
            "android_connectivity" => definition::<ConnectivityArgs>(name, description),
            "inspect_tls" => definition::<TlsInspectArgs>(name, description),
            _ => definition::<EmptyArgs>(name, description),
        }
    }

    async fn prepare(&self, ctx: &ToolContext<'_>, arguments: Value) -> Result<PreparedToolCall> {
        let name = self.0.name;
        let action = match name {
            "analyze_audio" => ExtendedAction::AnalyzeAudio(parse_args(name, arguments)?),
            "judge_audio_quality" => ExtendedAction::JudgeAudio(parse_args(name, arguments)?),
            "inspect_android_app" => ExtendedAction::InspectApp(parse_args(name, arguments)?),
            "list_android_apps" => ExtendedAction::ListApps(parse_args(name, arguments)?),
            "top_android_apps" => ExtendedAction::TopApps(parse_args(name, arguments)?),
            "android_dumpsys" => ExtendedAction::Dumpsys(parse_args(name, arguments)?),
            "android_logcat" => ExtendedAction::Logcat(parse_args(name, arguments)?),
            "android_settings" => ExtendedAction::Settings(parse_args(name, arguments)?),
            "android_content_query" => ExtendedAction::ContentQuery(parse_args(name, arguments)?),
            "http_request" => ExtendedAction::HttpRequest(parse_args(name, arguments)?),
            "inspect_android_ui" => ExtendedAction::InspectUi(parse_args(name, arguments)?),
            "view_screenshot" => ExtendedAction::ViewScreenshot(parse_args(name, arguments)?),
            "android_notification" => ExtendedAction::Notification(parse_args(name, arguments)?),
            "android_crash_report" => ExtendedAction::CrashReport(parse_args(name, arguments)?),
            "android_thermal_power" => {
                let _: EmptyArgs = parse_args(name, arguments)?;
                ExtendedAction::ThermalPower
            }
            "android_netstats" => ExtendedAction::Netstats(parse_args(name, arguments)?),
            "android_storage" => ExtendedAction::Storage(parse_args(name, arguments)?),
            "android_wifi_eth" => {
                let _: EmptyArgs = parse_args(name, arguments)?;
                ExtendedAction::WifiEth
            }
            "android_doze" => {
                let _: EmptyArgs = parse_args(name, arguments)?;
                ExtendedAction::Doze
            }
            "android_permission_audit" => {
                ExtendedAction::PermissionAudit(parse_args(name, arguments)?)
            }
            "android_media_query" => ExtendedAction::MediaQuery(parse_args(name, arguments)?),
            "android_connectivity" => ExtendedAction::Connectivity(parse_args(name, arguments)?),
            "inspect_tls" => ExtendedAction::InspectTls(parse_args(name, arguments)?),
            "download_url" => {
                let args: DownloadUrlArgs = parse_args(name, arguments)?;
                let prepared = web_tools::prepare_download(
                    ctx.config.context("tool config unavailable")?,
                    &args,
                )
                .await?;
                return Ok(PreparedToolCall::operation(
                    prepared.summary(),
                    Box::new(ExtendedAction::Download(prepared)),
                ));
            }
            "http_post" => {
                let args: HttpPostArgs = parse_args(name, arguments)?;
                let summary = web_tools::post_summary(&args)?;
                return Ok(PreparedToolCall::operation(
                    summary,
                    Box::new(ExtendedAction::HttpPost(args)),
                ));
            }
            "capture_android_screen" => {
                let args: CaptureAndroidScreenArgs = parse_args(name, arguments)?;
                let command = ui_tools::capture_command(&args.path)?;
                return Ok(PreparedToolCall::operation(
                    command,
                    Box::new(ExtendedAction::CaptureScreen(args)),
                ));
            }
            "inject_android_input" => {
                let args: AndroidInputArgs = parse_args(name, arguments)?;
                let initial = android_tools::prepare_input(
                    ctx.executor.context("tool executor unavailable")?,
                    &args,
                )
                .await?;
                return Ok(PreparedToolCall::operation(
                    format!("Validated UI input\n{initial}"),
                    Box::new(ExtendedAction::InjectInput(args)),
                ));
            }
            "android_clipboard" => {
                let args: ClipboardArgs = parse_args(name, arguments)?;
                if args.text.is_some() {
                    let command = android_tools::clipboard_write_command(&args)?;
                    return Ok(PreparedToolCall::operation_with_risk(
                        command.clone(),
                        Box::new(ExtendedAction::ClipboardWrite(command)),
                        ToolRisk::Mutating,
                    ));
                }
                ExtendedAction::ClipboardRead
            }
            "android_media_control" => {
                let args: MediaControlArgs = parse_args(name, arguments)?;
                if args.action != "status" {
                    let command = android_tools::media_control_command(&args)?;
                    return Ok(PreparedToolCall::operation_with_risk(
                        command.clone(),
                        Box::new(ExtendedAction::MediaControl(command)),
                        ToolRisk::Mutating,
                    ));
                }
                ExtendedAction::MediaStatus
            }
            "agent_memory" => {
                let args: AgentMemoryArgs = parse_args(name, arguments)?;
                if !matches!(args.action.as_str(), "get" | "list") {
                    let memory = AgentMemory::new(ctx.file_tools.base());
                    let summary = memory.mutation_summary(&args)?;
                    return Ok(PreparedToolCall::operation_with_risk(
                        summary,
                        Box::new(ExtendedAction::MemoryWrite(args)),
                        ToolRisk::Mutating,
                    ));
                }
                ExtendedAction::MemoryRead(args)
            }
            _ => bail!("unregistered tool {name}"),
        };
        Ok(PreparedToolCall::operation(String::new(), Box::new(action)))
    }
}

enum ExtendedAction {
    AnalyzeAudio(AnalyzeAudioArgs),
    JudgeAudio(JudgeAudioQualityArgs),
    InspectApp(InspectAndroidAppArgs),
    ListApps(ListAndroidAppsArgs),
    TopApps(TopAndroidAppsArgs),
    Dumpsys(AndroidDumpsysArgs),
    Logcat(AndroidLogcatArgs),
    Settings(AndroidSettingsArgs),
    ContentQuery(AndroidContentQueryArgs),
    HttpRequest(HttpRequestArgs),
    Download(PreparedDownload),
    InspectUi(InspectAndroidUiArgs),
    CaptureScreen(CaptureAndroidScreenArgs),
    ViewScreenshot(ViewScreenshotArgs),
    InjectInput(AndroidInputArgs),
    HttpPost(HttpPostArgs),
    Notification(PackageLimitArgs),
    CrashReport(PackageLimitArgs),
    ThermalPower,
    Netstats(PackageLimitArgs),
    Storage(PackageLimitArgs),
    WifiEth,
    Doze,
    PermissionAudit(PackageLimitArgs),
    ClipboardRead,
    ClipboardWrite(String),
    MediaStatus,
    MediaControl(String),
    MediaQuery(MediaQueryArgs),
    MemoryRead(AgentMemoryArgs),
    MemoryWrite(AgentMemoryArgs),
    Connectivity(ConnectivityArgs),
    InspectTls(TlsInspectArgs),
}

#[async_trait]
impl PreparedExecution for ExtendedAction {
    async fn execute(self: Box<Self>, ctx: &mut ToolContext<'_>) -> Result<ToolOutput> {
        let content = match *self {
            Self::AnalyzeAudio(mut args) => {
                let worker = ctx
                    .audio_tools
                    .context("audio tool executor unavailable")?
                    .clone();
                let path = args.path.clone();
                let worker_args = args.clone();
                let mut result = tokio::task::spawn_blocking(move || worker.analyze(&worker_args))
                    .await
                    .context("analyze_audio worker failed")??;
                if let AudioAnalysisResult::NeedsInput { missing, .. } = &result {
                    let language = ctx.config.context("tool config unavailable")?.ui_language;
                    let questions = crate::agent::audio_metadata_questions(missing, language);
                    let started = Instant::now();
                    let answers = ctx
                        .confirmer
                        .context("question interface unavailable")?
                        .ask_questions(&questions)
                        .await;
                    ctx.runtime
                        .as_deref_mut()
                        .context("tool runtime unavailable")?
                        .add_confirmation_time(started.elapsed());
                    if let Some(answers) = answers? {
                        crate::agent::apply_audio_answers(&mut args, &answers)?;
                        let worker = ctx
                            .audio_tools
                            .context("audio tool executor unavailable")?
                            .clone();
                        result = tokio::task::spawn_blocking(move || worker.analyze(&args))
                            .await
                            .context("analyze_audio retry worker failed")??;
                    }
                }
                let value = serde_json::to_value(&result)
                    .context("cannot serialize analyze_audio result")?;
                if matches!(result, AudioAnalysisResult::Ok { .. }) {
                    ctx.audio_cache
                        .as_deref_mut()
                        .context("audio cache unavailable")?
                        .insert(path, value.clone());
                }
                serde_json::to_string_pretty(&value)
                    .context("cannot serialize analyze_audio result")?
            }
            Self::JudgeAudio(mut args) => {
                let cache = ctx
                    .audio_cache
                    .as_deref()
                    .context("audio cache unavailable")?;
                let cached = args
                    .analysis_path
                    .as_ref()
                    .and_then(|path| cache.get(path))
                    .or_else(|| {
                        if cache.len() == 1 {
                            cache.values().next()
                        } else {
                            None
                        }
                    });
                if let Some(features) = cached {
                    args.features = Some(features.clone());
                } else if args.features.is_none() {
                    bail!("no completed analyze_audio result is available; call analyze_audio first and pass its exact path as analysis_path")
                }
                let judgment = judge_audio_quality(
                    ctx.config.context("tool config unavailable")?,
                    ctx.llm.context("LLM unavailable")?,
                    &args,
                )
                .await?;
                serde_json::to_string_pretty(&judgment)
                    .context("cannot serialize judge_audio_quality result")?
            }
            Self::InspectApp(args) => {
                android_diagnostics::inspect_android_app(executor(ctx)?, &args).await?
            }
            Self::ListApps(args) => {
                android_diagnostics::list_android_apps(executor(ctx)?, &args).await?
            }
            Self::TopApps(args) => {
                android_diagnostics::top_android_apps(executor(ctx)?, &args).await?
            }
            Self::Dumpsys(args) => {
                android_diagnostics::android_dumpsys(executor(ctx)?, &args).await?
            }
            Self::Logcat(args) => {
                android_diagnostics::android_logcat(executor(ctx)?, &args).await?
            }
            Self::Settings(args) => {
                android_diagnostics::android_settings(executor(ctx)?, &args).await?
            }
            Self::ContentQuery(args) => {
                android_diagnostics::android_content_query(executor(ctx)?, &args).await?
            }
            Self::HttpRequest(args) => {
                web_tools::http_request(ctx.config.context("tool config unavailable")?, &args)
                    .await?
            }
            Self::Download(prepared) => {
                tokio::task::spawn_blocking(move || prepared.apply())
                    .await
                    .context("download write worker failed")??;
                "Download written atomically after user confirmation.".into()
            }
            Self::InspectUi(args) => ui_tools::inspect_android_ui(executor(ctx)?, &args).await?,
            Self::CaptureScreen(args) => {
                let command = ui_tools::capture_command(&args.path)?;
                checked_command(ctx, &command).await?;
                format!("Screenshot written to {} after confirmation.", args.path)
            }
            Self::ViewScreenshot(args) => {
                let viewed = tokio::task::spawn_blocking(move || ui_tools::view_screenshot(&args))
                    .await
                    .context("view_screenshot worker failed")??;
                let mut output = ToolOutput::success(viewed.summary);
                output.attachments.push(viewed.attachment);
                return Ok(output);
            }
            Self::InjectInput(args) => {
                let command = android_tools::prepare_input(executor(ctx)?, &args).await?;
                checked_command(ctx, &command).await?;
                tokio::time::sleep(Duration::from_millis(500)).await;
                let state =
                    ui_tools::inspect_android_ui(executor(ctx)?, &InspectAndroidUiArgs::default())
                        .await?;
                format!("Android input injected after current-bounds revalidation and confirmation.\nPost-action UI state:\n{state}")
            }
            Self::HttpPost(args) => {
                web_tools::http_post(ctx.config.context("tool config unavailable")?, &args).await?
            }
            Self::Notification(args) => android_tools::notification(executor(ctx)?, &args).await?,
            Self::CrashReport(args) => android_tools::crash_report(executor(ctx)?, &args).await?,
            Self::ThermalPower => android_tools::thermal_power(executor(ctx)?).await?,
            Self::Netstats(args) => android_tools::netstats(executor(ctx)?, &args).await?,
            Self::Storage(args) => android_tools::storage(executor(ctx)?, &args).await?,
            Self::WifiEth => android_tools::wifi_eth(executor(ctx)?).await?,
            Self::Doze => android_tools::doze(executor(ctx)?).await?,
            Self::PermissionAudit(args) => {
                android_tools::permission_audit(executor(ctx)?, &args).await?
            }
            Self::ClipboardRead => android_tools::clipboard_read(executor(ctx)?).await?,
            Self::ClipboardWrite(command) => {
                checked_command(ctx, &command).await?;
                "Clipboard updated after confirmation.".into()
            }
            Self::MediaStatus => android_tools::media_status(executor(ctx)?).await?,
            Self::MediaControl(command) => {
                checked_command(ctx, &command).await?;
                "Media action completed after confirmation.".into()
            }
            Self::MediaQuery(args) => android_tools::media_query(executor(ctx)?, &args).await?,
            Self::MemoryRead(args) => AgentMemory::new(ctx.file_tools.base()).read(&args)?,
            Self::MemoryWrite(args) => {
                let memory = AgentMemory::new(ctx.file_tools.base());
                tokio::task::spawn_blocking(move || memory.apply(&args))
                    .await
                    .context("agent_memory worker failed")??
            }
            Self::Connectivity(args) => android_tools::connectivity(executor(ctx)?, &args).await?,
            Self::InspectTls(args) => {
                tls_tools::inspect_tls(
                    &args,
                    ctx.config
                        .context("tool config unavailable")?
                        .llm_request_timeout_secs,
                )
                .await?
            }
        };
        Ok(ToolOutput::success(content))
    }
}

fn executor<'a>(ctx: &'a ToolContext<'_>) -> Result<&'a dyn crate::shell::CommandExecutor> {
    ctx.executor.context("tool executor unavailable")
}

async fn checked_command(ctx: &ToolContext<'_>, command: &str) -> Result<()> {
    let result = executor(ctx)?.execute(command, false, false).await?;
    if result.exit_code != Some(0) || result.timed_out || result.interrupted {
        bail!(
            "confirmed command failed: exit={:?} stderr={}",
            result.exit_code,
            result.stderr
        )
    }
    Ok(())
}
