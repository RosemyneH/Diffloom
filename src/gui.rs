use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use eframe::egui;
use notify::RecommendedWatcher;
use notify_debouncer_mini::Debouncer;

use crate::{db, ingest, paths, view, watcher};

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
}

impl eframe::App for DiffloomGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(p) = self.file_rx.try_recv() {
            let _ = ingest::ingest_path(&mut self.conn, &self.root, &p);
        }
        self.refresh_lists();
        let sel = (!self.snaps.is_empty()).then_some(self.snap_sel);
        let detail = view::snapshot_detail(&self.conn, &self.snaps, sel);

        egui::Frame::new().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.heading("Diffloom");
                    ui.separator();
                    ui.label(
                        egui::RichText::new(self.root.display().to_string())
                            .weak()
                            .monospace(),
                    );
                });
                ui.separator();
                let h = ui.available_height();
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Sessions").strong());
                        egui::ScrollArea::vertical()
                            .max_height(h)
                            .show(ui, |ui| {
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
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.session_sel = i;
                                        self.focus_sessions = true;
                                    }
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Snapshots").strong());
                        egui::ScrollArea::vertical()
                            .max_height(h)
                            .show(ui, |ui| {
                                for (i, s) in self.snaps.iter().enumerate() {
                                    let short: String =
                                        s.content_sha256.chars().take(8).collect();
                                    let label =
                                        format!("#{} {} {}", s.id, s.path, short);
                                    let selected =
                                        self.snap_sel == i && !self.focus_sessions;
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.snap_sel = i;
                                        self.focus_sessions = false;
                                    }
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Summary / symbols").strong());
                        egui::ScrollArea::vertical()
                            .max_height(h)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&detail)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });
                    });
                });
            });
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(200));
    }
}

pub fn run(root: PathBuf) -> anyhow::Result<()> {
    let root = paths::normalize_path(&root).context("root path")?;
    let conn = db::open_db(&root)?;
    let (debouncer, file_rx) =
        watcher::watch_workspace(root.clone(), Duration::from_millis(400))?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Diffloom")
            .with_inner_size([1024.0, 640.0])
            .with_min_inner_size([640.0, 400.0]),
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

    eframe::run_native("Diffloom", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}
