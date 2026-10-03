# Sessions and audit logs

Private snapshots are saved after each complete Agent turn. The current model generates a title asynchronously after the first turn; failure does not affect the reply or save. Snapshots contain bounded conversations and tool results, excluding API keys, proxy passwords, balances, and temporary grants. Temporary images are not persisted.

```text
/sessions
/sessions resume NAME
/sessions rename OLD NEW
/sessions delete NAME
/new
/clear
```

`/sessions` sorts by update time; choose a number or Up/Down + Enter to restore. Stable names accept letters, digits, `-`, and `_`. Restore reapplies context/result limits. `/new` starts a new session; `/clear` removes current conversation, model context, and input recall while preserving audit logs and saved snapshots.

Direct deployment keeps state beside configuration; default Termux uses the XDG state `nl2sh` directory. Web has its own sidebar and in-progress diagnostics; see [Web](web.md).

Private JSONL audit records input, tools, output, and approvals after redaction and bounding. All Web sessions share a log; session deletion does not clear it. TUI interface settings can clear audit logs while allowing subsequent events. Reaching the file limit stops append; missing logs do not prove nothing happened.
