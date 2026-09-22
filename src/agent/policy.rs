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
#[async_trait]
/// UI-independent approval interface invoked after every assessment.
pub trait Confirmer: Send + Sync {
    /// Requests approval, rejection, or an edited replacement command.
    async fn confirm(
        &self,
        command: &str,
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
    async fn confirm(&self, command: &str, a: &SecurityAssessment) -> Result<ConfirmationDecision> {
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
