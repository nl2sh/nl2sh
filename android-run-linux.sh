#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="${ANDROID_DIR:-/data/local/tmp}"
REMOTE_BINARY="${ANDROID_DIR}/nl2sh"
REMOTE_CONFIG="${ANDROID_DIR}/config.toml"
WEB_ONLY=false

usage() {
  echo "Usage: android-run-linux.sh [--web-only]"
}

while (($# > 0)); do
  case "$1" in
    --web-only) WEB_ONLY=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "error: unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

die() {
  echo "error: $*" >&2
  exit 1
}

run_web_only() {
  local privilege_prefix="$1"
  local log_file="${ANDROID_DIR}/nl2sh-web.log"
  local command="cd '${ANDROID_DIR}' && nohup '${REMOTE_BINARY}' --web-only >'${log_file}' 2>&1 </dev/null &"
  if [[ "${privilege_prefix}" == "su" ]]; then
    "${ADB[@]}" shell su -c "${command}"
  else
    "${ADB[@]}" shell "${command}"
  fi
  echo "Web-only service started. Log: ${log_file}"
}

stop_existing_nl2sh() {
  local privilege_prefix="$1"
  local command='for process in /proc/[0-9]*; do [ -r "$process/comm" ] || continue; IFS= read -r name < "$process/comm" || continue; [ "$name" = nl2sh ] || continue; kill "${process##*/}" 2>/dev/null || true; done; sleep 1; for process in /proc/[0-9]*; do [ -r "$process/comm" ] || continue; IFS= read -r name < "$process/comm" || continue; [ "$name" = nl2sh ] || continue; kill -9 "${process##*/}" 2>/dev/null || true; done; exit 0'
  echo "Stopping existing nl2sh processes on the device..."
  if [[ "${privilege_prefix}" == "su" ]]; then
    "${ADB[@]}" shell su -c "${command}"
  else
    "${ADB[@]}" shell "${command}"
  fi
}

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -- "${path}" | awk '{print $1}'
  else
    die "sha256sum or shasum is required"
  fi
}

remote_sha256() {
  "${ADB[@]}" shell toybox sha256sum "${REMOTE_BINARY}" 2>/dev/null \
    | tr -d '\r' | awk 'NR == 1 {print $1}'
}

collect_devices() {
  DEVICE_SERIALS=()
  while read -r serial state _; do
    if [[ "${state:-}" == "device" ]]; then
      DEVICE_SERIALS+=("${serial}")
    fi
  done < <(adb devices 2>/dev/null | tr -d '\r' | tail -n +2)
}

select_device() {
  if [[ -n "${ADB_SERIAL:-}" ]]; then
    [[ "$(adb -s "${ADB_SERIAL}" get-state 2>/dev/null || true)" == "device" ]] \
      || die "ADB_SERIAL is not a usable device: ${ADB_SERIAL}"
    SELECTED_SERIAL="${ADB_SERIAL}"
    return
  fi

  collect_devices
  if ((${#DEVICE_SERIALS[@]} == 0)); then
    echo "No connected ADB device was found."
    read -r -p "Enter Android device IP or IP:port: " device_ip
    [[ -n "${device_ip:-}" ]] || die "no IP address was entered"
    adb connect "${device_ip}"
    collect_devices
  fi
  ((${#DEVICE_SERIALS[@]} > 0)) || die "no usable ADB device is connected"

  if ((${#DEVICE_SERIALS[@]} == 1)); then
    SELECTED_SERIAL="${DEVICE_SERIALS[0]}"
    return
  fi

  echo "Multiple ADB devices are connected:"
  local index
  for index in "${!DEVICE_SERIALS[@]}"; do
    printf '  %d. %s\n' "$((index + 1))" "${DEVICE_SERIALS[index]}"
  done
  read -r -p "Enter device number: " choice
  [[ "${choice:-}" =~ ^[1-9][0-9]*$ ]] || die "invalid device number"
  ((choice <= ${#DEVICE_SERIALS[@]})) || die "device number is out of range"
  SELECTED_SERIAL="${DEVICE_SERIALS[choice - 1]}"
}

command -v adb >/dev/null 2>&1 || die "adb was not found in PATH"
[[ "${ANDROID_DIR}" =~ ^/[A-Za-z0-9._/-]+$ ]] \
  || die "ANDROID_DIR must be a safe absolute Android path: ${ANDROID_DIR}"

select_device
ADB=(adb -s "${SELECTED_SERIAL}")
echo "Selected device: ${SELECTED_SERIAL}"

ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abilist 2>/dev/null | tr -d '\r')"
if [[ -z "${ABILIST}" ]]; then
  ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abi 2>/dev/null | tr -d '\r')"
fi
echo "Device ABI: ${ABILIST}"
if [[ ",${ABILIST}," == *",x86_64,"* ]]; then
  LOCAL_BINARY="${SCRIPT_DIR}/bin/x86_64/nl2sh"
  SELECTED_ABI="x86_64 (64-bit)"
elif [[ ",${ABILIST}," == *",arm64-v8a,"* ]]; then
  LOCAL_BINARY="${SCRIPT_DIR}/bin/arm64-v8a/nl2sh"
  SELECTED_ABI="arm64-v8a (64-bit)"
elif [[ ",${ABILIST}," == *",armeabi-v7a,"* ]]; then
  LOCAL_BINARY="${SCRIPT_DIR}/bin/armeabi-v7a/nl2sh"
  SELECTED_ABI="armeabi-v7a (32-bit)"
else
  die "unsupported device ABI '${ABILIST}'; this package supports arm64-v8a, armeabi-v7a and x86_64"
fi
[[ -f "${LOCAL_BINARY}" ]] || die "packaged binary is missing: ${LOCAL_BINARY}"
echo "Selected binary: ${SELECTED_ABI}"

ADB_IS_ROOT=false
echo "Restarting adbd with root privileges..."
ADB_ROOT_OUTPUT="$("${ADB[@]}" root 2>&1 || true)"
[[ -z "${ADB_ROOT_OUTPUT}" ]] || echo "${ADB_ROOT_OUTPUT}"
"${ADB[@]}" wait-for-device
if [[ "$("${ADB[@]}" shell id -u 2>/dev/null | tr -d '\r')" == "0" ]]; then
  ADB_IS_ROOT=true
  echo "adbd is running as root."
else
  echo "warning: adb root is unsupported or denied; trying normal adbd." >&2
fi

echo "Creating Android directory: ${ANDROID_DIR}"
"${ADB[@]}" shell mkdir -p "${ANDROID_DIR}"
LOCAL_SHA256="$(sha256_file "${LOCAL_BINARY}")"
REMOTE_SHA256="$(remote_sha256 || true)"
if [[ "${REMOTE_SHA256}" == "${LOCAL_SHA256}" ]]; then
  echo "Binary checksum matches; skipping adb push."
else
  echo "Pushing: ${LOCAL_BINARY} -> ${REMOTE_BINARY}"
  "${ADB[@]}" push "${LOCAL_BINARY}" "${REMOTE_BINARY}"
  REMOTE_SHA256="$(remote_sha256 || true)"
  [[ "${REMOTE_SHA256}" == "${LOCAL_SHA256}" ]] \
    || die "remote binary checksum verification failed after adb push"
  echo "Verified SHA-256: ${LOCAL_SHA256}"
fi
"${ADB[@]}" shell chmod 755 "${REMOTE_BINARY}"

if [[ -n "${NL2SH_CONFIG_SOURCE:-}" ]]; then
  [[ -f "${NL2SH_CONFIG_SOURCE}" ]] \
    || die "NL2SH_CONFIG_SOURCE is not a file: ${NL2SH_CONFIG_SOURCE}"
  echo "Deploying configuration: ${NL2SH_CONFIG_SOURCE} -> ${REMOTE_CONFIG}"
  if [[ "${ADB_IS_ROOT}" == true ]]; then
    "${ADB[@]}" push "${NL2SH_CONFIG_SOURCE}" "${REMOTE_CONFIG}" >/dev/null
    "${ADB[@]}" shell chmod 600 "${REMOTE_CONFIG}"
  else
    REMOTE_CONFIG_TEMP="${ANDROID_DIR}/.config.toml.nl2sh-adb"
    "${ADB[@]}" push "${NL2SH_CONFIG_SOURCE}" "${REMOTE_CONFIG_TEMP}" >/dev/null
    if "${ADB[@]}" shell su -c id >/dev/null 2>&1; then
      "${ADB[@]}" shell su -c "cp '${REMOTE_CONFIG_TEMP}' '${REMOTE_CONFIG}' && chmod 600 '${REMOTE_CONFIG}' && rm -f '${REMOTE_CONFIG_TEMP}'"
    else
      "${ADB[@]}" shell mv "${REMOTE_CONFIG_TEMP}" "${REMOTE_CONFIG}"
      "${ADB[@]}" shell chmod 600 "${REMOTE_CONFIG}"
    fi
  fi
fi

if [[ "${ADB_IS_ROOT}" == true ]]; then
  stop_existing_nl2sh root
  echo "Starting ${REMOTE_BINARY} through root adbd."
  if [[ "${WEB_ONLY}" == true ]]; then
    run_web_only root
    exit $?
  fi
  echo "Press Ctrl+Q in nl2sh to exit."
  exec "${ADB[@]}" shell -t "${REMOTE_BINARY}"
fi

echo "Trying Android su as a fallback..."
if "${ADB[@]}" shell su -c id >/dev/null 2>&1; then
  stop_existing_nl2sh su
  echo "su access granted; starting ${REMOTE_BINARY} as root."
  if [[ "${WEB_ONLY}" == true ]]; then
    run_web_only su
    exit $?
  fi
  echo "Press Ctrl+Q in nl2sh to exit."
  exec "${ADB[@]}" shell -t su -c "${REMOTE_BINARY}"
fi

if "${ADB[@]}" shell test -e "${REMOTE_CONFIG}" \
  && ! "${ADB[@]}" shell test -r "${REMOTE_CONFIG}"; then
  die "${REMOTE_CONFIG} exists but is unreadable; permissions remain unchanged to protect the API key"
fi

echo "warning: adb root and su are unavailable; starting as adb shell user." >&2
stop_existing_nl2sh shell
if [[ "${WEB_ONLY}" == true ]]; then
  run_web_only shell
  exit $?
fi
echo "Press Ctrl+Q in nl2sh to exit."
exec "${ADB[@]}" shell -t "${REMOTE_BINARY}"
