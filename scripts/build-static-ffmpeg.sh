#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="${1:-$root/vendor/ffmpeg}"
prefix="${2:-$root/target/ffmpeg-static}"
cache="${FFMPEG_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/flintplayer/ffmpeg-static}"
recipe_file=".flintplayer-recipe"

recipe() {
    local ffmpeg_rev
    ffmpeg_rev="$(git -C "$root" rev-parse HEAD:vendor/ffmpeg 2>/dev/null || echo missing)"
    printf '%s|%s\n' "$ffmpeg_rev" \
        "--disable-autodetect --enable-static --disable-shared --enable-pic --enable-v4l2-m2m --enable-indev=v4l2"
}

usable() {
    [ -f "$1/$recipe_file" ] \
        && [ "$(cat "$1/$recipe_file")" = "$stamp" ] \
        && [ -f "$1/lib/libavcodec.a" ] \
        && [ -f "$1/include/libavcodec/avcodec.h" ]
}

stamp="$(recipe)"

if usable "$prefix"; then
    echo "FFMPEG_DIR=$prefix"
    exit 0
fi

if usable "$cache"; then
    mkdir -p "$prefix"
    cp -a "$cache/." "$prefix/"
    echo "restored $cache -> $prefix"
    echo "FFMPEG_DIR=$prefix"
    exit 0
fi

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

printf '%s\n' "$stamp" > "$prefix/$recipe_file"
mkdir -p "$cache"
cp -a "$prefix/." "$cache/"

echo "FFMPEG_DIR=$prefix"
