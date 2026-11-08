#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod recorder;
mod window_exclude;

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use recorder::{CaptureArea, RecorderConfig, RecorderController};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
enum AreaPreset {
    FullScreen,
    P1080p,
    P720p,
    Square,
    Custom,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum DragMode {
    None,
    Moving,
    ResizingCorner,
    NewSelection,
}

struct ScreenRecorderApp {
    is_selecting_area: bool,
    drag_mode: DragMode,
    drag_start: egui::Pos2,

    area_rect: egui::Rect,
    selected_preset: AreaPreset,

    record_mic: bool,
    record_system_audio: bool,
    fps: u32,

    show_settings: bool,
    show_presets_menu: bool,

    controller: Option<RecorderController>,
    status_message: String,
    last_saved_file: Option<PathBuf>,
    has_excluded_window: bool,

    bar_pos: egui::Pos2,
}

impl Default for ScreenRecorderApp {
    fn default() -> Self {
        Self {
            is_selecting_area: false,
            drag_mode: DragMode::None,
            drag_start: egui::Pos2::ZERO,
            area_rect: egui::Rect::from_min_size(egui::pos2(120.0, 100.0), egui::vec2(1280.0, 720.0)),
            selected_preset: AreaPreset::P720p,
            record_mic: false,
            record_system_audio: false,
            fps: 30,
            show_settings: false,
            show_presets_menu: false,
            controller: None,
            status_message: "Pronto".to_string(),
            last_saved_file: None,
            has_excluded_window: false,
            bar_pos: egui::pos2(24.0, 24.0),
        }
    }
}

impl ScreenRecorderApp {
    fn apply_preset(&mut self, preset: AreaPreset, screen_size: egui::Vec2) {
        self.selected_preset = preset;
        match preset {
            AreaPreset::FullScreen => {
                self.area_rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), screen_size);
            }
            AreaPreset::P1080p => {
                let w = (1920.0_f32).min(screen_size.x);
                let h = (1080.0_f32).min(screen_size.y);
                let x = (screen_size.x - w) / 2.0;
                let y = (screen_size.y - h) / 2.0;
                self.area_rect = egui::Rect::from_min_size(egui::pos2(x.max(0.0), y.max(0.0)), egui::vec2(w, h));
            }
            AreaPreset::P720p => {
                let w = (1280.0_f32).min(screen_size.x);
                let h = (720.0_f32).min(screen_size.y);
                let x = (screen_size.x - w) / 2.0;
                let y = (screen_size.y - h) / 2.0;
                self.area_rect = egui::Rect::from_min_size(egui::pos2(x.max(0.0), y.max(0.0)), egui::vec2(w, h));
            }
            AreaPreset::Square => {
                let side = (720.0_f32).min(screen_size.y);
                let x = (screen_size.x - side) / 2.0;
                let y = (screen_size.y - side) / 2.0;
                self.area_rect = egui::Rect::from_min_size(egui::pos2(x.max(0.0), y.max(0.0)), egui::vec2(side, side));
            }
            AreaPreset::Custom => {}
        }
    }

    fn start_recording(&mut self) {
        if self.controller.is_some() {
            return;
        }

        let output_dir = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));

        let x = self.area_rect.min.x.max(0.0) as u32;
        let y = self.area_rect.min.y.max(0.0) as u32;
        let width = self.area_rect.width().max(64.0) as u32;
        let height = self.area_rect.height().max(64.0) as u32;

        let config = RecorderConfig {
            area: CaptureArea {
                x,
                y,
                width,
                height,
            },
            fps: self.fps,
            record_microphone: self.record_mic,
            record_system_audio: self.record_system_audio,
            output_dir,
        };

        match RecorderController::start(config) {
            Ok(ctrl) => {
                self.controller = Some(ctrl);
                self.status_message = "Gravando".to_string();
                self.is_selecting_area = false;
                self.show_presets_menu = false;
                self.show_settings = false;
            }
            Err(e) => {
                self.status_message = format!("Erro: {}", e);
            }
        }
    }

    fn toggle_pause(&mut self) {
        if let Some(ctrl) = &self.controller {
            let is_paused = ctrl.toggle_pause();
            if is_paused {
                self.status_message = "Pausado".to_string();
            } else {
                self.status_message = "Gravando".to_string();
            }
        }
    }

    fn stop_recording(&mut self) {
        if let Some(ctrl) = self.controller.take() {
            match ctrl.stop() {
                Ok(path) => {
                    self.status_message = "Salvo".to_string();
                    if let Some(parent) = path.parent() {
                        let _ = std::process::Command::new("explorer").arg(parent).spawn();
                    }
                    self.last_saved_file = Some(path);
                }
                Err(e) => {
                    self.status_message = format!("Erro: {}", e);
                }
            }
        }
    }
}

// Helpers de desenho vetorial limpo para botões compactos
fn draw_play_icon(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = resp.hovered();
        let bg = if hover {
            egui::Color32::from_rgb(239, 68, 68)
        } else {
            egui::Color32::from_rgb(220, 38, 38)
        };
        ui.painter().rect_filled(rect, 12.0, bg);

        // Triângulo de Play centralizado
        let c = rect.center();
        let points = vec![
            c + egui::vec2(-4.0, -5.0),
            c + egui::vec2(6.0, 0.0),
            c + egui::vec2(-4.0, 5.0),
        ];
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
    resp
}

fn draw_pause_icon(ui: &mut egui::Ui, is_paused: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = resp.hovered();
        let bg = if hover { egui::Color32::from_gray(60) } else { egui::Color32::from_gray(35) };
        ui.painter().rect_filled(rect, 6.0, bg);

        let c = rect.center();
        if is_paused {
            // Desenha triângulo resume
            let points = vec![
                c + egui::vec2(-3.0, -4.5),
                c + egui::vec2(5.0, 0.0),
                c + egui::vec2(-3.0, 4.5),
            ];
            ui.painter().add(egui::Shape::convex_polygon(points, egui::Color32::WHITE, egui::Stroke::NONE));
        } else {
            // Desenha duas barras de pause
            ui.painter().rect_filled(egui::Rect::from_center_size(c + egui::vec2(-3.5, 0.0), egui::vec2(2.5, 9.0)), 1.0, egui::Color32::WHITE);
            ui.painter().rect_filled(egui::Rect::from_center_size(c + egui::vec2(3.5, 0.0), egui::vec2(2.5, 9.0)), 1.0, egui::Color32::WHITE);
        }
    }
    resp
}

fn draw_stop_icon(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = resp.hovered();
        let bg = if hover { egui::Color32::from_rgb(185, 28, 28) } else { egui::Color32::from_rgb(153, 27, 27) };
        ui.painter().rect_filled(rect, 6.0, bg);

        let c = rect.center();
        ui.painter().rect_filled(egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)), 1.5, egui::Color32::WHITE);
    }
    resp
}

fn draw_crop_icon(ui: &mut egui::Ui, active: bool, label_text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(60.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = resp.hovered();
        let bg = if active {
            egui::Color32::from_gray(50)
        } else if hover {
            egui::Color32::from_gray(38)
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, bg);

        // Ícone retangular de seleção
        let icon_rect = egui::Rect::from_center_size(rect.left_center() + egui::vec2(12.0, 0.0), egui::vec2(10.0, 8.0));
        ui.painter().rect_stroke(
            icon_rect,
            1.0,
            egui::Stroke::new(1.2, if active { egui::Color32::WHITE } else { egui::Color32::from_gray(180) }),
            egui::StrokeKind::Outside,
        );

        // Texto do Preset
        ui.painter().text(
            rect.left_center() + egui::vec2(22.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label_text,
            egui::FontId::proportional(11.0),
            if active { egui::Color32::WHITE } else { egui::Color32::from_gray(190) },
        );
    }
    resp
}

fn draw_gear_icon(ui: &mut egui::Ui, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = resp.hovered();
        let bg = if active {
            egui::Color32::from_gray(50)
        } else if hover {
            egui::Color32::from_gray(38)
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, bg);

        let c = rect.center();
        let color = if active { egui::Color32::WHITE } else { egui::Color32::from_gray(180) };
        // Engrenagem limpa com anel e dentes
        ui.painter().circle_stroke(c, 4.0, egui::Stroke::new(1.4, color));
        ui.painter().circle_filled(c, 1.5, color);
    }
    resp
}

impl eframe::App for ScreenRecorderApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.has_excluded_window {
            if let Ok(handle) = frame.window_handle() {
                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                    let hwnd = win32_handle.hwnd.get();
                    if window_exclude::exclude_window_from_capture(hwnd) {
                        self.has_excluded_window = true;
                    }
                }
            }
        }

        let ctx = ui.ctx().clone();
        if self.controller.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        let screen_rect = ctx.content_rect();
        let is_recording = self.controller.is_some();
        let is_paused = self.controller.as_ref().map_or(false, |c| c.is_paused());

        // Cor da borda:
        // Gravando = Vermelho Vivo | Pausado = Âmbar | Pronto = Verde Suave
        let border_color = if is_recording {
            if is_paused {
                egui::Color32::from_rgb(245, 158, 11)
            } else {
                egui::Color32::from_rgb(239, 68, 68)
            }
        } else {
            egui::Color32::from_rgb(34, 197, 94)
        };

        let painter = ctx.layer_painter(egui::LayerId::background());

        // Overlay suave no fundo (escurece apenas a área externa)
        if self.is_selecting_area || !is_recording {
            let dim_color = egui::Color32::from_black_alpha(90);
            let r = self.area_rect;
            painter.rect_filled(
                egui::Rect::from_min_max(screen_rect.min, egui::pos2(screen_rect.max.x, r.min.y)),
                0.0,
                dim_color,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, r.max.y), screen_rect.max),
                0.0,
                dim_color,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)),
                0.0,
                dim_color,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(screen_rect.max.x, r.max.y)),
                0.0,
                dim_color,
            );
        }

        // Borda ultra fina e limpa
        let stroke = egui::Stroke::new(1.8, border_color);
        painter.rect_stroke(self.area_rect, 0.0, stroke, egui::StrokeKind::Outside);

        // Marcadores minimalistas nos 4 cantos
        let corners = [
            self.area_rect.left_top(),
            self.area_rect.right_top(),
            self.area_rect.left_bottom(),
            self.area_rect.right_bottom(),
        ];
        for c in corners {
            painter.rect_filled(
                egui::Rect::from_center_size(c, egui::vec2(5.0, 5.0)),
                1.0,
                border_color,
            );
        }

        // Badge de resolução super discreto
        let size_badge_text = format!("{:.0} × {:.0}", self.area_rect.width(), self.area_rect.height());
        let badge_pos = self.area_rect.left_top() + egui::vec2(6.0, 6.0);
        painter.rect_filled(
            egui::Rect::from_min_size(badge_pos, egui::vec2(68.0, 16.0)),
            3.0,
            egui::Color32::from_black_alpha(170),
        );
        painter.text(
            badge_pos + egui::vec2(34.0, 8.0),
            egui::Align2::CENTER_CENTER,
            size_badge_text,
            egui::FontId::proportional(10.0),
            egui::Color32::from_gray(210),
        );

        // Arrastar e redimensionar a área na tela
        if self.is_selecting_area && !is_recording {
            let pointer = ctx.input(|i| i.pointer.clone());
            if pointer.primary_down() {
                if let Some(pos) = pointer.interact_pos() {
                    match self.drag_mode {
                        DragMode::None => {
                            self.drag_start = pos;
                            let corner_rect = egui::Rect::from_center_size(self.area_rect.right_bottom(), egui::vec2(24.0, 24.0));
                            if corner_rect.contains(pos) {
                                self.drag_mode = DragMode::ResizingCorner;
                            } else if self.area_rect.contains(pos) {
                                self.drag_mode = DragMode::Moving;
                            } else {
                                self.drag_mode = DragMode::NewSelection;
                                self.area_rect = egui::Rect::from_min_max(pos, pos);
                            }
                        }
                        DragMode::ResizingCorner => {
                            let min = self.area_rect.min;
                            self.area_rect = egui::Rect::from_min_max(min, egui::pos2(pos.x.max(min.x + 64.0), pos.y.max(min.y + 64.0)));
                            self.selected_preset = AreaPreset::Custom;
                        }
                        DragMode::Moving => {
                            let delta = pointer.delta();
                            self.area_rect = self.area_rect.translate(delta);
                            self.selected_preset = AreaPreset::Custom;
                        }
                        DragMode::NewSelection => {
                            let min = egui::pos2(self.drag_start.x.min(pos.x), self.drag_start.y.min(pos.y));
                            let max = egui::pos2(self.drag_start.x.max(pos.x), self.drag_start.y.max(pos.y));
                            self.area_rect = egui::Rect::from_min_max(min, max);
                            self.selected_preset = AreaPreset::Custom;
                        }
                    }
                }
            } else if self.drag_mode != DragMode::None {
                self.drag_mode = DragMode::None;
                self.show_presets_menu = true;
            }
        }

        // --- BARRA FLUTUANTE DISCRETA E ELEGANTE (Micro Capsule) ---
        egui::Window::new("micro_capsule_bar")
            .title_bar(false)
            .resizable(false)
            .current_pos(self.bar_pos)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(16, 16, 18))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 38, 44)))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::symmetric(6, 4))
            )
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

                    if !is_recording {
                        if draw_play_icon(ui).on_hover_text("Gravar").clicked() {
                            self.start_recording();
                        }
                    } else {
                        if draw_pause_icon(ui, is_paused).on_hover_text(if is_paused { "Retomar" } else { "Pausar" }).clicked() {
                            self.toggle_pause();
                        }
                        if draw_stop_icon(ui).on_hover_text("Parar").clicked() {
                            self.stop_recording();
                        }

                        if let Some(ctrl) = &self.controller {
                            let elapsed = ctrl.elapsed();
                            let timer_color = if is_paused { egui::Color32::YELLOW } else { egui::Color32::from_rgb(239, 68, 68) };
                            ui.label(
                                egui::RichText::new(format!("{:02}:{:02}", elapsed.as_secs() / 60, elapsed.as_secs() % 60))
                                    .color(timer_color)
                                    .size(11.0)
                                    .strong()
                                    .monospace()
                            );
                        }
                    }

                    // Divisor sutil
                    ui.painter().vline(ui.cursor().min.x + 2.0, ui.cursor().y_range(), egui::Stroke::new(1.0, egui::Color32::from_gray(35)));
                    ui.add_space(5.0);

                    // Dropdown de Área
                    let preset_text = match self.selected_preset {
                        AreaPreset::FullScreen => "Full",
                        AreaPreset::P1080p => "1080p",
                        AreaPreset::P720p => "720p",
                        AreaPreset::Square => "1:1",
                        AreaPreset::Custom => "Área",
                    };
                    if draw_crop_icon(ui, self.show_presets_menu || self.is_selecting_area, preset_text).clicked() {
                        self.show_presets_menu = !self.show_presets_menu;
                        self.show_settings = false;
                    }

                    // Botão Configurações
                    if draw_gear_icon(ui, self.show_settings).on_hover_text("Configurações").clicked() {
                        self.show_settings = !self.show_settings;
                        self.show_presets_menu = false;
                    }

                    // Atalho Pasta do Vídeo
                    if let Some(saved_path) = &self.last_saved_file {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 24.0), egui::Sense::click());
                        if ui.is_rect_visible(rect) {
                            let hover = resp.hovered();
                            let bg = if hover { egui::Color32::from_gray(45) } else { egui::Color32::TRANSPARENT };
                            ui.painter().rect_filled(rect, 5.0, bg);
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "📁", egui::FontId::proportional(11.0), egui::Color32::WHITE);
                        }
                        if resp.on_hover_text("Abrir pasta do vídeo").clicked() {
                            if let Some(parent) = saved_path.parent() {
                                let _ = std::process::Command::new("explorer").arg(parent).spawn();
                            }
                        }
                    }
                });
            });

        // --- MENU FLUTUANTE DE PRESETS ---
        if self.show_presets_menu && !is_recording {
            egui::Window::new("presets_pop")
                .title_bar(false)
                .resizable(false)
                .current_pos(self.bar_pos + egui::vec2(44.0, 36.0))
                .frame(
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(20, 20, 24))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(42, 42, 48)))
                        .corner_radius(8.0)
                        .inner_margin(6.0)
                )
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);

                    let mut clicked_preset = None;
                    if ui.selectable_label(self.selected_preset == AreaPreset::FullScreen, "🖥  Tela Cheia").clicked() {
                        clicked_preset = Some(AreaPreset::FullScreen);
                    }
                    if ui.selectable_label(self.selected_preset == AreaPreset::P1080p, "📺  1920 × 1080 (16:9)").clicked() {
                        clicked_preset = Some(AreaPreset::P1080p);
                    }
                    if ui.selectable_label(self.selected_preset == AreaPreset::P720p, "💻  1280 × 720 (16:9)").clicked() {
                        clicked_preset = Some(AreaPreset::P720p);
                    }
                    if ui.selectable_label(self.selected_preset == AreaPreset::Square, "⬛  1:1 (Quadrado)").clicked() {
                        clicked_preset = Some(AreaPreset::Square);
                    }

                    ui.separator();

                    if ui.selectable_label(self.is_selecting_area, "✂  Arrastar Área na Tela").clicked() {
                        self.is_selecting_area = true;
                        self.show_presets_menu = false;
                    }

                    if let Some(p) = clicked_preset {
                        self.apply_preset(p, screen_rect.size());
                        self.show_presets_menu = false;
                        self.is_selecting_area = false;
                    }
                });
        }

        // --- MENU FLUTUANTE DE CONFIGURAÇÕES ---
        if self.show_settings {
            egui::Window::new("settings_pop")
                .title_bar(false)
                .resizable(false)
                .current_pos(self.bar_pos + egui::vec2(96.0, 36.0))
                .frame(
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(20, 20, 24))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(42, 42, 48)))
                        .corner_radius(8.0)
                        .inner_margin(8.0)
                )
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 4.0);
                    ui.label(egui::RichText::new("Opções de Gravação").small().strong().color(egui::Color32::from_gray(180)));
                    ui.separator();

                    ui.checkbox(&mut self.record_mic, "Microfone");
                    ui.checkbox(&mut self.record_system_audio, "Som do Sistema");

                    ui.horizontal(|ui| {
                        ui.label("FPS:");
                        ui.add(egui::DragValue::new(&mut self.fps).range(15..=60));
                    });

                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Status: {}", self.status_message))
                            .small()
                            .color(egui::Color32::from_gray(120)),
                    );
                });
        }
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_maximized(true)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_title("Screen Recorder"),
        ..Default::default()
    };

    eframe::run_native(
        "Screen Recorder",
        native_options,
        Box::new(|_cc| Ok(Box::new(ScreenRecorderApp::default()))),
    )
}
