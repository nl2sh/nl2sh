//! HTTP endpoints grouped by responsibility.
mod info;
pub(super) use info::*;
mod config;
pub(super) use config::*;
mod tools;
pub(super) use tools::*;
mod memory;
pub(super) use memory::*;
mod files;
pub(super) use files::*;
mod sessions;
pub(super) use sessions::*;
mod terminal;
pub(super) use terminal::*;

mod tailcat;
pub(super) use tailcat::*;

mod update;
pub(super) use update::*;
