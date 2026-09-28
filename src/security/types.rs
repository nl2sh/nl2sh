use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Ordered command risk classification.
pub enum RiskLevel {
    /// Expected not to change system state.
    ReadOnly,
    /// Changes files, processes, packages, or settings.
    Mutating,
    /// Can seriously disrupt the device and requires strong confirmation.
    Dangerous,
    /// Can destroy data, partitions, or the operating system.
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One security rule that matched the command.
pub struct MatchedRule {
    /// Stable rule identifier.
    pub id: String,
    /// User-facing explanation.
    pub message: String,
}

#[derive(Debug, Clone)]
/// Complete local security decision made before shell execution.
pub struct SecurityAssessment {
    /// Highest risk found across classifiers and rules.
    pub risk_level: RiskLevel,
    /// Every dangerous rule that matched.
    pub matched_rules: Vec<MatchedRule>,
    /// Whether an approval is required.
    pub requires_confirmation: bool,
    /// Whether a second exact confirmation is required.
    pub requires_double_confirmation: bool,
    /// Whether the selected execution plan will use root.
    pub requires_root: bool,
    /// Concise classification summary.
    pub explanation: String,
}

impl SecurityAssessment {
    /// Applies the common risk floor and confirmation policy to a prepared action.
    pub fn from_policy(
        risk_level: RiskLevel,
        matched_rules: Vec<MatchedRule>,
        requires_root: bool,
        explanation: String,
        confirm_read_only: bool,
    ) -> Self {
        let requires_double_confirmation = risk_level >= RiskLevel::Dangerous;
        let requires_confirmation =
            risk_level >= RiskLevel::Mutating || requires_root || confirm_read_only;
        Self {
            risk_level,
            matched_rules,
            requires_confirmation,
            requires_double_confirmation,
            requires_root,
            explanation,
        }
    }
}
