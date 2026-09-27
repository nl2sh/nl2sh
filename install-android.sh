#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="https://github.com/nl2sh/nl2sh"
INSTALL_DIR="${PWD}/nl2sh-android"
PROVIDER="openrouter"
MODEL=""
API_KEY=""
ENDPOINT=""

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage: install-android.sh [options]
  --provider NAME       openrouter, openai, deepseek, moonshot, siliconflow,
                        ollama, or custom (default: openrouter)
  --model NAME          model name; provider default is used when omitted
  --api-key KEY         API key; prefer NL2SH_API_KEY to avoid shell history
  --endpoint URL        required for custom; optional override for other providers
  --install-dir PATH    extraction directory (default: ./nl2sh-android)
  --repository URL      GitHub repository URL (default: official repository)
EOF
}

while (($# > 0)); do
  case "$1" in
    --provider) [[ $# -ge 2 ]] || die "--provider requires a value"; PROVIDER="$2"; shift 2 ;;
    --model) [[ $# -ge 2 ]] || die "--model requires a value"; MODEL="$2"; shift 2 ;;
    --api-key) [[ $# -ge 2 ]] || die "--api-key requires a value"; API_KEY="$2"; shift 2 ;;
    --endpoint) [[ $# -ge 2 ]] || die "--endpoint requires a value"; ENDPOINT="$2"; shift 2 ;;
    --install-dir) [[ $# -ge 2 ]] || die "--install-dir requires a value"; INSTALL_DIR="$2"; shift 2 ;;
    --repository) [[ $# -ge 2 ]] || die "--repository requires a value"; REPOSITORY="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

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

TEMP_DIR="$(mktemp -d)"
cleanup() { rm -rf -- "${TEMP_DIR}"; }
trap cleanup EXIT
ARCHIVE="${TEMP_DIR}/nl2sh-android.zip"
SUMS="${TEMP_DIR}/SHA256SUMS"
DOWNLOAD_BASE="${REPOSITORY}/releases/latest/download"

echo "Downloading the latest nl2sh Android release..."
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

[[ ! -e "${INSTALL_DIR}" ]] || die "install directory already exists: ${INSTALL_DIR}"
if command -v unzip >/dev/null 2>&1; then
  unzip -q "${ARCHIVE}" -d "${TEMP_DIR}/unpacked"
else
  die "unzip was not found in PATH"
fi
[[ -d "${TEMP_DIR}/unpacked/nl2sh-android" ]] || die "release ZIP has an unexpected layout"
mv "${TEMP_DIR}/unpacked/nl2sh-android" "${INSTALL_DIR}"
chmod +x "${INSTALL_DIR}/android-run-linux.sh"

toml_escape() {
  local escaped="$1"
  escaped="${escaped//\\/\\\\}"
  escaped="${escaped//\"/\\\"}"
  printf '%s' "${escaped}"
}
CONFIG_FILE="${INSTALL_DIR}/config.toml"
umask 077
{
  printf 'api_key = "%s"\n' "$(toml_escape "${API_KEY}")"
  printf 'model = "%s"\n' "$(toml_escape "${MODEL}")"
  printf 'endpoint = "%s"\n' "$(toml_escape "${ENDPOINT}")"
  printf 'api_type = "auto"\n'
} > "${CONFIG_FILE}"
chmod 600 "${CONFIG_FILE}"

echo "Installed and verified: ${INSTALL_DIR}"
NL2SH_CONFIG_SOURCE="${CONFIG_FILE}" exec "${INSTALL_DIR}/android-run-linux.sh"
