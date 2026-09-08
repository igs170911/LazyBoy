#!/usr/bin/env bash
# Build one LazyBoy image. Single entry point for everything in image/.
#
#   scripts/build-image.sh                                   # desktop image, host arch, into docker
#   scripts/build-image.sh --file image/api/Dockerfile       # any image, host arch
#   scripts/build-image.sh --platforms linux/arm64           # one foreign arch, needs QEMU
#   scripts/build-image.sh --multi                           # amd64 + arm64 -> OCI archive
#   scripts/build-image.sh --multi --push                    # amd64 + arm64 -> registry
#
# Supported arches: linux/amd64 and linux/arm64 -- see the ceiling note below.
# --multi/--platforms cannot use the default "docker" builder: that driver
# cannot export a manifest list. A docker-container builder plus QEMU binfmt is
# bootstrapped here.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

file="image/computer/Dockerfile"
platforms=""
multi=0
push=0
target=""
oci_name=""
builder="${BUILDX_BUILDER:-lazyboy}"
tag=""

while [ $# -gt 0 ]; do
  case "$1" in
    --file) file="$2"; shift 2 ;;
    --tag) tag="$2"; shift 2 ;;
    --target) target="$2"; shift 2 ;;
    --platforms) platforms="$2"; shift 2 ;;
    --oci) oci_name="$2"; shift 2 ;;
    --builder) builder="$2"; shift 2 ;;
    --multi) multi=1; shift ;;
    --push) push=1; shift ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

# Defaults follow docker-compose.yml: the desktop image is published under a
# lazyboy/ namespace, the services under the compose project name.
if [ -z "$tag" ]; then
  case "$file" in
    image/computer/Dockerfile) tag="lazyboy/computer:local" ;;
    image/supervisor/Dockerfile) tag="lazyboy-supervisor:latest" ;;
    image/api/Dockerfile) tag="lazyboy-api:latest" ;;
    *) echo "--tag is required for $file" >&2; exit 2 ;;
  esac
fi
if [ "$multi" = 1 ] && [ -z "$platforms" ]; then
  platforms="linux/amd64,linux/arm64"
fi
if [ -z "$oci_name" ]; then
  case "$file" in
    image/computer/Dockerfile) oci_name="dist/lazyboy-computer-multi.oci.tar" ;;
    *) oci_name="dist/$(basename "$(dirname "$file")")-multi.oci.tar" ;;
  esac
fi

if [ "$push" = 1 ] && [ -z "$platforms" ]; then
  echo "--push only makes sense with --multi or --platforms" >&2
  exit 2
fi

# Architecture ceiling, set by two upstream binary distributions: the Cua Driver
# (cua-driver-rs-v0.23.2) ships Linux builds for x86_64 and arm64 only, and
# ONNX Runtime (v1.24.1) for x64 and aarch64 only. image/computer/Dockerfile and
# scripts/fetch-onnxruntime.sh fail fast on anything else; checking here rejects
# a bad --platforms before binfmt + GBs of cross-compilation are spent.
supported_arches="amd64 arm64"
for platform in ${platforms//,/ }; do
  os="${platform%%/*}"
  arch="${platform#*/}"
  arch="${arch%%/*}"
  if [ "$os" != "linux" ]; then
    echo "--platforms: only Linux images are built (got $platform)" >&2
    exit 2
  fi
  case " $supported_arches " in
    *" $arch "*) ;;
    *)
      echo "--platforms: unsupported architecture '$arch' in '$platform'." >&2
      echo "  LazyBoy images exist for linux/amd64 and linux/arm64: the Cua" >&2
      echo "  Driver ships only linux-x86_64/linux-arm64 and ONNX Runtime only" >&2
      echo "  linux-x64/linux-aarch64, so no other Linux arch can run them." >&2
      exit 2
      ;;
  esac
done

if [ -z "$platforms" ]; then
  # Native build: the docker driver, so the result lands in the local store and
  # `docker compose up` picks it up without a registry.
  args=(docker build -f "$file" -t "$tag")
  [ -n "$target" ] && args+=(--target "$target")
  "${args[@]}" .
  echo "built $tag"
  exit 0
fi

docker buildx inspect "$builder" >/dev/null 2>&1 \
  || docker buildx create --name "$builder" --driver docker-container --bootstrap
# QEMU is only needed for architectures the host cannot run natively; asking
# binfmt to register the host's own handler fails inside a privileged container
# and would look like a real failure, so the native arch is skipped.
native="$(dpkg --print-architecture 2>/dev/null || true)"
if [ -z "$native" ]; then
  case "$(uname -m)" in
    x86_64) native=amd64 ;;
    aarch64) native=arm64 ;;
    *) native=unknown ;;
  esac
fi
for platform in ${platforms//,/ }; do
  arch="${platform#*/}"
  if [ "$arch" != "$native" ]; then
    docker run --privileged --rm tonistiigi/binfmt --install "$arch" >/dev/null 2>&1 \
      || echo "WARNING: no QEMU handler for $arch; that build fails with 'exec format error'" >&2
  fi
done
args=(docker buildx build --builder "$builder" -f "$file" -t "$tag" --platform "$platforms")
[ -n "$target" ] && args+=(--target "$target")
if [ "$push" = 1 ]; then
  args+=(--push)
else
  mkdir -p "$(dirname "$oci_name")"
  args+=(--output "type=oci,dest=$oci_name")
fi
"${args[@]}" .
if [ "$push" = 1 ]; then
  echo "published $tag for ${platforms}"
else
  echo "wrote $oci_name for ${platforms} (load with: nerdctl load -i $oci_name)"
fi
