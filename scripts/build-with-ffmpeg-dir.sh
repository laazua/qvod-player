#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════
# Build with pre-compiled FFmpeg static libraries (offline builds)
#
# Usage:
#   FFMPEG_DIR=/path/to/ffmpeg ./scripts/build-with-ffmpeg-dir.sh [cargo-args...]
#
# FFMPEG_DIR must contain:
#   include/  (FFmpeg headers, e.g. libavcodec/, libavformat/, ...)
#   lib/      (FFmpeg static .a / .lib files)
#
# Examples:
#   # Linux build with cached FFmpeg
#   export FFMPEG_DIR=$(pwd)/target/debug/build/ffmpeg-sys-*/out/dist
#   ./scripts/build-with-ffmpeg-dir.sh
#
#   # Windows cross-compile (see also build-mingw.sh)
#   export FFMPEG_DIR=$(pwd)/ffmpeg-mingw/install
#   export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
#   ./scripts/build-with-ffmpeg-dir.sh --target x86_64-pc-windows-gnu
# ═══════════════════════════════════════════════════════════
set -euo pipefail

cd "$(dirname "$0")/.."

if [ -z "${FFMPEG_DIR:-}" ]; then
    echo "ERROR: FFMPEG_DIR is not set."
    echo "Usage: FFMPEG_DIR=/path/to/ffmpeg $0 [cargo-args...]"
    exit 1
fi

if [ ! -d "${FFMPEG_DIR}/include" ] && [ ! -d "${FFMPEG_DIR}/lib" ]; then
    echo "ERROR: FFMPEG_DIR='${FFMPEG_DIR}' must contain include/ and lib/."
    exit 1
fi

echo "=== Building with FFMPEG_DIR=${FFMPEG_DIR} ==="

PKG_CONFIG_ALLOW_CROSS=1 \
FFMPEG_DIR="${FFMPEG_DIR}" \
PKG_CONFIG_PATH="${FFMPEG_DIR}/lib/pkgconfig${PKG_CONFIG_PATH:+:}${PKG_CONFIG_PATH:-}" \
    cargo build "$@"

echo "=== Build complete ==="
