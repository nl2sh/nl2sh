#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="https://github.com/nl2sh/nl2sh"
INSTALL_DIR="${PWD}/nl2sh-android"
PROVIDER="openrouter"
MODEL=""
API_KEY=""
ENDPOINT=""
CONFIG_REQUESTED=false
WEB_ONLY=false

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: install-android.sh [options]
Supported Android ABIs: arm64-v8a, armeabi-v7a, x86_64 (API 26+).
The launcher detects the device ABI and prefers native x86_64.
  --provider NAME       openrouter, openai, deepseek, moonshot, siliconflow,
                        ollama, or custom (default: openrouter)
  --model NAME          model name; provider default is used when omitted
  --api-key KEY         API key; prefer NL2SH_API_KEY to avoid shell history
  --endpoint URL        required for custom; optional override for other providers
  --install-dir PATH    extraction directory (default: ./nl2sh-android)
  --repository URL      GitHub or Gitee repository URL (default: GitHub)
  --web-only           start the Web UI in the background without a TUI
EOF
}

while (($# > 0)); do
  case "$1" in
    --provider) [[ $# -ge 2 ]] || die "--provider requires a value"; PROVIDER="$2"; CONFIG_REQUESTED=true; shift 2 ;;
    --model) [[ $# -ge 2 ]] || die "--model requires a value"; MODEL="$2"; CONFIG_REQUESTED=true; shift 2 ;;
    --api-key) [[ $# -ge 2 ]] || die "--api-key requires a value"; API_KEY="$2"; CONFIG_REQUESTED=true; shift 2 ;;
    --endpoint) [[ $# -ge 2 ]] || die "--endpoint requires a value"; ENDPOINT="$2"; CONFIG_REQUESTED=true; shift 2 ;;
    --install-dir) [[ $# -ge 2 ]] || die "--install-dir requires a value"; INSTALL_DIR="$2"; shift 2 ;;
    --repository) [[ $# -ge 2 ]] || die "--repository requires a value"; REPOSITORY="$2"; shift 2 ;;
    --web-only) WEB_ONLY=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

LAUNCH_ARGS=()
[[ "${WEB_ONLY}" == false ]] || LAUNCH_ARGS+=(--web-only)

command -v curl >/dev/null 2>&1 || die "curl was not found in PATH"
API_KEY="${API_KEY:-${NL2SH_API_KEY:-}}"
REPOSITORY="${REPOSITORY%/}"

case "${PROVIDER,,}" in
  openrouter) DEFAULT_ENDPOINT="https://openrouter.ai/api/v1"; DEFAULT_MODEL="openrouter/free" ;;
  openai) DEFAULT_ENDPOINT="https://api.openai.com/v1"; DEFAULT_MODEL="gpt-4o-mini" ;;
  deepseek) DEFAULT_ENDPOINT="https://api.deepseek.com"; DEFAULT_MODEL="deepseek-flash" ;;
  moonshot|kimi) DEFAULT_ENDPOINT="https://api.moonshot.cn/v1"; DEFAULT_MODEL="kimi-k2-turbo-preview" ;;
  siliconflow) DEFAULT_ENDPOINT="https://api.siliconflow.cn/v1"; DEFAULT_MODEL="Qwen/Qwen3-8B" ;;
  ollama) DEFAULT_ENDPOINT="http://127.0.0.1:11434/v1"; DEFAULT_MODEL="qwen3" ;;
  custom) [[ -n "${ENDPOINT}" ]] || die "--endpoint is required for provider custom"; DEFAULT_ENDPOINT="${ENDPOINT}"; DEFAULT_MODEL="custom-model" ;;
  *) die "unsupported provider: ${PROVIDER}" ;;
esac
ENDPOINT="${ENDPOINT:-${DEFAULT_ENDPOINT}}"
MODEL="${MODEL:-${DEFAULT_MODEL}}"

for value_name in ENDPOINT MODEL API_KEY; do
  value="${!value_name}"
  [[ "${value}" != *$'\n'* && "${value}" != *$'\r'* ]] \
    || die "${value_name,,} must not contain a newline"
done

toml_escape() {
  local escaped="$1"
  escaped="${escaped//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf '%s' "${escaped}"
}

upsert_toml_scalar() {
  local file="$1" key="$2" replacement="$3" temp current
  local found=false
  temp="$(mktemp "${file}.tmp.XXXXXX")"
  chmod 600 "${temp}"
  if [[ -f "${file}" ]]; then
    while IFS= read -r current || [[ -n "${current}" ]]; do
      if [[ "${current}" =~ ^${key}[[:space:]]*= ]]; then
        if [[ "${found}" == false ]]; then
          printf '%s\n' "${replacement}" >> "${temp}"
          found=true
        fi
      else
        printf '%s\n' "${current}" >> "${temp}"
      fi
    done < "${file}"
  fi
  if [[ "${found}" == false ]]; then
    printf '%s\n' "${replacement}" >> "${temp}"
  fi
  mv "${temp}" "${file}"
}

write_installer_config() {
  local file="$1" merge_existing="$2"
  if [[ "${merge_existing}" == false ]]; then
    (umask 077; : > "${file}")
  fi
  upsert_toml_scalar "${file}" "model" "model = \"$(toml_escape "${MODEL}")\""
  upsert_toml_scalar "${file}" "endpoint" "endpoint = \"$(toml_escape "${ENDPOINT}")\""
  upsert_toml_scalar "${file}" "api_type" 'api_type = "auto"'
  if [[ "${merge_existing}" == false || -n "${API_KEY}" ]]; then
    upsert_toml_scalar "${file}" "api_key" "api_key = \"$(toml_escape "${API_KEY}")\""
  fi
  chmod 600 "${file}"
}

EXISTING_CONFIG="${INSTALL_DIR}/config.toml"
if [[ -e "${INSTALL_DIR}" ]]; then
  [[ -d "${INSTALL_DIR}" ]] || die "install path exists but is not a directory: ${INSTALL_DIR}"
  if [[ ! -f "${INSTALL_DIR}/android-run-linux.sh" \
    || ! -f "${INSTALL_DIR}/bin/arm64-v8a/nl2sh" \
    || ! -f "${INSTALL_DIR}/bin/armeabi-v7a/nl2sh" ]]; then
    die "install directory exists but is incomplete: ${INSTALL_DIR}"
  fi
  if [[ ! -f "${INSTALL_DIR}/bin/x86_64/nl2sh" ]]; then
    echo "warning: existing package lacks x86_64; ARM devices can continue. For x86_64, back up config.toml and use --install-dir with a new directory and a release containing bin/x86_64/nl2sh." >&2
  fi
  chmod +x "${INSTALL_DIR}/android-run-linux.sh"
  if [[ "${CONFIG_REQUESTED}" == true || ! -f "${EXISTING_CONFIG}" ]]; then
    write_installer_config "${EXISTING_CONFIG}" true
    echo "Updated existing provider configuration without replacing other settings."
  else
    echo "Keeping the existing provider configuration."
  fi
  echo "Existing verified installation found: ${INSTALL_DIR}"
  if [[ -t 0 ]]; then
    NL2SH_CONFIG_SOURCE="${EXISTING_CONFIG}" exec "${INSTALL_DIR}/android-run-linux.sh" "${LAUNCH_ARGS[@]}"
  fi
  if exec 3</dev/tty; then
    NL2SH_CONFIG_SOURCE="${EXISTING_CONFIG}" exec "${INSTALL_DIR}/android-run-linux.sh" "${LAUNCH_ARGS[@]}" <&3
  fi
  die "installation is ready, but no controlling terminal is available; run ${INSTALL_DIR}/android-run-linux.sh from an interactive terminal"
fi

TEMP_DIR="$(mktemp -d)"
cleanup() { rm -rf -- "${TEMP_DIR}"; }
trap cleanup EXIT
ARCHIVE="${TEMP_DIR}/nl2sh-android.zip"
SUMS="${TEMP_DIR}/SHA256SUMS"

release_download_base() {
  local repository="$1"
  if [[ "${repository}" == https://gitee.com/*/* ]]; then
    local repository_path metadata tag
    repository_path="${repository#https://gitee.com/}"
    metadata="$(curl -fsSL --retry 3 --proto '=https' --tlsv1.2 \
      "https://gitee.com/api/v5/repos/${repository_path}/releases/latest")" \
      || die "failed to query the latest Gitee release; ensure the mirror has synchronized release assets"
    tag="$(printf '%s' "${metadata}" | sed -n 's/.*"tag_name":"\([^"]*\)".*/\1/p')"
    [[ "${tag}" =~ ^[A-Za-z0-9._-]+$ ]] \
      || die "the latest Gitee release returned an invalid or missing tag"
    printf '%s/releases/download/%s' "${repository}" "${tag}"
    return
  fi
  printf '%s/releases/latest/download' "${repository}"
}

DOWNLOAD_BASE="$(release_download_base "${REPOSITORY}")"

echo "Downloading the latest nl2sh Android release from ${REPOSITORY}..."
curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "${ARCHIVE}" "${DOWNLOAD_BASE}/nl2sh-android.zip"
curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "${SUMS}" "${DOWNLOAD_BASE}/SHA256SUMS"
EXPECTED_SHA256="$(awk '$2 == "nl2sh-android.zip" || $2 == "*nl2sh-android.zip" {print $1; exit}' "${SUMS}")"
[[ "${EXPECTED_SHA256}" =~ ^[0-9a-fA-F]{64}$ ]] || die "release checksum for nl2sh-android.zip is missing"
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(sha256sum -- "${ARCHIVE}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA256="$(shasum -a 256 -- "${ARCHIVE}" | awk '{print $1}')"
else
  die "sha256sum or shasum is required"
fi
[[ "${ACTUAL_SHA256,,}" == "${EXPECTED_SHA256,,}" ]] || die "downloaded ZIP checksum mismatch"

if command -v unzip >/dev/null 2>&1; then
  unzip -q "${ARCHIVE}" -d "${TEMP_DIR}/unpacked"
else
  die "unzip was not found in PATH"
fi
[[ -d "${TEMP_DIR}/unpacked/nl2sh-android" ]] || die "release ZIP has an unexpected layout"
mv "${TEMP_DIR}/unpacked/nl2sh-android" "${INSTALL_DIR}"
chmod +x "${INSTALL_DIR}/android-run-linux.sh"

CONFIG_FILE="${INSTALL_DIR}/config.toml"
write_installer_config "${CONFIG_FILE}" false

echo "Installed and verified: ${INSTALL_DIR}"
if [[ -t 0 ]]; then
  NL2SH_CONFIG_SOURCE="${CONFIG_FILE}" exec "${INSTALL_DIR}/android-run-linux.sh" "${LAUNCH_ARGS[@]}"
fi

# `curl ... | bash` makes the downloaded script occupy stdin. The launcher and
# nl2sh TUI need the host's controlling terminal instead, including for device
# selection prompts, so reconnect stdin only after Bash has consumed the script.
if exec 3</dev/tty; then
  NL2SH_CONFIG_SOURCE="${CONFIG_FILE}" exec "${INSTALL_DIR}/android-run-linux.sh" "${LAUNCH_ARGS[@]}" <&3
fi

die "installation completed, but no controlling terminal is available; run ${INSTALL_DIR}/android-run-linux.sh from an interactive terminal"
