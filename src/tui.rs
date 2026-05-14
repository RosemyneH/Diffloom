use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::DefaultTerminal;

use crate::{db, ingest, view, watcher};

#[derive(Copy, Clone)]
enum Focus {
    Sessions,
    Snapshots,
}

pub fn run(root: PathBuf) -> anyhow::Result<()> {
    let root = crate::paths::normalize_path(&root).context("root path")?;
    let mut conn = db::open_db(&root)?;
    let (debouncer, file_rx) = watcher::watch_workspace(root.clone(), Duration::from_millis(400))?;
    ratatui::run(|terminal: &mut DefaultTerminal| -> anyhow::Result<()> {
        let _keep = debouncer;
        let mut sess_state = ListState::default();
        let mut snap_state = ListState::default();
        let mut focus = Focus::Snapshots;
        let mut sessions = db::list_sessions(&conn, 40).unwrap_or_default();
        let mut snaps = db::list_recent_snapshots(&conn, 80).unwrap_or_default();
        if !snaps.is_empty() {
            snap_state.select(Some(0));
        }
        if !sessions.is_empty() {
            sess_state.select(Some(0));
        }
        loop {
            while let Ok(p) = file_rx.try_recv() {
                let _ = ingest::ingest_path(&mut conn, &root, &p);
            }
            sessions = db::list_sessions(&conn, 40).unwrap_or_default();
            snaps = db::list_recent_snapshots(&conn, 80).unwrap_or_default();
            if let Some(i) = snap_state.selected() {
                if snaps.is_empty() {
                    snap_state.select(None);
                } else if i >= snaps.len() {
                    snap_state.select(Some(snaps.len() - 1));
                }
            }
            if let Some(i) = sess_state.selected() {
                if sessions.is_empty() {
                    sess_state.select(None);
                } else if i >= sessions.len() {
                    sess_state.select(Some(sessions.len() - 1));
                }
            }
            let detail = view::snapshot_detail(&conn, &snaps, snap_state.selected());
            draw_ui(
                terminal,
                &sessions,
                &snaps,
                &mut sess_state,
                &mut snap_state,
                focus,
                &detail,
            )?;
            if !event::poll(Duration::from_millis(120))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Tab => {
                    focus = match focus {
                        Focus::Sessions => Focus::Snapshots,
                        Focus::Snapshots => Focus::Sessions,
                    };
                }
                KeyCode::Char('r') => {}
                KeyCode::Down | KeyCode::Char('j') => match focus {
                    Focus::Sessions => {
                        if !sessions.is_empty() {
                            let i = sess_state.selected().unwrap_or(0);
                            let n = (i + 1).min(sessions.len() - 1);
                            sess_state.select(Some(n));
                        }
                    }
                    Focus::Snapshots => {
                        if !snaps.is_empty() {
                            let i = snap_state.selected().unwrap_or(0);
                            let n = (i + 1).min(snaps.len() - 1);
                            snap_state.select(Some(n));
                        }
                    }
                },
                KeyCode::Up | KeyCode::Char('k') => match focus {
                    Focus::Sessions => {
                        if !sessions.is_empty() {
                            let i = sess_state.selected().unwrap_or(0);
                            let n = i.saturating_sub(1);
                            sess_state.select(Some(n));
                        }
                    }
                    Focus::Snapshots => {
                        if !snaps.is_empty() {
                            let i = snap_state.selected().unwrap_or(0);
                            let n = i.saturating_sub(1);
                            snap_state.select(Some(n));
                        }
                    }
                },
                _ => {}
            }
        }
    })
}

fn draw_ui(
    terminal: &mut DefaultTerminal,
    sessions: &[db::SessionRow],
    snaps: &[db::SnapshotListRow],
    sess_state: &mut ListState,
    snap_state: &mut ListState,
    focus: Focus,
    detail: &str,
) -> anyhow::Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        let [header, main, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);
        let title = Paragraph::new("diffloom — sessions | snapshots | detail (Tab switch)")
            .style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(title, header);
        let [left, mid, right] = Layout::horizontal([
            Constraint::Percentage(24),
            Constraint::Percentage(36),
            Constraint::Min(0),
        ])
        .areas(main);
        let sess_border = matches!(focus, Focus::Sessions);
        let sess_block = Block::default()
            .borders(Borders::ALL)
            .title("Sessions")
            .border_style(if sess_border {
                Style::default().cyan()
            } else {
                Style::default()
            });
        let sess_items: Vec<ListItem> = sessions
            .iter()
            .map(|s| {
                ListItem::new(format!(
                    "#{} {} [{}] {}",
                    s.id,
                    s.title,
                    s.kind,
                    if s.closed_at.is_some() {
                        "·closed"
                    } else {
                        ""
                    }
                ))
            })
            .collect();
        let sess_list = List::new(sess_items).block(sess_block);
        frame.render_stateful_widget(sess_list, left, sess_state);
        let snap_border = matches!(focus, Focus::Snapshots);
        let snap_block = Block::default()
            .borders(Borders::ALL)
            .title("Snapshots")
            .border_style(if snap_border {
                Style::default().cyan()
            } else {
                Style::default()
            });
        let snap_items: Vec<ListItem> = snaps
            .iter()
            .map(|s| {
                let short: String = s.content_sha256.chars().take(8).collect();
                ListItem::new(format!("#{} {} {}", s.id, s.path, short))
            })
            .collect();
        let snap_list = List::new(snap_items).block(snap_block);
        frame.render_stateful_widget(snap_list, mid, snap_state);
        let detail_w = Paragraph::new(detail).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Summary / symbols"),
        );
        frame.render_widget(detail_w, right);
        let foot = Paragraph::new("q quit | Tab focus | j/k move | r refresh");
        frame.render_widget(foot, footer);
    })?;
    Ok(())
}
