//! Root capability decisions and command-bound approval tokens.

use crate::{
    config::{Config, ExecuteUserMode},
    security::{assess, shell::effects::Effects, RiskLevel, SecurityAssessment},
};
use anyhow::{bail, Result};

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
    if name == "su" {
        effects.raise(
            RiskLevel::Mutating,
            "explicit-elevation",
            "explicit privilege elevation",
        );
        effects.require_root();
    }
    if name == "mount" && args.iter().any(|arg| arg.contains("remount")) {
        effects.require_root();
    }
}

pub(super) fn classify_target(target: &str, effects: &mut Effects) {
    if target.starts_with("/data/system")
        || target.starts_with("/dev/block/")
        || ((target == "/system" || target.starts_with("/system/"))
            && effects.risk >= RiskLevel::Mutating)
    {
        effects.require_root();
    }
}

/// A one-command capability bound to the locally assessed source and root decision.
pub struct ApprovedShellCommand {
    command: String,
    requires_root: bool,
}

impl ApprovedShellCommand {
    /// Returns the exact command that was assessed and approved.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Returns the approved privilege plan.
    pub fn requires_root(&self) -> bool {
        self.requires_root
    }
}

/// Issues command-bound execution capabilities after local assessment and approval.
pub struct PrivilegeBroker;

impl PrivilegeBroker {
    /// Reassesses source and issues a capability only when required approval occurred.
    pub fn authorize(
        command: &str,
        assessment: &SecurityAssessment,
        config: &Config,
        approved_command: Option<&str>,
    ) -> Result<ApprovedShellCommand> {
        let current = assess(command, config);
        if current.risk_level != assessment.risk_level
            || current.matched_rules != assessment.matched_rules
            || current.requires_root != assessment.requires_root
            || current.requires_confirmation != assessment.requires_confirmation
            || current.requires_double_confirmation != assessment.requires_double_confirmation
            || current.explanation != assessment.explanation
        {
            bail!("shell command or privilege assessment changed before execution");
        }
        if matches!(config.execute_user_mode, ExecuteUserMode::Normal)
            && current
                .matched_rules
                .iter()
                .any(|rule| rule.id == "explicit-elevation")
        {
            bail!("explicit su command is unavailable in normal-user mode");
        }
        if current.requires_confirmation {
            match approved_command {
                Some(approved) if approved == command => {}
                Some(_) => bail!("shell approval belongs to a different command"),
                None => bail!("shell execution requires approval"),
            }
        }
        Ok(ApprovedShellCommand {
            command: command.into(),
            requires_root: current.requires_root,
        })
    }
}
