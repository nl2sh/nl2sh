#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <aarch64|arm> <android-binary> <output-directory>" >&2
  exit 2
fi

TERMUX_ARCH="$1"
BINARY="$2"
OUTPUT_DIR="$3"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"

case "${TERMUX_ARCH}" in
  aarch64|arm) ;;
  *) echo "unsupported Termux architecture: ${TERMUX_ARCH}" >&2; exit 2 ;;
esac

[[ -n "${VERSION}" ]] || { echo "cannot read Cargo package version" >&2; exit 1; }
[[ -f "${BINARY}" ]] || { echo "binary is missing: ${BINARY}" >&2; exit 1; }

STAGING="$(mktemp -d)"
trap 'rm -rf "${STAGING}"' EXIT
chmod 0755 "${STAGING}"
PREFIX="${STAGING}/data/data/com.termux/files/usr"

install -Dm755 "${BINARY}" "${PREFIX}/bin/nl2sh"
install -Dm644 config.toml.example "${PREFIX}/share/nl2sh/config.toml.example"
install -Dm644 README.md "${PREFIX}/share/doc/nl2sh/README.md"
install -Dm644 LICENSE "${PREFIX}/share/doc/nl2sh/LICENSE"
mkdir -p "${STAGING}/DEBIAN" "${OUTPUT_DIR}"

cat > "${STAGING}/DEBIAN/control" <<EOF
Package: nl2sh
Version: ${VERSION}
Architecture: ${TERMUX_ARCH}
Maintainer: Ernest-su
Section: utils
Priority: optional
Homepage: https://github.com/nl2sh/nl2sh
Description: Android shell AI agent with local safety confirmation
 nl2sh provides a ratatui interface, tool calling, and a mandatory
 security and confirmation boundary before command execution.
EOF

dpkg-deb --root-owner-group --build "${STAGING}" \
  "${OUTPUT_DIR}/nl2sh_${VERSION}_${TERMUX_ARCH}.deb"
