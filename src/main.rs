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
    P1080p, // 1920x1080
    P720p,  // 1280x720
    Square, // 1080x1080 (1:1)
    Custom,
}

struct ScreenRecorderApp {
    // Configurações de Área
    selected_preset: AreaPreset,
    area_x: u32,
    area_y: u32,
    area_width: u32,
    area_height: u32,

    // Configurações de Áudio
    record_mic: bool,
    record_system_audio: bool,

    // Estado da Gravação
    controller: Option<RecorderController>,
    status_message: String,
    last_saved_file: Option<PathBuf>,
    has_excluded_window: bool,

    // Painéis flutuantes / modais
    show_settings: bool,
    show_area_selector: bool,

    fps: u32,
}

impl Default for ScreenRecorderApp {
    fn default() -> Self {
        Self {
            selected_preset: AreaPreset::FullScreen,
            area_x: 0,
            area_y: 0,
            area_width: 1920,
            area_height: 1080,
            record_mic: false,
            record_system_audio: false,
            controller: None,
            status_message: "Pronto".to_string(),
            last_saved_file: None,
            has_excluded_window: false,
            show_settings: false,
            show_area_selector: false,
            fps: 30,
        }
    }
}

impl ScreenRecorderApp {
    fn apply_preset(&mut self, preset: AreaPreset) {
        self.selected_preset = preset;
        match preset {
            AreaPreset::FullScreen => {
                self.area_x = 0;
                self.area_y = 0;
                self.area_width = 1920;
                self.area_height = 1080;
            }
            AreaPreset::P1080p => {
                self.area_x = 0;
                self.area_y = 0;
                self.area_width = 1920;
                self.area_height = 1080;
            }
            AreaPreset::P720p => {
                self.area_x = 0;
                self.area_y = 0;
                self.area_width = 1280;
                self.area_height = 720;
            }
            AreaPreset::Square => {
                self.area_x = 0;
                self.area_y = 0;
                self.area_width = 1080;
                self.area_height = 1080;
            }
            AreaPreset::Custom => {}
        }
    }

    fn start_recording(&mut self) {
        if self.controller.is_some() {
            return;
        }

        let output_dir = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));

        let config = RecorderConfig {
            area: CaptureArea {
                x: self.area_x,
                y: self.area_y,
                width: self.area_width,
                height: self.area_height,
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
                    self.last_saved_file = Some(path);
                }
                Err(e) => {
                    self.status_message = format!("Erro: {}", e);
                }
            }
        }
    }
}

impl eframe::App for ScreenRecorderApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Excluir a janela da captura de tela
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

        if self.controller.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }

        // Definir estilo moderno, escuro e com cantos arredondados
        let is_recording = self.controller.is_some();
        let is_paused = self.controller.as_ref().map_or(false, |c| c.is_paused());

        // --- BARRA COMPACTA PRINCIPAL ---
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

            // Botão Gravar / Play
            if !is_recording {
                let rec_btn = egui::Button::new(
                    egui::RichText::new("● REC").color(egui::Color32::WHITE).strong()
                ).fill(egui::Color32::from_rgb(220, 38, 38));

                if ui.add_sized([68.0, 28.0], rec_btn).on_hover_text("Iniciar Gravação").clicked() {
                    self.start_recording();
                }
            } else {
                // Botão Pausar / Retomar
                let pause_icon = if is_paused { "▶" } else { "⏸" };
                let pause_tooltip = if is_paused { "Retomar Gravação" } else { "Pausar Gravação" };
                if ui.add_sized([34.0, 28.0], egui::Button::new(pause_icon)).on_hover_text(pause_tooltip).clicked() {
                    self.toggle_pause();
                }

                // Botão Parar / Stop
                let stop_btn = egui::Button::new(
                    egui::RichText::new("⏹").color(egui::Color32::WHITE).strong()
                ).fill(egui::Color32::from_rgb(180, 20, 20));

                if ui.add_sized([34.0, 28.0], stop_btn).on_hover_text("Parar e Salvar Gravação").clicked() {
                    self.stop_recording();
                }

                // Timer decorrido
                if let Some(ctrl) = &self.controller {
                    let elapsed = ctrl.elapsed();
                    let mins = elapsed.as_secs() / 60;
                    let secs = elapsed.as_secs() % 60;
                    let timer_text = format!("{:02}:{:02}", mins, secs);
                    let color = if is_paused {
                        egui::Color32::YELLOW
                    } else {
                        egui::Color32::from_rgb(239, 68, 68)
                    };
                    ui.label(egui::RichText::new(timer_text).color(color).strong().monospace());
                }
            }

            ui.separator();

            // Botão Seletor de Área / Presets
            let area_btn = ui.selectable_label(
                self.show_area_selector,
                egui::RichText::new("⛶ Área").strong()
            );
            if area_btn.on_hover_text("Selecionar área de gravação ou preset").clicked() {
                self.show_area_selector = !self.show_area_selector;
            }

            // Botão Configurações (Áudio, FPS, etc)
            let config_btn = ui.selectable_label(
                self.show_settings,
                egui::RichText::new("⚙").strong()
            );
            if config_btn.on_hover_text("Configurações de áudio e vídeo").clicked() {
                self.show_settings = !self.show_settings;
            }

            // Atalho para abrir pasta do vídeo gravado
            if let Some(saved_path) = &self.last_saved_file {
                if ui.button("📂").on_hover_text("Abrir pasta do último vídeo gravado").clicked() {
                    if let Some(parent) = saved_path.parent() {
                        let _ = std::process::Command::new("explorer").arg(parent).spawn();
                    }
                }
            }
        });

        // --- SUBPAINEL DE SELEÇÃO DE ÁREA E PRESETS ---
        if self.show_area_selector {
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Presets:").small().strong());
                if ui.selectable_label(self.selected_preset == AreaPreset::FullScreen, "Tela Cheia").clicked() {
                    self.apply_preset(AreaPreset::FullScreen);
                }
                if ui.selectable_label(self.selected_preset == AreaPreset::P1080p, "1080p").clicked() {
                    self.apply_preset(AreaPreset::P1080p);
                }
                if ui.selectable_label(self.selected_preset == AreaPreset::P720p, "720p").clicked() {
                    self.apply_preset(AreaPreset::P720p);
                }
                if ui.selectable_label(self.selected_preset == AreaPreset::Square, "1:1").clicked() {
                    self.apply_preset(AreaPreset::Square);
                }
                if ui.selectable_label(self.selected_preset == AreaPreset::Custom, "Personalizado").clicked() {
                    self.selected_preset = AreaPreset::Custom;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Posição:");
                ui.label("X");
                ui.add(egui::DragValue::new(&mut self.area_x).range(0..=7680).speed(5));
                ui.label("Y");
                ui.add(egui::DragValue::new(&mut self.area_y).range(0..=4320).speed(5));

                ui.separator();

                ui.label("Tam:");
                ui.label("L");
                ui.add(egui::DragValue::new(&mut self.area_width).range(160..=7680).speed(5));
                ui.label("A");
                ui.add(egui::DragValue::new(&mut self.area_height).range(120..=4320).speed(5));
            });
        }

        // --- SUBPAINEL DE CONFIGURAÇÕES (ÁUDIO / FPS / DESTINO) ---
        if self.show_settings {
            ui.add_space(4.0);
            ui.separator();
            ui.label(egui::RichText::new("Opções de Gravação").small().strong());
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.record_mic, "Microfone");
                ui.checkbox(&mut self.record_system_audio, "Áudio do Sistema");
                ui.separator();
                ui.label("FPS:");
                ui.add(egui::DragValue::new(&mut self.fps).range(15..=60).speed(1));
            });

            ui.label(
                egui::RichText::new(format!("Status: {}", self.status_message))
                    .small()
                    .color(egui::Color32::GRAY),
            );
        }
    }
}

fn main() -> eframe::Result<()> {
    // Janela compacta tipo barra de ferramentas moderna (estilo widget/dock)
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([380.0, 48.0])
            .with_min_inner_size([320.0, 44.0])
            .with_resizable(true)
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
