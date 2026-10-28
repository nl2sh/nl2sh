use crate::security::{RiskLevel, SecurityAssessment};
use anyhow::Result;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    io::{self, IsTerminal, Write},
};

/// One selectable answer shown for a structured user question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    /// Short label displayed in interactive interfaces.
    pub label: String,
    /// Machine-readable value returned to the Agent.
    pub value: String,
    /// Optional explanation of the choice.
    pub description: String,
}

/// One structured question that accepts a listed option or custom text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestion {
    /// Stable identifier used to associate the answer with a tool argument.
    pub id: String,
    /// Compact field heading.
    pub header: String,
    /// User-facing prompt.
    pub prompt: String,
    /// Suggested answers; interactive interfaces must also permit custom text.
    pub options: Vec<QuestionOption>,
}

/// Answers returned from a structured user-question interface.
pub type QuestionAnswers = BTreeMap<String, String>;
/// Stable media-delivery scope produced by trusted tool preparation.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaApprovalScope {
    /// Stable channel/account/conversation/sender destination, without runtime target IDs.
    pub destination: String,
    /// Resolved image, audio, video, or file kind.
    pub kind: String,
}
/// Assessed operation and its trusted authorization scope.
pub struct ConfirmationRequest<'a> {
    /// Human-readable approval preview.
    pub preview: &'a str,
    /// Registered tool name; direct shell entries use `execute_shell_command`.
    pub tool: Option<&'a str>,
    /// Validated Android package or prepared UI target package, if known.
    pub package: Option<&'a str>,
    /// Prepared media category and destination; never parsed from the approval preview.
    pub media_scope: Option<MediaApprovalScope>,
}
impl<'a> ConfirmationRequest<'a> {
    /// Construct a shell request with no inferred Android package.
    pub fn shell(preview: &'a str) -> Self {
        Self {
            preview,
            tool: Some("execute_shell_command"),
            package: None,
            media_scope: None,
        }
    }
}

/// Resolve grants after assessment, then delegate unmatched requests to the existing UI.
pub async fn confirm_assessed(
    config: &crate::config::Config,
    confirmer: &dyn Confirmer,
    request: &ConfirmationRequest<'_>,
    assessment: &SecurityAssessment,
) -> Result<ConfirmationDecision> {
    loop {
        if confirmer.approval_cancelled() {
            return Ok(ConfirmationDecision::Reject);
        }
        let policy = confirmer
            .confirmation_policy_override()
            .unwrap_or(config.execute_confirm_policy);
        if crate::security::access::full_access_enabled()
            || policy == crate::config::ConfirmPolicy::FullAccess
        {
            return Ok(ConfirmationDecision::ApproveByFullAccess);
        }
        if confirmer.confirmation_policy_override().is_some()
            && policy != crate::config::ConfirmPolicy::Always
            && config.security_level != crate::config::SecurityLevel::Strict
            && assessment.risk_level == RiskLevel::ReadOnly
            && !assessment.requires_root
            && !assessment.requires_double_confirmation
        {
            return Ok(ConfirmationDecision::ApproveBySessionPolicy);
        }
        let grant_request = crate::security::grants::GrantRequest {
            tool: request.tool,
            package: request.package,
            risk: assessment.risk_level,
            blocked: assessment.requires_root || assessment.requires_double_confirmation,
        };
        if let Some(id) = crate::security::grants::try_release(config, &grant_request).await? {
            if confirmer.approval_cancelled() {
                return Ok(ConfirmationDecision::Reject);
            }
            return Ok(ConfirmationDecision::ApproveByGrant(id));
        }
        match confirmer
            .confirm_with_policy(request, assessment, policy)
            .await?
        {
            ConfirmationDecision::SetPolicy(selected) => {
                if confirmer.confirmation_policy_override() != Some(selected) {
                    anyhow::bail!("confirmation policy change was not applied to the session")
                }
            }
            decision => return Ok(decision),
        }
    }
}

/// Includes a live session's `always` selection without altering the bound risk assessment.
pub fn confirmation_required(
    config: &crate::config::Config,
    confirmer: &dyn Confirmer,
    assessment: &SecurityAssessment,
) -> bool {
    assessment.requires_confirmation
        || confirmer
            .confirmation_policy_override()
            .unwrap_or(config.execute_confirm_policy)
            == crate::config::ConfirmPolicy::Always
}

#[async_trait]
/// UI-independent approval interface invoked after every assessment.
pub trait Confirmer: Send + Sync {
    /// A live owner choice for this session, independent of the task's configuration snapshot.
    fn confirmation_policy_override(&self) -> Option<crate::config::ConfirmPolicy> {
        None
    }

    /// Presents the effective preference alongside the assessed operation.
    async fn confirm_with_policy(
        &self,
        request: &ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
        _policy: crate::config::ConfirmPolicy,
    ) -> Result<ConfirmationDecision> {
        self.confirm(request, assessment).await
    }
    /// Input images for this task, never supplied by local text interfaces.
    fn input_images(&self) -> Vec<crate::llm::ToolAttachment> {
        Vec::new()
    }

    /// Route-bound delivery capability, available only inside message channels.
    fn file_delivery(&self) -> Option<std::sync::Arc<dyn FileDelivery>> {
        None
    }

    /// Identifies the local approval interface in execution audits.
    fn audit_source(&self) -> &'static str {
        "internal"
    }

    /// Returns an optional session identity; only its hash is recorded.
    fn audit_session(&self) -> Option<String> {
        None
    }

    /// Whether cancellation already prevents an automatic authorization.
    fn approval_cancelled(&self) -> bool {
        false
    }

    /// Requests approval, rejection, or an edited replacement command.
    async fn confirm(
        &self,
        request: &ConfirmationRequest<'_>,
        assessment: &SecurityAssessment,
    ) -> Result<ConfirmationDecision>;

    /// Requests missing factual input without granting execution approval.
    async fn ask_questions(&self, _questions: &[UserQuestion]) -> Result<Option<QuestionAnswers>> {
        Ok(None)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// User decision returned by a confirmation interface.
pub enum ConfirmationDecision {
    /// Execute the currently assessed command.
    Approve,
    /// Execute under the owner's explicit Full access configuration or CLI policy.
    ApproveByFullAccess,
    /// A live ordinary session preference released an eligible read-only operation.
    ApproveBySessionPolicy,
    /// Owner UI saved and installed a policy; the shared boundary must reassess approval.
    SetPolicy(crate::config::ConfirmPolicy),
    /// Execute using a consumed, scoped approval grant; never remember this decision.
    ApproveByGrant(String),
    /// Execute using the bidirectional interactive terminal bridge.
    ApproveInteractive,
    /// Force captured execution inside the ordinary output stream.
    ApproveCaptured,
    /// Execute and remember this exact command for the current Agent task.
    ApproveForTask,
    /// Execute and allow eligible mutations for the remainder of this process run.
    ///
    /// Callers must still reject this decision for root, strong-confirmation,
    /// Dangerous, or Critical assessments.
    ApproveForRun,
    /// Do not execute it.
    Reject,
    /// Replace it and run security assessment again.
    Edit(String),
}
/// Line-oriented confirmer that refuses approval when no TTY is available.
pub struct StdioConfirmer;

/// Returns whether one approval may be remembered for an identical command in this Agent task.
///
/// Root, strong-confirmation, Dangerous, and Critical commands must always be approved
/// individually, regardless of the confirmation interface.
pub fn can_remember_approval(assessment: &SecurityAssessment) -> bool {
    !assessment.requires_root
        && !assessment.requires_double_confirmation
        && assessment.risk_level <= RiskLevel::Mutating
}

#[async_trait]
impl Confirmer for StdioConfirmer {
    fn audit_source(&self) -> &'static str {
        "cli"
    }
    async fn confirm(
        &self,
        command: &crate::agent::ConfirmationRequest<'_>,
        a: &SecurityAssessment,
    ) -> Result<ConfirmationDecision> {
        let command = command.preview;
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            eprintln!(
                "Refusing {:?} command without an interactive TTY: {command}",
                a.risk_level
            );
            return Ok(ConfirmationDecision::Reject);
        }
        println!(
            "⚠️ {:?}{}: {command}",
            a.risk_level,
            if a.requires_root { " ROOT" } else { "" }
        );
        println!("  1. Allow once [y]");
        if can_remember_approval(a) {
            println!("  2. Always allow this exact command for this Agent task [a]");
        } else {
            println!("  2. Always allow is unavailable for root or high-risk commands");
        }
        if can_remember_approval(a) {
            println!("  3. Allow all eligible mutations for this run [r]");
        } else {
            println!("  3. Run-wide allow is unavailable for root or high-risk commands");
        }
        println!("  4. Reject [n]");
        println!("  5. Edit and reassess [e]");
        println!("  6. Run in interactive terminal [i]");
        println!("  7. Run with captured output [t]");
        print!("Select an option [1-7/y/n/a/r/e/i/t]: ");
        io::stdout().flush()?;
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        if matches!(choice.trim(), "5" | "e" | "E") {
            print!("Edited command [{command}]: ");
            io::stdout().flush()?;
            let mut edited = String::new();
            io::stdin().read_line(&mut edited)?;
            let edited = edited.trim();
            return Ok(if edited.is_empty() {
                ConfirmationDecision::Reject
            } else {
                ConfirmationDecision::Edit(edited.into())
            });
        }
        let approval = match choice.trim() {
            "1" | "y" | "Y" => ConfirmationDecision::Approve,
            "2" | "a" | "A" if can_remember_approval(a) => ConfirmationDecision::ApproveForTask,
            "3" | "r" | "R" if can_remember_approval(a) => ConfirmationDecision::ApproveForRun,
            "6" | "i" | "I" => ConfirmationDecision::ApproveInteractive,
            "7" | "t" | "T" => ConfirmationDecision::ApproveCaptured,
            _ => return Ok(ConfirmationDecision::Reject),
        };
        if a.requires_double_confirmation {
            Ok(if ask_exact_yes("High risk: type YES to confirm: ")? {
                approval
            } else {
                ConfirmationDecision::Reject
            })
        } else {
            Ok(approval)
        }
    }

    async fn ask_questions(&self, questions: &[UserQuestion]) -> Result<Option<QuestionAnswers>> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Ok(None);
        }
        let mut answers = BTreeMap::new();
        for question in questions {
            println!("{}: {}", question.header, question.prompt);
            for (index, option) in question.options.iter().enumerate() {
                println!("  {}. {} — {}", index + 1, option.label, option.description);
            }
            print!("Select a number or enter a custom value (empty cancels): ");
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            let answer = answer.trim();
            if answer.is_empty() {
                return Ok(None);
            }
            let value = answer
                .parse::<usize>()
                .ok()
                .and_then(|index| question.options.get(index.saturating_sub(1)))
                .map(|option| option.value.clone())
                .unwrap_or_else(|| answer.to_owned());
            answers.insert(question.id.clone(), value);
        }
        Ok(Some(answers))
    }
}
fn ask_exact_yes(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    Ok(s.trim() == "YES")
}

#[cfg(test)]
mod grant_boundary_tests {
    use super::*;
    use crate::config::{ApprovalGrantConfig, Config};
    use crate::security::{assess, RiskLevel};
    use anyhow::Context;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Reject(AtomicUsize);
    #[async_trait]
    impl Confirmer for Reject {
        async fn confirm(
            &self,
            _: &ConfirmationRequest<'_>,
            _: &SecurityAssessment,
        ) -> Result<ConfirmationDecision> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ConfirmationDecision::Reject)
        }
    }
    fn configured(path: &std::path::Path) -> Result<Config> {
        let config = Config {
            source: Some(path.into()),
            approval_grants: vec![ApprovalGrantConfig {
                id: "one".into(),
                tool: Some("android.*".into()),
                package: Some("com.example.app".into()),
                max_risk: "mutating".into(),
                expires_at: None,
                uses: Some(1),
            }],
            ..Config::default()
        };
        crate::config::save_config(path, &config)?;
        Ok(config)
    }
    #[tokio::test]
    async fn grant_boundary_preserves_scope_risk_root_and_strong_confirmation() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let config = configured(&directory.path().join("config.toml"))?;
        let fallback = Reject(AtomicUsize::new(0));
        let mut request = ConfirmationRequest {
            preview: "launch",
            tool: Some("android.launch_app"),
            package: Some("com.other.app"),
            media_scope: None,
        };
        let mut assessment = assess("touch result", &config);
        assert_eq!(assessment.risk_level, RiskLevel::Mutating);
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        request.package = None;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        request.package = Some("com.example.app");
        request.tool = Some("apply_patch");
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        request.tool = Some("android.launch_app");
        assessment.requires_root = true;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        assessment.requires_root = false;
        assessment.requires_double_confirmation = true;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        assessment.requires_double_confirmation = false;
        assessment.risk_level = RiskLevel::Dangerous;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        assessment.risk_level = RiskLevel::Critical;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        assessment.risk_level = RiskLevel::Mutating;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::ApproveByGrant("one".into())
        );
        assert_eq!(fallback.0.load(Ordering::SeqCst), 7);
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        Ok(())
    }
    #[tokio::test]
    async fn approval_grant_cannot_bypass_classifier_rule_escalation() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("config.toml");
        let mut config = configured(&path)?;
        config.approval_grants[0].tool = Some("execute_shell_command".into());
        config.approval_grants[0].package = None;
        config
            .security_rules
            .push(crate::config::SecurityRuleConfig {
                id: "raise".into(),
                pattern: "touch".into(),
                risk: "dangerous".into(),
                message: "raised by local rule".into(),
            });
        crate::config::save_config(&path, &config)?;
        let fallback = Reject(AtomicUsize::new(0));
        let assessment = assess("touch result", &config);
        assert_eq!(assessment.risk_level, RiskLevel::Dangerous);
        assert_eq!(
            confirm_assessed(
                &config,
                &fallback,
                &ConfirmationRequest::shell("touch result"),
                &assessment
            )
            .await?,
            ConfirmationDecision::Reject
        );
        let status = crate::security::grants::statuses(&config)?;
        assert_eq!(status[0].uses_remaining, Some(1));
        Ok(())
    }

    #[tokio::test]
    async fn expiry_revocation_and_template_removal_apply_to_running_snapshot() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let mut config = configured(&directory.path().join("config.toml"))?;
        config.approval_grants[0].uses = None;
        config.approval_grants[0].expires_at = Some("2000-01-01T00:00:00Z".into());
        crate::config::save_config(config.source.as_deref().context("source")?, &config)?;
        let fallback = Reject(AtomicUsize::new(0));
        let request = ConfirmationRequest {
            preview: "launch",
            tool: Some("android.launch_app"),
            package: Some("com.example.app"),
            media_scope: None,
        };
        let assessment = assess("touch result", &config);
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        config.approval_grants[0].expires_at = None;
        crate::config::save_config(config.source.as_deref().context("source")?, &config)?;
        crate::security::grants::revoke(&config, "one")?;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        config.approval_grants[0].id = "new".into();
        crate::config::save_config(config.source.as_deref().context("source")?, &config)?;
        let mut removed = config.clone();
        removed.approval_grants.clear();
        crate::config::save_config(config.source.as_deref().context("source")?, &removed)?;
        assert_eq!(
            confirm_assessed(&config, &fallback, &request, &assessment).await?,
            ConfirmationDecision::Reject
        );
        Ok(())
    }
}

/// A route-bound file transport; only assessed and confirmed operations invoke it.
#[async_trait]
pub trait FileDelivery: Send + Sync {
    /// Conversation destination shown before approval.
    fn destination(&self) -> String;
    /// Stable identity used for media authorization, excluding runtime target handles.
    fn approval_destination(&self) -> String {
        self.destination()
    }
    /// Retain a tool-produced attachment for this task and return an opaque reference.
    async fn stage_attachment(
        &self,
        _attachment: &crate::llm::ToolAttachment,
    ) -> Result<Option<String>> {
        Ok(None)
    }
    /// Resolve a current-task reference into an immutable filename and byte snapshot.
    fn attachment(&self, _id: &str) -> Result<(String, Vec<u8>)> {
        anyhow::bail!("attachment references are unavailable in this delivery context")
    }
    /// Whether this destination accepts native command buttons.
    fn supports_buttons(&self) -> bool {
        false
    }
    /// Send assessed text/Markdown with optional native command buttons.
    async fn send_message(
        &self,
        _text: &str,
        _format: &str,
        _buttons: &[crate::channels::outbound::MessageButton],
    ) -> Result<serde_json::Value> {
        anyhow::bail!("text delivery unavailable in this context")
    }
    /// Delivers the exact bytes captured before approval, without reopening a path.
    async fn send_file(&self, name: &str, kind: &str, bytes: Vec<u8>, caption: &str) -> Result<()>;
}
