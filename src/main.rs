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
    selected_preset: AreaPreset,
    area_x: u32,
    area_y: u32,
    area_width: u32,
    area_height: u32,

    record_mic: bool,
    record_system_audio: bool,

    controller: Option<RecorderController>,
    status_message: String,
    last_saved_file: Option<PathBuf>,
    has_excluded_window: bool,

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
            status_message: "Pronto para gravar".to_string(),
            last_saved_file: None,
            has_excluded_window: false,
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
                self.status_message = "Gravando...".to_string();
            }
            Err(e) => {
                self.status_message = format!("Erro ao iniciar: {}", e);
            }
        }
    }

    fn toggle_pause(&mut self) {
        if let Some(ctrl) = &self.controller {
            let is_paused = ctrl.toggle_pause();
            if is_paused {
                self.status_message = "Gravação pausada".to_string();
            } else {
                self.status_message = "Gravando...".to_string();
            }
        }
    }

    fn stop_recording(&mut self) {
        if let Some(ctrl) = self.controller.take() {
            match ctrl.stop() {
                Ok(path) => {
                    self.status_message = format!("Salvo em: {}", path.display());
                    self.last_saved_file = Some(path);
                }
                Err(e) => {
                    self.status_message = format!("Erro ao salvar: {}", e);
                }
            }
        }
    }
}

impl eframe::App for ScreenRecorderApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Excluir a própria janela da captura (SetWindowDisplayAffinity)
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

        ui.heading("🎬 Gravador de Tela");
        ui.add_space(8.0);

        // Seção de Controles Principais
        ui.horizontal(|ui| {
            let is_recording = self.controller.is_some();
            let is_paused = self.controller.as_ref().map_or(false, |c| c.is_paused());

            if !is_recording {
                if ui.button("▶ Gravar").clicked() {
                    self.start_recording();
                }
            } else {
                let pause_label = if is_paused { "▶ Retomar" } else { "⏸ Pausar" };
                if ui.button(pause_label).clicked() {
                    self.toggle_pause();
                }

                if ui.button("⏹ Parar").clicked() {
                    self.stop_recording();
                }
            }

            if let Some(ctrl) = &self.controller {
                let elapsed = ctrl.elapsed();
                let mins = elapsed.as_secs() / 60;
                let secs = elapsed.as_secs() % 60;
                ui.label(format!("⏱ {:02}:{:02}", mins, secs));
            }
        });

        ui.separator();

        // Presets de Resolução / Área
        ui.label(egui::RichText::new("Área de Captura / Presets").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.selected_preset == AreaPreset::FullScreen, "Tela Cheia").clicked() {
                self.apply_preset(AreaPreset::FullScreen);
            }
            if ui.selectable_label(self.selected_preset == AreaPreset::P1080p, "1080p (16:9)").clicked() {
                self.apply_preset(AreaPreset::P1080p);
            }
            if ui.selectable_label(self.selected_preset == AreaPreset::P720p, "720p (16:9)").clicked() {
                self.apply_preset(AreaPreset::P720p);
            }
            if ui.selectable_label(self.selected_preset == AreaPreset::Square, "1:1 (Quadrado)").clicked() {
                self.apply_preset(AreaPreset::Square);
            }
            if ui.selectable_label(self.selected_preset == AreaPreset::Custom, "Personalizado").clicked() {
                self.selected_preset = AreaPreset::Custom;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Posição X:");
            ui.add(egui::DragValue::new(&mut self.area_x).range(0..=7680));
            ui.label("Y:");
            ui.add(egui::DragValue::new(&mut self.area_y).range(0..=4320));
        });

        ui.horizontal(|ui| {
            ui.label("Largura:");
            ui.add(egui::DragValue::new(&mut self.area_width).range(160..=7680));
            ui.label("Altura:");
            ui.add(egui::DragValue::new(&mut self.area_height).range(120..=4320));
        });

        ui.separator();

        // Configurações de Áudio
        ui.label(egui::RichText::new("Opções de Áudio").strong());
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.record_mic, "Gravar Microfone");
            ui.checkbox(&mut self.record_system_audio, "Gravar Som do Sistema");
        });

        ui.separator();

        // Status e Ações Finais
        ui.label(format!("Status: {}", self.status_message));

        if let Some(saved_path) = &self.last_saved_file {
            if ui.button("📂 Abrir pasta do vídeo").clicked() {
                if let Some(parent) = saved_path.parent() {
                    let _ = std::process::Command::new("explorer").arg(parent).spawn();
                }
            }
        }

        ui.add_space(4.0);
        ui.small("💡 Dica: Esta janela é excluída automaticamente da gravação final pelo Windows.");
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 360.0])
            .with_min_inner_size([400.0, 320.0])
            .with_title("Gravador de Tela"),
        ..Default::default()
    };

    eframe::run_native(
        "Gravador de Tela",
        native_options,
        Box::new(|_cc| Ok(Box::new(ScreenRecorderApp::default()))),
    )
}
