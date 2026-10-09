//! Non-secret identity of the running executable, distinct from its installed path.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;

/// Return compile-time provenance and SHA-256 of the executing image.
/// A missing Git revision or build identifier is explicitly unknown.
pub async fn running_identity() -> Result<Value> {
    tokio::task::spawn_blocking(|| {
        // /proc/self/exe keeps referring to the old image after an atomic update.
        // Hashing current_exe's pathname would incorrectly identify the replacement.
        let mut file = std::fs::File::open("/proc/self/exe")
            .context("cannot open the running executable for identity verification")?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).context("cannot hash running executable")?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        Ok(json!({
            "binary_version": env!("CARGO_PKG_VERSION"),
            "git_commit": nonempty(env!("NL2SH_GIT_COMMIT")),
            "git_dirty": match env!("NL2SH_GIT_DIRTY") { "true" => Some(true), "false" => Some(false), _ => None },
            "build_id": nonempty(env!("NL2SH_BUILD_ID")),
            "build_target": env!("NL2SH_BUILD_TARGET"),
            "build_profile": env!("NL2SH_BUILD_PROFILE"),
            "binary_sha256": format!("{:x}", hash.finalize()),
            "protocol_version": "2025-11-25",
            "runtime_uid": unsafe { libc::geteuid() },
        }))
    }).await.context("build identity worker failed")?
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn running_image_has_exact_digest_and_compile_time_target() -> Result<()> {
        let identity = running_identity().await?;
        let executable = std::fs::read("/proc/self/exe")?;
        assert_eq!(
            identity["binary_sha256"],
            format!("{:x}", Sha256::digest(executable))
        );
        assert_eq!(identity["build_target"], env!("NL2SH_BUILD_TARGET"));
        assert_eq!(identity["runtime_uid"], unsafe { libc::geteuid() });
        Ok(())
    }
}
