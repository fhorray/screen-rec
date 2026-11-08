//! Módulo de gravação de tela com Windows Graphics Capture (WGC) e codificação H.264 / MP4

use chrono::Local;
use openh264::encoder::Encoder;
use openh264::formats::YUVSource;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureArea {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct RecorderConfig {
    pub area: CaptureArea,
    pub fps: u32,
    pub record_microphone: bool,
    pub record_system_audio: bool,
    pub output_dir: PathBuf,
}

pub struct RecorderController {
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
    control: Option<CaptureControl<WgcHandler, Box<dyn std::error::Error + Send + Sync>>>,
    start_time: Instant,
    output_path: PathBuf,
}

impl RecorderController {
    pub fn start(config: RecorderConfig) -> Result<Self, String> {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let pause_signal = Arc::new(AtomicBool::new(false));

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = format!("gravacao_{}.mp4", timestamp);
        let output_path = config.output_dir.join(filename);

        let monitor = Monitor::primary().map_err(|e| format!("Falha ao obter monitor principal: {:?}", e))?;

        let flags = CaptureFlags {
            config,
            stop_signal: stop_signal.clone(),
            pause_signal: pause_signal.clone(),
            output_path: output_path.clone(),
        };

        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            flags,
        );

        let control = WgcHandler::start_free_threaded(settings)
            .map_err(|e| format!("Erro na captura WGC: {:?}", e))?;

        Ok(Self {
            stop_signal,
            pause_signal,
            control: Some(control),
            start_time: Instant::now(),
            output_path,
        })
    }

    pub fn toggle_pause(&self) -> bool {
        let current = self.pause_signal.load(Ordering::SeqCst);
        let next = !current;
        self.pause_signal.store(next, Ordering::SeqCst);
        next
    }

    pub fn is_paused(&self) -> bool {
        self.pause_signal.load(Ordering::SeqCst)
    }

    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.stop_signal.store(true, Ordering::SeqCst);
        if let Some(ctrl) = self.control.take() {
            let _ = ctrl.stop();
        }
        // Aguarda brevemente para que a thread de captura finalize o flush e escreva o moov atom no MP4
        std::thread::sleep(Duration::from_millis(250));
        Ok(self.output_path)
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

struct YuvFrame {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    width: usize,
    height: usize,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
}

impl YUVSource for YuvFrame {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.y_stride, self.u_stride, self.v_stride)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

fn rgba_to_yuv420p_crop(
    rgba: &[u8],
    row_pitch: usize,
    frame_width: usize,
    frame_height: usize,
    crop: &CaptureArea,
) -> YuvFrame {
    let crop_x = (crop.x as usize).min(frame_width.saturating_sub(2));
    let crop_y = (crop.y as usize).min(frame_height.saturating_sub(2));
    let crop_w = ((crop.width as usize).min(frame_width - crop_x)) & !1;
    let crop_h = ((crop.height as usize).min(frame_height - crop_y)) & !1;

    let target_w = if crop_w < 16 { 16 } else { crop_w };
    let target_h = if crop_h < 16 { 16 } else { crop_h };

    let y_size = target_w * target_h;
    let uv_size = (target_w / 2) * (target_h / 2);

    let mut y_plane = vec![0u8; y_size];
    let mut u_plane = vec![0u8; uv_size];
    let mut v_plane = vec![0u8; uv_size];

    let uv_w = target_w / 2;

    for row in 0..target_h {
        let src_row = (crop_y + row).min(frame_height.saturating_sub(1));
        // Usa row_pitch da GPU para indexar as linhas sem corromper memória
        let src_row_offset = src_row * row_pitch;
        let y_row_offset = row * target_w;
        let is_even_row = (row % 2) == 0;
        let uv_row_offset = (row / 2) * uv_w;

        for col in 0..target_w {
            let src_col = (crop_x + col).min(frame_width.saturating_sub(1));
            let src_idx = src_row_offset + src_col * 4;

            if src_idx + 3 < rgba.len() {
                let r = rgba[src_idx] as i32;
                let g = rgba[src_idx + 1] as i32;
                let b = rgba[src_idx + 2] as i32;

                let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
                y_plane[y_row_offset + col] = y.clamp(0, 255) as u8;

                if is_even_row && (col % 2 == 0) {
                    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                    let uv_idx = uv_row_offset + (col / 2);
                    if uv_idx < uv_size {
                        u_plane[uv_idx] = u.clamp(0, 255) as u8;
                        v_plane[uv_idx] = v.clamp(0, 255) as u8;
                    }
                }
            }
        }
    }

    YuvFrame {
        y: y_plane,
        u: u_plane,
        v: v_plane,
        width: target_w,
        height: target_h,
        y_stride: target_w,
        u_stride: uv_w,
        v_stride: uv_w,
    }
}

/// Converte o bitstream H.264 do formato Annex B (00 00 00 01) para AVCC (comprimento de 4 bytes)
/// e extrai os parâmetros SPS e PPS para o cabeçalho do arquivo MP4
fn split_annex_b_to_avcc(annex_b: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>, bool) {
    let mut avcc_payload = Vec::with_capacity(annex_b.len() + 16);
    let mut sps = Vec::new();
    let mut pps = Vec::new();
    let mut is_keyframe = false;

    let mut nalu_indices = Vec::new();
    let mut i = 0;
    while i < annex_b.len() {
        if i + 3 < annex_b.len() && annex_b[i] == 0 && annex_b[i + 1] == 0 && annex_b[i + 2] == 0 && annex_b[i + 3] == 1 {
            nalu_indices.push((i, 4));
            i += 4;
        } else if i + 2 < annex_b.len() && annex_b[i] == 0 && annex_b[i + 1] == 0 && annex_b[i + 2] == 1 {
            nalu_indices.push((i, 3));
            i += 3;
        } else {
            i += 1;
        }
    }

    for idx in 0..nalu_indices.len() {
        let (start, prefix_len) = nalu_indices[idx];
        let payload_start = start + prefix_len;
        let payload_end = if idx + 1 < nalu_indices.len() {
            nalu_indices[idx + 1].0
        } else {
            annex_b.len()
        };

        if payload_start < payload_end {
            let nalu = &annex_b[payload_start..payload_end];
            let nalu_type = nalu[0] & 0x1F;

            match nalu_type {
                7 => {
                    if sps.is_empty() {
                        sps = nalu.to_vec();
                    }
                }
                8 => {
                    if pps.is_empty() {
                        pps = nalu.to_vec();
                    }
                }
                5 => {
                    is_keyframe = true;
                    let len = nalu.len() as u32;
                    avcc_payload.extend_from_slice(&len.to_be_bytes());
                    avcc_payload.extend_from_slice(nalu);
                }
                1 => {
                    let len = nalu.len() as u32;
                    avcc_payload.extend_from_slice(&len.to_be_bytes());
                    avcc_payload.extend_from_slice(nalu);
                }
                _ => {
                    let len = nalu.len() as u32;
                    avcc_payload.extend_from_slice(&len.to_be_bytes());
                    avcc_payload.extend_from_slice(nalu);
                }
            }
        }
    }

    (avcc_payload, sps, pps, is_keyframe)
}

pub struct CaptureFlags {
    pub config: RecorderConfig,
    pub stop_signal: Arc<AtomicBool>,
    pub pause_signal: Arc<AtomicBool>,
    pub output_path: PathBuf,
}

pub struct WgcHandler {
    flags: CaptureFlags,
    encoder: Option<Encoder>,
    mp4_writer: Option<mp4::Mp4Writer<BufWriter<File>>>,
    frame_count: u64,
    last_frame_instant: Instant,
    frame_interval: Duration,
    is_initialized: bool,
    target_width: u16,
    target_height: u16,
}

impl GraphicsCaptureApiHandler for WgcHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let fps = if ctx.flags.config.fps == 0 { 30 } else { ctx.flags.config.fps };
        let frame_interval = Duration::from_millis((1000 / fps) as u64);

        let encoder = Encoder::new()?;

        Ok(Self {
            flags: ctx.flags,
            encoder: Some(encoder),
            mp4_writer: None,
            frame_count: 0,
            last_frame_instant: Instant::now(),
            frame_interval,
            is_initialized: false,
            target_width: 1280,
            target_height: 720,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.flags.stop_signal.load(Ordering::SeqCst) {
            capture_control.stop();
            return Ok(());
        }

        if self.flags.pause_signal.load(Ordering::SeqCst) {
            return Ok(());
        }

        let now = Instant::now();
        if self.is_initialized && now.duration_since(self.last_frame_instant) < self.frame_interval {
            return Ok(());
        }
        self.last_frame_instant = now;

        let frame_w = frame.width() as usize;
        let frame_h = frame.height() as usize;

        let crop_w = ((self.flags.config.area.width as usize).min(frame_w)) & !1;
        let crop_h = ((self.flags.config.area.height as usize).min(frame_h)) & !1;
        let target_w = (if crop_w < 16 { 16 } else { crop_w }) as u16;
        let target_h = (if crop_h < 16 { 16 } else { crop_h }) as u16;

        self.target_width = target_w;
        self.target_height = target_h;

        let mut buffer = match frame.buffer() {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };

        let row_pitch = buffer.row_pitch() as usize;
        let rgba_data = buffer.as_raw_buffer();

        let yuv = rgba_to_yuv420p_crop(rgba_data, row_pitch, frame_w, frame_h, &self.flags.config.area);

        if let Some(encoder) = &mut self.encoder
            && let Ok(bitstream) = encoder.encode(&yuv) {
                let stream_bytes = bitstream.to_vec();
                if !stream_bytes.is_empty() {
                    let (avcc_data, sps, pps, is_key) = split_annex_b_to_avcc(&stream_bytes);

                    // Inicializa o arquivo MP4 assim que os parâmetros SPS/PPS estiverem disponíveis
                    if !self.is_initialized
                        && !sps.is_empty() && !pps.is_empty()
                            && let Ok(file) = File::create(&self.flags.output_path) {
                                let writer = BufWriter::new(file);
                                if let Ok(mut mp4_writer) = mp4::Mp4Writer::write_start(
                                    writer,
                                    &mp4::Mp4Config {
                                        major_brand: str::parse("isom").unwrap(),
                                        minor_version: 512,
                                        compatible_brands: vec![
                                            str::parse("isom").unwrap(),
                                            str::parse("iso2").unwrap(),
                                            str::parse("avc1").unwrap(),
                                            str::parse("mp41").unwrap(),
                                        ],
                                        timescale: 1000,
                                    },
                                ) {
                                    let fps = if self.flags.config.fps == 0 { 30 } else { self.flags.config.fps };
                                    let _ = mp4_writer.add_track(&mp4::TrackConfig {
                                        track_type: mp4::TrackType::Video,
                                        timescale: fps * 1000,
                                        language: "und".to_string(),
                                        media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
                                            width: self.target_width,
                                            height: self.target_height,
                                            seq_param_set: sps,
                                            pic_param_set: pps,
                                        }),
                                    });
                                    self.mp4_writer = Some(mp4_writer);
                                    self.is_initialized = true;
                                }
                            }

                    if let Some(writer) = &mut self.mp4_writer
                        && !avcc_data.is_empty() {
                            let sample = mp4::Mp4Sample {
                                start_time: self.frame_count * 1000,
                                duration: 1000,
                                rendering_offset: 0,
                                is_sync: is_key,
                                bytes: mp4::Bytes::from(avcc_data),
                            };

                            let _ = writer.write_sample(1u32, &sample);
                            self.frame_count += 1;
                        }
                }
            }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        if let Some(mut writer) = self.mp4_writer.take() {
            let _ = writer.write_end();
        }
        Ok(())
    }
}

impl Drop for WgcHandler {
    fn drop(&mut self) {
        if let Some(mut writer) = self.mp4_writer.take() {
            let _ = writer.write_end();
        }
    }
}
