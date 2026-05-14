use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use eframe::egui;

use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;

use crate::{db, git_info, ingest, paths, view, watcher};

const CANVAS: egui::Color32 = egui::Color32::from_rgb(11, 12, 16);
const TOPBAR: egui::Color32 = egui::Color32::from_rgb(18, 19, 26);
const RAIL: egui::Color32 = egui::Color32::from_rgb(20, 22, 30);
const RAIL_ICON: egui::Color32 = egui::Color32::from_rgb(16, 17, 24);
const RAIL_EDGE: egui::Color32 = egui::Color32::from_rgb(42, 46, 58);
const CENTER_BG: egui::Color32 = egui::Color32::from_rgb(14, 15, 20);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(167, 139, 250);
const ACCENT_DIM: egui::Color32 = egui::Color32::from_rgb(120, 100, 180);
const TEXT: egui::Color32 = egui::Color32::from_rgb(230, 232, 240);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(130, 136, 156);
const ADD_ROW: egui::Color32 = egui::Color32::from_rgb(16, 72, 52);
const ADD_GUTTER: egui::Color32 = egui::Color32::from_rgb(8, 52, 38);
const ADD_STRIP: egui::Color32 = egui::Color32::from_rgb(46, 205, 120);
const ADD_FG: egui::Color32 = egui::Color32::from_rgb(210, 255, 225);
const DEL_ROW: egui::Color32 = egui::Color32::from_rgb(88, 26, 38);
const DEL_GUTTER: egui::Color32 = egui::Color32::from_rgb(58, 16, 26);
const DEL_STRIP: egui::Color32 = egui::Color32::from_rgb(255, 82, 99);
const DEL_FG: egui::Color32 = egui::Color32::from_rgb(255, 220, 224);
const CTX_ROW: egui::Color32 = egui::Color32::from_rgb(22, 24, 30);
const CTX_ROW_ALT: egui::Color32 = egui::Color32::from_rgb(18, 20, 26);
const BOTH_LEFT: egui::Color32 = egui::Color32::from_rgb(48, 28, 34);
const BOTH_RIGHT: egui::Color32 = egui::Color32::from_rgb(18, 58, 42);
const TAB_ACTIVE: egui::Color32 = egui::Color32::from_rgb(36, 32, 52);
const STATUS_BG: egui::Color32 = egui::Color32::from_rgb(16, 17, 22);

type ChangeEntry = (db::SnapshotListRow, view::DiffLineStats);
type CommitEntry = (db::CommitGroupRow, view::DiffLineStats);

struct DiffloomGui {
    root: PathBuf,
    conn: rusqlite::Connection,
    file_rx: std::sync::mpsc::Receiver<PathBuf>,
    _debouncer: Debouncer<RecommendedWatcher>,
    sessions: Vec<db::SessionRow>,
    snaps: Vec<db::SnapshotListRow>,
    changes: Vec<ChangeEntry>,
    commits: Vec<CommitEntry>,
    session_filter: Option<i64>,
    change_sel: usize,
    rail_tab: usize,
    search: String,
    bottom_tab: usize,
    diff_cache_id: Option<i64>,
    diff_cache_text: String,
    sbs_rows: Vec<view::SbsRow>,
    sbs_stats: view::DiffLineStats,
}

fn project_label(root: &std::path::Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace")
        .to_string()
}

fn short_sha(s: &str) -> String {
    s.chars().take(8).collect()
}

impl DiffloomGui {
    fn refresh_lists(&mut self) {
        self.sessions = db::list_sessions(&self.conn, 40).unwrap_or_default();
        let mut snaps = match self.session_filter {
            Some(sid) => db::list_snapshots_for_session(&self.conn, sid, 200).unwrap_or_default(),
            None => db::list_recent_snapshots(&self.conn, 200).unwrap_or_default(),
        };
        let q = self.search.trim().to_lowercase();
        if !q.is_empty() {
            snaps.retain(|s| s.path.to_lowercase().contains(&q));
        }
        self.snaps = snaps;

        let mut seen = HashSet::<String>::new();
        let mut changes = Vec::new();
        for s in &self.snaps {
            if seen.contains(&s.path) {
                continue;
            }
            seen.insert(s.path.clone());
            let has_prev = db::previous_snapshot_id(&self.conn, &s.path, s.id)
                .ok()
                .flatten()
                .is_some();
            if !has_prev {
                continue;
            }
            if let Ok(Some((ref old, ref new))) = view::snapshot_old_new_strings(&self.conn, s.id) {
                let stats = view::count_line_changes(old, new);
                changes.push((s.clone(), stats));
            }
        }
        self.changes = changes;
        if self.changes.is_empty() {
            self.change_sel = 0;
        } else {
            self.change_sel = self.change_sel.min(self.changes.len() - 1);
        }

        let groups = db::list_commit_groups(&self.conn, 24).unwrap_or_default();
        self.commits = groups
            .into_iter()
            .map(|g| {
                let st = view::snapshot_old_new_strings(&self.conn, g.snapshot_id)
                    .ok()
                    .flatten()
                    .map(|(o, n)| view::count_line_changes(&o, &n))
                    .unwrap_or_default();
                (g, st)
            })
            .collect();
    }

    fn recompute_diff_cache(&mut self) {
        let id = self.changes.get(self.change_sel).map(|(s, _)| s.id);
        if self.diff_cache_id == id {
            return;
        }
        self.diff_cache_id = id;
        match id {
            Some(sid) => {
                self.diff_cache_text = view::unified_diff_for_snapshot(&self.conn, sid)
                    .unwrap_or_else(|e| format!("(diff unavailable)\n{e:#}\n"));
                if let Ok(Some((ref old, ref new))) = view::snapshot_old_new_strings(&self.conn, sid)
                {
                    let (rows, stats) = view::side_by_side_rows(old, new);
                    self.sbs_rows = rows;
                    self.sbs_stats = stats;
                } else {
                    self.sbs_rows.clear();
                    self.sbs_stats = view::DiffLineStats::default();
                }
            }
            None => {
                self.diff_cache_text.clear();
                self.sbs_rows.clear();
                self.sbs_stats = view::DiffLineStats::default();
            }
        }
    }

    fn totals_footer(&self) -> (u32, u32) {
        self.changes.iter().fold((0u32, 0u32), |(a, d), (_, s)| {
            (a + s.insertions, d + s.deletions)
        })
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let branch = git_info::current_branch(&self.root)
            .unwrap_or_else(|| "detached".to_string());
        let proj = project_label(&self.root);
        egui::Frame::new()
            .fill(TOPBAR)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .show(ui, |ui| {
                let full = ui.available_width();
                let left_w = 260.0_f32;
                let right_w = 140.0_f32;
                let mid = (full - left_w - right_w).max(120.0);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new("Diffloom")
                                    .size(19.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            ui.add_space(10.0);
                            egui::ComboBox::from_id_salt("diffloom_proj")
                                .selected_text(
                                    egui::RichText::new(&proj)
                                        .size(12.5)
                                        .color(ACCENT),
                                )
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(true, &proj);
                                });
                            egui::ComboBox::from_id_salt("diffloom_branch")
                                .selected_text(
                                    egui::RichText::new(&branch)
                                        .size(12.0)
                                        .family(egui::FontFamily::Monospace)
                                        .color(TEXT_DIM),
                                )
                                .width(90.0)
                                .show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(true, &branch);
                                });
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(mid, ui.spacing().interact_size.y),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    egui::RichText::new("f")
                                        .small()
                                        .color(TEXT_DIM)
                                        .background_color(RAIL)
                                        .code(),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.search)
                                        .desired_width((mid - 36.0).clamp(80.0, 420.0))
                                        .hint_text("f to search"),
                                );
                            });
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(right_w, ui.spacing().interact_size.y),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new("?").color(TEXT_DIM).size(16.0));
                            ui.add_space(12.0);
                            ui.label(egui::RichText::new("⚙").color(TEXT_DIM).size(15.0));
                        },
                    );
                });
            });
    }

    fn status_bar(ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(STATUS_BG)
            .inner_margin(egui::Margin::symmetric(12, 6))
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.label(
                        egui::RichText::new("j / k")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("next file").small().color(TEXT_DIM));
                    ui.label(
                        egui::RichText::new("← / →")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("prev / next file").small().color(TEXT_DIM));
                    ui.label(
                        egui::RichText::new("enter")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("stage").small().color(TEXT_DIM));
                    ui.label(
                        egui::RichText::new("1 · 2 · 3")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("bottom tab").small().color(TEXT_DIM));
                });
            });
    }

    fn icon_rail(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(RAIL_ICON)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .inner_margin(egui::Margin::symmetric(0, 10))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let icons = ["📄", "🕐", "⑂", "✓", "⚙"];
                    for (i, icon) in icons.iter().enumerate() {
                        let sel = self.rail_tab == i;
                        let fill = if sel { TAB_ACTIVE } else { egui::Color32::TRANSPARENT };
                        if ui
                            .add_sized(
                                [36.0, 36.0],
                                egui::Button::new(
                                    egui::RichText::new(*icon).size(17.0).color(if sel {
                                        ACCENT
                                    } else {
                                        TEXT_DIM
                                    }),
                                )
                                .fill(fill)
                                .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.rail_tab = i;
                        }
                        ui.add_space(4.0);
                    }
                });
            });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let (tot_add, tot_del) = self.totals_footer();
        egui::Frame::new()
            .fill(RAIL)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .inner_margin(egui::Margin::symmetric(10, 10))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("diffloom_sidebar_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("CHANGES ({})", self.changes.len()))
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(6.0);
                        for (i, (s, stats)) in self.changes.iter().enumerate() {
                            ui.push_id(("chg", s.id), |ui| {
                                let sel = self.change_sel == i;
                                let label = format!(
                                    "{}  +{} -{}",
                                    s.path, stats.insertions, stats.deletions
                                );
                                if ui
                                    .selectable_label(
                                        sel,
                                        egui::RichText::new(label)
                                            .size(12.0)
                                            .family(egui::FontFamily::Monospace)
                                            .color(if sel { TEXT } else { TEXT_DIM }),
                                    )
                                    .clicked()
                                {
                                    self.change_sel = i;
                                    self.diff_cache_id = None;
                                }
                            });
                        }
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(format!("SESSIONS ({})", self.sessions.len()))
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(6.0);
                        ui.push_id("all_sessions", |ui| {
                            let sel = self.session_filter.is_none();
                            if ui.selectable_label(sel, "All timelines").clicked() {
                                self.session_filter = None;
                                self.diff_cache_id = None;
                            }
                        });
                        ui.add_space(4.0);
                        for s in &self.sessions {
                            ui.push_id(s.id, |ui| {
                                let sel = self.session_filter == Some(s.id);
                                let closed = if s.closed_at.is_some() {
                                    " · closed"
                                } else {
                                    ""
                                };
                                let label = format!("{} [{}]{}", s.title, s.kind, closed);
                                if ui.selectable_label(sel, label).clicked() {
                                    self.session_filter = Some(s.id);
                                    self.diff_cache_id = None;
                                }
                            });
                        }
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(format!("COMMITS ({})", self.commits.len()))
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(6.0);
                        for (g, st) in &self.commits {
                            ui.push_id(("cmt", g.snapshot_id), |ui| {
                                let sha = short_sha(&g.git_commit);
                                let line = format!(
                                    "{}  +{} -{}",
                                    sha, st.insertions, st.deletions
                                );
                                ui.label(
                                    egui::RichText::new(line)
                                        .size(11.5)
                                        .family(egui::FontFamily::Monospace)
                                        .color(TEXT_DIM),
                                );
                            });
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} files  +{} -{}",
                        self.changes.len(),
                        tot_add,
                        tot_del
                    ))
                    .small()
                    .color(TEXT_DIM),
                );
            });
    }

    fn paint_sbs_cell(
        ui: &mut egui::Ui,
        width: f32,
        row_h: f32,
        line_no: Option<usize>,
        text: &str,
        row_fill: egui::Color32,
        gutter_fill: egui::Color32,
        strip: egui::Color32,
        fg: egui::Color32,
        sign: Option<&'static str>,
    ) {
        egui::Frame::new()
            .fill(row_fill)
            .inner_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    if let Some(s) = sign {
                        let _ = s;
                        if strip != egui::Color32::TRANSPARENT {
                            egui::Frame::new().fill(strip).show(ui, |ui| {
                                ui.set_width(4.0);
                                ui.set_min_height(row_h);
                            });
                        }
                    } else if strip != egui::Color32::TRANSPARENT {
                        egui::Frame::new().fill(strip).show(ui, |ui| {
                            ui.set_width(4.0);
                            ui.set_min_height(row_h);
                        });
                    }
                    egui::Frame::new()
                        .fill(gutter_fill)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.set_width(40.0);
                            ui.set_min_height(row_h);
                            let ln = line_no
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| " ".to_string());
                            let rt = if let Some(s) = sign {
                                egui::RichText::new(format!("{s} {ln}"))
                                    .family(egui::FontFamily::Monospace)
                                    .size(11.0)
                                    .color(fg)
                            } else {
                                egui::RichText::new(&ln)
                                    .family(egui::FontFamily::Monospace)
                                    .size(11.0)
                                    .color(TEXT_DIM)
                            };
                            ui.label(rt);
                        });
                    egui::Frame::new()
                        .fill(row_fill)
                        .inner_margin(egui::Margin::symmetric(6, 2))
                        .show(ui, |ui| {
                            ui.set_min_height(row_h);
                            ui.label(
                                egui::RichText::new(text)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.5)
                                    .color(fg),
                            );
                        });
                });
            });
    }

    fn paint_sbs_spacer(ui: &mut egui::Ui, width: f32, row_h: f32) {
        egui::Frame::new()
            .fill(CENTER_BG)
            .show(ui, |ui| {
                ui.set_width(width);
                ui.set_min_height(row_h);
            });
    }

    fn paint_sbs_rows(ui: &mut egui::Ui, rows: &[view::SbsRow]) {
        let row_h = ui.spacing().interact_size.y.max(19.0);
        let half = (ui.available_width() - 8.0) * 0.5;
        for (idx, row) in rows.iter().enumerate() {
            ui.push_id(("sbs", idx), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    match row {
                        view::SbsRow::Equal {
                            old_ln,
                            new_ln,
                            left,
                            right,
                        } => {
                            let fill = if idx % 2 == 0 { CTX_ROW } else { CTX_ROW_ALT };
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*old_ln),
                                left,
                                fill,
                                egui::Color32::from_rgb(26, 28, 36),
                                egui::Color32::TRANSPARENT,
                                TEXT_DIM,
                                None,
                            );
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*new_ln),
                                right,
                                fill,
                                egui::Color32::from_rgb(26, 28, 36),
                                egui::Color32::TRANSPARENT,
                                TEXT_DIM,
                                None,
                            );
                        }
                        view::SbsRow::DeleteLine { old_ln, text } => {
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*old_ln),
                                text,
                                DEL_ROW,
                                DEL_GUTTER,
                                DEL_STRIP,
                                DEL_FG,
                                None,
                            );
                            Self::paint_sbs_spacer(ui, half, row_h);
                        }
                        view::SbsRow::InsertLine { new_ln, text } => {
                            Self::paint_sbs_spacer(ui, half, row_h);
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*new_ln),
                                text,
                                ADD_ROW,
                                ADD_GUTTER,
                                ADD_STRIP,
                                ADD_FG,
                                Some("+"),
                            );
                        }
                        view::SbsRow::Both {
                            old_ln,
                            new_ln,
                            left,
                            right,
                        } => {
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*old_ln),
                                left,
                                BOTH_LEFT,
                                DEL_GUTTER,
                                DEL_STRIP,
                                DEL_FG,
                                None,
                            );
                            Self::paint_sbs_cell(
                                ui,
                                half,
                                row_h,
                                Some(*new_ln),
                                right,
                                BOTH_RIGHT,
                                ADD_GUTTER,
                                ADD_STRIP,
                                ADD_FG,
                                Some("+"),
                            );
                        }
                    }
                });
            });
        }
    }

    fn center_column(&mut self, ui: &mut egui::Ui) {
        self.recompute_diff_cache();
        let snap = self.changes.get(self.change_sel).map(|(s, _)| s);
        egui::Frame::new()
            .fill(CENTER_BG)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(s) = snap {
                        let st = self.sbs_stats;
                        let header = s.path.clone();
                        egui::Frame::new()
                            .fill(TOPBAR)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&header)
                                            .strong()
                                            .color(TEXT)
                                            .size(13.5)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.menu_button("⋯", |ui| {
                                                if ui.button("Copy path").clicked() {
                                                    ui.ctx()
                                                        .copy_text(s.path.clone());
                                                    ui.close();
                                                }
                                            });
                                            ui.add_space(8.0);
                                            ui.add_enabled(
                                                false,
                                                egui::Button::new(
                                                    egui::RichText::new("Stage")
                                                        .strong()
                                                        .color(TEXT),
                                                )
                                                .fill(ACCENT)
                                                .min_size(egui::vec2(72.0, 28.0)),
                                            );
                                            ui.add_space(10.0);
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "+{} -{}",
                                                    st.insertions, st.deletions
                                                ))
                                                .small()
                                                .color(TEXT_DIM),
                                            );
                                        },
                                    );
                                });
                            });
                        let diff_h = (ui.available_height() * 0.55).clamp(120.0, 800.0);
                        egui::ScrollArea::vertical()
                            .id_salt("diffloom_main_sbs")
                            .max_height(diff_h)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                if self.sbs_rows.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&self.diff_cache_text)
                                            .family(egui::FontFamily::Monospace)
                                            .size(12.0)
                                            .color(TEXT_DIM),
                                    );
                                } else {
                                    Self::paint_sbs_rows(ui, &self.sbs_rows);
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let sym_count = db::load_symbol_changes(&self.conn, s.id)
                                .map(|v| v.len())
                                .unwrap_or(0);
                            let sym_lbl = format!("SYMBOLS ({sym_count})");
                            for (i, name) in ["SUMMARY", sym_lbl.as_str(), "HISTORY"]
                                .into_iter()
                                .enumerate()
                            {
                                let sel = self.bottom_tab == i;
                                let fill = if sel { TAB_ACTIVE } else { egui::Color32::TRANSPARENT };
                                let text_color = if sel { ACCENT } else { TEXT_DIM };
                                if ui
                                    .add_sized(
                                        [100.0, 28.0],
                                        egui::Button::new(
                                            egui::RichText::new(name)
                                                .small()
                                                .strong()
                                                .color(text_color),
                                        )
                                        .fill(fill)
                                        .stroke(egui::Stroke::NONE),
                                    )
                                    .clicked()
                                {
                                    self.bottom_tab = i;
                                }
                            }
                        });
                        let tab_body_h = ui.available_height().max(40.0);
                        egui::ScrollArea::vertical()
                            .id_salt("diffloom_tab_scroll")
                            .max_height(tab_body_h)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                match self.bottom_tab {
                                    0 => {
                                        let txt = db::snapshot_summary(&self.conn, s.id)
                                            .ok()
                                            .flatten()
                                            .unwrap_or_else(|| "(no summary yet)".to_string());
                                        ui.label(
                                            egui::RichText::new(txt)
                                                .size(13.0)
                                                .color(TEXT)
                                                .line_height(Some(20.0)),
                                        );
                                        if let Some(sid) = self.session_filter {
                                            if let Some(sess) =
                                                self.sessions.iter().find(|x| x.id == sid)
                                            {
                                                ui.add_space(8.0);
                                                ui.label(
                                                    egui::RichText::new(&sess.title)
                                                        .small()
                                                        .strong()
                                                        .color(ACCENT),
                                                );
                                            }
                                        }
                                    }
                                    1 => {
                                        let rows = db::load_symbol_changes(&self.conn, s.id)
                                            .unwrap_or_default();
                                        if rows.is_empty() {
                                            ui.label(
                                                egui::RichText::new("No symbol changes")
                                                    .color(TEXT_DIM),
                                            );
                                        } else {
                                            for (c, n, k) in rows {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{c:9} {k:8} {n}"
                                                    ))
                                                    .family(egui::FontFamily::Monospace)
                                                    .size(12.0)
                                                    .color(TEXT),
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        let path = s.path.clone();
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Snapshots for `{path}`"
                                            ))
                                            .small()
                                            .color(TEXT_DIM),
                                        );
                                        ui.add_space(6.0);
                                        for row in db::list_snapshots_for_path(
                                            &self.conn,
                                            &path,
                                            40,
                                        )
                                        .unwrap_or_default()
                                        {
                                            ui.push_id(("hist", row.id), |ui| {
                                                let short = short_sha(&row.content_sha256);
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "#{}  {}  {}",
                                                        row.id, short, row.created_at
                                                    ))
                                                    .family(egui::FontFamily::Monospace)
                                                    .size(11.0)
                                                    .color(TEXT_DIM),
                                                );
                                            });
                                        }
                                    }
                                }
                            });
                    } else {
                        ui.label(
                            egui::RichText::new(
                                "No comparable snapshots yet (need a prior version of a file).",
                            )
                            .color(TEXT_DIM),
                        );
                    }
                });
            });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if !self.changes.is_empty() {
            let n = self.changes.len();
            if ctx.input(|i| i.key_pressed(egui::Key::J)) {
                self.change_sel = (self.change_sel + 1).min(n - 1);
                self.diff_cache_id = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K)) {
                self.change_sel = self.change_sel.saturating_sub(1);
                self.diff_cache_id = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                self.change_sel = (self.change_sel + 1).min(n - 1);
                self.diff_cache_id = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                self.change_sel = self.change_sel.saturating_sub(1);
                self.diff_cache_id = None;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
            self.bottom_tab = 0;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
            self.bottom_tab = 1;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num3)) {
            self.bottom_tab = 2;
        }
    }
}

impl eframe::App for DiffloomGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(p) = self.file_rx.try_recv() {
            let _ = ingest::ingest_path(&mut self.conn, &self.root, &p);
        }
        self.refresh_lists();
        self.recompute_diff_cache();

        ui.visuals_mut().override_text_color = Some(TEXT);

        ui.vertical(|ui| {
            self.top_bar(ui);
            let body_h = (ui.available_height() - 36.0).max(160.0);
            ui.horizontal(|ui| {
                ui.set_min_height(body_h);
                ui.set_max_height(body_h);
                ui.vertical(|ui| {
                    ui.set_width(44.0);
                    ui.set_min_height(body_h);
                    self.icon_rail(ui);
                });
                ui.vertical(|ui| {
                    ui.set_width(272.0);
                    ui.set_min_height(body_h);
                    self.sidebar(ui);
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_min_width(320.0);
                    ui.set_min_height(body_h);
                    self.center_column(ui);
                });
            });
            Self::status_bar(ui);
        });

        self.handle_keys(ui.ctx());

        ui.ctx()
            .request_repaint_after(Duration::from_millis(200));
    }
}

fn setup_style(cc: &eframe::CreationContext<'_>) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = CANVAS;
    visuals.panel_fill = CANVAS;
    visuals.extreme_bg_color = RAIL;
    visuals.faint_bg_color = RAIL;
    visuals.widgets.noninteractive.bg_fill = RAIL;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_DIM;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(30, 33, 44);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 42, 56);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(46, 50, 66);
    visuals.selection.bg_fill = TAB_ACTIVE;
    visuals.selection.stroke.color = ACCENT;
    visuals.hyperlink_color = ACCENT_DIM;
    cc.egui_ctx.set_visuals(visuals);

    let mut style = (*cc.egui_ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(8);
    cc.egui_ctx.set_global_style(style);
}

pub fn run(root: PathBuf) -> anyhow::Result<()> {
    let root = paths::normalize_path(&root).context("root path")?;
    let conn = db::open_db(&root)?;
    let (debouncer, file_rx) =
        watcher::watch_workspace(root.clone(), Duration::from_millis(400))?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Diffloom")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([920.0, 540.0]),
        ..Default::default()
    };

    let app = DiffloomGui {
        root,
        conn,
        file_rx,
        _debouncer: debouncer,
        sessions: vec![],
        snaps: vec![],
        changes: vec![],
        commits: vec![],
        session_filter: None,
        change_sel: 0,
        rail_tab: 0,
        search: String::new(),
        bottom_tab: 0,
        diff_cache_id: None,
        diff_cache_text: String::new(),
        sbs_rows: vec![],
        sbs_stats: view::DiffLineStats::default(),
    };

    eframe::run_native(
        "Diffloom",
        options,
        Box::new(move |cc| {
            setup_style(cc);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
