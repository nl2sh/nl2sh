//! Optional postconditions for UI mutations. Observation never repeats or approves an action.
use super::{automation, companion};
use crate::{
    shell::CommandExecutor,
    tools::ui::domain::{inspect_android_ui, InspectAndroidUiArgs},
};
use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::{timeout, Instant};

/// Read-only checks to perform after a normally approved Android action.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiVerification {
    /// Compare complete UI trees immediately before and after dispatch; not proof of business success.
    #[serde(default)]
    pub verify_change: bool,
    /// Exact package owning the current focused window, not merely a package present in the tree.
    pub expected_package: Option<String>,
    /// Exact visible text or content description that must appear.
    pub text_present: Option<String>,
    /// Exact visible text or content description that must be absent from a complete current viewport tree.
    pub text_absent: Option<String>,
    /// Deadline for the baseline read and separately for all post-action observation, 100–10000 ms (default 2000).
    pub timeout_ms: Option<u32>,
}
impl UiVerification {
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.verify_change
            && self.expected_package.is_none()
            && self.text_present.is_none()
            && self.text_absent.is_none()
        {
            bail!("verification requires a change, package or text check")
        }
        if let Some(package) = &self.expected_package {
            automation::validate_package(package)?;
        }
        for text in [&self.text_present, &self.text_absent]
            .into_iter()
            .flatten()
        {
            if text.is_empty() || text.len() > 1024 || text.contains('\0') {
                bail!("invalid UI verification text")
            }
        }
        if self.text_present.is_some() && self.text_present == self.text_absent {
            bail!("contradictory UI text verification")
        }
        if self
            .timeout_ms
            .is_some_and(|ms| !(100..=10000).contains(&ms))
        {
            bail!("verification timeout must be 100–10000 ms")
        }
        Ok(())
    }
}
pub(crate) fn supports(name: &str) -> bool {
    matches!(
        name,
        "android.launch_app"
            | "android.stop_app"
            | "android.tap"
            | "android.tap_text"
            | "android.tap_node"
            | "android.input_text"
            | "android.swipe"
            | "android.scroll"
            | "android.press_back"
            | "android.press_home"
            | "android.press_enter"
    )
}
async fn snapshot(executor: &dyn CommandExecutor) -> Result<Value> {
    let snapshot = match companion::screen_dump(executor).await {
        Ok(value) => value,
        Err(_) => serde_json::from_str(
            &inspect_android_ui(executor, &InspectAndroidUiArgs { full: true }).await?,
        )?,
    };
    // Partial/failed reads cannot prove absence, change or lack of change.
    complete_nodes(&snapshot)?;
    Ok(snapshot)
}
fn complete_nodes(value: &Value) -> Result<&Vec<Value>> {
    if value["status"] != "complete" || value["truncated"] == true || value["mode"] == "partial" {
        bail!("UI observation incomplete")
    }
    value["nodes"]
        .as_array()
        .context("UI observation has no nodes")
}
fn signature(value: &Value) -> Result<String> {
    let nodes = complete_nodes(value)?;
    let stable: Vec<Value> = nodes.iter().map(|node| json!({
        "package": node["package"], "class": node["class"], "resource_id": node["resource_id"],
        "text": node["text"], "description": node["content_description"],
        "text_hash": node["text_hash"], "description_hash": node["description_hash"],
        "bounds": node["bounds"], "focused": node["focused"], "enabled": node["enabled"], "checked": node["checked"]
    })).collect();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&stable)?)
    ))
}
pub(crate) async fn baseline(
    executor: &dyn CommandExecutor,
    request: &UiVerification,
) -> Option<Value> {
    // Shell uiautomator reads can legitimately take more than two seconds. Let the
    // caller's bounded observation budget apply to this pre-action read as well.
    timeout(
        Duration::from_millis(request.timeout_ms.unwrap_or(2000) as u64),
        snapshot(executor),
    )
    .await
    .ok()?
    .ok()
}
fn contains(nodes: &[Value], text: &str) -> bool {
    let hash = automation::full_text_hash(text);
    nodes
        .iter()
        .any(|node| automation::node_matches_text(node, text, &hash))
}
fn check(
    request: &UiVerification,
    current: Option<&Value>,
    before: Option<&Value>,
    focus: Option<&str>,
) -> (Value, bool) {
    let nodes = current.and_then(|value| complete_nodes(value).ok());
    let change = if request.verify_change {
        before
            .and_then(|b| signature(b).ok())
            .zip(current.and_then(|a| signature(a).ok()))
            .map(|(b, a)| b != a)
    } else {
        None
    };
    let package = request
        .expected_package
        .as_deref()
        .map(|expected| focus.map(|p| p == expected));
    let present = request
        .text_present
        .as_deref()
        .map(|text| nodes.map(|n| contains(n, text)));
    let absent = request
        .text_absent
        .as_deref()
        .map(|text| nodes.map(|n| !contains(n, text)));
    let checks = [
        request.verify_change.then_some(change),
        package,
        present,
        absent,
    ];
    let passed = checks
        .into_iter()
        .flatten()
        .all(|value| value == Some(true));
    let expectation = if package.is_none() && present.is_none() && absent.is_none() {
        None
    } else if [package, present, absent]
        .into_iter()
        .flatten()
        .any(|value| value == Some(false))
    {
        Some(false)
    } else if [package, present, absent]
        .into_iter()
        .flatten()
        .any(|value| value.is_none())
    {
        None
    } else {
        Some(true)
    };
    (
        json!({"ui_changed": change, "expectation_met": expectation, "package_matches": package.flatten(),
        "text_present": present.flatten(), "text_absent": absent.flatten(),
        "observation_complete": nodes.is_some(), "passed": passed,
        "scope": "current viewport; UI change alone does not prove business success"}),
        passed,
    )
}
async fn focused_package(executor: &dyn CommandExecutor) -> Result<Option<String>> {
    let result = executor
        .execute_quiet("dumpsys window | grep mCurrentFocus", false, false)
        .await?;
    if result.exit_code != Some(0) || result.timed_out || result.interrupted {
        return Ok(None);
    }
    let re = regex::Regex::new(r"mCurrentFocus=.*?\s([A-Za-z0-9_.]+)/[^\s}]+")?;
    Ok(re
        .captures(&result.stdout)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned()))
}
/// Return evidence and a conservative success flag; missing observations remain unknown.
pub(crate) async fn observe(
    executor: &dyn CommandExecutor,
    request: &UiVerification,
    before: Option<&Value>,
) -> Result<(Value, bool)> {
    let deadline =
        Instant::now() + Duration::from_millis(request.timeout_ms.unwrap_or(2000) as u64);
    let mut latest = check(request, None, before, None);
    loop {
        // Wrap the entire observation, so slow probes cannot exceed the requested deadline.
        let observed = tokio::time::timeout_at(deadline, async {
            let tree = if request.verify_change
                || request.text_present.is_some()
                || request.text_absent.is_some()
            {
                snapshot(executor).await.ok()
            } else {
                None
            };
            let focus = if request.expected_package.is_some() {
                focused_package(executor).await.ok().flatten()
            } else {
                None
            };
            check(request, tree.as_ref(), before, focus.as_deref())
        })
        .await;
        match observed {
            Ok(value) => latest = value,
            Err(_) => break,
        }
        if latest.1 || Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep_until((Instant::now() + Duration::from_millis(200)).min(deadline)).await;
    }
    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct SlowProbe;
    #[async_trait::async_trait]
    impl CommandExecutor for SlowProbe {
        async fn execute(
            &self,
            _: &str,
            _: bool,
            _: bool,
        ) -> Result<crate::shell::ExecutionResult> {
            tokio::time::sleep(Duration::from_secs(5)).await;
            bail!("probe unavailable")
        }
    }
    #[tokio::test]
    async fn baseline_uses_the_requested_budget_and_remains_bounded() {
        let request = UiVerification {
            verify_change: true,
            timeout_ms: Some(100),
            ..Default::default()
        };
        let start = Instant::now();
        assert!(baseline(&SlowProbe, &request).await.is_none());
        assert!(start.elapsed() < Duration::from_millis(500));
    }
    #[tokio::test]
    async fn slow_observation_respects_deadline_and_reports_unknown() -> Result<()> {
        let request = UiVerification {
            expected_package: Some("com.example".into()),
            timeout_ms: Some(100),
            ..Default::default()
        };
        let start = Instant::now();
        let (report, passed) = observe(&SlowProbe, &request, None).await?;
        assert!(!passed);
        assert!(report["expectation_met"].is_null());
        assert!(start.elapsed() < Duration::from_millis(500));
        Ok(())
    }

    #[test]
    fn partial_trees_never_prove_absence_or_change() {
        let request = UiVerification {
            verify_change: true,
            text_absent: Some("Pay".into()),
            ..Default::default()
        };
        let full = json!({"status":"complete","nodes":[]});
        let partial = json!({"status":"partial","nodes":[],"truncated":true});
        let (report, passed) = check(&request, Some(&partial), Some(&full), None);
        assert!(!passed);
        assert!(report["ui_changed"].is_null());
        assert!(report["expectation_met"].is_null());
    }
    #[test]
    fn changed_ui_does_not_satisfy_failed_postcondition() {
        let request = UiVerification {
            verify_change: true,
            text_present: Some("Done".into()),
            ..Default::default()
        };
        let before = json!({"status":"complete","nodes":[]});
        let after = json!({"status":"complete","nodes":[{"text":"Error","bounds":"[0,0][1,1]"}]});
        let (report, passed) = check(&request, Some(&after), Some(&before), None);
        assert!(!passed);
        assert_eq!(report["ui_changed"], true);
        assert_eq!(report["expectation_met"], false);
    }
    #[test]
    fn package_check_uses_focus_and_exact_text_checks() {
        let request = UiVerification {
            expected_package: Some("com.example".into()),
            text_present: Some("Done".into()),
            text_absent: Some("Loading".into()),
            ..Default::default()
        };
        let tree = json!({"status":"complete","nodes":[{"package":"com.example","text":"Done"}]});
        assert!(!check(&request, Some(&tree), None, Some("com.overlay")).1);
        assert!(check(&request, Some(&tree), None, Some("com.example")).1);
        assert!(!check(&request, Some(&tree), None, None).1);
    }
    #[test]
    fn invalid_or_empty_verifications_are_rejected() {
        assert!(UiVerification::default().validate().is_err());
        assert!(UiVerification {
            verify_change: true,
            timeout_ms: Some(10001),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(UiVerification {
            text_present: Some("X".into()),
            text_absent: Some("X".into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
