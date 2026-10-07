//! Reviewed query forms shared by typed diagnostics and shell risk classification.

/// Recognizes observational dumpsys arguments; unknown forms need shell approval.
pub(crate) fn readonly_dumpsys(service: &str, arguments: &[&str]) -> bool {
    if arguments.is_empty() {
        return true;
    }
    let selector = |value: &str| {
        !value.is_empty()
            && !value.starts_with('-')
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '$')
            })
    };
    let flags = |values: &[&str]| {
        values
            .iter()
            .all(|value| matches!(*value, "-a" | "-h" | "--proto"))
    };
    match service {
        "package" => arguments.len() == 1 && selector(arguments[0]),
        "meminfo" => arguments.iter().all(|value| {
            matches!(*value, "-a" | "-d" | "--oom" | "--local" | "--checkin") || selector(value)
        }),
        "activity" => {
            matches!(
                arguments[0],
                "activities"
                    | "processes"
                    | "services"
                    | "broadcasts"
                    | "providers"
                    | "recents"
                    | "lastanr"
                    | "top"
                    | "settings"
                    | "oom"
                    | "lru"
            ) && arguments.len() <= 2
                && arguments.get(1).is_none_or(|value| selector(value))
        }
        "window" => {
            arguments.len() == 1
                && matches!(
                    arguments[0],
                    "windows"
                        | "displays"
                        | "policy"
                        | "animator"
                        | "sessions"
                        | "tokens"
                        | "all"
                        | "visible"
                        | "-a"
                        | "-h"
                        | "--proto"
                )
        }
        "batterystats" => arguments.iter().all(|value| {
            matches!(
                *value,
                "-a" | "-h" | "--proto" | "--history" | "--charged" | "--daily" | "--usage"
            )
        }),
        _ => flags(arguments),
    }
}

/// Filter rules are data rather than getopt options.
pub(crate) fn readonly_log_filter(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && value.split_whitespace().all(|rule| {
            let (tag, priority) = rule
                .split_once(':')
                .map_or((rule, None), |(tag, priority)| (tag, Some(priority)));
            !tag.is_empty()
                && !tag.starts_with('-')
                && tag.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '.' | '_' | '$' | '-' | '*')
                })
                && priority.is_none_or(|priority| {
                    matches!(
                        priority,
                        "V" | "D"
                            | "I"
                            | "W"
                            | "E"
                            | "F"
                            | "S"
                            | "v"
                            | "d"
                            | "i"
                            | "w"
                            | "e"
                            | "f"
                            | "s"
                    )
                })
        })
}

/// Recognizes read-only logcat options; file output, buffer changes and unknown flags escalate.
pub(crate) fn readonly_logcat(arguments: &[String]) -> bool {
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        match argument.as_str() {
            "-d" | "-g" | "-s" | "-h" | "--help" | "-D" | "--dividers" => index += 1,
            "-t" | "-T" | "-b" | "-v" | "--pid" | "--uid" | "-m" | "--max-count" => {
                if arguments.get(index + 1).is_none() {
                    return false;
                }
                index += 2;
            }
            value
                if value.starts_with("--pid=")
                    || value.starts_with("--uid=")
                    || value.starts_with("--max-count=") =>
            {
                index += 1
            }
            value if !value.starts_with('-') && readonly_log_filter(value) => index += 1,
            _ => return false,
        }
    }
    true
}
