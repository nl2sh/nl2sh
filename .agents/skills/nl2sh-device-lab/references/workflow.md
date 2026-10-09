# Host helpers

All helpers run on the development host with Python 3.11+, Git and ADB; Android
needs only nl2sh, `/system/bin/sh` and toybox. They are a client/experiment
workflow, not a gateway. Run from the repository root:

```sh
python3 scripts/device_lab.py build --target x86_64-linux-android --manifest /private/lab/build.json
python3 scripts/device_lab.py deploy --serial emulator-5554 \
  --binary /data/local/tmp/nl2sh-lab/nl2sh --config /data/local/tmp/nl2sh-lab/config.toml \
  --manifest /private/lab/build.json --connection-output /private/lab/connection.json \
  --output /private/lab/deploy.json
python3 scripts/device_lab.py run --connection /private/lab/connection.json \
  --manifest /private/lab/build.json --case .agents/skills/nl2sh-device-lab/assets/environment-smoke.json \
  --output /private/lab/before.json
# After a code change: build, deploy and run the same case to after.json.
python3 scripts/device_lab.py compare --before /private/lab/before.json \
  --after /private/lab/after.json --output /private/lab/comparison.json
```

Use a private host directory outside the repository. Build sets a unique
`NL2SH_BUILD_ID` and binds relevant source files and the final artifact digest to
the manifest. It rejects source changes during compilation. Check/run compare
current sources to the manifest; stale sources or running images prevent testing.
Keep each build manifest with its corresponding report.

Deploy requires an existing readable private device configuration with
`protocol_start_with_service = true`. Create it explicitly for the authorized
test; helpers never modify policy or model settings. Run under the existing ADB
UID without `adb root` or `su`. Existing Helper ownership markers are refused.
Use a dedicated lab installation rather than a package-managed installation.
An independently started protocol must first be stopped by its owner.

Deploy verifies ABI and the staged digest, stops only the configuration's managed
service, atomically renames files, starts the service and verifies its running
identity. It retains the previous binary and tries rollback on failure. Read
`rollback` in the private report: failed rollback requires investigation, never
blind retries. This does not establish crash-consistent recovery after host death.

Connection discovery uses native verified service status and owner Web connection
details through short-lived ADB forwarding. A dedicated MCP forward remains until
removed with `adb -s SERIAL forward --remove tcp:LOCAL_PORT`; the connection file
is 0600 and contains a token. Rediscover after restart/rotation by deploying again
or use direct native MCP tools. Direct HTTP check/run accepts `--url` and a
`--token-env` reference; non-loopback plaintext requires `--allow-insecure-http`.
The bounded helper expects the native server's JSON responses, not SSE proxies.

Case schema 1 has an ID, preconditions (`android_min_api`, `required_tools`),
ordered steps (`id`, MCP `name`, `arguments`, optional `expect_error`), nonempty
assertions (`step`, JSON `pointer` into the MCP result, `equals` or
`manual_review: true`) and cleanup steps (`id`, invoke `arguments`).
Cleanup uses normal device approval. Unknown/partial evidence is inconclusive;
expected errors require explicit assertions. A task's COMPLETED state alone is
insufficient. Reports distinguish unverified Agent prose from actual observations;
root-cause hypotheses and recommendations remain reviewer-authored unless an
explicit diagnostic analysis is validated against observation IDs.

Reports and comparison files are 0600, bounded to 4 MiB and redact known API key
and token environment values. Device output can contain other private data;
review before sharing. Case changes make comparisons inconclusive; relevant
environment changes require manual review. Exit 0 means pass; other outcomes
return nonzero and remain recorded.
