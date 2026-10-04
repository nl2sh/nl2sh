#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="${PROJECT_DIR}/dist"

for command in cargo rustup dpkg-deb; do
  command -v "${command}" >/dev/null 2>&1 || {
    echo "error: ${command} was not found in PATH" >&2
    exit 1
  }
done

cd "${PROJECT_DIR}"
mkdir -p "${DIST_DIR}"

echo "Building aarch64 Termux package..."
RUST_TARGET=aarch64-linux-android \
  NL2SH_PACKAGE_MANAGER_BUILD=1 \
  ./cross-compile.sh
./packaging/termux/build-deb.sh \
  aarch64 \
  target/aarch64-linux-android/release/nl2sh \
  "${DIST_DIR}"

echo "Building arm Termux package..."
RUST_TARGET=armv7-linux-androideabi \
  NL2SH_PACKAGE_MANAGER_BUILD=1 \
  ./cross-compile.sh
./packaging/termux/build-deb.sh \
  arm \
  target/armv7-linux-androideabi/release/nl2sh \
  "${DIST_DIR}"

echo "Building x86_64 Termux package..."
RUST_TARGET=x86_64-linux-android \
  NL2SH_PACKAGE_MANAGER_BUILD=1 \
  ./cross-compile.sh
./packaging/termux/build-deb.sh \
  x86_64 \
  target/x86_64-linux-android/release/nl2sh \
  "${DIST_DIR}"

echo "Created Termux packages:"
find "${DIST_DIR}" -maxdepth 1 -type f \
  \( -name 'nl2sh_*_aarch64.deb' -o -name 'nl2sh_*_arm.deb' -o -name 'nl2sh_*_x86_64.deb' \) \
  -print
