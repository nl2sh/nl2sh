#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TERMUX_PACKAGE="com.termux"
REMOTE_DIR="${ANDROID_TMP_DIR:-/data/local/tmp}"
LOCAL_PORT="${TERMUX_SSH_LOCAL_PORT:-8022}"
REMOTE_PORT="${TERMUX_SSH_REMOTE_PORT:-8022}"
TMUX_SESSION="${TERMUX_TMUX_SESSION:-nl2sh}"

die() { echo "error: $*" >&2; exit 1; }

restore_host_terminal() {
  local reset_sequence=$'\033[?1000l\033[?1002l\033[?1003l\033[?1015l\033[?1006l\033[?1049l\033[?25h'
  if [[ -w /dev/tty ]]; then
    printf '%s' "${reset_sequence}" > /dev/tty
  else
    printf '%s' "${reset_sequence}"
  fi
}

collect_devices() {
  DEVICE_SERIALS=()
  local serial="" state=""
  while read -r serial state _; do
    if [[ "${state:-}" == "device" ]] \
      && adb -s "${serial}" shell pm path "${TERMUX_PACKAGE}" 2>/dev/null \
        | tr -d '\r' | grep -q '^package:'; then
      DEVICE_SERIALS+=("${serial}")
    fi
  done < <(adb devices 2>/dev/null | tr -d '\r' | tail -n +2)
}

select_device() {
  if [[ -n "${ADB_SERIAL:-}" ]]; then
    [[ "$(adb -s "${ADB_SERIAL}" get-state 2>/dev/null || true)" == "device" ]] \
      || die "ADB_SERIAL is not a usable device: ${ADB_SERIAL}"
    adb -s "${ADB_SERIAL}" shell pm path "${TERMUX_PACKAGE}" 2>/dev/null \
      | tr -d '\r' | grep -q '^package:' \
      || die "${TERMUX_PACKAGE} is not installed on ${ADB_SERIAL}"
    SELECTED_SERIAL="${ADB_SERIAL}"
    return
  fi
  collect_devices
  ((${#DEVICE_SERIALS[@]} > 0)) \
    || die "no connected ADB device with ${TERMUX_PACKAGE} installed was found"
  if ((${#DEVICE_SERIALS[@]} == 1)); then
    SELECTED_SERIAL="${DEVICE_SERIALS[0]}"
    return
  fi
  echo "Multiple ADB devices with Termux installed were found:"
  local index choice
  for index in "${!DEVICE_SERIALS[@]}"; do
    printf '  %d. %s\n' "$((index + 1))" "${DEVICE_SERIALS[index]}"
  done
  read -r -p "Enter device number [1]: " choice
  choice="${choice:-1}"
  [[ "${choice}" =~ ^[1-9][0-9]*$ ]] || die "invalid device number"
  ((choice <= ${#DEVICE_SERIALS[@]})) || die "device number is out of range"
  SELECTED_SERIAL="${DEVICE_SERIALS[choice - 1]}"
}

termux_user_for_device() {
  local package_uid=""
  package_uid="$("${ADB[@]}" shell cmd package list packages -U \
    "${TERMUX_PACKAGE}" 2>/dev/null | tr -d '\r' \
    | sed -n 's/.*uid:\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  if [[ -z "${package_uid}" ]]; then
    package_uid="$("${ADB[@]}" shell dumpsys package "${TERMUX_PACKAGE}" \
      2>/dev/null | tr -d '\r' \
      | sed -n 's/^[[:space:]]*userId=\([0-9][0-9]*\).*/\1/p' \
      | head -n 1)"
  fi
  [[ "${package_uid}" =~ ^[0-9]+$ ]] || die "could not determine the Termux UID"
  local android_user_id=$((package_uid / 100000))
  local app_uid=$((package_uid % 100000))
  ((app_uid >= 10000)) || die "unexpected Termux UID: ${package_uid}"
  printf 'u%d_a%d\n' "${android_user_id}" "$((app_uid - 10000))"
}

for command in adb ssh cargo rustup dpkg-deb; do
  command -v "${command}" >/dev/null 2>&1 || die "${command} was not found in PATH"
done
[[ "${REMOTE_DIR}" =~ ^/[A-Za-z0-9._/-]+$ ]] \
  || die "ANDROID_TMP_DIR must be a safe absolute Android path: ${REMOTE_DIR}"
if ! [[ "${LOCAL_PORT}" =~ ^[0-9]+$ ]] \
  || ((LOCAL_PORT < 1 || LOCAL_PORT > 65535)); then
  die "TERMUX_SSH_LOCAL_PORT must be between 1 and 65535"
fi
if ! [[ "${REMOTE_PORT}" =~ ^[0-9]+$ ]] \
  || ((REMOTE_PORT < 1 || REMOTE_PORT > 65535)); then
  die "TERMUX_SSH_REMOTE_PORT must be between 1 and 65535"
fi
[[ "${TMUX_SESSION}" =~ ^[A-Za-z0-9._-]+$ ]] \
  || die "TERMUX_TMUX_SESSION contains unsupported characters"

select_device
ADB=(adb -s "${SELECTED_SERIAL}")
echo "Selected device: ${SELECTED_SERIAL}"
ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abilist 2>/dev/null | tr -d '\r')"
[[ -n "${ABILIST}" ]] \
  || ABILIST="$("${ADB[@]}" shell getprop ro.product.cpu.abi 2>/dev/null | tr -d '\r')"
case ",${ABILIST}," in
  *,x86_64,*) TARGET="x86_64-linux-android"; TERMUX_ARCH="x86_64" ;;
  *,arm64-v8a,*) TARGET="aarch64-linux-android"; TERMUX_ARCH="aarch64" ;;
  *,armeabi-v7a,*) TARGET="armv7-linux-androideabi"; TERMUX_ARCH="arm" ;;
  *) die "unsupported device ABI '${ABILIST}'" ;;
esac
echo "Device ABI: ${ABILIST}"
echo "Selected Rust target: ${TARGET} (${TERMUX_ARCH})"

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "${PROJECT_DIR}/Cargo.toml" | head -n 1)"
[[ -n "${VERSION}" ]] || die "could not read the package version from Cargo.toml"
LOCAL_BINARY="${PROJECT_DIR}/target/${TARGET}/release/nl2sh"
LOCAL_DEB="${PROJECT_DIR}/dist/nl2sh_${VERSION}_${TERMUX_ARCH}.deb"
REMOTE_DEB="${REMOTE_DIR}/nl2sh_${VERSION}_${TERMUX_ARCH}.deb"

cd "${PROJECT_DIR}"
echo "Building the Termux package..."
RUST_TARGET="${TARGET}" NL2SH_PACKAGE_MANAGER_BUILD=1 ./cross-compile.sh
./packaging/termux/build-deb.sh "${TERMUX_ARCH}" "${LOCAL_BINARY}" "${PROJECT_DIR}/dist"
[[ -f "${LOCAL_DEB}" ]] || die "the built package was not found: ${LOCAL_DEB}"

echo "Pushing ${LOCAL_DEB} to ${REMOTE_DEB}..."
"${ADB[@]}" shell mkdir -p "${REMOTE_DIR}"
"${ADB[@]}" push "${LOCAL_DEB}" "${REMOTE_DEB}"
"${ADB[@]}" shell chmod 0644 "${REMOTE_DEB}"

echo "Starting Termux and creating the SSH forward..."
"${ADB[@]}" shell am start -n "${TERMUX_PACKAGE}/.app.TermuxActivity" >/dev/null
TERMUX_USER="$(termux_user_for_device)"
"${ADB[@]}" forward --remove "tcp:${LOCAL_PORT}" >/dev/null 2>&1 || true
"${ADB[@]}" forward "tcp:${LOCAL_PORT}" "tcp:${REMOTE_PORT}" >/dev/null

echo "Connecting as ${TERMUX_USER}; tmux will install the package and run nl2sh."
echo "Prerequisites in Termux: pkg install openssh tmux; passwd; sshd"
REMOTE_COMMAND="apt install -y '${REMOTE_DEB}' && exec nl2sh"
REMOTE_SSH_COMMAND="if tmux has-session -t '${TMUX_SESSION}' 2>/dev/null; then tmux new-window -t '${TMUX_SESSION}' -n nl2sh-deploy \"${REMOTE_COMMAND}\"; else tmux new-session -d -s '${TMUX_SESSION}' -n nl2sh-deploy \"${REMOTE_COMMAND}\"; fi && exec tmux attach-session -t '${TMUX_SESSION}'"
trap restore_host_terminal EXIT
ssh -t -o "HostKeyAlias=termux-${SELECTED_SERIAL}" -p "${LOCAL_PORT}" \
  "${TERMUX_USER}@127.0.0.1" "${REMOTE_SSH_COMMAND}"
