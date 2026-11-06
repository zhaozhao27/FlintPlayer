#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="${1:-$root/vendor/ffmpeg}"
prefix="${2:-$root/target/ffmpeg-static}"

if [ ! -x "$src/configure" ]; then
    git -C "$root" submodule update --init --depth 1 vendor/ffmpeg
fi

(
    cd "$src"
    ./configure \
        --prefix="$prefix" \
        --disable-autodetect \
        --enable-static \
        --disable-shared \
        --enable-pic \
        --enable-v4l2-m2m \
        --enable-indev=v4l2
    make -j"$(nproc)"
    make install
)

echo "FFMPEG_DIR=$prefix"
