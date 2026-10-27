#!/usr/bin/env bash
# Builds llama.cpp's llama-server (the built-in clean-up engine) from the
# pinned release's source, as one static file with the Metal shaders inside,
# and puts it where tauri's externalBin (src-tauri/tauri.macos.conf.json)
# expects it: src-tauri/llama-server/bin/llama-server-<target triple>.
#
# The official release archive is not used because its llama-server needs a
# dozen .dylibs next to it. llama.cpp is not linked into the app itself:
# transcribe.cpp already compiles its own ggml, and two copies crash.
#
# Usage: scripts/fetch-llama-server.sh [target triple]   (default: this Mac)
set -euo pipefail

TAG="b11935"
SOURCE_SHA256="0bfc7af823a4c25fb2d21dfdce40eb878aa76dbbcc16648e679cc56796f6692c"
SOURCE_URL="https://github.com/ggml-org/llama.cpp/archive/refs/tags/${TAG}.tar.gz"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TRIPLE="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$TRIPLE" in
  aarch64-apple-darwin) ARCH=arm64 ;;
  x86_64-apple-darwin) ARCH=x86_64 ;;
  *) echo "llama-server is only bundled for macOS (got $TRIPLE)" >&2; exit 1 ;;
esac
OUT_DIR="$ROOT/src-tauri/llama-server/bin"
OUT="$OUT_DIR/llama-server-$TRIPLE"
STAMP="$OUT_DIR/.llama-server-$TRIPLE.tag"

if [[ -x "$OUT" && "$(cat "$STAMP" 2>/dev/null)" == "$TAG" ]]; then
  echo "llama-server $TAG already at $OUT"
  exit 0
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Downloading llama.cpp $TAG source"
curl -fsSL --retry 3 -o "$WORK/src.tar.gz" "$SOURCE_URL"
ACTUAL="$(shasum -a 256 "$WORK/src.tar.gz" | cut -d' ' -f1)"
if [[ "$ACTUAL" != "$SOURCE_SHA256" ]]; then
  echo "Checksum mismatch for $SOURCE_URL: expected $SOURCE_SHA256, got $ACTUAL" >&2
  exit 1
fi
tar -xzf "$WORK/src.tar.gz" -C "$WORK"
SRC="$WORK/llama.cpp-$TAG"

# GGML_NATIVE=OFF: a binary built on a newer chip must still run on an M1.
# No HTTPS, web UI or tests: the app only talks to it on 127.0.0.1.
echo "Building llama-server for $TRIPLE"
cmake -S "$SRC" -B "$WORK/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_OSX_ARCHITECTURES="$ARCH" \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=13.3 \
  -DBUILD_SHARED_LIBS=OFF \
  -DGGML_METAL=ON \
  -DGGML_METAL_EMBED_LIBRARY=ON \
  -DGGML_NATIVE=OFF \
  -DLLAMA_OPENSSL=OFF \
  -DLLAMA_BUILD_TESTS=OFF \
  -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_UI=OFF \
  -DLLAMA_USE_PREBUILT_UI=OFF \
  -DLLAMA_BUILD_APP=OFF \
  >"$WORK/configure.log" || { cat "$WORK/configure.log" >&2; exit 1; }
cmake --build "$WORK/build" --config Release --target llama-server -j "${LLAMA_BUILD_JOBS:-6}" \
  >"$WORK/build.log" || { tail -50 "$WORK/build.log" >&2; exit 1; }

BIN="$WORK/build/bin/llama-server"
otool -L "$BIN" >/dev/null
if otool -L "$BIN" | tail -n +2 | grep -v -e '^\s*/usr/lib/' -e '^\s*/System/'; then
  echo "llama-server links libraries outside the system; it would not run from the app bundle" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
cp "$BIN" "$OUT.tmp"
strip -x "$OUT.tmp"
mv "$OUT.tmp" "$OUT"
cp "$SRC/LICENSE" "$OUT_DIR/LICENSE-llama.cpp"
echo "$TAG" >"$STAMP"
echo "llama-server $TAG -> $OUT ($(du -h "$OUT" | cut -f1))"
