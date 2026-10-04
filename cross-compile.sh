#!/usr/bin/env bash
set -euo pipefail

TARGET="${RUST_TARGET:-aarch64-linux-android}"
API_LEVEL="${ANDROID_API_LEVEL:-26}"
NDK_DIR="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"

if [[ -z "${NDK_DIR}" ]]; then
  echo "error: set ANDROID_NDK_HOME or ANDROID_NDK_ROOT" >&2
  exit 1
fi
if [[ ! -d "${NDK_DIR}" ]]; then
  echo "error: Android NDK directory does not exist: ${NDK_DIR}" >&2
  exit 1
fi

case "$(uname -s)" in
  Linux) HOST_TAG="linux-x86_64" ;;
  Darwin)
    case "$(uname -m)" in
      arm64) HOST_TAG="darwin-x86_64" ;;
      *) HOST_TAG="darwin-x86_64" ;;
    esac
    ;;
  *) echo "error: unsupported build host; use Linux or macOS" >&2; exit 1 ;;
esac

TOOLCHAIN="${NDK_DIR}/toolchains/llvm/prebuilt/${HOST_TAG}"
case "${TARGET}" in
  aarch64-linux-android)
    CLANG_TARGET="aarch64-linux-android"
    CARGO_PREFIX="AARCH64_LINUX_ANDROID"
    CC_SUFFIX="aarch64_linux_android"
    ;;
  x86_64-linux-android)
    CLANG_TARGET="x86_64-linux-android"
    CARGO_PREFIX="X86_64_LINUX_ANDROID"
    CC_SUFFIX="x86_64_linux_android"
    ;;
  armv7-linux-androideabi)
    CLANG_TARGET="armv7a-linux-androideabi"
    CARGO_PREFIX="ARMV7_LINUX_ANDROIDEABI"
    CC_SUFFIX="armv7_linux_androideabi"
    ;;
  *)
    echo "error: unsupported Rust target: ${TARGET}" >&2
    exit 1
    ;;
esac
CLANG="${TOOLCHAIN}/bin/${CLANG_TARGET}${API_LEVEL}-clang"
AR="${TOOLCHAIN}/bin/llvm-ar"
if [[ ! -x "${CLANG}" || ! -x "${AR}" ]]; then
  echo "error: NDK LLVM tools not found under ${TOOLCHAIN}" >&2
  exit 1
fi
if ! rustup target list --installed | grep -qx "${TARGET}"; then
  echo "error: Rust target is missing. Run: rustup target add ${TARGET}" >&2
  exit 1
fi

# Native dependencies such as ring are compiled through cc-rs rather than the
# final Cargo linker, so give their target-specific build scripts the same NDK
# tools explicitly.
declare "CARGO_TARGET_${CARGO_PREFIX}_LINKER=${CLANG}"
declare "CARGO_TARGET_${CARGO_PREFIX}_AR=${AR}"
declare "CC_${CC_SUFFIX}=${CLANG}"
declare "AR_${CC_SUFFIX}=${AR}"
export "CARGO_TARGET_${CARGO_PREFIX}_LINKER"
export "CARGO_TARGET_${CARGO_PREFIX}_AR"
export "CC_${CC_SUFFIX}"
export "AR_${CC_SUFFIX}"
CARGO_ARGS=(build --locked --release --target "${TARGET}")
if [[ "${NL2SH_PACKAGE_MANAGER_BUILD:-0}" == "1" ]]; then
  CARGO_ARGS+=(--no-default-features)
fi
cargo "${CARGO_ARGS[@]}"
echo "Built: target/${TARGET}/release/nl2sh"
