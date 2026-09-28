//! Compatibility normalization for configured and exceptional regex rules only.

pub(in crate::security) fn normalize(command: &str) -> String {
    command
        .chars()
        .filter(|character| *character != '\\')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
