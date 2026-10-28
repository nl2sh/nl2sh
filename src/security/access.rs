//! Explicit development approval selected by the owner through configuration or CLI.

use std::sync::atomic::{AtomicBool, Ordering};

static FULL_ACCESS: AtomicBool = AtomicBool::new(false);

/// Automatically approve every assessed operation, including dangerous and Root actions.
///
/// Call only for an explicit owner CLI choice. This does not change risk classification,
/// command capability binding, tool availability, execution identity or OS permissions.
pub fn enable_full_access() {
    FULL_ACCESS.store(true, Ordering::Release);
}

/// Whether the owner enabled unrestricted approval for this process at startup.
pub fn full_access_enabled() -> bool {
    FULL_ACCESS.load(Ordering::Acquire)
}

/// Whether this task uses the owner's persisted Full access policy or CLI override.
pub fn full_access_for(config: &crate::config::Config) -> bool {
    full_access_enabled()
        || config.execute_confirm_policy == crate::config::ConfirmPolicy::FullAccess
}
