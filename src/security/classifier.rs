use super::{
    legacy::{detector, rules},
    shell::ShellAstAnalyzer,
    MatchedRule, RiskLevel, SecurityAssessment,
};
use crate::config::{Config, ConfirmPolicy, ExecuteUserMode, SecurityLevel};
use regex::Regex;

/// Classifies a raw command and applies configured confirmation policy.
pub fn assess(command: &str, config: &Config) -> SecurityAssessment {
    let normalized = detector::normalize(command);
    let effects = ShellAstAnalyzer::analyze(command);
    let mut risk = effects.risk;
    let mut matches = Vec::new();
    for (id, compiled, level, msg) in rules::builtins() {
        match compiled {
            Ok(re) if re.is_match(&normalized) => {
                risk = risk.max(level);
                matches.push(rules::matched(id, msg));
            }
            Err(_) => {
                risk = RiskLevel::Critical;
                matches.push(rules::matched(
                    id,
                    "internal security rule failed to compile",
                ));
            }
            Ok(_) => {}
        }
    }
    for rule in &config.security_rules {
        match Regex::new(&rule.pattern) {
            Ok(re) if re.is_match(&normalized) => {
                let level = parse_risk(&rule.risk);
                risk = risk.max(level);
                matches.push(MatchedRule {
                    id: rule.id.clone(),
                    message: rule.message.clone(),
                });
            }
            Err(_) => {
                risk = RiskLevel::Critical;
                matches.push(MatchedRule {
                    id: rule.id.clone(),
                    message: "custom security rule failed to compile".into(),
                });
            }
            Ok(_) => {}
        }
    }
    matches.extend(effects.rules);
    SecurityAssessment::from_policy(
        risk,
        matches,
        match config.execute_user_mode {
            ExecuteUserMode::Root => true,
            ExecuteUserMode::Normal => false,
            ExecuteUserMode::Auto => effects.requires_root,
        },
        format!("classified as {risk:?}"),
        matches!(config.security_level, SecurityLevel::Strict)
            || matches!(config.execute_confirm_policy, ConfirmPolicy::Always),
    )
}
fn parse_risk(s: &str) -> RiskLevel {
    match s.to_ascii_lowercase().as_str() {
        "readonly" | "read_only" => RiskLevel::ReadOnly,
        "mutating" => RiskLevel::Mutating,
        "dangerous" => RiskLevel::Dangerous,
        _ => RiskLevel::Critical,
    }
}
