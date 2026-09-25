use super::{MatchedRule, RiskLevel};
use regex::Regex;

pub fn builtins() -> Vec<(
    &'static str,
    Result<Regex, regex::Error>,
    RiskLevel,
    &'static str,
)> {
    [
        ("delete-system", r#"(?i)(?:^|[;&|`]|\$\()\s*(?:su\s+-c\s+|sh\s+-c\s+)?['\"]?rm\s+[^\n;&|]*(?:-[^\s]*r[^\s]*f|-[^\s]*f[^\s]*r)[^\n;&|]*\s+['\"]?/(?:\*|data(?:/\*)?|system(?:/\*)?)?(?:\s|['\"]|\)|$)"#, RiskLevel::Critical, "recursive deletion of a critical path"),
        ("delete-system-split-flags", r#"(?i)(?:^|[;&|`]|\$\()\s*(?:(?:su|sh)\s+-c\s+['\"]?)?rm\s+(?:-[^\s;&|]*r[^\s;&|]*\s+[^;&|]*-[^\s;&|]*f|-[^\s;&|]*f[^\s;&|]*\s+[^;&|]*-[^\s;&|]*r)[^;&|]*\s+['\"]?/(?:\*|data(?:/\*)?|system(?:/\*)?)?(?:\s|['\"]|\)|$)"#, RiskLevel::Critical, "recursive deletion of a critical path"),
        ("filesystem-format", r"(?i)(?:^|[;&|`]|\$\()\s*mkfs(?:\.[a-z0-9]+)?\b", RiskLevel::Critical, "filesystem formatting"),
        ("block-write", r"(?i)\bdd\b[^\n;&|]*\bof\s*=\s*/dev/(?:block/)?", RiskLevel::Critical, "raw block-device write"),
        ("root-permissions", r"(?i)\bchmod\b[^\n;&|]*-R[^\n;&|]*777\s+/(?:\*|\s|$)", RiskLevel::Critical, "recursive world-writable root"),
        ("fork-bomb", r":\s*\(\s*\)\s*\{[^}]*:\s*\|\s*:\s*&[^}]*\}\s*;?\s*:", RiskLevel::Critical, "fork bomb"),
        ("power", r#"(?i)(?:^|[;&|`]|\$\()\s*(?:(?:su|sh)\s+-c\s+['\"]?)?(?:reboot|shutdown|halt|poweroff|wipe)\b"#, RiskLevel::Dangerous, "device power or wipe operation"),
        ("fastboot-erase", r"(?i)\bfastboot\s+erase\b", RiskLevel::Critical, "partition erase"),
        ("remount-rw", r"(?i)\bmount\b[^\n;&|]*-o\s+[^\n;&|]*remount[^\n;&|]*rw", RiskLevel::Dangerous, "read-write remount"),
        ("device-redirect", r"(?i)(?:>\s*|tee\s+)/dev/(?:sd\w*|block/)" , RiskLevel::Critical, "device write redirection"),
        ("android-state-change", r"(?i)\b(?:settings\s+(?:put|delete)|(?:pm|cmd\s+package)\s+(?:install|uninstall|clear|enable|disable|grant|revoke)|am\s+(?:start|force-stop|kill)|svc\s+)" , RiskLevel::Mutating, "Android service or package state change"),
        ("mount-change", r"(?i)\bmount\s+(?:-[^\s|;&]+|/[^\s|;&]+|[a-z][^\s|;&]*)" , RiskLevel::Mutating, "mount operation with arguments"),
        ("explicit-elevation", r"(?i)(?:^|[;&|`]|\$\()\s*su(?:\s|$)" , RiskLevel::Mutating, "explicit privilege elevation"),
    ].into_iter().map(|(id,p,r,m)|(id,Regex::new(p),r,m)).collect()
}

pub fn matched(id: &str, message: &str) -> MatchedRule {
    MatchedRule {
        id: id.into(),
        message: message.into(),
    }
}
