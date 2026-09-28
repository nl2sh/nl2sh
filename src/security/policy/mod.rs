//! Domain policies over statically resolved shell commands and effects.

mod android;
mod filesystem;
mod network;
pub mod privilege;

use super::shell::effects::Effects;

pub(super) fn classify_command(name: &str, args: &[String], effects: &mut Effects) {
    filesystem::classify_command(name, args, effects);
    android::classify_command(name, args, effects);
    network::classify_command(name, args, effects);
    privilege::classify_command(name, args, effects);
}

pub(super) fn classify_redirect(target: &str, effects: &mut Effects) {
    filesystem::classify_redirect(target, effects);
    privilege::classify_target(target, effects);
}

pub(super) fn classify_target(target: &str, effects: &mut Effects) {
    privilege::classify_target(target, effects);
}
