use crate::security::{shell::effects::Effects, RiskLevel};

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
    if name == "tailcat" {
        let action = args
            .iter()
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str);
        let risk = match action {
            Some("version" | "help" | "ping" | "ls") => RiskLevel::ReadOnly,
            Some("recv") | None
                if !args
                    .iter()
                    .any(|arg| arg.starts_with("--serve") || arg.starts_with("--files")) =>
            {
                RiskLevel::Mutating
            }
            _ => RiskLevel::Dangerous,
        };
        if risk > RiskLevel::ReadOnly {
            effects.raise(
                risk,
                "tailcat-network",
                "Tailcat may receive files, send data, or expose a service",
            );
        }
    }
    match name {
        "curl"
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-o" | "--output"
                        | "-O"
                        | "--remote-name"
                        | "-d"
                        | "--data"
                        | "-F"
                        | "--form"
                        | "-T"
                        | "--upload-file"
                        | "--request"
                        | "-X"
                )
            }) =>
        {
            effects.raise(
                RiskLevel::Mutating,
                "network-transfer",
                "network request may write or upload data",
            );
        }
        "wget"
            if args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "-O" | "--output-document" | "--post-data" | "--post-file"
                )
            }) =>
        {
            effects.raise(
                RiskLevel::Mutating,
                "network-transfer",
                "network request may write or upload data",
            );
        }
        _ => {}
    }
}
