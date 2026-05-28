#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-dev.sh [OPTIONS]

Build local development artifacts and install only the extension manifest and
wasm into the local Zed extensions directory. The LSP server is built but not
bundled into the extension; configure its path with Zed settings or
ZED_DIFF_TOOL_LSP for local development.

Options:
  --no-build        Do not run cargo build before installing.
  --release-wasm    Build/copy the extension wasm from the release profile.
  --data-dir DIR    Override the Zed data directory.
  -h, --help        Show this help message.

Examples:
  scripts/install-dev.sh
  scripts/install-dev.sh --no-build
  scripts/install-dev.sh --data-dir "$HOME/Library/Application Support/Zed"
USAGE
}

build=1
wasm_profile=debug
data_dir=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-build)
      build=0
      ;;
    --release-wasm)
      wasm_profile=release
      ;;
    --data-dir)
      shift
      if [ "$#" -eq 0 ]; then
        echo "error: --data-dir requires a value" >&2
        exit 2
      fi
      data_dir="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"

detect_data_dir() {
  case "$(uname -s)" in
    Darwin)
      printf '%s\n' "$HOME/Library/Application Support/Zed"
      ;;
    Linux)
      printf '%s\n' "${XDG_DATA_HOME:-$HOME/.local/share}/zed"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      if [ -n "${APPDATA:-}" ]; then
        printf '%s\n' "$APPDATA/Zed"
      else
        printf '%s\n' "$HOME/AppData/Roaming/Zed"
      fi
      ;;
    *)
      echo "error: unsupported OS. Pass --data-dir explicitly." >&2
      exit 2
      ;;
  esac
}

if [ -z "$data_dir" ]; then
  data_dir="$(detect_data_dir)"
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) server_binary="diff-tool-lsp.exe" ;;
  *) server_binary="diff-tool-lsp" ;;
esac

wasm_args=(--target wasm32-wasip2)
if [ "$wasm_profile" = "release" ]; then
  wasm_args+=(--release)
fi

if [ "$build" -eq 1 ]; then
  cargo build --release --manifest-path "$repo_dir/server/Cargo.toml"
  cargo build "${wasm_args[@]}" --manifest-path "$repo_dir/Cargo.toml"
fi

wasm_path="$repo_dir/target/wasm32-wasip2/$wasm_profile/diff_tool.wasm"
server_path="$repo_dir/server/target/release/$server_binary"

if [ ! -f "$wasm_path" ]; then
  echo "error: extension wasm not found: $wasm_path" >&2
  echo "hint: run cargo build --target wasm32-wasip2 from the repository root." >&2
  exit 1
fi

if [ ! -f "$server_path" ]; then
  echo "error: LSP binary not found: $server_path" >&2
  echo "hint: run cargo build --release --manifest-path server/Cargo.toml." >&2
  exit 1
fi

install_dir="$data_dir/extensions/installed/diff-tool"
rm -rf "$install_dir"
mkdir -p "$install_dir"

cp "$repo_dir/extension.toml" "$install_dir/extension.toml"
cp "$wasm_path" "$install_dir/extension.wasm"

cat <<EOF
Installed diff-tool into:
  $install_dir

Built local LSP at:
  $server_path

For local development, configure:
  "lsp": { "diff-tool": { "binary": { "path": "$server_path" } } }

Or export:
  ZED_DIFF_TOOL_LSP="$server_path"

Restart Zed to reload the local extension.
EOF
