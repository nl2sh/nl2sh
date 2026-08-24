#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="${ANDROID_DIR:-/data/local/tmp}"
REMOTE_BINARY="${ANDROID_DIR}/nl2sh"

die() {
  echo "error: $*" >&2
  exit 1
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
if [[ ",${ABILIST}," == *",arm64-v8a,"* ]]; then
  LOCAL_BINARY="${SCRIPT_DIR}/bin/arm64-v8a/nl2sh"
  SELECTED_ABI="arm64-v8a (64-bit)"
elif [[ ",${ABILIST}," == *",armeabi-v7a,"* ]]; then
  LOCAL_BINARY="${SCRIPT_DIR}/bin/armeabi-v7a/nl2sh"
  SELECTED_ABI="armeabi-v7a (32-bit)"
else
  die "unsupported device ABI '${ABILIST}'; this package supports arm64-v8a and armeabi-v7a"
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
echo "Pushing: ${LOCAL_BINARY} -> ${REMOTE_BINARY}"
"${ADB[@]}" push "${LOCAL_BINARY}" "${REMOTE_BINARY}"
"${ADB[@]}" shell chmod 755 "${REMOTE_BINARY}"

if [[ "${ADB_IS_ROOT}" == true ]]; then
  echo "Starting ${REMOTE_BINARY} through root adbd."
  echo "Press Ctrl+Q in nl2sh to exit."
  exec "${ADB[@]}" shell -t "${REMOTE_BINARY}"
fi

echo "Trying Android su as a fallback..."
if "${ADB[@]}" shell su -c id >/dev/null 2>&1; then
  echo "su access granted; starting ${REMOTE_BINARY} as root."
  echo "Press Ctrl+Q in nl2sh to exit."
  exec "${ADB[@]}" shell -t su -c "${REMOTE_BINARY}"
fi

REMOTE_CONFIG="${ANDROID_DIR}/config.toml"
if "${ADB[@]}" shell test -e "${REMOTE_CONFIG}" \
  && ! "${ADB[@]}" shell test -r "${REMOTE_CONFIG}"; then
  die "${REMOTE_CONFIG} exists but is unreadable; permissions remain unchanged to protect the API key"
fi

echo "warning: adb root and su are unavailable; starting as adb shell user." >&2
echo "Press Ctrl+Q in nl2sh to exit."
exec "${ADB[@]}" shell -t "${REMOTE_BINARY}"
