#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════
# Cross-compile for Windows (x86_64-pc-windows-gnu) with
# statically-linked FFmpeg.
#
# Prerequisites:
#   rustup target add x86_64-pc-windows-gnu
#   dnf install mingw64-gcc mingw64-winpthreads-static
#   nasm yasm          (for FFmpeg assembly)
#
# Usage:
#   ./scripts/build-mingw.sh [--release] [cargo-args...]
#
# Output: target/x86_64-pc-windows-gnu/{debug,release}/*.exe
# ═══════════════════════════════════════════════════════════
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="x86_64-pc-windows-gnu"
FFMPEG_MINGW_DIR="${FFMPEG_MINGW_DIR:-$(pwd)/ffmpeg-mingw}"
FFMPEG_INSTALL="${FFMPEG_MINGW_DIR}/install"
FFMPEG_SOURCE="${FFMPEG_MINGW_DIR}/source"

MODE="${1:---debug}"
CARGO_ARGS=()
if [ "$MODE" = "--release" ]; then
    CARGO_ARGS+=("--release")
    shift
fi
CARGO_ARGS+=("$@")

build_ffmpeg() {
    if [ -f "${FFMPEG_INSTALL}/lib/libavcodec.a" ]; then
        echo "=== FFmpeg MinGW libs already built ==="
        return 0
    fi

    if [ ! -d "$FFMPEG_SOURCE" ]; then
        echo "ERROR: FFmpeg source not found at ${FFMPEG_SOURCE}"
        echo "Extract FFmpeg source tarball there, or symlink from:"
        echo "  target/debug/build/ffmpeg-sys-*/out/ffmpeg-*/"
        exit 1
    fi

    echo "=== Configuring FFmpeg for MinGW ==="
    cd "$FFMPEG_SOURCE"
    make clean 2>/dev/null || true

    ./configure \
      --prefix="${FFMPEG_INSTALL}" \
      --cross-prefix=x86_64-w64-mingw32- \
      --target-os=mingw32 \
      --arch=x86_64 \
      --enable-cross-compile \
      --extra-cflags='-w -pthread' \
      --extra-libs='-lwinpthread' \
      --disable-stripping \
      --enable-static --disable-shared \
      --enable-pic \
      --disable-autodetect \
      --disable-programs --disable-doc \
      --disable-gpl --disable-version3 --disable-nonfree \
      --enable-avcodec --enable-avdevice --enable-avfilter \
      --enable-avformat --enable-swresample --enable-swscale \
      --disable-indev=dshow

    echo "=== Building FFmpeg (this may take a while) ==="
    make -j"$(nproc)" install
    cd "$OLDPWD"
}

build_ffmpeg

echo "=== Cross-compiling Rust workspace for ${TARGET} ==="
FFMPEG_DIR="${FFMPEG_INSTALL}" \
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
    cargo build --target "${TARGET}" "${CARGO_ARGS[@]}"

echo "=== Build complete ==="
ls -lh "${FFMPEG_MINGW_DIR}/../target/${TARGET}/${MODE#--}/"*.exe 2>/dev/null || true
