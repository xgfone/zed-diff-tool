#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/clean.sh [OPTIONS]

Remove local build intermediates and build outputs for this repository.

Options:
  -h, --help    Show this help message.

Examples:
  scripts/clean.sh
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
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
done

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"

removed=0
for path in \
  "$repo_dir/target" \
  "$repo_dir/server/target" \
  "$repo_dir/debug" \
  "$repo_dir/server/debug"
do
  if [ -e "$path" ]; then
    rm -rf "$path"
    echo "Removed: $path"
    removed=1
  fi
done

if [ "$removed" -eq 0 ]; then
  echo "Nothing to clean."
fi
