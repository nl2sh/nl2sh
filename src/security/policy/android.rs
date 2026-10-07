use crate::security::{shell::effects::Effects, RiskLevel};

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
    match name {
        "dumpsys"
            if args.first().is_some_and(|service| {
                !crate::security::readonly_dumpsys(
                    service,
                    &args.iter().skip(1).map(String::as_str).collect::<Vec<_>>(),
                )
            }) =>
        {
            effects.raise(
                RiskLevel::Dangerous,
                "android-dump-options",
                "unreviewed dumpsys arguments may change Android state",
            )
        }
        "logcat" if !crate::security::readonly_logcat(args) => effects.raise(
            RiskLevel::Dangerous,
            "android-log-options",
            "unreviewed logcat options may clear logs, change buffers or write files",
        ),
        "reboot" | "shutdown" | "halt" | "poweroff" | "wipe" => {
            effects.raise(
                RiskLevel::Dangerous,
                "power",
                "device power or wipe operation",
            );
        }
        "setprop" | "svc" => effects.raise(
            RiskLevel::Mutating,
            "android-state-change",
            "Android system state change",
        ),
        "settings" if matches!(args.first().map(String::as_str), Some("put" | "delete")) => {
            effects.raise(
                RiskLevel::Mutating,
                "android-state-change",
                "Android settings change",
            );
        }
        "pm" if args.first().is_some_and(|arg| {
            matches!(
                arg.as_str(),
                "install" | "uninstall" | "clear" | "enable" | "disable" | "grant" | "revoke"
            )
        }) =>
        {
            effects.raise(
                RiskLevel::Mutating,
                "android-package-change",
                "Android package state change",
            );
        }
        "cmd"
            if args.first().is_some_and(|arg| arg == "package")
                && args.get(1).is_some_and(|arg| {
                    matches!(
                        arg.as_str(),
                        "install"
                            | "uninstall"
                            | "clear"
                            | "enable"
                            | "disable"
                            | "grant"
                            | "revoke"
                    )
                }) =>
        {
            effects.raise(
                RiskLevel::Mutating,
                "android-package-change",
                "Android package state change",
            );
        }
        "am" if args
            .first()
            .is_some_and(|arg| matches!(arg.as_str(), "start" | "force-stop" | "kill")) =>
        {
            effects.raise(
                RiskLevel::Mutating,
                "android-activity-change",
                "Android activity state change",
            );
        }
        "mount" if !args.is_empty() => {
            effects.raise(
                RiskLevel::Mutating,
                "mount-change",
                "mount operation with arguments",
            );
            if args
                .iter()
                .any(|arg| arg.contains("remount") && arg.contains("rw"))
                || args
                    .windows(2)
                    .any(|pair| pair[0] == "remount" && pair[1] == "rw")
            {
                effects.raise(RiskLevel::Dangerous, "remount-rw", "read-write remount");
            }
        }
        "fastboot" if args.first().is_some_and(|arg| arg == "erase") => {
            effects.raise(RiskLevel::Critical, "fastboot-erase", "partition erase");
        }
        _ => {}
    }
}
