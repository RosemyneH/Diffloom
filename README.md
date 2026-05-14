# Diffloom

Diffloom is a **workspace timeline** for your repo: it watches files, records **snapshots** when content changes, and groups work into **sessions** (for example AI-assisted edits, refactors, or bug hunts). State lives in `.diffloom/db.sqlite` at the project root.

## Why it belongs in a heavy agentic toolkit

Agents touch many files quickly. Git shows *intent* (commits); your editor shows *now*. Diffloom fills the gap: a **durable, queryable history of what actually changed on disk**, step by step, without you committing every intermediate state.

- **Debugging “what did the agent just do?”** Browse snapshots in time order, open unified diffs against the previous version of the same path, and see **Rust symbol-level deltas** (adds/removes/renames of items tree-sitter understands) when you are working in `.rs` files.
- **Context that survives the session** Each snapshot can carry **git head and dirty state**, so you can correlate filesystem drift with where the repo thought it was.
- **Machine-readable for automation** Run `diffloom mcp` and wire the MCP server into your agent host: tools cover opening a workspace, creating and closing sessions, listing snapshots, fetching diffs and symbol changes, and attaching **summary** text to a snapshot (great for human or model notes on “what this step was for”).

Together, that turns noisy agent runs into something you can **navigate, diff, and explain**—the same way you would use logs for services, but for your codebase while agents are editing it.

## Modes

| Command | Role |
|--------|------|
| `diffloom` | Default: **GUI** (pick or reuse a workspace folder). |
| `diffloom tui` | **Terminal UI** when you want everything in the shell. |
| `diffloom mcp` | **MCP server on stdio** for tools/agents; optional `DIFFLOOM_AUTO_WORKSPACE` env to open a root on startup. |

Use `--root /path/to/project` to skip the picker and pin a workspace.

## Build

Requires a Rust toolchain (see `Cargo.toml` for edition and dependencies).

```bash
cargo build --release
```

The binary is `target/release/diffloom`.

## Quick mental model

1. Open a workspace → Diffloom ensures the local DB exists and starts watching.
2. Start a **session** when you begin a coherent stretch of work (or let your automation do it via MCP).
3. Edits create **snapshots** (content-addressed; unchanged files are skipped).
4. Inspect timelines in the UI, or pull **diffs / symbols / summaries** through MCP when building agent workflows or post-mortems.

Diffloom does not replace version control; it **complements** it for the messy middle where agents iterate fast and you still need a clear trail.
