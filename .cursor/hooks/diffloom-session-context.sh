#!/usr/bin/env bash
set -euo pipefail
cat >/dev/null
python3 <<'PY'
import json

msg = """This repository ships the Diffloom MCP server.

The Cursor MCP entry runs `diffloom mcp` with DIFFLOOM_AUTO_WORKSPACE set to the workspace root, so the project database under `.diffloom/db.sqlite` is opened automatically when the server starts.

When you begin substantive work, call `session_create` with a title and kind (e.g. ai, refactor) so new file snapshots attach to that session. Use `session_list`, `snapshots_for_session`, `diff_snapshot`, `symbols_for_snapshot`, and `summary_get` / `summary_set` to read the timeline.

Filesystem snapshots during normal typing are captured when the Diffloom GUI or `diffloom tui` watcher is running; MCP tools read whatever is already stored."""

print(json.dumps({"additional_context": msg.strip()}))
PY
