//! Time-boxed approval grants: pure matching plus locked usage counters.
//!
//! A grant releases an already-classified operation without a human prompt. It
//! never changes the assessed risk, never releases `Critical` operations, and
//! never bypasses root or double confirmation. This module is deliberately free
//! of confirmation wiring: the approval path owns the decision to consult it.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use time::OffsetDateTime;

use crate::{config, security::RiskLevel};

const ROOT_NAME: &str = ".nl2sh-g";
const STATE_FILE: &str = "grants.json";
const LOCK_FILE: &str = "lock";
const VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 256 * 1024;
const LOCK_TIMEOUT: Duration = Duration::from_secs(3);

/// Immutable grant template loaded from configuration.
#[derive(Debug, Clone)]
pub struct ApprovalGrant {
    /// Stable identifier recorded in audit events.
    pub id: String,
    /// Exact tool name or `prefix*` wildcard; `None` matches any tool.
    pub tool: Option<String>,
    /// Android package scope; `None` matches operations without a package scope.
    pub package: Option<String>,
    /// Highest risk this grant may release.
    pub max_risk: RiskLevel,
    /// Latest release instant; `None` never expires.
    pub expires_at: Option<OffsetDateTime>,
    /// Remaining uses; `None` is unlimited until expiry.
    pub uses_remaining: Option<u32>,
}

/// One assessed operating scope to be released without a human prompt.
pub struct GrantRequest<'a> {
    /// Registered tool name, when the request comes from a tool dispatch.
    pub tool: Option<&'a str>,
    /// Android package scope, when reliably known.
    pub package: Option<&'a str>,
    /// Risk level produced by the classifier; never a pre-classification value.
    pub risk: RiskLevel,
    /// Whether the operation needs root or a second confirmation.
    pub blocked: bool,
}

static DANGEROUS_CLI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Enables dangerous grants for this process only after the explicit CLI flag.
pub fn enable_dangerous_grants() {
    DANGEROUS_CLI.store(true, std::sync::atomic::Ordering::Release);
}

/// Returns whether the process received the dangerous-grant CLI flag.
pub fn dangerous_grants_enabled() -> bool {
    DANGEROUS_CLI.load(std::sync::atomic::Ordering::Acquire)
}

/// Release an assessed request and durably consume its budget on a blocking worker.
/// Reloads only grant templates so revocation takes effect even during an active task.
pub async fn try_release(
    config: &crate::config::Config,
    request: &GrantRequest<'_>,
) -> Result<Option<String>> {
    // Root and strong confirmation are unconditional exclusion boundaries.
    if request.blocked || request.risk >= RiskLevel::Critical {
        return Ok(None);
    }
    let Some(path) = config.source.clone() else {
        return Ok(None);
    };
    let tool = request.tool.map(str::to_owned);
    let package = request.package.map(str::to_owned);
    let risk = request.risk;
    tokio::task::spawn_blocking(move || -> Result<Option<String>> {
        let current = config::load_or_default_unvalidated(&path)?;
        current.validate_runtime()?;
        if current.approval_grants.is_empty() {
            return Ok(None);
        }
        let grants = templates(&current)?;
        let store = GrantStore::open(&path, &grants)?;
        let request = GrantRequest {
            tool: tool.as_deref(),
            package: package.as_deref(),
            risk,
            blocked: false,
        };
        // Exhausted overlapping grants must not hide later usable grants.
        for grant in &grants {
            if evaluate(
                std::slice::from_ref(grant),
                &request,
                OffsetDateTime::now_utc(),
                current.allow_dangerous_grants && dangerous_grants_enabled(),
            )
            .is_some()
                && store.consume(&grant.id)?
            {
                return Ok(Some(grant.id.clone()));
            }
        }
        Ok(None)
    })
    .await
    .context("approval grant worker failed")?
}

fn templates(config: &crate::config::Config) -> Result<Vec<ApprovalGrant>> {
    config
        .approval_grants
        .iter()
        .map(|grant| {
            Ok(ApprovalGrant {
                id: grant.id.clone(),
                tool: grant.tool.clone(),
                package: grant.package.clone(),
                max_risk: match grant.max_risk.as_str() {
                    "read_only" | "readonly" => RiskLevel::ReadOnly,
                    "mutating" => RiskLevel::Mutating,
                    "dangerous" => RiskLevel::Dangerous,
                    "critical" => RiskLevel::Critical,
                    _ => bail!("invalid approval grant risk"),
                },
                expires_at: grant
                    .expires_at
                    .as_deref()
                    .map(|expiry| {
                        OffsetDateTime::parse(
                            expiry,
                            &time::format_description::well_known::Rfc3339,
                        )
                    })
                    .transpose()?,
                uses_remaining: grant.uses,
            })
        })
        .collect()
}

/// Read-only grant status, shared by local settings interfaces.
#[derive(Serialize)]
pub struct GrantStatus {
    /// Configured template.
    pub grant: crate::config::ApprovalGrantConfig,
    /// Persisted remaining budget; null means unlimited.
    pub uses_remaining: Option<u32>,
    /// Whether expiry, revocation or exhaustion prevents authorization.
    pub inactive: bool,
}

/// Read current templates and persisted counters without granting an operation.
pub fn statuses(config: &crate::config::Config) -> Result<Vec<GrantStatus>> {
    let Some(path) = config.source.as_deref() else {
        return Ok(Vec::new());
    };
    if config.approval_grants.is_empty() {
        return Ok(Vec::new());
    }
    let templates = templates(config)?;
    let store = GrantStore::open(path, &templates)?;
    let _lock = GrantLock::acquire(&store.root)?;
    let state = store.read_state()?;
    Ok(config
        .approval_grants
        .iter()
        .zip(templates)
        .map(|(config, grant)| {
            let usage = state.grants.get(&grant.id);
            let remaining = remaining_budget(grant.uses_remaining, usage);
            GrantStatus {
                grant: config.clone(),
                uses_remaining: remaining,
                inactive: grant.is_expired(OffsetDateTime::now_utc())
                    || remaining == Some(0)
                    || usage.is_some_and(|usage| usage.revoked),
            }
        })
        .collect())
}

/// Permanently revoke an ID, including unlimited grants, for all active entry points.
pub fn revoke(config: &crate::config::Config, id: &str) -> Result<()> {
    let path = config
        .source
        .as_deref()
        .context("grant configuration source unavailable")?;
    let grants = templates(config)?;
    if !grants.iter().any(|grant| grant.id == id) {
        bail!("unknown approval grant");
    }
    let store = GrantStore::open(path, &grants)?;
    let _lock = GrantLock::acquire(&store.root)?;
    let mut state = store.read_state()?;
    let first_used = state
        .grants
        .get(id)
        .and_then(|usage| usage.first_used.clone());
    state.grants.insert(
        id.into(),
        GrantUsage {
            uses_remaining: Some(0),
            first_used,
            revoked: true,
        },
    );
    store.write_state(&state)
}

impl ApprovalGrant {
    fn is_expired(&self, now: OffsetDateTime) -> bool {
        self.expires_at.is_some_and(|expiry| now >= expiry)
    }

    fn covers_risk(&self, risk: RiskLevel) -> bool {
        risk <= self.max_risk
    }

    fn matches_tool(&self, tool: Option<&str>) -> bool {
        let Some(pattern) = self.tool.as_deref() else {
            return true;
        };
        let Some(name) = tool else {
            return false;
        };
        match pattern.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => pattern == name,
        }
    }

    fn matches_package(&self, package: Option<&str>) -> bool {
        match (self.package.as_deref(), package) {
            (None, None) => true,
            (Some(expected), Some(actual)) => expected == actual,
            _ => false,
        }
    }
}

/// Returns the id of the grant that releases this request, if any. Pure and deterministic.
///
/// Enforces the four invariants: the caller passes an already-classified risk; a
/// grant only covers `risk <= max_risk`; `Critical` requests and blocked requests
/// are never released; `Dangerous` requests require `allow_dangerous`.
pub fn evaluate(
    grants: &[ApprovalGrant],
    request: &GrantRequest<'_>,
    now: OffsetDateTime,
    allow_dangerous: bool,
) -> Option<String> {
    if request.blocked || request.risk >= RiskLevel::Critical {
        return None;
    }
    if request.risk >= RiskLevel::Dangerous && !allow_dangerous {
        return None;
    }
    grants
        .iter()
        .find(|grant| {
            !grant.is_expired(now)
                && grant.uses_remaining != Some(0)
                && grant.covers_risk(request.risk)
                && grant.matches_tool(request.tool)
                && grant.matches_package(request.package)
        })
        .map(|grant| grant.id.clone())
}

/// Shared, file-locked usage counters for configured grants.
///
/// Every confirmer in the process shares one counter per grant, so the counts
/// live in a private, user-owned state file serialized with `flock` rather than
/// in memory.
#[derive(Clone)]
pub struct GrantStore {
    root: PathBuf,
    budgets: BTreeMap<String, Option<u32>>,
    expiries: BTreeMap<String, Option<OffsetDateTime>>,
}

impl GrantStore {
    /// Opens the private grant state directory for `config_path`.
    ///
    /// Configured templates seed each grant's budget: `Some(n)` is a finite
    /// count shared across processes, `None` is unlimited until explicitly revoked.
    pub fn open(config_path: &Path, grants: &[ApprovalGrant]) -> Result<Self> {
        let budgets = grants
            .iter()
            .map(|grant| (grant.id.clone(), grant.uses_remaining))
            .collect();
        Ok(Self {
            root: state_root(config_path)?,
            budgets,
            expiries: grants
                .iter()
                .map(|grant| (grant.id.clone(), grant.expires_at))
                .collect(),
        })
    }

    /// Consumes one use of `id`, returning `false` when its budget is exhausted.
    ///
    /// Unlimited grants record first use and respect persisted revocation. The read-decide-write cycle runs under `flock`.
    pub fn consume(&self, id: &str) -> Result<bool> {
        let budget = match self.budgets.get(id) {
            Some(budget) => *budget,
            None => bail!("unknown approval grant {id}"),
        };
        let _lock = GrantLock::acquire(&self.root)?;
        if self
            .expiries
            .get(id)
            .copied()
            .flatten()
            .is_some_and(|expiry| OffsetDateTime::now_utc() >= expiry)
        {
            return Ok(false);
        }
        let mut state = self.read_state()?;
        let existing = state.grants.get(id);
        if existing.is_some_and(|usage| usage.revoked) {
            return Ok(false);
        }
        let remaining = remaining_budget(budget, existing);
        if remaining == Some(0) {
            return Ok(false);
        }
        let first_used = existing
            .and_then(|usage| usage.first_used.clone())
            .or_else(|| Some(now_rfc3339()));
        state.grants.insert(
            id.to_owned(),
            GrantUsage {
                uses_remaining: remaining.map(|remaining| remaining - 1),
                first_used,
                revoked: false,
            },
        );
        self.write_state(&state)?;
        Ok(true)
    }

    fn read_state(&self) -> Result<GrantState> {
        let path = self.root.join(STATE_FILE);
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(GrantState::default())
            }
            Err(error) => return Err(error).context("cannot open approval grant state"),
        };
        validate_state_file(&file)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_STATE_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("cannot read approval grant state")?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            bail!("approval grant state exceeds size limit")
        }
        let state: GrantState =
            serde_json::from_slice(&bytes).context("invalid approval grant state")?;
        if state.version != VERSION {
            bail!("unsupported approval grant state version {}", state.version)
        }
        Ok(state)
    }

    fn write_state(&self, state: &GrantState) -> Result<()> {
        let bytes = serde_json::to_vec(state).context("cannot encode approval grant state")?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            bail!("approval grant state exceeds size limit")
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.root)
            .context("cannot create private approval grant state")?;
        validate_state_file(file.as_file())?;
        file.write_all(&bytes)
            .context("cannot write approval grant state")?;
        file.as_file()
            .sync_all()
            .context("cannot flush approval grant state")?;
        file.persist(self.root.join(STATE_FILE))
            .context("cannot publish approval grant state")?;
        File::open(&self.root)?
            .sync_all()
            .context("cannot sync approval grant directory")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantState {
    version: u32,
    grants: BTreeMap<String, GrantUsage>,
}

impl Default for GrantState {
    fn default() -> Self {
        Self {
            version: VERSION,
            grants: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantUsage {
    uses_remaining: Option<u32>,
    first_used: Option<String>,
    #[serde(default)]
    revoked: bool,
}

fn remaining_budget(budget: Option<u32>, usage: Option<&GrantUsage>) -> Option<u32> {
    match (budget, usage.and_then(|usage| usage.uses_remaining)) {
        (Some(base), Some(saved)) => Some(base.min(saved)),
        (Some(base), None) => Some(base),
        (None, saved) => saved,
    }
}

struct GrantLock(File);

impl GrantLock {
    fn acquire(root: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join(LOCK_FILE))
            .context("cannot open approval grant lock")?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("approval grant lock must be a private file owned by the current user")
        }
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Self(file));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error).context("cannot lock approval grant state");
            }
            if Instant::now() >= deadline {
                bail!("approval grant state is busy")
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for GrantLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn state_root(config_path: &Path) -> Result<PathBuf> {
    // Bind counters to canonical config identity, including multiple configs in one directory.
    let identity = if config_path.exists() {
        fs::canonicalize(config_path)?
    } else {
        fs::canonicalize(
            config_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .join(
            config_path
                .file_name()
                .context("configuration has no filename")?,
        )
    };
    let state = config::state_dir(&identity)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)
        .with_context(|| {
            format!(
                "cannot create approval grant state directory {}",
                state.display()
            )
        })?;
    use sha2::{Digest, Sha256};
    use std::os::unix::ffi::OsStrExt;
    let root = state.join(format!(
        "{ROOT_NAME}-{:x}",
        Sha256::digest(identity.as_os_str().as_bytes())
    ));
    match fs::symlink_metadata(&root) {
        Ok(metadata) => validate_root(&metadata)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match DirBuilder::new().mode(0o700).create(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("cannot create approval grant directory {}", root.display())
                    })
                }
            }
            validate_root(&fs::symlink_metadata(&root)?)?;
        }
        Err(error) => return Err(error).context("cannot inspect approval grant directory"),
    }
    Ok(root)
}

fn validate_root(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("approval grant directory must be a private directory owned by the current user")
    }
    Ok(())
}

fn validate_state_file(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("approval grant state must be a private file owned by the current user")
    }
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate, state_root, ApprovalGrant, GrantRequest, GrantState, GrantStore, RiskLevel,
        STATE_FILE,
    };
    use anyhow::{Context, Result};
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};
    use time::OffsetDateTime;

    const NOW: i64 = 1_700_000_000;
    const FUTURE: i64 = NOW + 3600;

    fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn grant(
        id: &str,
        tool: Option<&str>,
        package: Option<&str>,
        max_risk: RiskLevel,
        uses: Option<u32>,
    ) -> ApprovalGrant {
        ApprovalGrant {
            id: id.into(),
            tool: tool.map(str::to_owned),
            package: package.map(str::to_owned),
            max_risk,
            expires_at: Some(at(FUTURE)),
            uses_remaining: uses,
        }
    }

    fn read_state(root: &Path) -> Result<GrantState> {
        let bytes = fs::read_to_string(root.join(STATE_FILE))?;
        Ok(serde_json::from_str(&bytes)?)
    }

    #[test]
    fn grant_hits_within_scope_and_budget() {
        let grants = [grant(
            "g",
            Some("android.tap"),
            Some("com.konka.tv"),
            RiskLevel::Mutating,
            Some(3),
        )];
        let request = GrantRequest {
            tool: Some("android.tap"),
            package: Some("com.konka.tv"),
            risk: RiskLevel::Mutating,
            blocked: false,
        };
        assert_eq!(
            evaluate(&grants, &request, at(NOW), false),
            Some("g".into())
        );
    }

    #[test]
    fn exhausted_grant_does_not_hit() {
        let grants = [grant("g", None, None, RiskLevel::Mutating, Some(0))];
        let request = GrantRequest {
            tool: Some("android.tap"),
            package: None,
            risk: RiskLevel::Mutating,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), false), None);
    }

    #[test]
    fn expired_grant_does_not_hit() {
        let mut expired = grant("g", None, None, RiskLevel::Mutating, Some(3));
        expired.expires_at = Some(at(NOW - 1));
        let request = GrantRequest {
            tool: None,
            package: None,
            risk: RiskLevel::ReadOnly,
            blocked: false,
        };
        assert_eq!(evaluate(&[expired], &request, at(NOW), false), None);
    }

    #[test]
    fn risk_above_max_does_not_hit() {
        let grants = [grant("g", None, None, RiskLevel::Mutating, Some(3))];
        let request = GrantRequest {
            tool: None,
            package: None,
            risk: RiskLevel::Dangerous,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), true), None);
    }

    #[test]
    fn package_mismatch_does_not_hit() {
        let grants = [grant(
            "g",
            None,
            Some("com.konka.tv"),
            RiskLevel::Mutating,
            None,
        )];
        let request = GrantRequest {
            tool: None,
            package: Some("com.other.app"),
            risk: RiskLevel::ReadOnly,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), false), None);
    }

    #[test]
    fn scoped_grant_does_not_cover_request_without_package() {
        let grants = [grant(
            "g",
            None,
            Some("com.konka.tv"),
            RiskLevel::Mutating,
            None,
        )];
        let request = GrantRequest {
            tool: None,
            package: None,
            risk: RiskLevel::ReadOnly,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), false), None);
    }

    #[test]
    fn tool_wildcard_matches_prefix() {
        let grants = [grant(
            "g",
            Some("android.*"),
            None,
            RiskLevel::Mutating,
            None,
        )];
        let request = GrantRequest {
            tool: Some("android.tap"),
            package: None,
            risk: RiskLevel::ReadOnly,
            blocked: false,
        };
        assert_eq!(
            evaluate(&grants, &request, at(NOW), false),
            Some("g".into())
        );
    }

    #[test]
    fn critical_request_is_never_released() {
        let grants = [grant("g", None, None, RiskLevel::Critical, None)];
        let request = GrantRequest {
            tool: None,
            package: None,
            risk: RiskLevel::Critical,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), true), None);
    }

    #[test]
    fn blocked_request_is_never_released() {
        let grants = [grant("g", None, None, RiskLevel::Critical, None)];
        let request = GrantRequest {
            tool: None,
            package: None,
            risk: RiskLevel::ReadOnly,
            blocked: true,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), true), None);
    }

    #[test]
    fn dangerous_request_needs_allow_flag() {
        let grants = [grant("g", None, None, RiskLevel::Dangerous, Some(5))];
        let request = GrantRequest {
            tool: Some("android.tap"),
            package: None,
            risk: RiskLevel::Dangerous,
            blocked: false,
        };
        assert_eq!(evaluate(&grants, &request, at(NOW), false), None);
        assert_eq!(evaluate(&grants, &request, at(NOW), true), Some("g".into()));
    }

    #[test]
    fn tool_none_grant_matches_any_tool() {
        let grants = [grant("g", None, None, RiskLevel::Mutating, None)];
        let request = GrantRequest {
            tool: Some("execute_shell_command"),
            package: None,
            risk: RiskLevel::Mutating,
            blocked: false,
        };
        assert_eq!(
            evaluate(&grants, &request, at(NOW), false),
            Some("g".into())
        );
    }

    #[test]
    fn consume_decrements_and_persists() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join("config.toml");
        let mut g = grant("g", None, None, RiskLevel::Mutating, Some(1));
        g.expires_at = None;
        let grants = [g];
        let store = GrantStore::open(&config_path, &grants)?;
        assert!(store.consume("g")?);
        assert!(!store.consume("g")?);
        let state = read_state(&store.root)?;
        assert_eq!(state.version, super::VERSION);
        assert_eq!(
            state.grants.get("g").map(|usage| usage.uses_remaining),
            Some(Some(0))
        );
        let mode = fs::metadata(store.root.join(STATE_FILE))?
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "state file must not be group/world readable"
        );
        Ok(())
    }

    #[test]
    fn unlimited_grant_records_first_use_without_finite_budget() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join("config.toml");
        let mut g = grant("g", None, None, RiskLevel::Mutating, None);
        g.expires_at = None;
        let grants = [g];
        let store = GrantStore::open(&config_path, &grants)?;
        assert!(store.consume("g")?);
        assert!(store.consume("g")?);
        let state = read_state(&store.root)?;
        let usage = state.grants.get("g").context("usage missing")?;
        assert_eq!(usage.uses_remaining, None);
        assert!(usage
            .first_used
            .as_deref()
            .is_some_and(|time| !time.is_empty()));
        Ok(())
    }

    #[test]
    fn unknown_grant_id_is_rejected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join("config.toml");
        let store = GrantStore::open(&config_path, &[])?;
        assert!(store.consume("missing").is_err());
        Ok(())
    }

    #[test]
    fn corrupt_state_returns_error_without_panic() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join("config.toml");
        let mut g = grant("g", None, None, RiskLevel::Mutating, Some(2));
        g.expires_at = None;
        let grants = [g];
        let store = GrantStore::open(&config_path, &grants)?;
        let root = state_root(&config_path)?;
        fs::write(root.join(STATE_FILE), b"{ not json")?;
        let result = store.consume("g");
        assert!(
            result.is_err(),
            "corrupt state must be an error, not a panic"
        );
        Ok(())
    }

    #[test]
    fn state_symlinks_permissions_and_distinct_config_counters_are_checked() -> Result<()> {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir()?;
        let mut template = grant("g", None, None, RiskLevel::Mutating, Some(1));
        template.expires_at = None;
        let grants = [template];
        let first = GrantStore::open(&dir.path().join("first.toml"), &grants)?;
        let second = GrantStore::open(&dir.path().join("second.toml"), &grants)?;
        assert_ne!(first.root, second.root);
        assert!(first.consume("g")?);
        assert!(!first.consume("g")?);
        assert!(second.consume("g")?);
        let victim = dir.path().join("victim");
        fs::write(&victim, "keep")?;
        fs::remove_file(first.root.join(STATE_FILE))?;
        symlink(&victim, first.root.join(STATE_FILE))?;
        assert!(first.consume("g").is_err());
        assert_eq!(fs::read_to_string(&victim)?, "keep");
        fs::remove_file(first.root.join(STATE_FILE))?;
        fs::write(first.root.join(STATE_FILE), "{}")?;
        fs::set_permissions(
            first.root.join(STATE_FILE),
            fs::Permissions::from_mode(0o644),
        )?;
        assert!(first.consume("g").is_err());
        Ok(())
    }

    #[test]
    fn canonical_config_aliases_share_one_usage_budget() -> Result<()> {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir()?;
        let config = dir.path().join("config.toml");
        fs::write(&config, "")?;
        let aliases = dir.path().join("aliases");
        fs::create_dir(&aliases)?;
        let alias = aliases.join("config.toml");
        symlink(&config, &alias)?;
        let mut g = grant("g", None, None, RiskLevel::Mutating, Some(1));
        g.expires_at = None;
        let store = GrantStore::open(&config, std::slice::from_ref(&g))?;
        let other = GrantStore::open(&alias, &[g])?;
        assert_eq!(store.root, other.root);
        assert!(store.consume("g")?);
        assert!(!other.consume("g")?);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_consume_only_one_succeeds() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config_path = dir.path().join("config.toml");
        let mut g = grant("g", None, None, RiskLevel::Mutating, Some(1));
        g.expires_at = None;
        let grants = [g];
        let store = GrantStore::open(&config_path, &grants)?;
        let first = store.clone();
        let second = store.clone();
        let (left, right) = tokio::join!(
            tokio::task::spawn_blocking(move || first.consume("g")),
            tokio::task::spawn_blocking(move || second.consume("g")),
        );
        let outcomes = [left??, right??];
        assert_eq!(outcomes.iter().filter(|value| **value).count(), 1);
        let state = read_state(&store.root)?;
        assert_eq!(
            state.grants.get("g").map(|usage| usage.uses_remaining),
            Some(Some(0))
        );
        Ok(())
    }
}
