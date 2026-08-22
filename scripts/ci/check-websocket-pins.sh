#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root_manifest="$repo_root/Cargo.toml"
tauri_manifest="$repo_root/apps/src-tauri/Cargo.toml"
root_lock="$repo_root/Cargo.lock"
tauri_lock="$repo_root/apps/src-tauri/Cargo.lock"

tokio_tungstenite_rev="0e5b2d73aa18dd9f0a50ee9ff199d5aef7594186"
tungstenite_rev="4fffad30fe373adbdcffab9545e9e9bf4f2fc19f"

require_line() {
  local file="$1"
  local needle="$2"
  if ! grep -Fq "$needle" "$file"; then
    printf 'missing expected WebSocket pin in %s: %s\n' "$file" "$needle" >&2
    exit 1
  fi
}

for manifest in "$root_manifest" "$tauri_manifest"; do
  require_line "$manifest" "tokio-tungstenite = { git = \"https://github.com/openai-oss-forks/tokio-tungstenite\", rev = \"$tokio_tungstenite_rev\" }"
  require_line "$manifest" "tungstenite = { git = \"https://github.com/openai-oss-forks/tungstenite-rs\", rev = \"$tungstenite_rev\" }"
done

for lockfile in "$root_lock" "$tauri_lock"; do
  require_line "$lockfile" "source = \"git+https://github.com/openai-oss-forks/tokio-tungstenite?rev=$tokio_tungstenite_rev#$tokio_tungstenite_rev\""
  require_line "$lockfile" "source = \"git+https://github.com/openai-oss-forks/tungstenite-rs?rev=$tungstenite_rev#$tungstenite_rev\""
done

printf 'WebSocket dependency pins are synchronized across the root and Tauri workspaces.\n'
