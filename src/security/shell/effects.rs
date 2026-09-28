use crate::security::{MatchedRule, RiskLevel};

pub(in crate::security) struct Effects {
    pub risk: RiskLevel,
    pub rules: Vec<MatchedRule>,
    pub requires_root: bool,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            risk: RiskLevel::ReadOnly,
            rules: Vec::new(),
            requires_root: false,
        }
    }
}

impl Effects {
    pub(in crate::security) fn raise(&mut self, risk: RiskLevel, id: &str, message: &str) {
        let highest_so_far = self.risk;
        self.risk = self.risk.max(risk);
        if !self.rules.iter().any(|rule| rule.id == id) {
            let matched = MatchedRule {
                id: id.into(),
                message: message.into(),
            };
            if risk > highest_so_far {
                self.rules.insert(0, matched);
            } else {
                self.rules.push(matched);
            }
        }
    }

    pub(in crate::security) fn require_root(&mut self) {
        self.requires_root = true;
    }
}
