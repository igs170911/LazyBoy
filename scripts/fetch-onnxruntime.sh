#!/usr/bin/env bash
# Download Microsoft's CPU-dispatched ONNX Runtime (not pyke's AVX2-static build).
# Usage: fetch-onnxruntime.sh [DEST_DIR]
set -euo pipefail

VERSION="${ORT_VERSION:-1.24.1}"
DEST="${1:-}"
if [ -z "$DEST" ]; then
  DEST="${DATA_DIR:-./data}/onnxruntime"
fi

os_raw="${TARGETOS:-$(uname -s | tr '[:upper:]' '[:lower:]')}"
arch_raw="${TARGETARCH:-$(uname -m)}"

case "$os_raw" in
  linux|darwin|macos) ;;
  *) echo "unsupported OS: $os_raw" >&2; exit 1 ;;
esac
[ "$os_raw" = macos ] && os_raw=darwin

case "$arch_raw" in
  amd64|x86_64|x64) arch_raw=x86_64 ;;
  arm64|aarch64) arch_raw=aarch64 ;;
esac

if [ "$os_raw" = linux ]; then
  if [ "$arch_raw" = x86_64 ]; then
    artifact="onnxruntime-linux-x64-${VERSION}.tgz"
    sha256="9142552248b735920f9390027e4512a2cacf8946a1ffcbe9071a5c210531026f"
  elif [ "$arch_raw" = aarch64 ]; then
    artifact="onnxruntime-linux-aarch64-${VERSION}.tgz"
    sha256="0f56edd68f7602df790b68b874a46b115add037e88385c6c842bb763b39b9f89"
  else
    echo "unsupported Linux arch: $arch_raw" >&2
    exit 1
  fi
  dylib_glob="libonnxruntime.so*"
elif [ "$os_raw" = darwin ]; then
  if [ "$arch_raw" = aarch64 ]; then
    artifact="onnxruntime-osx-arm64-${VERSION}.tgz"
  else
    artifact="onnxruntime-osx-universal2-${VERSION}.tgz"
  fi
  sha256=""
  dylib_glob="libonnxruntime.dylib"
else
  echo "unsupported OS: $os_raw" >&2
  exit 1
fi

url="https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}/${artifact}"
mkdir -p "$DEST"
stamp="$DEST/.ort-${VERSION}-$(basename "$artifact" .tgz)"
if [ -f "$stamp" ]; then
  dylib=$(find "$DEST" -maxdepth 3 \( -name 'libonnxruntime.so.1.*' -o -name 'libonnxruntime.dylib' \) | head -n 1)
  if [ -n "$dylib" ]; then
    printf '%s\n' "$dylib"
    exit 0
  fi
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
echo "downloading ONNX Runtime ${VERSION} ($artifact)" >&2
curl -fsSL "$url" -o "$tmp"
if [ -n "$sha256" ]; then
  echo "$sha256  $tmp" | sha256sum -c - >&2
fi
tar -xzf "$tmp" -C "$DEST" --strip-components=1
touch "$stamp"
dylib=$(find "$DEST" -maxdepth 3 \( -name 'libonnxruntime.so.1.*' -o -name 'libonnxruntime.dylib' \) | head -n 1)
if [ -z "$dylib" ]; then
  echo "ONNX Runtime dylib missing after extract in $DEST" >&2
  exit 1
fi
printf '%s\n' "$dylib"
