#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
export DIFFLOOM_AUTO_WORKSPACE="${DIFFLOOM_AUTO_WORKSPACE:-$repo}"
bin="$repo/target/release/diffloom"
if [[ -x "$bin" ]]; then
  exec "$bin" mcp
fi
exec cargo run --manifest-path "$repo/Cargo.toml" --quiet -- mcp
