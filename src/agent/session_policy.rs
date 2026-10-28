//! Live owner-selected confirmation preference, isolated to one UI session.

use crate::config::ConfirmPolicy;
use std::sync::atomic::{AtomicU8, Ordering};

/// An ephemeral UI-session override; it is never restored from conversation history.
#[derive(Default)]
pub struct SessionConfirmationPolicy(AtomicU8);

impl SessionConfirmationPolicy {
    /// Returns the live preference, or the task's saved preference when unset.
    pub fn get(&self) -> Option<ConfirmPolicy> {
        match self.0.load(Ordering::Acquire) {
            1 => Some(ConfirmPolicy::Always),
            2 => Some(ConfirmPolicy::RiskOnly),
            3 => Some(ConfirmPolicy::Never),
            4 => Some(ConfirmPolicy::FullAccess),
            _ => None,
        }
    }

    /// Applies an explicit owner selection after its configuration has been saved.
    pub fn set(&self, policy: ConfirmPolicy) {
        self.0.store(
            match policy {
                ConfirmPolicy::Always => 1,
                ConfirmPolicy::RiskOnly => 2,
                ConfirmPolicy::Never => 3,
                ConfirmPolicy::FullAccess => 4,
            },
            Ordering::Release,
        );
    }
}
