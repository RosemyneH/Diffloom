use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use eframe::egui;

use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;

use crate::{db, git_info, ingest, paths, view, watcher};

const CANVAS: egui::Color32 = egui::Color32::from_rgb(11, 12, 16);
const TOPBAR: egui::Color32 = egui::Color32::from_rgb(18, 19, 26);
const RAIL: egui::Color32 = egui::Color32::from_rgb(20, 22, 30);
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
const HUNK_BAR: egui::Color32 = egui::Color32::from_rgb(40, 36, 58);
const STATUS_BG: egui::Color32 = egui::Color32::from_rgb(16, 17, 22);
const DIFF_CONTEXT_LINES: usize = 3;
const GUTTER_W: f32 = 34.0;
const STRIP_W: f32 = 3.0;
const CODE_PAD_X: i8 = 3;
const CODE_PAD_Y: i8 = 1;
const DIFF_COL_GAP: f32 = 2.0;
const ICON_COL_W: f32 = 18.0;
const DIFF_ROW_H: f32 = 14.0;
const CODE_FONT: f32 = 11.0;
const GUTTER_FONT: f32 = 9.5;
const SIDEBAR_W: f32 = 208.0;

type PathWithStats = (db::PathHistoryRow, view::DiffLineStats);

struct DiffloomGui {
    root: PathBuf,
    conn: rusqlite::Connection,
    file_rx: std::sync::mpsc::Receiver<PathBuf>,
    _debouncer: Debouncer<RecommendedWatcher>,
    sessions: Vec<db::SessionRow>,
    paths_with_stats: Vec<PathWithStats>,
    path_sel: usize,
    path_versions_path: Option<String>,
    path_snaps: Vec<db::SnapshotListRow>,
    snap_stats: Vec<Option<view::DiffLineStats>>,
    snap_sel: usize,
    session_filter: Option<i64>,
    search: String,
    bottom_tab: usize,
    diff_cache_id: Option<i64>,
    diff_cache_text: String,
    sbs_rows: Vec<view::SbsRow>,
    sbs_stats: view::DiffLineStats,
    diff_row_sel: Option<usize>,
    paths_scan_key: String,
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

fn file_glyph(path: &str) -> &'static str {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| match ext.to_ascii_lowercase().as_str() {
            "rs" => "⚙",
            "toml" | "json" | "yaml" | "yml" => "⌗",
            "js" | "ts" | "tsx" | "jsx" | "mjs" | "cjs" => "◇",
            "cpp" | "cc" | "cxx" | "h" | "hpp" | "c" => "◆",
            "md" | "mdx" => "📄",
            "py" => "⌬",
            "go" => "◎",
            _ => "⎘",
        })
        .unwrap_or("⎘")
}

impl DiffloomGui {
    fn rebuild_path_listings(&mut self) {
        self.sessions = db::list_sessions(&self.conn, 80).unwrap_or_default();
        let mut rows = db::list_paths_by_scope(&self.conn, self.session_filter, 150)
            .unwrap_or_default();
        let q = self.search.trim().to_lowercase();
        if !q.is_empty() {
            rows.retain(|r| r.path.to_lowercase().contains(&q));
        }
        self.paths_with_stats = rows
            .into_iter()
            .map(|r| {
                let st = view::snapshot_old_new_strings(&self.conn, r.latest_id)
                    .ok()
                    .flatten()
                    .map(|(o, n)| view::count_line_changes(&o, &n))
                    .unwrap_or_default();
                (r, st)
            })
            .collect();

        if self.path_sel >= self.paths_with_stats.len() {
            self.path_sel = self.paths_with_stats.len().saturating_sub(1);
        }
        self.path_versions_path = None;
        self.diff_row_sel = None;
    }

    fn sync_selected_path_versions(&mut self) {
        let want_path = self
            .paths_with_stats
            .get(self.path_sel)
            .map(|(r, _)| r.path.clone());
        if want_path.as_ref() != self.path_versions_path.as_ref() {
            self.path_versions_path = want_path.clone();
            self.snap_sel = 0;
            self.diff_cache_id = None;
            self.diff_row_sel = None;
            match want_path.as_deref() {
                None | Some("") => {
                    self.path_snaps.clear();
                    self.snap_stats.clear();
                }
                Some(p) => {
                    self.path_snaps =
                        db::list_snapshots_for_path(&self.conn, p, 64).unwrap_or_default();
                    self.snap_stats = self
                        .path_snaps
                        .iter()
                        .map(|s| {
                            view::snapshot_old_new_strings(&self.conn, s.id)
                                .ok()
                                .flatten()
                                .map(|(o, n)| view::count_line_changes(&o, &n))
                        })
                        .collect();
                }
            }
        }
        if !self.path_snaps.is_empty() {
            self.snap_sel = self.snap_sel.min(self.path_snaps.len() - 1);
        } else {
            self.snap_sel = 0;
        }
    }

    fn refresh_lists(&mut self) {
        self.rebuild_path_listings();
        self.sync_selected_path_versions();
    }

    fn recompute_diff_cache(&mut self) {
        let sid = self.path_snaps.get(self.snap_sel).map(|s| s.id);
        if self.diff_cache_id == sid {
            return;
        }
        self.diff_cache_id = sid;
        self.diff_row_sel = None;
        match sid {
            Some(id) => {
                if let Ok(Some((ref old, ref new))) = view::snapshot_old_new_strings(&self.conn, id)
                {
                    let (rows, stats) =
                        view::side_by_side_rows_focused(old, new, DIFF_CONTEXT_LINES);
                    self.sbs_rows = rows;
                    self.sbs_stats = stats;
                    self.diff_cache_text.clear();
                } else {
                    self.diff_cache_text = view::unified_diff_for_snapshot(&self.conn, id)
                        .unwrap_or_else(|e| format!("(diff unavailable)\n{e:#}\n"));
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
        self.paths_with_stats
            .iter()
            .fold((0u32, 0u32), |(a, d), (_, s)| (a + s.insertions, d + s.deletions))
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let branch = git_info::current_branch(&self.root)
            .unwrap_or_else(|| "detached".to_string());
        let proj = project_label(&self.root);
        egui::Frame::new()
            .fill(TOPBAR)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .show(ui, |ui| {
                let full = ui.available_width();
                let left_w = 190.0_f32;
                let mid = (full - left_w - 16.0).max(120.0);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(
                                egui::RichText::new("Diffloom")
                                    .size(17.0)
                                    .strong()
                                    .color(TEXT),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(proj)
                                    .size(12.0)
                                    .color(ACCENT),
                            );
                            ui.label(
                                egui::RichText::new(format!("· {branch}"))
                                    .size(11.0)
                                    .family(egui::FontFamily::Monospace)
                                    .color(TEXT_DIM),
                            );
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
                                        .desired_width((mid - 36.0).clamp(80.0, 520.0))
                                        .hint_text("filter paths"),
                                );
                            });
                        },
                    );
                });
            });
    }

    fn status_bar(ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(STATUS_BG)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    ui.label(
                        egui::RichText::new("j / k")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("file").small().color(TEXT_DIM));
                    ui.label(
                        egui::RichText::new("[ / ]")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("version").small().color(TEXT_DIM));
                    ui.label(
                        egui::RichText::new("1 · 2")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("summary / symbols").small().color(TEXT_DIM));
                });
            });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let (tot_add, tot_del) = self.totals_footer();
        egui::Frame::new()
            .fill(RAIL)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                egui::ComboBox::from_id_salt("diffloom_scope")
                    .width(ui.available_width())
                    .selected_text(match self.session_filter {
                        None => "All sessions".to_string(),
                        Some(id) => self
                            .sessions
                            .iter()
                            .find(|s| s.id == id)
                            .map(|s| format!("{} · {}", s.title, s.kind))
                            .unwrap_or_else(|| format!("session #{id}")),
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.session_filter.is_none(), "All sessions")
                            .clicked()
                        {
                            self.session_filter = None;
                            self.path_versions_path = None;
                            self.diff_cache_id = None;
                        }
                        for s in &self.sessions {
                            let sel = self.session_filter == Some(s.id);
                            let label = format!("{} [{}]", s.title, s.kind);
                            if ui.selectable_label(sel, &label).clicked() {
                                self.session_filter = Some(s.id);
                                self.path_versions_path = None;
                                self.diff_cache_id = None;
                            }
                        }
                    });
                ui.add_space(5.0);
                egui::ScrollArea::vertical()
                    .id_salt("diffloom_sidebar_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("FILES ({})", self.paths_with_stats.len()))
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(3.0);
                        for (i, (r, st)) in self.paths_with_stats.iter().enumerate() {
                            ui.push_id(("path", i), |ui| {
                                let sel = self.path_sel == i;
                                let fill = if sel {
                                    TAB_ACTIVE
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                let fr = egui::Frame::new()
                                    .fill(fill)
                                    .inner_margin(egui::Margin::symmetric(2, 3))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(ICON_COL_W, 28.0),
                                                egui::Layout::top_down(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(file_glyph(&r.path))
                                                            .size(12.0),
                                                    );
                                                },
                                            );
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                let path_color = if sel { TEXT } else { TEXT_DIM };
                                                ui.label(
                                                    egui::RichText::new(&r.path)
                                                        .family(egui::FontFamily::Monospace)
                                                        .size(11.0)
                                                        .color(path_color),
                                                )
                                                .on_hover_text(&r.path);
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 5.0;
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "+{}",
                                                            st.insertions
                                                        ))
                                                        .size(10.0)
                                                        .color(ADD_STRIP),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "-{}",
                                                            st.deletions
                                                        ))
                                                        .size(10.0)
                                                        .color(DEL_STRIP),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "· {}v",
                                                            r.snapshot_count
                                                        ))
                                                        .size(10.0)
                                                        .color(TEXT_DIM),
                                                    );
                                                });
                                            });
                                        });
                                    });
                                if fr.response.clicked() {
                                    self.path_sel = i;
                                    self.path_versions_path = None;
                                    self.diff_cache_id = None;
                                }
                            });
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "VERSIONS ({})",
                                self.path_snaps.len()
                            ))
                            .small()
                            .strong()
                            .color(ACCENT),
                        );
                        ui.add_space(3.0);
                        for (i, s) in self.path_snaps.iter().enumerate() {
                            ui.push_id(("ver", s.id), |ui| {
                                let sel = self.snap_sel == i;
                                let short = short_sha(&s.content_sha256);
                                let st = self.snap_stats.get(i).copied().flatten();
                                let fill = if sel {
                                    TAB_ACTIVE
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                let fr = egui::Frame::new()
                                    .fill(fill)
                                    .inner_margin(egui::Margin::symmetric(2, 3))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(ICON_COL_W, 28.0),
                                                egui::Layout::top_down(egui::Align::Center),
                                                |ui| {
                                                    ui.label(egui::RichText::new("⏱").size(11.0));
                                                },
                                            );
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                let line1 = format!("#{}  {}", s.id, short);
                                                let c = if sel { TEXT } else { TEXT_DIM };
                                                ui.label(
                                                    egui::RichText::new(line1)
                                                        .family(egui::FontFamily::Monospace)
                                                        .size(10.5)
                                                        .color(c),
                                                );
                                                if let Some(st) = st {
                                                    ui.horizontal(|ui| {
                                                        ui.spacing_mut().item_spacing.x = 5.0;
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "+{}",
                                                                st.insertions
                                                            ))
                                                            .size(10.0)
                                                            .color(ADD_STRIP),
                                                        );
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                "-{}",
                                                                st.deletions
                                                            ))
                                                            .size(10.0)
                                                            .color(DEL_STRIP),
                                                        );
                                                    });
                                                } else {
                                                    ui.label(
                                                        egui::RichText::new("(first)")
                                                            .size(10.0)
                                                            .color(TEXT_DIM),
                                                    );
                                                }
                                            });
                                        });
                                    });
                                if fr.response.clicked() {
                                    self.snap_sel = i;
                                    self.diff_cache_id = None;
                                }
                            });
                        }
                    });
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} tracked files  +{} -{}",
                        self.paths_with_stats.len(),
                        tot_add,
                        tot_del
                    ))
                    .small()
                    .color(TEXT_DIM),
                );
            });
    }

    fn use_insert_only_layout(stats: &view::DiffLineStats, rows: &[view::SbsRow]) -> bool {
        if stats.deletions != 0 || rows.is_empty() {
            return false;
        }
        !rows.iter().any(|r| {
            matches!(
                r,
                view::SbsRow::DeleteLine { .. } | view::SbsRow::Both { .. }
            )
        })
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
        let strip_w = if strip == egui::Color32::TRANSPARENT {
            0.0
        } else {
            STRIP_W
        };
        let code_w = (width - strip_w - GUTTER_W).max(1.0);
        let ln = line_no
            .map(|n| n.to_string())
            .unwrap_or_else(|| " ".to_string());
        let gutter_rt = if let Some(s) = sign {
            egui::RichText::new(format!("{s} {ln}"))
                .family(egui::FontFamily::Monospace)
                .size(GUTTER_FONT)
                .color(fg)
        } else {
            egui::RichText::new(&ln)
                .family(egui::FontFamily::Monospace)
                .size(GUTTER_FONT)
                .color(TEXT_DIM)
        };
        let code_rt = egui::RichText::new(text)
            .family(egui::FontFamily::Monospace)
            .size(CODE_FONT)
            .color(fg);

        egui::Frame::new()
            .fill(row_fill)
            .inner_margin(egui::Margin::ZERO)
            .show(ui, |ui| {
                ui.set_width(width);
                ui.set_height(row_h);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    if strip_w > 0.0 {
                        egui::Frame::new()
                            .fill(strip)
                            .inner_margin(egui::Margin::ZERO)
                            .show(ui, |ui| {
                                ui.set_width(strip_w);
                                ui.set_height(row_h);
                            });
                    }
                    egui::Frame::new()
                        .fill(gutter_fill)
                        .inner_margin(egui::Margin::symmetric(2, CODE_PAD_Y))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(GUTTER_W, row_h),
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(gutter_rt);
                                },
                            );
                        });
                    egui::Frame::new()
                        .fill(row_fill)
                        .inner_margin(egui::Margin::symmetric(CODE_PAD_X, CODE_PAD_Y))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(code_w, row_h),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.label(code_rt);
                                },
                            );
                        });
                });
            });
    }

    fn paint_insert_only_cell(
        ui: &mut egui::Ui,
        row_h: f32,
        line_no: Option<usize>,
        text: &str,
        row_fill: egui::Color32,
        gutter_fill: egui::Color32,
        strip: egui::Color32,
        fg: egui::Color32,
        sign: Option<&'static str>,
    ) {
        let w = ui.available_width();
        Self::paint_sbs_cell(
            ui,
            w,
            row_h,
            line_no,
            text,
            row_fill,
            gutter_fill,
            strip,
            fg,
            sign,
        );
    }

    fn paint_insert_only_rows(
        ui: &mut egui::Ui,
        rows: &[view::SbsRow],
        diff_row_sel: &mut Option<usize>,
    ) {
        let row_h = DIFF_ROW_H;
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        for (idx, row) in rows.iter().enumerate() {
            let ir = ui.allocate_ui_with_layout(
                egui::vec2(w, row_h),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    ui.set_height(row_h);
                    match row {
                        view::SbsRow::Equal {
                            new_ln,
                            right,
                            ..
                        } => {
                            let fill = if idx % 2 == 0 { CTX_ROW } else { CTX_ROW_ALT };
                            Self::paint_insert_only_cell(
                                ui,
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
                        view::SbsRow::InsertLine { new_ln, text } => {
                            Self::paint_insert_only_cell(
                                ui,
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
                        view::SbsRow::Skipped { unchanged } => {
                            egui::Frame::new()
                                .fill(HUNK_BAR)
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.set_height(row_h);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "· · ·  {unchanged} unchanged lines  · · ·"
                                            ))
                                            .small()
                                            .italics()
                                            .color(TEXT_DIM),
                                        );
                                    });
                                });
                        }
                        view::SbsRow::DeleteLine { .. } | view::SbsRow::Both { .. } => {}
                    }
                },
            );
            if ir.response.clicked() {
                *diff_row_sel = Some(idx);
            }
            if *diff_row_sel == Some(idx) {
                ui.painter().rect_stroke(
                    ir.response.rect,
                    1.0,
                    egui::Stroke::new(1.0, ACCENT_DIM),
                    egui::epaint::StrokeKind::Inside,
                );
            }
        }
    }

    fn paint_sbs_spacer(ui: &mut egui::Ui, width: f32, row_h: f32) {
        egui::Frame::new()
            .fill(CENTER_BG)
            .show(ui, |ui| {
                ui.set_width(width);
                ui.set_height(row_h);
            });
    }

    fn paint_sbs_rows(
        ui: &mut egui::Ui,
        rows: &[view::SbsRow],
        diff_row_sel: &mut Option<usize>,
    ) {
        let row_h = DIFF_ROW_H;
        let half = (ui.available_width() - DIFF_COL_GAP) * 0.5;
        ui.spacing_mut().item_spacing.y = 0.0;
        let w = ui.available_width();
        for (idx, row) in rows.iter().enumerate() {
            let ir = ui.allocate_ui_with_layout(
                egui::vec2(w, row_h),
                egui::Layout::left_to_right(egui::Align::Min),
                |ui| {
                    ui.set_height(row_h);
                    ui.horizontal_top(|ui| {
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
                            view::SbsRow::Skipped { unchanged } => {
                                let sw = half * 2.0 + DIFF_COL_GAP;
                                egui::Frame::new()
                                    .fill(HUNK_BAR)
                                    .inner_margin(egui::Margin::symmetric(4, 2))
                                    .show(ui, |ui| {
                                        ui.set_width(sw);
                                        ui.set_height(row_h);
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "· · ·  {unchanged} unchanged lines  · · ·"
                                                ))
                                                .small()
                                                .italics()
                                                .color(TEXT_DIM),
                                            );
                                        });
                                    });
                            }
                        }
                    });
                },
            );
            if ir.response.clicked() {
                *diff_row_sel = Some(idx);
            }
            if *diff_row_sel == Some(idx) {
                ui.painter().rect_stroke(
                    ir.response.rect,
                    1.0,
                    egui::Stroke::new(1.0, ACCENT_DIM),
                    egui::epaint::StrokeKind::Inside,
                );
            }
        }
    }

    fn center_column(&mut self, ui: &mut egui::Ui) {
        self.recompute_diff_cache();
        let snap = self.path_snaps.get(self.snap_sel);
        egui::Frame::new()
            .fill(CENTER_BG)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(s) = snap {
                        let st = self.sbs_stats;
                        let insert_only =
                            Self::use_insert_only_layout(&st, &self.sbs_rows);
                        let header = s.path.clone();
                        egui::Frame::new()
                            .fill(TOPBAR)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}  ·  #{}",
                                            header, s.id
                                        ))
                                        .strong()
                                        .color(TEXT)
                                        .size(12.5)
                                        .family(egui::FontFamily::Monospace),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.menu_button("⋯", |ui| {
                                                if ui.button("Copy path").clicked() {
                                                    ui.ctx().copy_text(s.path.clone());
                                                    ui.close();
                                                }
                                            });
                                            ui.add_space(6.0);
                                            ui.add_enabled(
                                                false,
                                                egui::Button::new(
                                                    egui::RichText::new("Stage")
                                                        .strong()
                                                        .color(TEXT),
                                                )
                                                .fill(ACCENT)
                                                .min_size(egui::vec2(64.0, 24.0)),
                                            );
                                            ui.add_space(8.0);
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
                        let diff_h = (ui.available_height() * 0.74).clamp(140.0, 920.0);
                        egui::ScrollArea::vertical()
                            .id_salt("diffloom_main_sbs")
                            .max_height(diff_h)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                if self.sbs_rows.is_empty() {
                                    ui.label(
                                        egui::RichText::new(&self.diff_cache_text)
                                            .family(egui::FontFamily::Monospace)
                                            .size(11.0)
                                            .color(TEXT_DIM),
                                    );
                                } else if insert_only {
                                    Self::paint_insert_only_rows(
                                        ui,
                                        &self.sbs_rows,
                                        &mut self.diff_row_sel,
                                    );
                                } else {
                                    Self::paint_sbs_rows(
                                        ui,
                                        &self.sbs_rows,
                                        &mut self.diff_row_sel,
                                    );
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let sym_count = db::load_symbol_changes(&self.conn, s.id)
                                .map(|v| v.len())
                                .unwrap_or(0);
                            let sym_lbl = format!("SYMBOLS ({sym_count})");
                            for (i, name) in ["SUMMARY", sym_lbl.as_str()]
                                .into_iter()
                                .enumerate()
                            {
                                let sel = self.bottom_tab == i;
                                let fill = if sel { TAB_ACTIVE } else { egui::Color32::TRANSPARENT };
                                let text_color = if sel { ACCENT } else { TEXT_DIM };
                                if ui
                                    .add_sized(
                                        [96.0, 24.0],
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
                                                .size(12.0)
                                                .color(TEXT)
                                                .line_height(Some(18.0)),
                                        );
                                    }
                                    _ => {
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
                                                    .size(11.0)
                                                    .color(TEXT),
                                                );
                                            }
                                        }
                                    }
                                }
                            });
                    } else {
                        ui.label(
                            egui::RichText::new("Pick a file with history to inspect versions.")
                                .color(TEXT_DIM),
                        );
                    }
                });
            });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if !self.paths_with_stats.is_empty() {
            let n = self.paths_with_stats.len();
            if ctx.input(|i| i.key_pressed(egui::Key::J)) {
                self.path_sel = (self.path_sel + 1).min(n - 1);
                self.path_versions_path = None;
                self.diff_cache_id = None;
                self.diff_row_sel = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::K)) {
                self.path_sel = self.path_sel.saturating_sub(1);
                self.path_versions_path = None;
                self.diff_cache_id = None;
                self.diff_row_sel = None;
            }
        }
        if self.path_snaps.len() > 1 {
            let m = self.path_snaps.len();
            if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                self.snap_sel = (self.snap_sel + 1).min(m - 1);
                self.diff_cache_id = None;
                self.diff_row_sel = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                self.snap_sel = self.snap_sel.saturating_sub(1);
                self.diff_cache_id = None;
                self.diff_row_sel = None;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num1)) {
            self.bottom_tab = 0;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Num2)) {
            self.bottom_tab = 1;
        }
    }
}

impl eframe::App for DiffloomGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut ingested = false;
        while let Ok(p) = self.file_rx.try_recv() {
            ingested = true;
            let _ = ingest::ingest_path(&mut self.conn, &self.root, &p);
        }
        let scan_key = format!("{:?}|{}", self.session_filter, self.search.trim());
        if ingested || scan_key != self.paths_scan_key {
            self.paths_scan_key = scan_key;
            self.rebuild_path_listings();
        }
        self.sync_selected_path_versions();
        self.recompute_diff_cache();

        ui.visuals_mut().override_text_color = Some(TEXT);

        ui.vertical(|ui| {
            self.top_bar(ui);
            let body_h = (ui.available_height() - 32.0).max(160.0);
            ui.horizontal(|ui| {
                ui.set_min_height(body_h);
                ui.set_max_height(body_h);
                ui.vertical(|ui| {
                    ui.set_width(SIDEBAR_W);
                    ui.set_min_height(body_h);
                    self.sidebar(ui);
                });
                ui.add_space(4.0);
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
            .request_repaint_after(Duration::from_millis(900));
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
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(6);
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
            .with_min_inner_size([880.0, 500.0]),
        ..Default::default()
    };

    let app = DiffloomGui {
        root,
        conn,
        file_rx,
        _debouncer: debouncer,
        sessions: vec![],
        paths_with_stats: vec![],
        path_sel: 0,
        path_versions_path: None,
        path_snaps: vec![],
        snap_stats: vec![],
        snap_sel: 0,
        session_filter: None,
        search: String::new(),
        bottom_tab: 0,
        diff_cache_id: None,
        diff_cache_text: String::new(),
        sbs_rows: vec![],
        sbs_stats: view::DiffLineStats::default(),
        diff_row_sel: None,
        paths_scan_key: String::new(),
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
