use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router, ServiceExt};
use serde::Deserialize;

use crate::db;

#[derive(Default)]
struct McpInner {
    root: Option<PathBuf>,
    conn: Option<rusqlite::Connection>,
}

#[derive(Clone)]
pub struct DiffloomMcp {
    inner: Arc<Mutex<McpInner>>,
}

impl Default for DiffloomMcp {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(McpInner::default())),
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
    let mut p = PathBuf::from(raw.trim());
    if !p.is_absolute() {
        let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
        p = cwd.join(p);
    }
    crate::paths::normalize_path(&p).map_err(|e| format!("{e}"))
}

fn apply_open_workspace(srv: &DiffloomMcp, raw: &str) -> String {
    let p = match resolve_workspace_path(raw) {
        Ok(p) => p,
        Err(e) => return format!("error: {e}"),
    };
    let conn = match db::open_db(&p) {
        Ok(c) => c,
        Err(e) => return format!("error: {e:#}"),
    };
    let mut g = srv.inner.lock().unwrap();
    g.root = Some(p.clone());
    g.conn = Some(conn);
    format!("opened workspace {}", p.display())
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenWorkspaceParams {
    #[schemars(description = "Workspace root (absolute or relative to cwd)")]
    root: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionCreateParams {
    title: String,
    #[schemars(description = "Session kind label, e.g. ai, refactor, fix")]
    kind: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SessionCloseParams {
    #[schemars(description = "Session id; omit to close the active session")]
    session_id: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SnapshotsForSessionParams {
    session_id: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SnapshotIdParams {
    snapshot_id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SummarySetParams {
    snapshot_id: i64,
    text: String,
}

#[tool_router(server_handler)]
impl DiffloomMcp {
    #[tool(description = "Open a workspace root and create or reuse .diffloom/db.sqlite")]
    fn open_workspace(
        &self,
        Parameters(OpenWorkspaceParams { root }): Parameters<OpenWorkspaceParams>,
    ) -> String {
        apply_open_workspace(self, &root)
    }

    #[tool(description = "Create a session and set it as active for subsequent file snapshots")]
    fn session_create(
        &self,
        Parameters(SessionCreateParams { title, kind }): Parameters<SessionCreateParams>,
    ) -> String {
        let mut g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_mut() else {
            return "error: call open_workspace first".into();
        };
        let now = now_ms();
        if let Err(e) = conn.execute(
            "INSERT INTO sessions (title, kind, created_at, closed_at) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params![title, kind, now],
        ) {
            return format!("error: {e}");
        }
        let id = conn.last_insert_rowid();
        if let Err(e) = db::meta_set(conn, "active_session_id", &id.to_string()) {
            return format!("error: {e}");
        }
        format!("session_id={id}")
    }

    #[tool(description = "Close a session (sets closed_at); clears active session when it matches")]
    fn session_close(
        &self,
        Parameters(SessionCloseParams { session_id }): Parameters<SessionCloseParams>,
    ) -> String {
        let mut g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_mut() else {
            return "error: call open_workspace first".into();
        };
        let now = now_ms();
        let sid = match session_id {
            Some(id) => id,
            None => match db::active_session_id(conn) {
                Ok(Some(id)) => id,
                Ok(None) => return "error: no active session".into(),
                Err(e) => return format!("error: {e}"),
            },
        };
        if let Err(e) = conn.execute(
            "UPDATE sessions SET closed_at = ?1 WHERE id = ?2 AND closed_at IS NULL",
            rusqlite::params![now, sid],
        ) {
            return format!("error: {e}");
        }
        if let Ok(Some(cur)) = db::active_session_id(conn) {
            if cur == sid {
                let _ = conn.execute(
                    "DELETE FROM meta WHERE key = ?1",
                    rusqlite::params!["active_session_id"],
                );
            }
        }
        format!("closed session {sid}")
    }

    #[tool(description = "List recent sessions")]
    fn session_list(&self) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        match db::list_sessions(conn, 50) {
            Ok(rows) => serde_json::to_string_pretty(&rows).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "Return the active session id if any")]
    fn session_current(&self) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        match db::active_session_id(conn) {
            Ok(Some(id)) => format!(r#"{{"active_session_id":{id}}}"#),
            Ok(None) => r#"{"active_session_id":null}"#.into(),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "List snapshots for a session")]
    fn snapshots_for_session(
        &self,
        Parameters(SnapshotsForSessionParams { session_id, limit }): Parameters<
            SnapshotsForSessionParams,
        >,
    ) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        match db::list_snapshots_for_session(conn, session_id, limit) {
            Ok(rows) => serde_json::to_string_pretty(&rows).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(
        description = "Unified line diff between a snapshot and the previous snapshot for the same path (requires stored bodies)"
    )]
    fn diff_snapshot(
        &self,
        Parameters(SnapshotIdParams { snapshot_id }): Parameters<SnapshotIdParams>,
    ) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        let path_opt = match db::snapshot_path(conn, snapshot_id) {
            Ok(p) => p,
            Err(e) => return format!("error: {e}"),
        };
        let Some(path) = path_opt else {
            return "error: snapshot not found".into();
        };
        let prev = match db::previous_snapshot_id(conn, &path, snapshot_id) {
            Ok(p) => p,
            Err(e) => return format!("error: {e}"),
        };
        let Some(pid) = prev else {
            return "no previous snapshot for this path".into();
        };
        let cur = match db::snapshot_body(conn, snapshot_id) {
            Ok(b) => b,
            Err(e) => return format!("error: {e}"),
        };
        let Some(cur) = cur else {
            return "missing stored body for current snapshot".into();
        };
        let old = match db::snapshot_body(conn, pid) {
            Ok(b) => b,
            Err(e) => return format!("error: {e}"),
        };
        let Some(old) = old else {
            return "missing stored body for previous snapshot".into();
        };
        let old_s = String::from_utf8_lossy(&old);
        let new_s = String::from_utf8_lossy(&cur);
        let diff = similar::TextDiff::from_lines(&*old_s, &*new_s);
        let mut out = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Delete => "-",
                similar::ChangeTag::Insert => "+",
                similar::ChangeTag::Equal => " ",
            };
            for line in change.value().lines() {
                out.push_str(sign);
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    #[tool(description = "Symbol change rows for a snapshot")]
    fn symbols_for_snapshot(
        &self,
        Parameters(SnapshotIdParams { snapshot_id }): Parameters<SnapshotIdParams>,
    ) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        match db::load_symbol_changes(conn, snapshot_id) {
            Ok(rows) => serde_json::to_string_pretty(&rows).unwrap_or_else(|e| e.to_string()),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "Read snapshot summary text")]
    fn summary_get(
        &self,
        Parameters(SnapshotIdParams { snapshot_id }): Parameters<SnapshotIdParams>,
    ) -> String {
        let g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_ref() else {
            return "error: call open_workspace first".into();
        };
        match db::snapshot_summary(conn, snapshot_id) {
            Ok(Some(s)) => s,
            Ok(None) => "(none)".into(),
            Err(e) => format!("error: {e}"),
        }
    }

    #[tool(description = "Set snapshot summary text")]
    fn summary_set(
        &self,
        Parameters(SummarySetParams { snapshot_id, text }): Parameters<SummarySetParams>,
    ) -> String {
        let mut g = self.inner.lock().unwrap();
        let Some(conn) = g.conn.as_mut() else {
            return "error: call open_workspace first".into();
        };
        let now = now_ms();
        if let Err(e) = conn.execute(
            "INSERT INTO snapshot_summaries (snapshot_id, summary_text, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(snapshot_id) DO UPDATE SET summary_text = excluded.summary_text, updated_at = excluded.updated_at",
            rusqlite::params![snapshot_id, text, now],
        ) {
            return format!("error: {e}");
        }
        "ok".into()
    }
}

pub async fn run_stdio() -> anyhow::Result<()> {
    let srv = DiffloomMcp::default();
    if let Ok(raw) = std::env::var("DIFFLOOM_AUTO_WORKSPACE") {
        let s = raw.trim();
        if !s.is_empty() {
            let out = apply_open_workspace(&srv, s);
            if out.starts_with("error") {
                tracing::warn!("diffloom mcp auto-workspace: {out}");
            } else {
                tracing::info!("diffloom mcp auto-workspace: {out}");
            }
        }
    }
    let transport = rmcp::transport::io::stdio();
    let running = srv.serve(transport).await?;
    running.waiting().await?;
    Ok(())
}
