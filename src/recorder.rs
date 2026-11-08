//! Módulo de gravação de tela com codificação MP4 H.264

use chrono::Local;
use openh264::encoder::Encoder;
use openh264::formats::YUVSource;
use scrap::{Capturer, Display};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
    handle: Option<thread::JoinHandle<Result<PathBuf, String>>>,
    start_time: Instant,
    output_path: PathBuf,
}

impl RecorderController {
    pub fn start(config: RecorderConfig) -> Result<Self, String> {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let pause_signal = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_signal.clone();
        let pause_clone = pause_signal.clone();

        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let filename = format!("gravacao_{}.mp4", timestamp);
        let output_path = config.output_dir.join(filename);
        let path_clone = output_path.clone();

        let handle = thread::spawn(move || {
            run_recording_pipeline(config, path_clone, stop_clone, pause_clone)
        });

        Ok(Self {
            stop_signal,
            pause_signal,
            handle: Some(handle),
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
        if let Some(h) = self.handle.take() {
            match h.join() {
                Ok(res) => res,
                Err(_) => Err("A thread de gravação encontrou um erro crítico".to_string()),
            }
        } else {
            Ok(self.output_path)
        }
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

fn bgra_to_yuv420p_crop(
    bgra: &[u8],
    screen_width: usize,
    screen_height: usize,
    crop: &CaptureArea,
) -> YuvFrame {
    let crop_x = (crop.x as usize).min(screen_width.saturating_sub(2));
    let crop_y = (crop.y as usize).min(screen_height.saturating_sub(2));
    let crop_w = ((crop.width as usize).min(screen_width - crop_x)) & !1;
    let crop_h = ((crop.height as usize).min(screen_height - crop_y)) & !1;

    let y_size = crop_w * crop_h;
    let uv_size = (crop_w / 2) * (crop_h / 2);

    let mut y_plane = vec![0u8; y_size];
    let mut u_plane = vec![0u8; uv_size];
    let mut v_plane = vec![0u8; uv_size];

    let uv_w = crop_w / 2;

    for row in 0..crop_h {
        let src_row = crop_y + row;
        let src_row_offset = src_row * screen_width * 4;
        let y_row_offset = row * crop_w;
        let is_even_row = (row % 2) == 0;
        let uv_row_offset = (row / 2) * uv_w;

        for col in 0..crop_w {
            let src_col = crop_x + col;
            let src_idx = src_row_offset + src_col * 4;

            if src_idx + 3 < bgra.len() {
                let b = bgra[src_idx] as i32;
                let g = bgra[src_idx + 1] as i32;
                let r = bgra[src_idx + 2] as i32;

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
        width: crop_w,
        height: crop_h,
        y_stride: crop_w,
        u_stride: uv_w,
        v_stride: uv_w,
    }
}

fn run_recording_pipeline(
    config: RecorderConfig,
    output_path: PathBuf,
    stop_signal: Arc<AtomicBool>,
    pause_signal: Arc<AtomicBool>,
) -> Result<PathBuf, String> {
    let display = Display::primary().map_err(|e| format!("Falha ao acessar monitor: {:?}", e))?;
    let screen_w = display.width();
    let screen_h = display.height();

    let mut capturer = Capturer::new(display).map_err(|e| format!("Falha ao iniciar capturador de tela: {:?}", e))?;

    let crop_w = (config.area.width as usize).min(screen_w) & !1;
    let crop_h = (config.area.height as usize).min(screen_h) & !1;

    let target_w = if crop_w < 16 { 16 } else { crop_w };
    let target_h = if crop_h < 16 { 16 } else { crop_h };

    let mut encoder = Encoder::new()
        .map_err(|e| format!("Falha ao inicializar OpenH264: {:?}", e))?;

    let file = File::create(&output_path).map_err(|e| format!("Erro ao criar arquivo de vídeo: {:?}", e))?;
    let writer = BufWriter::new(file);

    let mut mp4_writer = mp4::Mp4Writer::write_start(
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
    ).map_err(|e| format!("Erro ao criar cabeçalho MP4: {:?}", e))?;

    let fps = if config.fps == 0 { 30 } else { config.fps };
    let frame_duration_ms = 1000 / fps;
    let frame_interval = Duration::from_millis(frame_duration_ms as u64);

    let video_track_id = 1u32;
    mp4_writer.add_track(&mp4::TrackConfig {
        track_type: mp4::TrackType::Video,
        timescale: fps * 1000,
        language: "und".to_string(),
        media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
            width: target_w as u16,
            height: target_h as u16,
            seq_param_set: vec![],
            pic_param_set: vec![],
        }),
    }).map_err(|e| format!("Erro ao adicionar track de vídeo MP4: {:?}", e))?;

    let mut frame_count: u64 = 0;
    let mut next_frame_time = Instant::now();

    while !stop_signal.load(Ordering::SeqCst) {
        if pause_signal.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
            continue;
        }

        let now = Instant::now();
        if now < next_frame_time {
            let sleep_dur = next_frame_time - now;
            thread::sleep(sleep_dur);
        }
        next_frame_time = Instant::now() + frame_interval;

        let frame_buffer = match capturer.frame() {
            Ok(buffer) => buffer.to_vec(),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(e) => {
                eprintln!("Aviso na captura de quadro: {:?}", e);
                continue;
            }
        };

        let yuv = bgra_to_yuv420p_crop(&frame_buffer, screen_w, screen_h, &config.area);
        
        match encoder.encode(&yuv) {
            Ok(bitstream) => {
                let stream_bytes = bitstream.to_vec();
                if !stream_bytes.is_empty() {
                    let is_sync = stream_bytes.windows(4).any(|w| (w[4 - 1] & 0x1F) == 5);
                    let sample = mp4::Mp4Sample {
                        start_time: frame_count * 1000,
                        duration: 1000,
                        rendering_offset: 0,
                        is_sync,
                        bytes: mp4::Bytes::from(stream_bytes),
                    };

                    let _ = mp4_writer.write_sample(video_track_id, &sample);
                    frame_count += 1;
                }
            }
            Err(e) => {
                eprintln!("Aviso ao codificar frame com H264: {:?}", e);
            }
        }
    }

    mp4_writer.write_end().map_err(|e| format!("Erro ao finalizar arquivo MP4: {:?}", e))?;
    Ok(output_path)
}
