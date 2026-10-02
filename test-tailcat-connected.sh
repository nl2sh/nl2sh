#!/usr/bin/env bash
# Exercise raw Tailcat transfers in both directions between this host and one ADB device.
set -euo pipefail

HOST_BIN="${TAILCAT_HOST_BIN:-tailcat}"
DEVICE_BIN="${TAILCAT_DEVICE_BIN:-/data/local/tmp/tailcat}"
ADB_BIN="${ADB_BIN:-adb}"
SERIAL="${ADB_SERIAL:-}"
if [[ -z "$SERIAL" ]]; then
  mapfile -t devices < <("$ADB_BIN" devices | awk 'NR > 1 && $2 == "device" {print $1}')
  if [[ ${#devices[@]} -ne 1 ]]; then
    echo 'Set ADB_SERIAL when exactly one connected device cannot be selected.' >&2
    exit 1
  fi
  SERIAL="${devices[0]}"
fi
adb_device=("$ADB_BIN" -s "$SERIAL")
"$HOST_BIN" version
"${adb_device[@]}" shell "$DEVICE_BIN" version
work=$(mktemp -d)
remote="/data/local/tmp/nl2sh-tailcat-test-$$"
receiver_pid=''
cleanup() {
  if [[ -n "$receiver_pid" ]]; then kill "$receiver_pid" 2>/dev/null || true; wait "$receiver_pid" 2>/dev/null || true; fi
  "${adb_device[@]}" shell rm -rf "$remote" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT
"${adb_device[@]}" shell mkdir "$remote"
printf 'nl2sh tailcat cross-device test\n' > "$work/source"
"${adb_device[@]}" push "$work/source" "$remote/source" >/dev/null

# Device -> host.
"$HOST_BIN" --key=new > "$work/from-device" 2> "$work/host-listener.log" &
receiver_pid=$!
for _ in {1..100}; do
  address=$(sed -n 's/.*address: \(tc[A-Za-z0-9_-]*\).*/\1/p' "$work/host-listener.log" | head -n 1)
  [[ -n "$address" ]] && break
  kill -0 "$receiver_pid" 2>/dev/null || { cat "$work/host-listener.log" >&2; exit 1; }
  sleep .1
done
[[ -n "$address" ]] || { echo 'Host Tailcat address did not appear.' >&2; exit 1; }
"${adb_device[@]}" shell "$DEVICE_BIN" "$address" '<' "$remote/source"
wait "$receiver_pid"
receiver_pid=''
cmp "$work/source" "$work/from-device"

# Host -> device.
"${adb_device[@]}" shell "$DEVICE_BIN" --key=new '>' "$remote/from-host" > "$work/device-listener.log" 2>&1 &
receiver_pid=$!
address=''
for _ in {1..100}; do
  address=$(sed -n 's/.*address: \(tc[A-Za-z0-9_-]*\).*/\1/p' "$work/device-listener.log" | head -n 1)
  [[ -n "$address" ]] && break
  kill -0 "$receiver_pid" 2>/dev/null || { cat "$work/device-listener.log" >&2; exit 1; }
  sleep .1
done
[[ -n "$address" ]] || { echo 'Device Tailcat address did not appear.' >&2; exit 1; }
"$HOST_BIN" "$address" < "$work/source"
wait "$receiver_pid"
receiver_pid=''
"${adb_device[@]}" exec-out cat "$remote/from-host" > "$work/from-host"
cmp "$work/source" "$work/from-host"
echo "Tailcat transfers passed in both directions for $SERIAL."
