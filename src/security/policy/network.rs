use crate::security::{shell::effects::Effects, RiskLevel};

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
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
