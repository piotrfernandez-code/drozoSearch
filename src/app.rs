use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

use eframe::egui;
use tantivy::Index;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

use crate::config::Config;
use crate::index::reader::SearchEngine;
use crate::index::schema;
use crate::indexer::coordinator;
use crate::types::*;

pub struct DrozoSearchApp {
    query: String,
    last_query_sent: String,
    last_keystroke: Instant,
    results: Vec<SearchResult>,
    selected_index: Option<usize>,
    first_frame: bool,
    scroll_to_selected: bool,
    context_menu_index: Option<usize>,

    search_tx: Sender<String>,
    results_rx: Receiver<Vec<SearchResult>>,
    progress_rx: Receiver<IndexProgress>,

    files_indexed: u64,
    estimated_total: u64,
    index_status: IndexStatus,

    logo_texture: Option<egui::TextureHandle>,

    // Tray icon (must stay alive)
    _tray_icon: Option<tray_icon::TrayIcon>,
    tray_show_id: tray_icon::menu::MenuId,
    tray_quit_id: tray_icon::menu::MenuId,
    window_visible: bool,
}

impl DrozoSearchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Dark theme
        let mut visuals = egui::Visuals::dark();
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_gray(22);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_gray(32);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_gray(42);
        visuals.widgets.active.bg_fill = egui::Color32::from_gray(50);
        visuals.selection.bg_fill = egui::Color32::from_rgb(35, 75, 130);
        visuals.extreme_bg_color = egui::Color32::from_gray(16);
        cc.egui_ctx.set_visuals(visuals);

        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(6.0, 1.0);
        cc.egui_ctx.set_style(style);

        let config = Config::default();
        std::fs::create_dir_all(&config.index_path).expect("Failed to create index directory");

        let tantivy_schema = schema::build_schema();
        // Open existing index or create a new one
        let index = Index::open_in_dir(&config.index_path).unwrap_or_else(|_| {
            Index::create_in_dir(&config.index_path, tantivy_schema.clone())
                .expect("Failed to create tantivy index")
        });

        let (search_tx, search_rx) = mpsc::channel::<String>();
        let (results_tx, results_rx) = mpsc::channel::<Vec<SearchResult>>();
        let (progress_tx, progress_rx) = mpsc::channel::<IndexProgress>();

        let search_index = index.clone();
        let search_ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            search_thread(search_index, search_rx, results_tx, search_ctx);
        });

        // Always run incremental indexing — it will skip unchanged files
        let _indexer_handle =
            coordinator::start_indexing(index, config, progress_tx, cc.egui_ctx.clone());

        // Load logo texture
        let logo_texture = {
            let icon_bytes = include_bytes!("../assets/icon.png");
            let img = image::load_from_memory(icon_bytes)
                .expect("Failed to load logo")
                .into_rgba8();
            let (w, h) = img.dimensions();
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                &img.into_raw(),
            );
            Some(cc.egui_ctx.load_texture("logo", color_image, egui::TextureOptions::LINEAR))
        };

        // ── Build tray icon ──
        let show_item = MenuItem::new("Show drozoSearch", true, None);
        let quit_item = MenuItem::new("Quit", true, None);
        let show_id = show_item.id().clone();
        let quit_id = quit_item.id().clone();

        let tray_menu = Menu::new();
        let _ = tray_menu.append(&show_item);
        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let _ = tray_menu.append(&quit_item);

        let tray_icon = {
            let icon_bytes = include_bytes!("../assets/icon.png");
            let img = image::load_from_memory(icon_bytes)
                .expect("Failed to load tray icon")
                .into_rgba8();
            let (w, h) = img.dimensions();
            let icon = tray_icon::Icon::from_rgba(img.into_raw(), w, h)
                .expect("Failed to create tray icon");

            TrayIconBuilder::new()
                .with_menu(Box::new(tray_menu))
                .with_tooltip("drozoSearch")
                .with_icon(icon)
                .build()
                .ok()
        };

        DrozoSearchApp {
            query: String::new(),
            last_query_sent: String::new(),
            last_keystroke: Instant::now(),
            results: Vec::new(),
            selected_index: None,
            first_frame: true,
            scroll_to_selected: false,
            context_menu_index: None,
            search_tx,
            results_rx,
            progress_rx,
            files_indexed: 0,
            estimated_total: 0,
            index_status: IndexStatus::Starting,
            logo_texture,
            _tray_icon: tray_icon,
            tray_show_id: show_id,
            tray_quit_id: quit_id,
            window_visible: true,
        }
    }
}

fn search_thread(
    index: Index,
    rx: Receiver<String>,
    tx: Sender<Vec<SearchResult>>,
    ctx: egui::Context,
) {
    let engine = SearchEngine::new(index);
    loop {
        let mut query = match rx.recv() {
            Ok(q) => q,
            Err(_) => return,
        };
        while let Ok(newer) = rx.try_recv() {
            query = newer;
        }
        let results = engine.search(&query, 200);
        let _ = tx.send(results);
        ctx.request_repaint();
    }
}

impl eframe::App for DrozoSearchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Handle window close → hide to tray ──
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_visible = false;
            #[cfg(target_os = "macos")]
            macos_hide_app();
        }

        // ── Poll tray events ──
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            // Click on tray icon toggles window
            if matches!(event, TrayIconEvent::Click { .. }) {
                if self.window_visible {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.window_visible = false;
                    #[cfg(target_os = "macos")]
                    macos_hide_app();
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.window_visible = true;
                    #[cfg(target_os = "macos")]
                    macos_show_app();
                }
            }
        }
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == &self.tray_show_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                self.window_visible = true;
                #[cfg(target_os = "macos")]
                macos_show_app();
            } else if event.id() == &self.tray_quit_id {
                std::process::exit(0);
            }
        }

        // ── Poll channels ──
        while let Ok(results) = self.results_rx.try_recv() {
            self.results = results;
        }
        while let Ok(progress) = self.progress_rx.try_recv() {
            self.files_indexed = progress.files_indexed;
            self.estimated_total = progress.estimated_total;
            self.index_status = progress.status;
        }

        // ── Debounced search ──
        if self.query != self.last_query_sent
            && self.last_keystroke.elapsed().as_millis() >= 150
        {
            let _ = self.search_tx.send(self.query.clone());
            self.last_query_sent = self.query.clone();
        }
        if self.query != self.last_query_sent {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // ── Keyboard navigation ──
        let down = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
        let up = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if escape {
            self.query.clear();
            self.results.clear();
            self.selected_index = None;
        }
        if down && !self.results.is_empty() {
            let max = self.results.len().saturating_sub(1);
            self.selected_index = Some(self.selected_index.map_or(0, |i| (i + 1).min(max)));
            self.scroll_to_selected = true;
        }
        if up && !self.results.is_empty() {
            self.selected_index = Some(self.selected_index.map_or(0, |i| i.saturating_sub(1)));
            self.scroll_to_selected = true;
        }
        if enter {
            if let Some(idx) = self.selected_index {
                if let Some(result) = self.results.get(idx) {
                    let _ = open::that(&result.file_path);
                }
            }
        }

        // ═══════════════════════════════════════
        // ── TOP PANEL: Search + Status ──
        // ═══════════════════════════════════════
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .fill(egui::Color32::from_gray(26)),
            )
            .show(ctx, |ui| {
                // Search row
                ui.horizontal(|ui| {
                    // Logo image
                    if let Some(tex) = &self.logo_texture {
                        let logo_size = egui::vec2(28.0, 28.0);
                        ui.image(egui::load::SizedTexture::new(tex.id(), logo_size));
                    }

                    // Search input with custom frame
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .corner_radius(egui::CornerRadius::same(6))
                        .fill(egui::Color32::from_gray(16))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(50)))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.query)
                                    .hint_text(
                                        egui::RichText::new("  Search files, content, metadata...")
                                            .color(egui::Color32::from_gray(70)),
                                    )
                                    .desired_width(ui.available_width())
                                    .frame(false)
                                    .font(egui::FontId::proportional(16.0)),
                            );

                            if response.changed() {
                                self.last_keystroke = Instant::now();
                                self.selected_index = None;
                            }
                            if self.first_frame {
                                response.request_focus();
                                self.first_frame = false;
                            }
                        });
                });

                ui.add_space(6.0);

                // Status row
                ui.horizontal(|ui| {
                    // Status dot + text
                    let (dot_color, status_str, is_active) = match &self.index_status {
                        IndexStatus::Counting => (
                            egui::Color32::from_rgb(150, 130, 255),
                            format!("Scanning... found {} files", format_count(self.estimated_total)),
                            true,
                        ),
                        IndexStatus::Starting => (
                            egui::Color32::from_rgb(255, 220, 50),
                            format!("Preparing to index {} files...", format_count(self.estimated_total)),
                            true,
                        ),
                        IndexStatus::Indexing => {
                            let pct = if self.estimated_total > 0 {
                                (self.files_indexed as f64 / self.estimated_total as f64 * 100.0).min(100.0)
                            } else {
                                0.0
                            };
                            (
                                egui::Color32::from_rgb(255, 150, 30),
                                format!(
                                    "Indexing  {} / {}  ({:.0}%)",
                                    format_count(self.files_indexed),
                                    format_count(self.estimated_total),
                                    pct,
                                ),
                                true,
                            )
                        }
                        IndexStatus::Committing => (
                            egui::Color32::from_rgb(255, 220, 50),
                            "Saving index...".into(),
                            true,
                        ),
                        IndexStatus::Ready(ref stats) => {
                            let mut text = format!("{} files indexed", format_count(self.files_indexed));
                            if let Some(s) = stats {
                                let mut parts = Vec::new();
                                if s.added > 0 {
                                    parts.push(format!("+{} new", s.added));
                                }
                                if s.updated > 0 {
                                    parts.push(format!("{} updated", s.updated));
                                }
                                if s.deleted > 0 {
                                    parts.push(format!("-{} removed", s.deleted));
                                }
                                if !parts.is_empty() {
                                    text.push_str(&format!("  ({})", parts.join(", ")));
                                }
                            }
                            (
                                egui::Color32::from_rgb(60, 200, 80),
                                text,
                                false,
                            )
                        }
                        IndexStatus::Error(e) => (
                            egui::Color32::from_rgb(255, 80, 80),
                            format!("Error: {}", e),
                            false,
                        ),
                    };

                    // Animated dot
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let pulse = if is_active {
                        let t = ui.input(|i| i.time) as f32;
                        0.5 + 0.5 * (t * 3.0).sin()
                    } else {
                        1.0
                    };
                    let dot_alpha = (pulse * 255.0) as u8;
                    let pulsing_color = egui::Color32::from_rgba_premultiplied(
                        dot_color.r(),
                        dot_color.g(),
                        dot_color.b(),
                        dot_alpha,
                    );
                    ui.painter().circle_filled(rect.center(), 4.0, pulsing_color);

                    if is_active {
                        ctx.request_repaint();
                    }

                    ui.label(
                        egui::RichText::new(status_str)
                            .size(11.0)
                            .color(egui::Color32::from_gray(120)),
                    );

                    // Progress bar during indexing (real percentage)
                    if matches!(self.index_status, IndexStatus::Indexing) && self.estimated_total > 0 {
                        let bar_width = 120.0;
                        let (bar_rect, _) = ui.allocate_exact_size(
                            egui::vec2(bar_width, 6.0),
                            egui::Sense::hover(),
                        );
                        // Background track
                        ui.painter().rect_filled(
                            bar_rect,
                            egui::CornerRadius::same(3),
                            egui::Color32::from_gray(40),
                        );
                        // Fill based on real progress
                        let progress_frac = (self.files_indexed as f32 / self.estimated_total as f32).min(1.0);
                        let fill_width = bar_rect.width() * progress_frac;
                        if fill_width > 0.0 {
                            let fill_rect = egui::Rect::from_min_size(
                                bar_rect.min,
                                egui::vec2(fill_width, bar_rect.height()),
                            );
                            ui.painter().rect_filled(
                                fill_rect,
                                egui::CornerRadius::same(3),
                                egui::Color32::from_rgb(90, 160, 255),
                            );
                        }
                    }

                    // Indeterminate bar during counting
                    if matches!(self.index_status, IndexStatus::Counting) {
                        let bar_width = 80.0;
                        let (bar_rect, _) = ui.allocate_exact_size(
                            egui::vec2(bar_width, 4.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(
                            bar_rect,
                            egui::CornerRadius::same(2),
                            egui::Color32::from_gray(35),
                        );
                        let t = ui.input(|i| i.time) as f32;
                        let pos = ((t * 1.5).sin() * 0.5 + 0.5) * 0.7;
                        let fill_rect = egui::Rect::from_min_size(
                            egui::pos2(bar_rect.min.x + bar_rect.width() * pos, bar_rect.min.y),
                            egui::vec2(bar_rect.width() * 0.3, bar_rect.height()),
                        );
                        ui.painter().rect_filled(
                            fill_rect.intersect(bar_rect),
                            egui::CornerRadius::same(2),
                            egui::Color32::from_rgb(150, 130, 255),
                        );
                    }

                    // Result count on the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.results.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("{} results", self.results.len()))
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(100)),
                            );
                        }
                    });
                });
            });

        // ═══════════════════════════════════════
        // ── BOTTOM STATUS BAR ──
        // ═══════════════════════════════════════
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(16, 4))
                    .fill(egui::Color32::from_gray(22)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let sep = |ui: &mut egui::Ui| {
                        ui.label(
                            egui::RichText::new("|")
                                .size(10.0)
                                .color(egui::Color32::from_gray(40)),
                        );
                    };
                    let hint = |ui: &mut egui::Ui, text: &str| {
                        ui.label(
                            egui::RichText::new(text)
                                .size(10.0)
                                .color(egui::Color32::from_gray(70)),
                        );
                    };
                    hint(ui, "Click open");
                    sep(ui);
                    hint(ui, "Shift+Click open with...");
                    sep(ui);
                    hint(ui, "Up/Down navigate");
                    sep(ui);
                    hint(ui, "Enter open");
                    sep(ui);
                    hint(ui, "ESC clear");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(idx) = self.selected_index {
                            if let Some(result) = self.results.get(idx) {
                                let path_display = result.file_path.to_string_lossy();
                                let display = truncate_path(&path_display, 80);
                                ui.label(
                                    egui::RichText::new(display)
                                        .size(10.0)
                                        .color(egui::Color32::from_gray(90)),
                                );
                            }
                        }
                    });
                });
            });

        // ═══════════════════════════════════════
        // ── CENTRAL PANEL: Results ──
        // ═══════════════════════════════════════
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(0))
                    .fill(egui::Color32::from_gray(18)),
            )
            .show(ctx, |ui| {
                // Empty state
                if self.query.is_empty() {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.vertical_centered(|ui| {
                        // Logo + title
                        if let Some(tex) = &self.logo_texture {
                            let logo_size = egui::vec2(64.0, 64.0);
                            ui.image(egui::load::SizedTexture::new(tex.id(), logo_size));
                        }
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("drozoSearch")
                                .size(36.0)
                                .strong()
                                .color(egui::Color32::from_gray(50)),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Search files, content & metadata instantly")
                                .size(14.0)
                                .color(egui::Color32::from_gray(65)),
                        );
                        ui.add_space(24.0);
                        ui.horizontal(|ui| {
                            ui.add_space(ui.available_width() / 2.0 - 120.0);
                            for (key, desc) in [("name:", "file names"), ("ext:", "extensions"), ("size>1mb", "by size")] {
                                egui::Frame::NONE
                                    .inner_margin(egui::Margin::symmetric(8, 3))
                                    .corner_radius(egui::CornerRadius::same(4))
                                    .fill(egui::Color32::from_gray(28))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new(key)
                                                    .size(11.0)
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(90, 160, 255)),
                                            );
                                            ui.label(
                                                egui::RichText::new(desc)
                                                    .size(11.0)
                                                    .color(egui::Color32::from_gray(80)),
                                            );
                                        });
                                    });
                            }
                        });
                    });
                    return;
                }

                if self.results.is_empty() {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No results")
                                .size(20.0)
                                .color(egui::Color32::from_gray(60)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Try a different search term")
                                .size(12.0)
                                .color(egui::Color32::from_gray(50)),
                        );
                    });
                    return;
                }

                // ── Column headers ──
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(16, 5))
                    .fill(egui::Color32::from_gray(24))
                    .show(ui, |ui| {
                        let widths = compute_column_widths(ui.available_width());
                        ui.horizontal(|ui| {
                            header_label(ui, "Name", widths.name);
                            header_label(ui, "Location", widths.path);
                            header_label(ui, "Type", widths.match_type);
                            header_label_right(ui, "Size", widths.size);
                            header_label_right(ui, "Modified", widths.modified);
                        });
                    });

                // Thin separator line
                let sep_rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                ui.painter()
                    .rect_filled(sep_rect, egui::CornerRadius::ZERO, egui::Color32::from_gray(35));

                // ── Results scroll area ──
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        let widths = compute_column_widths(ui.available_width() - 32.0);

                        for (i, result) in self.results.iter().enumerate() {
                            let is_selected = self.selected_index == Some(i);

                            let bg = if is_selected {
                                egui::Color32::from_rgb(25, 55, 100)
                            } else if i % 2 == 0 {
                                egui::Color32::from_gray(19)
                            } else {
                                egui::Color32::from_gray(16)
                            };

                            let hover_bg = if is_selected {
                                egui::Color32::from_rgb(30, 65, 115)
                            } else {
                                egui::Color32::from_gray(28)
                            };

                            let row_frame = egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(16, 4))
                                .fill(bg);

                            let row_resp = row_frame
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // ── Name column ──
                                        ui.allocate_ui(egui::vec2(widths.name, 20.0), |ui| {
                                            ui.horizontal(|ui| {
                                                let (icon, icon_color) = file_icon(result);
                                                ui.label(
                                                    egui::RichText::new(icon)
                                                        .size(13.0)
                                                        .strong()
                                                        .color(icon_color),
                                                );
                                                ui.label(
                                                    egui::RichText::new(&result.file_name)
                                                        .size(13.0)
                                                        .color(if is_selected {
                                                            egui::Color32::WHITE
                                                        } else {
                                                            egui::Color32::from_gray(220)
                                                        }),
                                                );
                                            });
                                        });

                                        // ── Path column ──
                                        ui.allocate_ui(egui::vec2(widths.path, 20.0), |ui| {
                                            let path_str = result
                                                .file_path
                                                .parent()
                                                .map(|p| {
                                                    let s = p.to_string_lossy().to_string();
                                                    // Shorten home dir
                                                    if let Some(home) = dirs::home_dir() {
                                                        let home_str = home.to_string_lossy().to_string();
                                                        if s.starts_with(&home_str) {
                                                            return format!("~{}", &s[home_str.len()..]);
                                                        }
                                                    }
                                                    s
                                                })
                                                .unwrap_or_default();
                                            let display_path = truncate_path(&path_str, 55);
                                            ui.label(
                                                egui::RichText::new(display_path)
                                                    .size(11.0)
                                                    .color(egui::Color32::from_gray(95)),
                                            );
                                        });

                                        // ── Match type badge ──
                                        ui.allocate_ui(egui::vec2(widths.match_type, 20.0), |ui| {
                                            let (label, badge_bg, badge_fg) = match result.match_type {
                                                MatchType::FileName => (
                                                    "NAME",
                                                    egui::Color32::from_rgb(25, 60, 30),
                                                    egui::Color32::from_rgb(90, 210, 90),
                                                ),
                                                MatchType::Content => (
                                                    "CONTENT",
                                                    egui::Color32::from_rgb(20, 40, 70),
                                                    egui::Color32::from_rgb(90, 155, 255),
                                                ),
                                                MatchType::Metadata => (
                                                    "META",
                                                    egui::Color32::from_rgb(60, 45, 15),
                                                    egui::Color32::from_rgb(255, 190, 60),
                                                ),
                                            };
                                            egui::Frame::NONE
                                                .inner_margin(egui::Margin::symmetric(6, 1))
                                                .corner_radius(egui::CornerRadius::same(3))
                                                .fill(badge_bg)
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(label)
                                                            .size(9.0)
                                                            .strong()
                                                            .color(badge_fg),
                                                    );
                                                });
                                        });

                                        // ── Size column ──
                                        ui.allocate_ui(egui::vec2(widths.size, 20.0), |ui| {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(format_size(
                                                            result.file_size,
                                                        ))
                                                        .size(11.0)
                                                        .color(egui::Color32::from_gray(110)),
                                                    );
                                                },
                                            );
                                        });

                                        // ── Modified column ──
                                        ui.allocate_ui(egui::vec2(widths.modified, 20.0), |ui| {
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(format_time_ago(
                                                            result.modified,
                                                        ))
                                                        .size(11.0)
                                                        .color(egui::Color32::from_gray(110)),
                                                    );
                                                },
                                            );
                                        });
                                    });
                                })
                                .response;

                            // Hover highlight
                            let interact = row_resp.interact(egui::Sense::click());
                            if interact.hovered() && !is_selected {
                                let painter = ui.painter();
                                painter.rect_filled(
                                    row_resp.rect,
                                    egui::CornerRadius::ZERO,
                                    hover_bg,
                                );
                            }

                            // Click: open file; Shift+click: "Open With" chooser
                            if interact.clicked() {
                                let shift_held = ui.input(|i| i.modifiers.shift);
                                if shift_held {
                                    open_with_chooser(&result.file_path);
                                } else {
                                    let _ = open::that(&result.file_path);
                                }
                                self.selected_index = Some(i);
                            }

                            // Right-click context menu
                            interact.context_menu(|ui| {
                                self.context_menu_index = Some(i);
                                if ui.button("Open file").clicked() {
                                    let _ = open::that(&result.file_path);
                                    ui.close_menu();
                                }
                                if ui.button("Open containing folder").clicked() {
                                    if let Some(parent) = result.file_path.parent() {
                                        let _ = open::that(parent);
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button("Copy full path").clicked() {
                                    ctx.copy_text(result.file_path.to_string_lossy().to_string());
                                    ui.close_menu();
                                }
                                if ui.button("Copy file name").clicked() {
                                    ctx.copy_text(result.file_name.clone());
                                    ui.close_menu();
                                }
                            });

                            // Scroll to selected item
                            if self.scroll_to_selected && is_selected {
                                ui.scroll_to_rect(row_resp.rect, Some(egui::Align::Center));
                            }

                            // Tooltip
                            if interact.hovered() {
                                interact.on_hover_text_at_pointer(
                                    result.file_path.to_string_lossy().to_string(),
                                );
                            }
                        }

                        self.scroll_to_selected = false;
                    });
            });
    }
}

// ── File type icon based on extension ──
fn file_icon(result: &SearchResult) -> (&'static str, egui::Color32) {
    if result.is_dir {
        return ("D", egui::Color32::from_rgb(90, 170, 255));
    }

    let ext = result
        .file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Rust
        "rs" => ("Rs", egui::Color32::from_rgb(255, 120, 50)),
        // JavaScript / TypeScript
        "js" | "jsx" | "mjs" => ("Js", egui::Color32::from_rgb(255, 220, 60)),
        "ts" | "tsx" => ("Ts", egui::Color32::from_rgb(50, 130, 240)),
        // Python
        "py" => ("Py", egui::Color32::from_rgb(80, 180, 80)),
        // Go
        "go" => ("Go", egui::Color32::from_rgb(0, 190, 220)),
        // C / C++
        "c" | "h" => ("C", egui::Color32::from_rgb(100, 150, 220)),
        "cpp" | "hpp" | "cc" | "cxx" => ("C+", egui::Color32::from_rgb(100, 150, 220)),
        // Java / Kotlin
        "java" => ("Jv", egui::Color32::from_rgb(230, 100, 50)),
        "kt" | "kts" => ("Kt", egui::Color32::from_rgb(170, 100, 255)),
        // Ruby
        "rb" => ("Rb", egui::Color32::from_rgb(220, 50, 50)),
        // Swift
        "swift" => ("Sw", egui::Color32::from_rgb(255, 130, 50)),
        // Shell
        "sh" | "bash" | "zsh" => ("Sh", egui::Color32::from_rgb(130, 200, 100)),
        // Web
        "html" | "htm" => ("Ht", egui::Color32::from_rgb(230, 100, 50)),
        "css" | "scss" | "sass" => ("Cs", egui::Color32::from_rgb(80, 140, 230)),
        "vue" => ("Vu", egui::Color32::from_rgb(65, 184, 131)),
        "svelte" => ("Sv", egui::Color32::from_rgb(255, 62, 0)),
        // Data / Config
        "json" => ("Js", egui::Color32::from_rgb(200, 200, 100)),
        "yaml" | "yml" => ("Ym", egui::Color32::from_rgb(200, 100, 100)),
        "toml" => ("Tm", egui::Color32::from_rgb(150, 150, 200)),
        "xml" => ("Xm", egui::Color32::from_rgb(200, 150, 100)),
        "csv" => ("Cv", egui::Color32::from_rgb(100, 200, 100)),
        "sql" => ("Sq", egui::Color32::from_rgb(200, 150, 50)),
        // Documents
        "md" | "markdown" => ("Md", egui::Color32::from_rgb(100, 180, 230)),
        "txt" => ("Tx", egui::Color32::from_gray(160)),
        "pdf" => ("Pd", egui::Color32::from_rgb(230, 70, 70)),
        "doc" | "docx" => ("Dc", egui::Color32::from_rgb(50, 120, 220)),
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => {
            ("Im", egui::Color32::from_rgb(200, 120, 220))
        }
        // Audio / Video
        "mp3" | "wav" | "flac" | "ogg" | "aac" => {
            ("Au", egui::Color32::from_rgb(255, 150, 100))
        }
        "mp4" | "mkv" | "avi" | "mov" | "webm" => {
            ("Vi", egui::Color32::from_rgb(200, 100, 200))
        }
        // Archives
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => {
            ("Zp", egui::Color32::from_rgb(180, 150, 100))
        }
        // Binary / Executable
        "exe" | "dll" | "so" | "dylib" => ("Bn", egui::Color32::from_rgb(200, 80, 80)),
        // Git
        "gitignore" | "gitattributes" | "gitmodules" => {
            ("Gt", egui::Color32::from_rgb(240, 80, 50))
        }
        // Docker
        "dockerfile" => ("Dk", egui::Color32::from_rgb(50, 150, 220)),
        // Log
        "log" => ("Lg", egui::Color32::from_gray(130)),
        // Env
        "env" => ("En", egui::Color32::from_rgb(255, 200, 50)),
        // Default
        _ => ("F", egui::Color32::from_gray(120)),
    }
}

fn header_label(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.allocate_ui(egui::vec2(width, 16.0), |ui| {
        ui.label(
            egui::RichText::new(text)
                .size(10.0)
                .strong()
                .color(egui::Color32::from_gray(100)),
        );
    });
}

fn header_label_right(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.allocate_ui(egui::vec2(width, 16.0), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(10.0)
                    .strong()
                    .color(egui::Color32::from_gray(100)),
            );
        });
    });
}

struct ColumnWidths {
    name: f32,
    path: f32,
    match_type: f32,
    size: f32,
    modified: f32,
}

fn compute_column_widths(total: f32) -> ColumnWidths {
    let match_type = 70.0;
    let size = 65.0;
    let modified = 70.0;
    let fixed = match_type + size + modified + 40.0;
    let remaining = (total - fixed).max(200.0);
    let name = remaining * 0.35;
    let path = remaining * 0.65;

    ColumnWidths {
        name,
        path,
        match_type,
        size,
        modified,
    }
}

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - (max_len - 3)..])
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Open the system "Open With" chooser for a file.
fn open_with_chooser(path: &std::path::Path) {
    let path = path.to_path_buf();
    // Run in a thread so we don't block the GUI
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            // AppleScript: ask user to choose an application, then open the file with it
            let script = format!(
                r#"set chosenApp to choose application with prompt "Open with..."
set appPath to POSIX path of (path to chosenApp)
do shell script "open -a " & quoted form of appPath & " " & quoted form of "{}"
"#,
                path.to_string_lossy().replace('"', "\\\"")
            );
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .spawn();
        }

        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("rundll32")
                .arg("shell32.dll,OpenAs_RunDll")
                .arg(&path)
                .spawn();
        }

        #[cfg(target_os = "linux")]
        {
            // Try mimeopen --ask first, fall back to xdg-open
            let status = std::process::Command::new("mimeopen")
                .arg("--ask")
                .arg(&path)
                .status();
            if status.is_err() {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&path)
                    .spawn();
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn macos_hide_app() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.hide(None);
    }
}

#[cfg(target_os = "macos")]
fn macos_show_app() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.unhideWithoutActivation() };
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}
