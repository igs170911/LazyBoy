#!/usr/bin/env bash
# Installs what LazyBoy needs and this Linux host is missing, then builds LazyBoy (scripts/bootstrap.sh).
# System packages come from the distribution's package manager through sudo; Node.js and uv go into ~/.local
# without sudo. Tools that are already installed are left alone, so running it again is safe.
#   scripts/install.sh          install what is missing, then build LazyBoy
#   scripts/install.sh --check  list what is missing and how it would be installed; change nothing
#   scripts/install.sh --deps   install what is missing, but do not build LazyBoy
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$project_dir"
mode=${1:-}
case "$mode" in ""|--check|--deps) ;; *) printf '用法：scripts/install.sh [--check|--deps]\n' >&2; exit 2 ;; esac
if [ "$(uname -s)" != Linux ]; then printf 'LazyBoy 目前只支援 Linux。\n' >&2; exit 1; fi

NODE_MAJOR=24
export PATH="$HOME/.local/bin:$PATH"
user=$(id -un 2>/dev/null || printf %s "${USER:-$(id -u)}")
sudo=sudo
if [ "$(id -u)" -eq 0 ]; then sudo=""; fi
has() { command -v "$1" >/dev/null 2>&1; }
node_major() { node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0; }
in_docker_group() { id -nG "$user" | tr ' ' '\n' | grep -qx docker; }
if has apt-get; then pm=apt; elif has dnf; then pm=dnf; elif has pacman; then pm=pacman; elif has zypper; then pm=zypper; else pm=""; fi

steps=(); plan=(); root_steps=0
step() { steps+=("$1"); plan+=("$2"); if [ "$3" = root ]; then root_steps=$((root_steps + 1)); fi; }

basics=()
for tool in git make python3 curl tar; do has "$tool" || basics+=("$tool"); done
if [ ${#basics[@]} -gt 0 ]; then step install_basics "安裝基本工具：${basics[*]}" root; fi
docker_ok=false
if has docker && docker info >/dev/null 2>&1; then docker_ok=true; fi
if ! has docker; then step install_docker "安裝 Docker Engine（含 Buildx 與 Compose）" root
elif ! docker buildx version >/dev/null 2>&1; then step install_buildx "安裝 Docker Buildx" root; fi
if ! $docker_ok; then
  if has systemctl && ! systemctl is-active --quiet docker 2>/dev/null; then step start_docker "啟動 Docker，並設定開機自動啟動" root; fi
  if [ -n "$sudo" ] && ! in_docker_group; then step join_docker_group "把 $user 加入 docker 群組，之後不用 sudo 就能使用 Docker" root; fi
fi
if ! has node || [ "$(node_major)" -lt 20 ]; then step install_node "安裝 Node.js $NODE_MAJOR 到 ~/.local（不需要 sudo）" user; fi
if ! has uv; then step install_uv "安裝 uv 到 ~/.local/bin（不需要 sudo）" user; fi

if [ ${#steps[@]} -eq 0 ]; then
  printf '需要的系統套件都已安裝。\n'
else
  printf '需要安裝的項目%s：\n' "${pm:+（套件管理員：$pm）}"
  for line in "${plan[@]}"; do printf '  - %s\n' "$line"; done
fi
if [ "$mode" = --check ]; then [ ${#steps[@]} -eq 0 ] && exit 0 || exit 1; fi
if [ "$root_steps" -gt 0 ] && [ -n "$sudo" ] && ! has sudo; then
  printf '上面的系統套件需要 root 權限，但這台主機沒有 sudo。請管理員用 root 安裝這些套件（並把 %s 加入 docker 群組），再執行 make install。\n' "$user" >&2
  exit 1
fi
if [ "$root_steps" -gt 0 ] && [ -n "$sudo" ] && ! sudo -n true 2>/dev/null && [ ! -t 0 ]; then
  printf '上面標示的系統套件需要 sudo 密碼，但這裡沒有終端機可以輸入。請在終端機自己執行 make install（或 ./scripts/install.sh）。\n' >&2
  exit 1
fi

package_install() {
  case "$pm" in
    apt) $sudo apt-get update && $sudo apt-get install -y --no-install-recommends "$@" ;;
    dnf) $sudo dnf install -y "$@" ;;
    # Arch supports installing only on an up-to-date system; the full upgrade stays the user's call.
    pacman) $sudo pacman -S --needed --noconfirm "$@" || { printf 'pacman 安裝失敗。Arch 需要先更新系統：sudo pacman -Syu，然後再執行一次 make install。\n' >&2; exit 1; } ;;
    zypper) $sudo zypper --non-interactive install "$@" ;;
    *) printf '找不到支援的套件管理員（apt、dnf、pacman、zypper），請手動安裝：%s\n' "$*" >&2; exit 1 ;;
  esac
}
install_basics() {
  local names=() tool
  for tool in "${basics[@]}"; do
    if [ "$pm:$tool" = pacman:python3 ]; then names+=(python); else names+=("$tool"); fi
  done
  if [ "$pm" = apt ]; then names+=(ca-certificates); fi
  package_install "${names[@]}"
}
install_docker() {
  case "$pm" in
    pacman|zypper) package_install docker docker-buildx docker-compose ;;
    *)
      # Docker's official installer adds Docker's package repository and installs the engine, Buildx and Compose.
      local script; script=$(mktemp)
      curl -fsSL https://get.docker.com -o "$script"
      $sudo sh "$script"
      rm -f "$script" ;;
  esac
}
install_buildx() {
  case "$pm" in
    apt) package_install docker-buildx-plugin || package_install docker-buildx ;;
    dnf) package_install docker-buildx-plugin ;;
    *) package_install docker-buildx ;;
  esac
}
start_docker() { $sudo systemctl enable --now docker; }
join_docker_group() { $sudo usermod -aG docker "$user"; }
install_node() {
  local arch base name dir tmp
  case "$(uname -m)" in x86_64) arch=x64 ;; aarch64|arm64) arch=arm64 ;; *) printf '不支援的 CPU 架構：%s\n' "$(uname -m)" >&2; exit 1 ;; esac
  base="https://nodejs.org/dist/latest-v$NODE_MAJOR.x"
  tmp=$(mktemp -d)
  curl -fsSL "$base/SHASUMS256.txt" -o "$tmp/SHASUMS256.txt"
  name=$(grep -oE "node-v[0-9.]+-linux-$arch\.tar\.gz$" "$tmp/SHASUMS256.txt" | head -n 1)
  curl -fsSL "$base/$name" -o "$tmp/$name"
  (cd "$tmp" && grep "  $name\$" SHASUMS256.txt | sha256sum -c --status)
  mkdir -p "$HOME/.local/lib/nodejs" "$HOME/.local/bin"
  tar -xzf "$tmp/$name" -C "$HOME/.local/lib/nodejs"
  dir="$HOME/.local/lib/nodejs/${name%.tar.gz}"
  for bin in node npm npx; do ln -sf "$dir/bin/$bin" "$HOME/.local/bin/$bin"; done
  rm -rf "$tmp"
  printf 'Node.js %s 已安裝到 %s\n' "$("$HOME/.local/bin/node" --version)" "$dir"
}
install_uv() {
  local script; script=$(mktemp)
  curl -LsSf https://astral.sh/uv/install.sh -o "$script"
  sh "$script"
  rm -f "$script"
}

for name in "${steps[@]}"; do "$name"; done
hash -r

# A new docker group membership only applies to new logins; until then, run Docker inside it with sg.
in_group_now=true
if [ -n "$sudo" ] && ! docker info >/dev/null 2>&1 && in_docker_group; then in_group_now=false; fi
relogin='你剛加入 docker 群組：請登出再登入（或執行 newgrp docker），之後的 make start 才能使用 Docker。'
if [ "$mode" = --deps ]; then
  printf '系統套件已就緒。執行 ./scripts/bootstrap.sh 建置 LazyBoy。\n'
  if ! $in_group_now; then printf '%s\n' "$relogin"; fi
  exit 0
fi
if $in_group_now; then
  ./scripts/bootstrap.sh
elif has sg; then
  sg docker -c ./scripts/bootstrap.sh
  printf '%s\n' "$relogin"
else
  printf '%s 然後再執行一次 make install 完成安裝。\n' "$relogin" >&2
  exit 1
fi
