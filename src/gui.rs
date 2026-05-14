use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use eframe::egui;
use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;

use crate::{db, ingest, paths, view, watcher};

const PANEL: egui::Color32 = egui::Color32::from_rgb(24, 26, 34);
const PANEL_EDGE: egui::Color32 = egui::Color32::from_rgb(52, 58, 74);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(94, 234, 212);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(148, 156, 176);
const CANVAS: egui::Color32 = egui::Color32::from_rgb(13, 14, 18);

struct DiffloomGui {
    root: PathBuf,
    conn: rusqlite::Connection,
    file_rx: std::sync::mpsc::Receiver<PathBuf>,
    _debouncer: Debouncer<RecommendedWatcher>,
    sessions: Vec<db::SessionRow>,
    snaps: Vec<db::SnapshotListRow>,
    session_sel: usize,
    snap_sel: usize,
    focus_sessions: bool,
}

impl DiffloomGui {
    fn refresh_lists(&mut self) {
        self.sessions = db::list_sessions(&self.conn, 40).unwrap_or_default();
        self.snaps = db::list_recent_snapshots(&self.conn, 80).unwrap_or_default();
        if self.snaps.is_empty() {
            self.snap_sel = 0;
        } else {
            self.snap_sel = self.snap_sel.min(self.snaps.len() - 1);
        }
        if self.sessions.is_empty() {
            self.session_sel = 0;
        } else {
            self.session_sel = self.session_sel.min(self.sessions.len() - 1);
        }
    }

    fn panel_wrap(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0, PANEL_EDGE))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(14))
            .show(ui, add_contents);
    }
}

impl eframe::App for DiffloomGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(p) = self.file_rx.try_recv() {
            let _ = ingest::ingest_path(&mut self.conn, &self.root, &p);
        }
        self.refresh_lists();
        let sel = (!self.snaps.is_empty()).then_some(self.snap_sel);
        let detail = view::snapshot_detail(&self.conn, &self.snaps, sel);

        ui.vertical(|ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Diffloom")
                        .size(22.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(12.0);
                ui.label(egui::RichText::new("·").color(TEXT_DIM));
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(self.root.display().to_string())
                        .size(13.0)
                        .family(egui::FontFamily::Monospace)
                        .color(ACCENT),
                );
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Live workspace timeline — pick a session or snapshot to inspect symbols and summaries.")
                    .size(12.0)
                    .color(TEXT_DIM),
            );
            ui.add_space(14.0);

            let h = ui.available_height();
            ui.horizontal(|ui| {
                ui.set_min_height(h);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.22);
                    Self::panel_wrap(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Sessions")
                                .size(14.0)
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (i, s) in self.sessions.iter().enumerate() {
                                let closed = if s.closed_at.is_some() {
                                    " · closed"
                                } else {
                                    ""
                                };
                                let label =
                                    format!("#{} {} [{}]{}", s.id, s.title, s.kind, closed);
                                let selected =
                                    self.session_sel == i && self.focus_sessions;
                                let r = ui.selectable_label(selected, &label);
                                if r.clicked() {
                                    self.session_sel = i;
                                    self.focus_sessions = true;
                                }
                            }
                        });
                    });
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() * 0.34);
                    Self::panel_wrap(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Snapshots")
                                .size(14.0)
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (i, s) in self.snaps.iter().enumerate() {
                                let short: String =
                                    s.content_sha256.chars().take(8).collect();
                                let label =
                                    format!("#{} {} {}", s.id, s.path, short);
                                let selected =
                                    self.snap_sel == i && !self.focus_sessions;
                                let r = ui.selectable_label(selected, &label);
                                if r.clicked() {
                                    self.snap_sel = i;
                                    self.focus_sessions = false;
                                }
                            }
                        });
                    });
                });

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    Self::panel_wrap(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Summary / symbols")
                                .size(14.0)
                                .strong()
                                .color(ACCENT),
                        );
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&detail)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.0),
                            );
                        });
                    });
                });
            });
        });

        ui.ctx()
            .request_repaint_after(Duration::from_millis(200));
    }
}

fn setup_style(cc: &eframe::CreationContext<'_>) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = CANVAS;
    visuals.panel_fill = CANVAS;
    visuals.extreme_bg_color = PANEL;
    visuals.faint_bg_color = PANEL;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_DIM;
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(32, 36, 48);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(40, 46, 62);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(48, 56, 76);
    visuals.selection.bg_fill = egui::Color32::from_rgb(40, 72, 68);
    visuals.selection.stroke.color = ACCENT;
    cc.egui_ctx.set_visuals(visuals);

    let mut style = (*cc.egui_ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10);
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
            .with_inner_size([1080.0, 680.0])
            .with_min_inner_size([720.0, 440.0]),
        ..Default::default()
    };

    let app = DiffloomGui {
        root,
        conn,
        file_rx,
        _debouncer: debouncer,
        sessions: vec![],
        snaps: vec![],
        session_sel: 0,
        snap_sel: 0,
        focus_sessions: false,
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
