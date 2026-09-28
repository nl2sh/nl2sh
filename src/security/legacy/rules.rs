//! Exceptional textual signatures that cannot be described reliably by one AST command.

use crate::security::{MatchedRule, RiskLevel};
use regex::Regex;

pub(in crate::security) fn builtins() -> Vec<(
    &'static str,
    Result<Regex, regex::Error>,
    RiskLevel,
    &'static str,
)> {
    [(
        "fork-bomb",
        r":\s*\(\s*\)\s*\{[^}]*:\s*\|\s*:\s*&[^}]*\}\s*;?\s*:",
        RiskLevel::Critical,
        "fork bomb",
    )]
    .into_iter()
    .map(|(id, pattern, risk, message)| (id, Regex::new(pattern), risk, message))
    .collect()
}

pub(in crate::security) fn matched(id: &str, message: &str) -> MatchedRule {
    MatchedRule {
        id: id.into(),
        message: message.into(),
    }
}
