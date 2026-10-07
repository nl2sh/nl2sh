//! Read-only capability discovery. A snapshot never grants execution authority.

use crate::{
    config::Config,
    shell::{CommandExecutor, RootProbe, SystemRootProbe},
    tools::{
        android::companion::{self, BridgeCapabilities},
        Capability, ToolCategory, ToolMetadata,
    },
};
use serde::Serialize;
use std::os::unix::fs::PermissionsExt;

/// Environment and independently discovered optional runtime adapters.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeCapabilities {
    /// Native Android userspace, including Termux.
    pub android: bool,
    /// Android UI commands can run under the current UID without prompting for root.
    pub android_shell: bool,
    /// Current process already has UID zero. This does not approve any action.
    pub root: bool,
    /// Effective UID used for unprivileged probes.
    pub uid: u32,
    /// Negotiated version and current service flags, absent for unavailable/legacy providers.
    pub bridge: Option<BridgeCapabilities>,
    /// A configured executable successfully reported a Tailcat version.
    pub tailcat: Option<String>,
    /// ART can acquire a validated helper after approval; this is not installed readiness.
    pub jadx_provisionable: bool,
    /// Installed helper with a supported runtime protocol; unavailable/legacy helpers are absent.
    pub jadx: Option<crate::runtime_dependencies::jadx::JadxInfo>,
    /// A fully configured ima connector is available.
    pub ima: bool,
}

impl RuntimeCapabilities {
    /// Discover without installing, modifying settings, authorizing root, or downloading assets.
    pub async fn discover(config: &Config, executor: &dyn CommandExecutor) -> Self {
        let uid = SystemRootProbe.uid();
        let android = executor.is_android();
        let android_shell = android && matches!(uid, 0 | 2000);
        let bridge = if android_shell {
            companion::capabilities(executor).await.ok().flatten()
        } else {
            None
        };
        let path = &config.tailcat_binary_path;
        let executable = tokio::fs::metadata(path)
            .await
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
        let tailcat = if executable {
            let command = format!("{} version", shell_quote(&path.to_string_lossy()));
            executor
                .execute_probe(&command)
                .await
                .ok()
                .filter(|result| {
                    result.exit_code == Some(0) && !result.timed_out && !result.interrupted
                })
                .map(|result| result.stdout.trim().to_owned())
                .filter(|version| !version.is_empty() && version.len() <= 256)
        } else {
            None
        };
        let jadx = crate::runtime_dependencies::jadx::installed_info(executor)
            .await
            .ok()
            .flatten();
        Self {
            android,
            android_shell,
            root: uid == 0,
            uid,
            bridge,
            tailcat,
            jadx,
            jadx_provisionable: android && crate::runtime_dependencies::jadx::provisionable(),
            ima: crate::ima::ImaClient::from_config(config)
                .ok()
                .flatten()
                .is_some(),
        }
    }

    /// Model/direct-call availability, separate from the unchanged local security policy.
    pub(crate) fn supports(&self, metadata: &ToolMetadata) -> bool {
        let name = metadata.name;
        if name == "decompile_apk_class" {
            return self.jadx_provisionable;
        }
        if name == "tailcat_install" {
            return cfg!(any(target_os = "android", target_os = "linux"));
        }
        if name == "tailcat_adb_pair" {
            return self.android_shell;
        }
        if matches!(
            name,
            "tailcat_receive" | "tailcat_receive_stream" | "tailcat_send_file" | "tailcat_serve"
        ) {
            return self.tailcat.is_some();
        }
        if metadata.category == ToolCategory::Android && name != "view_screenshot" {
            if name.starts_with("android.")
                || matches!(
                    name,
                    "inject_android_input" | "capture_android_screen" | "inspect_android_ui"
                )
            {
                return self.android_shell;
            }
            return self.android;
        }
        true
    }

    pub(crate) fn configured(&self) -> Vec<Capability> {
        if self.ima {
            vec![Capability::Ima]
        } else {
            Vec::new()
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;

    #[test]
    fn unavailable_execution_tools_are_filtered_but_recovery_and_static_analysis_remain() {
        let runtime = RuntimeCapabilities {
            android: false,
            android_shell: false,
            root: true,
            uid: 0,
            bridge: None,
            tailcat: None,
            jadx: None,
            jadx_provisionable: false,
            ima: false,
        };
        let mut config = Config::default();
        config.tool_groups.insert("jadx".into(), true);
        config.tool_groups.insert("tailcat".into(), true);
        let registry = ToolRegistry::for_runtime(&config, &runtime);
        for name in [
            "inspect_apk",
            "list_dex_classes",
            "view_screenshot",
            "tailcat_check",
            "tailcat_status",
        ] {
            assert!(registry.get(name).is_some(), "{name}");
        }
        for name in [
            "android.tap",
            "android.screen_dump",
            "decompile_apk_class",
            "tailcat_send_file",
            "tailcat_serve",
        ] {
            assert!(registry.get(name).is_none(), "{name}");
        }
        let android = RuntimeCapabilities {
            android: true,
            android_shell: true,
            jadx_provisionable: true,
            ..runtime
        };
        let registry = ToolRegistry::for_runtime(&config, &android);
        assert!(registry.get("android.screen_dump").is_some()); // shell fallback without Bridge
        assert!(registry.get("tailcat_install").is_some());
        assert!(registry.get("decompile_apk_class").is_some()); // acquisition still requires approval
        let termux = RuntimeCapabilities {
            android_shell: false,
            uid: 10001,
            root: false,
            ..android
        };
        let registry = ToolRegistry::for_runtime(&config, &termux);
        assert!(registry.get("android.tap").is_none());
        assert!(registry.get("inspect_android_environment").is_some());
    }
}
