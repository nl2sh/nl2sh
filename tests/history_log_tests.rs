use nl2sh::history::HistoryLog;
use std::fs;
use tempfile::tempdir;

#[test]
fn history_log_is_json_lines_and_resolves_beside_config() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    let log = HistoryLog::open(&config, std::path::Path::new("history.jsonl"))?;
    log.record("user", "查看 com.example 的版本")?;

    let line = fs::read_to_string(directory.path().join("history.jsonl"))?;
    let value: serde_json::Value = serde_json::from_str(line.trim())?;
    assert_eq!(value["event"], "user");
    assert_eq!(value["message"], "查看 com.example 的版本");
    assert!(value["timestamp_ms"].as_u64().is_some());
    Ok(())
}

#[test]
fn history_limits_are_explicit_and_bounded() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.toml");
    let log =
        HistoryLog::open_with_limits(&config, std::path::Path::new("bounded.jsonl"), 160, 700)?;
    log.record("large", &"界".repeat(200))?;
    for _ in 0..20 {
        log.record("repeat", &"x".repeat(100))?;
    }
    let contents = fs::read_to_string(directory.path().join("bounded.jsonl"))?;
    assert!(contents.len() <= 700);
    assert!(contents.contains("NL2SH OUTPUT TRUNCATED"));
    assert!(contents.contains("log_limit"));
    Ok(())
}

#[test]
fn clearing_history_resets_the_file_and_allows_new_records() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config = directory.path().join("config.toml");
    let path = directory.path().join("history.jsonl");
    let log = HistoryLog::open(&config, std::path::Path::new("history.jsonl"))?;
    log.record("before", "old")?;
    log.clear()?;
    assert_eq!(fs::metadata(&path)?.len(), 0);
    log.record("after", "new")?;
    let contents = fs::read_to_string(path)?;
    assert!(!contents.contains("old"));
    assert!(contents.contains("new"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn new_history_log_is_private() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir()?;
    let path = directory.path().join("nl2sh.log");
    let _log = HistoryLog::open(
        &directory.path().join("config.toml"),
        std::path::Path::new("nl2sh.log"),
    )?;
    assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
    Ok(())
}
