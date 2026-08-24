#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="${ANDROID_DIR:-/data/local/tmp}"
REMOTE_BINARY="${ANDROID_DIR}/nl2sh"

restore_host_terminal() {
  # adb transports the remote TUI's control sequences to this terminal.  If
  # the remote process or adb dies before its RAII guard runs, disable every
  # mouse tracking mode here so SGR mouse reports do not leak into the shell.
  if [[ -w /dev/tty ]]; then
    printf '\033[?1000l\033[?1002l\033[?1003l\033[?1015l\033[?1006l\033[?1049l\033[?25h' > /dev/tty
  else
    printf '\033[?1000l\033[?1002l\033[?1003l\033[?1015l\033[?1006l\033[?1049l\033[?25h'
  fi
}

run_remote() {
  trap restore_host_terminal EXIT
  local rows=24
  local cols=80
  local tty_size=""
  if [[ -r /dev/tty ]] && tty_size="$(stty size < /dev/tty 2>/dev/null)"; then
    if [[ "${tty_size}" =~ ^([0-9]+)[[:space:]]+([0-9]+)$ ]]; then
      rows="${BASH_REMATCH[1]}"
      cols="${BASH_REMATCH[2]}"
    fi
  fi

  local remote_command="stty rows ${rows} cols ${cols} 2>/dev/null; exec"
  local argument=""
  local quoted=""
  for argument in "$@"; do
    printf -v quoted "'%s'" "${argument//\'/\'\\\'\'}"
    remote_command+=" ${quoted}"
  done
  "${ADB[@]}" shell -t "${remote_command}"
}

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
if [[ ! "${ANDROID_DIR}" =~ ^/[A-Za-z0-9._/-]+$ ]]; then
  echo "error: ANDROID_DIR must be a safe absolute Android path: ${ANDROID_DIR}" >&2
  exit 1
fi

select_device
ADB=(adb -s "${SELECTED_SERIAL}")
echo "Selected device: ${SELECTED_SERIAL}"

ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abilist 2>/dev/null | tr -d '\r')"
if [[ -z "${ABILIST}" ]]; then
  ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abi 2>/dev/null | tr -d '\r')"
fi
echo "Device ABI: ${ABILIST}"
if [[ ",${ABILIST}," == *",arm64-v8a,"* ]]; then
  DETECTED_TARGET="aarch64-linux-android"
elif [[ ",${ABILIST}," == *",armeabi-v7a,"* ]]; then
  DETECTED_TARGET="armv7-linux-androideabi"
else
  die "unsupported device ABI '${ABILIST}'; supported ABIs are arm64-v8a and armeabi-v7a"
fi
if [[ -n "${RUST_TARGET:-}" && "${RUST_TARGET}" != "${DETECTED_TARGET}" ]]; then
  die "RUST_TARGET=${RUST_TARGET} does not match device ABI ${ABILIST} (${DETECTED_TARGET})"
fi
TARGET="${RUST_TARGET:-${DETECTED_TARGET}}"
LOCAL_BINARY="${PROJECT_DIR}/target/${TARGET}/release/nl2sh"
export RUST_TARGET="${TARGET}"
echo "Selected Rust target: ${TARGET}"

ADB_IS_ROOT=false
echo "Restarting adbd with root privileges..."
ADB_ROOT_OUTPUT="$("${ADB[@]}" root 2>&1 || true)"
if [[ -n "${ADB_ROOT_OUTPUT}" ]]; then
  echo "${ADB_ROOT_OUTPUT}"
fi
"${ADB[@]}" wait-for-device
if [[ "$("${ADB[@]}" shell id -u 2>/dev/null | tr -d '\r')" == "0" ]]; then
  ADB_IS_ROOT=true
  echo "adbd is running as root."
else
  echo "warning: adb root is unsupported or was denied; adbd remains non-root." >&2
fi

cd "${PROJECT_DIR}"
./cross-compile.sh

if [[ ! -x "${LOCAL_BINARY}" ]]; then
  echo "error: compiled binary was not found: ${LOCAL_BINARY}" >&2
  exit 1
fi

echo "Creating Android directory: ${ANDROID_DIR}"
"${ADB[@]}" shell mkdir -p "${ANDROID_DIR}"
echo "Pushing: ${LOCAL_BINARY} -> ${REMOTE_BINARY}"
"${ADB[@]}" push "${LOCAL_BINARY}" "${REMOTE_BINARY}"
"${ADB[@]}" shell chmod 755 "${REMOTE_BINARY}"

if [[ "${ADB_IS_ROOT}" == true ]]; then
  echo "Starting ${REMOTE_BINARY} through root adbd."
  echo "Press Ctrl+Q in nl2sh to exit."
  run_remote "${REMOTE_BINARY}"
  exit $?
fi

echo "Trying Android su as a fallback..."
if "${ADB[@]}" shell su -c id >/dev/null 2>&1; then
  echo "su access granted; starting ${REMOTE_BINARY} as root."
  echo "Press Ctrl+Q in nl2sh to exit."
  run_remote su -c "${REMOTE_BINARY}"
  exit $?
fi

REMOTE_CONFIG="${ANDROID_DIR}/config.toml"
if "${ADB[@]}" shell test -e "${REMOTE_CONFIG}" \
  && ! "${ADB[@]}" shell test -r "${REMOTE_CONFIG}"; then
  echo "error: adb root and su are unavailable, and ${REMOTE_CONFIG} is not readable." >&2
  echo "error: enable root adbd or repair the config owner; permissions remain unchanged to protect the API key." >&2
  exit 1
fi

echo "warning: adb root and su are unavailable; starting as adb shell user." >&2
echo "Press Ctrl+Q in nl2sh to exit."
run_remote "${REMOTE_BINARY}"
