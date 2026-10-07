#!/usr/bin/env bash
set -euo pipefail
[[ $# -ge 1 ]] || { echo 'usage: sign-runtime-assets.sh asset...' >&2; exit 1; }
[[ -n "${TERMUX_APT_GPG_PRIVATE_KEY:-}" ]] || { echo 'TERMUX_APT_GPG_PRIVATE_KEY is required' >&2; exit 1; }
SIGNING_HOME="$(mktemp -d)"
chmod 700 "$SIGNING_HOME"
trap 'rm -rf "$SIGNING_HOME"' EXIT
export GNUPGHOME="$SIGNING_HOME"
printf '%s' "$TERMUX_APT_GPG_PRIVATE_KEY" | gpg --batch --import
SIGNING_FINGERPRINT=5230D3A7CCBEED4616D39C51FC6AD1BC63F7D4D8
gpg --batch --list-secret-keys "$SIGNING_FINGERPRINT" >/dev/null
for asset in "$@"; do
  gpg --batch --yes --local-user "${SIGNING_FINGERPRINT}!" --digest-algo SHA256 --detach-sign --output "${asset}.sig" "$asset"
  gpg --batch --verify "${asset}.sig" "$asset"
done
