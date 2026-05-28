#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/uninstall-dev.sh [OPTIONS]

Remove the locally installed diff-tool development extension from the Zed data directory.

Options:
  --data-dir DIR    Override the Zed data directory.
  -h, --help        Show this help message.

Examples:
  scripts/uninstall-dev.sh
  scripts/uninstall-dev.sh --data-dir "$HOME/Library/Application Support/Zed"
USAGE
}

data_dir=""

while [ "$#" -gt 0 ]; do
  case "$1" in
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

install_dir="$data_dir/extensions/installed/diff-tool"

removed=0
for path in "$install_dir"; do
  if [ -e "$path" ]; then
    rm -rf "$path"
    echo "Removed: $path"
    removed=1
  fi
done

if [ "$removed" -eq 0 ]; then
  echo "Nothing to remove for diff-tool under:"
  echo "  $data_dir"
else
  echo
  echo "Restart Zed to finish unloading the local extension."
fi
