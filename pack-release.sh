#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_NAME="nl2sh-android"
DIST_DIR="${PROJECT_DIR}/dist"
STAGING_ROOT="$(mktemp -d)"
PACKAGE_DIR="${STAGING_ROOT}/${PACKAGE_NAME}"

cleanup() {
  rm -rf -- "${STAGING_ROOT}"
}
trap cleanup EXIT

for command in cargo rustup; do
  command -v "${command}" >/dev/null 2>&1 || {
    echo "error: ${command} was not found in PATH" >&2
    exit 1
  }
done

cd "${PROJECT_DIR}"
for target in aarch64-linux-android armv7-linux-androideabi; do
  echo "Building ${target}..."
  RUST_TARGET="${target}" ./cross-compile.sh
done

mkdir -p "${PACKAGE_DIR}/bin/arm64-v8a" "${PACKAGE_DIR}/bin/armeabi-v7a" "${DIST_DIR}"
cp target/aarch64-linux-android/release/nl2sh "${PACKAGE_DIR}/bin/arm64-v8a/nl2sh"
cp target/armv7-linux-androideabi/release/nl2sh "${PACKAGE_DIR}/bin/armeabi-v7a/nl2sh"
cp android-run-linux.sh android-run-windows.bat config.toml.example 使用说明.md "${PACKAGE_DIR}/"
cp -R screenshots "${PACKAGE_DIR}/screenshots"
chmod +x "${PACKAGE_DIR}/android-run-linux.sh" \
  "${PACKAGE_DIR}/bin/arm64-v8a/nl2sh" \
  "${PACKAGE_DIR}/bin/armeabi-v7a/nl2sh"

ARCHIVE="${DIST_DIR}/${PACKAGE_NAME}.zip"
rm -f -- "${ARCHIVE}"
if command -v zip >/dev/null 2>&1; then
  (cd "${STAGING_ROOT}" && zip -q -r "${ARCHIVE}" "${PACKAGE_NAME}")
elif command -v python3 >/dev/null 2>&1; then
  python3 - "${STAGING_ROOT}" "${PACKAGE_NAME}" "${ARCHIVE}" <<'PY'
import os
import pathlib
import sys
import zipfile

root = pathlib.Path(sys.argv[1])
package = sys.argv[2]
archive = pathlib.Path(sys.argv[3])
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
    for path in sorted((root / package).rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_dir():
            relative += "/"
        info = zipfile.ZipInfo.from_file(path, relative)
        if path.is_dir():
            output.writestr(info, b"")
        else:
            with path.open("rb") as source:
                output.writestr(info, source.read(), compress_type=zipfile.ZIP_DEFLATED)
PY
else
  echo "error: zip or python3 is required to create ${ARCHIVE}" >&2
  exit 1
fi

(cd "${DIST_DIR}" && sha256sum -- "${PACKAGE_NAME}.zip" > SHA256SUMS)
echo "Created: ${ARCHIVE}"
echo "Checksum: ${DIST_DIR}/SHA256SUMS"
