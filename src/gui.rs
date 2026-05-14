use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use eframe::egui;
use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;

use crate::{db, ingest, paths, view, watcher};

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
const HUNK_BAR: egui::Color32 = egui::Color32::from_rgb(40, 36, 58);
const TAB_ACTIVE: egui::Color32 = egui::Color32::from_rgb(36, 32, 52);
const STATUS_BG: egui::Color32 = egui::Color32::from_rgb(16, 17, 22);

struct DiffloomGui {
    root: PathBuf,
    conn: rusqlite::Connection,
    file_rx: std::sync::mpsc::Receiver<PathBuf>,
    _debouncer: Debouncer<RecommendedWatcher>,
    sessions: Vec<db::SessionRow>,
    snaps: Vec<db::SnapshotListRow>,
    session_filter: Option<i64>,
    snap_sel: usize,
    search: String,
    bottom_tab: usize,
    diff_cache_id: Option<i64>,
    diff_cache_text: String,
}

impl DiffloomGui {
    fn refresh_lists(&mut self) {
        self.sessions = db::list_sessions(&self.conn, 40).unwrap_or_default();
        let mut snaps = match self.session_filter {
            Some(sid) => db::list_snapshots_for_session(&self.conn, sid, 120).unwrap_or_default(),
            None => db::list_recent_snapshots(&self.conn, 120).unwrap_or_default(),
        };
        let q = self.search.trim().to_lowercase();
        if !q.is_empty() {
            snaps.retain(|s| s.path.to_lowercase().contains(&q));
        }
        self.snaps = snaps;
        if self.snaps.is_empty() {
            self.snap_sel = 0;
        } else {
            self.snap_sel = self.snap_sel.min(self.snaps.len() - 1);
        }
    }

    fn recompute_diff_cache(&mut self) {
        let id = self.snaps.get(self.snap_sel).map(|s| s.id);
        if self.diff_cache_id == id {
            return;
        }
        self.diff_cache_id = id;
        self.diff_cache_text = match id {
            Some(sid) => view::unified_diff_for_snapshot(&self.conn, sid)
                .unwrap_or_else(|e| format!("(diff unavailable)\n{e:#}\n")),
            None => String::new(),
        };
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(TOPBAR)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Diffloom")
                            .size(20.0)
                            .strong()
                            .color(TEXT),
                    );
                    ui.label(
                        egui::RichText::new("·")
                            .color(TEXT_DIM)
                            .size(16.0),
                    );
                    ui.label(
                        egui::RichText::new(self.root.display().to_string())
                            .size(12.5)
                            .family(egui::FontFamily::Monospace)
                            .color(ACCENT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .desired_width(220.0)
                                .hint_text("Filter paths…"),
                        );
                        ui.label(
                            egui::RichText::new("f")
                                .small()
                                .color(TEXT_DIM)
                                .background_color(RAIL)
                                .code(),
                        );
                        ui.add_space(6.0);
                    });
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
                    ui.spacing_mut().item_spacing.x = 18.0;
                    ui.label(
                        egui::RichText::new("j / k")
                            .strong()
                            .color(ACCENT)
                            .small(),
                    );
                    ui.label(egui::RichText::new("move snapshot").small().color(TEXT_DIM));
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

    fn left_rail(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(RAIL)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("SESSIONS")
                        .small()
                        .strong()
                        .color(ACCENT),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("diffloom_rail_scroll")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.push_id("all_sessions", |ui| {
                            let sel = self.session_filter.is_none();
                            if ui
                                .selectable_label(sel, "All timelines")
                                .clicked()
                            {
                                self.session_filter = None;
                                self.snap_sel = 0;
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
                                if ui.selectable_label(sel, &label).clicked() {
                                    self.session_filter = Some(s.id);
                                    self.snap_sel = 0;
                                }
                            });
                        }
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "SNAPSHOTS ({})",
                                self.snaps.len()
                            ))
                            .small()
                            .strong()
                            .color(ACCENT),
                        );
                        ui.add_space(6.0);
                        for (i, s) in self.snaps.iter().enumerate() {
                            ui.push_id(("snap", s.id), |ui| {
                                let short: String = s.content_sha256.chars().take(7).collect();
                                let label = format!("{}  {}", s.path, short);
                                let sel = self.snap_sel == i;
                                if ui.selectable_label(sel, label).clicked() {
                                    self.snap_sel = i;
                                }
                            });
                        }
                    });
            });
    }

    fn paint_diff_lines(ui: &mut egui::Ui, text: &str) {
        let row_h = ui.spacing().interact_size.y.max(20.0);
        let full_w = ui.available_width();
        for (idx, line) in text.lines().enumerate() {
            ui.push_id(("diffline", idx), |ui| {
                let is_path_new = line.starts_with("+++");
                let is_path_old = line.starts_with("---");
                let is_hunk = line.starts_with("@@");
                let is_add = line.starts_with('+') && !is_path_new;
                let is_del = line.starts_with('-') && !is_path_old;

                if is_path_new || is_path_old || is_hunk {
                    let fill = if is_hunk { HUNK_BAR } else { TOPBAR };
                    egui::Frame::new()
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            ui.set_width(full_w);
                            ui.label(
                                egui::RichText::new(line)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.0)
                                    .color(if is_hunk { ACCENT } else { TEXT_DIM }),
                            );
                        });
                    return;
                }

                let (strip, gutter, row, sign, rest, fg) = if is_add {
                    (
                        ADD_STRIP,
                        ADD_GUTTER,
                        ADD_ROW,
                        "+",
                        line.get(1..).unwrap_or(""),
                        ADD_FG,
                    )
                } else if is_del {
                    (
                        DEL_STRIP,
                        DEL_GUTTER,
                        DEL_ROW,
                        "−",
                        line.get(1..).unwrap_or(""),
                        DEL_FG,
                    )
                } else {
                    let rest = line.strip_prefix(' ').unwrap_or(line);
                    let row = if idx % 2 == 0 { CTX_ROW } else { CTX_ROW_ALT };
                    (
                        egui::Color32::TRANSPARENT,
                        egui::Color32::from_rgb(26, 28, 36),
                        row,
                        " ",
                        rest,
                        TEXT_DIM,
                    )
                };

                egui::Frame::new()
                    .fill(row)
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                        ui.set_width(full_w);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            if strip != egui::Color32::TRANSPARENT {
                                egui::Frame::new()
                                    .fill(strip)
                                    .show(ui, |ui| {
                                        ui.set_width(5.0);
                                        ui.set_min_height(row_h);
                                    });
                            }
                            egui::Frame::new()
                                .fill(gutter)
                                .inner_margin(egui::Margin::symmetric(0, 3))
                                .show(ui, |ui| {
                                    ui.set_width(26.0);
                                    ui.set_min_height(row_h);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            egui::RichText::new(sign)
                                                .strong()
                                                .size(13.0)
                                                .color(fg),
                                        );
                                    });
                                });
                            egui::Frame::new()
                                .fill(row)
                                .inner_margin(egui::Margin::symmetric(8, 3))
                                .show(ui, |ui| {
                                    ui.set_min_height(row_h);
                                    ui.label(
                                        egui::RichText::new(rest)
                                            .family(egui::FontFamily::Monospace)
                                            .size(12.5)
                                            .color(fg),
                                    );
                                });
                        });
                    });
            });
        }
    }

    fn center_column(&mut self, ui: &mut egui::Ui) {
        self.recompute_diff_cache();
        let snap = self.snaps.get(self.snap_sel);
        egui::Frame::new()
            .fill(CENTER_BG)
            .stroke(egui::Stroke::new(1.0, RAIL_EDGE))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(s) = snap {
                        let adds = self.diff_cache_text.matches('\n').count();
                        let header = format!("{}  ·  #{}", s.path, s.id);
                        egui::Frame::new()
                            .fill(TOPBAR)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(header)
                                            .strong()
                                            .color(TEXT)
                                            .size(14.0),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("{adds} lines"))
                                                    .small()
                                                    .color(TEXT_DIM),
                                            );
                                        },
                                    );
                                });
                            });
                        let diff_h = (ui.available_height() * 0.55).clamp(120.0, 800.0);
                        egui::ScrollArea::vertical()
                            .id_salt("diffloom_main_diff")
                            .max_height(diff_h)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                Self::paint_diff_lines(ui, &self.diff_cache_text);
                            });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for (i, name) in ["SUMMARY", "SYMBOLS", "HISTORY"].iter().enumerate() {
                                let sel = self.bottom_tab == i;
                                let fill = if sel { TAB_ACTIVE } else { egui::Color32::TRANSPARENT };
                                let text_color = if sel { ACCENT } else { TEXT_DIM };
                                if ui
                                    .add_sized(
                                        [88.0, 28.0],
                                        egui::Button::new(
                                            egui::RichText::new(*name)
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
                                            .unwrap_or_else(|| {
                                                "(no summary yet)".to_string()
                                            });
                                        ui.label(
                                            egui::RichText::new(txt)
                                                .size(13.0)
                                                .color(TEXT)
                                                .line_height(Some(20.0)),
                                        );
                                    }
                                    1 => {
                                        let rows =
                                            db::load_symbol_changes(&self.conn, s.id)
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
                                                "Earlier snapshots for `{path}`"
                                            ))
                                            .small()
                                            .color(TEXT_DIM),
                                        );
                                        ui.add_space(6.0);
                                        for row in self
                                            .snaps
                                            .iter()
                                            .filter(|r| r.path == path)
                                            .take(25)
                                        {
                                            ui.push_id(("hist", row.id), |ui| {
                                                let short: String =
                                                    row.content_sha256.chars().take(8).collect();
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
                            egui::RichText::new("No snapshots match this filter.")
                                .color(TEXT_DIM),
                        );
                    }
                });
            });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.snaps.is_empty() {
            return;
        }
        let n = self.snaps.len();
        if ctx.input(|i| i.key_pressed(egui::Key::J)) {
            self.snap_sel = (self.snap_sel + 1).min(n - 1);
            self.diff_cache_id = None;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::K)) {
            self.snap_sel = self.snap_sel.saturating_sub(1);
            self.diff_cache_id = None;
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
                    ui.set_width(288.0);
                    ui.set_min_height(body_h);
                    self.left_rail(ui);
                });
                ui.add_space(10.0);
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
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 520.0]),
        ..Default::default()
    };

    let app = DiffloomGui {
        root,
        conn,
        file_rx,
        _debouncer: debouncer,
        sessions: vec![],
        snaps: vec![],
        session_filter: None,
        snap_sel: 0,
        search: String::new(),
        bottom_tab: 0,
        diff_cache_id: None,
        diff_cache_text: String::new(),
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
