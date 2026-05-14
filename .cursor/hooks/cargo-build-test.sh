#!/usr/bin/env bash
set -euo pipefail
input=$(cat)
root=$(printf '%s\n' "$input" | python3 -c "import json,sys; d=json.load(sys.stdin); r=d.get('workspace_roots') or []; print(r[0] if r else '')")
if [[ -z "$root" ]]; then
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  root="$(cd "$script_dir/../.." && pwd)"
fi
cd "$root"
if [[ ! -f Cargo.toml ]] || ! grep -q '^name = "diffloom"' Cargo.toml 2>/dev/null; then
  printf '%s\n' '{}'
  exit 0
fi
cargo build --bins
cargo test
printf '%s\n' '{}'
