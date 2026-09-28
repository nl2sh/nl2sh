use crate::security::{shell::effects::Effects, RiskLevel};

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
    if matches!(
        name,
        "rm" | "mv"
            | "cp"
            | "chmod"
            | "chown"
            | "mkdir"
            | "rmdir"
            | "touch"
            | "ln"
            | "truncate"
            | "tee"
            | "kill"
            | "pkill"
    ) {
        effects.raise(
            RiskLevel::Mutating,
            "filesystem-command",
            "command may change files or processes",
        );
    }
    if name == "rm" {
        let recursive = args
            .iter()
            .any(|arg| short_flag(arg, 'r') || arg == "--recursive");
        let force = args
            .iter()
            .any(|arg| short_flag(arg, 'f') || arg == "--force");
        let critical = args.iter().any(|arg| is_critical_path(arg));
        if recursive && force && critical {
            effects.raise(
                RiskLevel::Critical,
                "delete-system",
                "recursive deletion of a critical path",
            );
        }
    }
    if name.starts_with("mkfs") && (name == "mkfs" || name.starts_with("mkfs.")) {
        effects.raise(
            RiskLevel::Critical,
            "filesystem-format",
            "filesystem formatting",
        );
    }
    if name == "dd" {
        effects.raise(
            RiskLevel::Mutating,
            "filesystem-copy",
            "raw data copy may write a target",
        );
        if args
            .iter()
            .any(|arg| arg.starts_with("of=/dev/block/") || arg.starts_with("of=/dev/"))
        {
            effects.raise(RiskLevel::Critical, "block-write", "raw block-device write");
        }
    }
    if name == "chmod"
        && args.iter().any(|arg| short_flag(arg, 'R'))
        && args.iter().any(|arg| arg == "777")
        && args.iter().any(|arg| arg == "/" || arg == "/*")
    {
        effects.raise(
            RiskLevel::Critical,
            "root-permissions",
            "recursive world-writable root",
        );
    }
    if name == "find"
        && args
            .iter()
            .any(|arg| arg == "-delete" || arg == "-exec" || arg == "-execdir")
    {
        effects.raise(
            RiskLevel::Mutating,
            "find-action",
            "find action may change files",
        );
    }
    if name == "sed" && args.iter().any(|arg| arg == "-i" || arg.starts_with("-i")) {
        effects.raise(RiskLevel::Mutating, "in-place-edit", "in-place file edit");
    }
    if name == "tee" && args.iter().any(|arg| is_device_target(arg)) {
        effects.raise(RiskLevel::Critical, "device-write", "device write target");
    }
    for arg in args {
        super::classify_target(arg, effects);
    }
}

pub(super) fn classify_redirect(target: &str, effects: &mut Effects) {
    if is_device_target(target) {
        effects.raise(
            RiskLevel::Critical,
            "device-redirect",
            "device write redirection",
        );
    }
}

fn short_flag(arg: &str, flag: char) -> bool {
    arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(flag)
}

fn is_critical_path(arg: &str) -> bool {
    if !arg.starts_with('/') {
        return false;
    }
    let mut parts = Vec::new();
    for component in arg.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    matches!(
        parts.as_slice(),
        [] | ["*"] | ["data"] | ["data", "*"] | ["system"] | ["system", "*"]
    )
}

fn is_device_target(arg: &str) -> bool {
    arg.starts_with("/dev/block/") || arg.starts_with("/dev/sd")
}
