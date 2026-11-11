//! Módulo de captura e mixagem de áudio com suporte a microfone e áudio do sistema (WASAPI Loopback).

#![allow(dead_code)]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use crossbeam_channel::{bounded, Receiver};
use fdk_aac_rs::encoder::{Aot, Bitrate, ChannelMode, Encoder, Transport};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct AudioPacket {
    pub sample_time_ms: u64,
    pub duration_samples: u32,
    pub data: Vec<u8>,
}

pub struct AudioRecorder {
    _mic_stream: Option<Stream>,
    _system_stream: Option<Stream>,
    is_running: Arc<AtomicBool>,
    packet_rx: Option<Receiver<AudioPacket>>,
}

/// Converte um buffer de entrada com qualquer taxa de amostragem e número de canais para
/// amostras estéreo f32 normalizadas a 48.000 Hz.
pub fn resample_to_stereo_48k(
    input: &[f32],
    in_channels: usize,
    in_sample_rate: u32,
    out_stereo: &mut Vec<f32>,
) {
    if in_channels == 0 || in_sample_rate == 0 || input.is_empty() {
        return;
    }

    let frames = input.len() / in_channels;
    if frames == 0 {
        return;
    }

    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);

    for f in 0..frames {
        let base = f * in_channels;
        let l = input[base];
        let r = if in_channels >= 2 { input[base + 1] } else { l };
        left.push(l);
        right.push(r);
    }

    if in_sample_rate == 48_000 {
        for f in 0..frames {
            out_stereo.push(left[f]);
            out_stereo.push(right[f]);
        }
        return;
    }

    let ratio = 48_000.0 / (in_sample_rate as f64);
    let out_frames = ((frames as f64) * ratio).round() as usize;

    for i in 0..out_frames {
        let src_pos = (i as f64) / ratio;
        let idx0 = (src_pos.floor() as usize).min(frames.saturating_sub(1));
        let idx1 = (idx0 + 1).min(frames.saturating_sub(1));
        let frac = (src_pos - idx0 as f64) as f32;

        let l_sample = left[idx0] * (1.0 - frac) + left[idx1] * frac;
        let r_sample = right[idx0] * (1.0 - frac) + right[idx1] * frac;

        out_stereo.push(l_sample);
        out_stereo.push(r_sample);
    }
}

/// Faz a soma de dois sinais estéreo com saturação (clamping) em [-1.0, 1.0].
pub fn mix_stereo_signals(signal_a: &[f32], signal_b: &[f32]) -> Vec<f32> {
    let max_len = signal_a.len().max(signal_b.len());
    let mut mixed = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let a = signal_a.get(i).copied().unwrap_or(0.0);
        let b = signal_b.get(i).copied().unwrap_or(0.0);
        let sum = (a + b).clamp(-1.0, 1.0);
        mixed.push(sum);
    }

    mixed
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            _mic_stream: None,
            _system_stream: None,
            is_running: Arc::new(AtomicBool::new(false)),
            packet_rx: None,
        }
    }

    /// Inicia a captura de microfone e/ou áudio do sistema com codificação AAC em tempo real.
    pub fn start(
        &mut self,
        record_mic: bool,
        record_system_audio: bool,
        pause_signal: Arc<AtomicBool>,
    ) -> Result<Receiver<AudioPacket>, String> {
        if !record_mic && !record_system_audio {
            let (_, rx) = bounded(1);
            return Ok(rx);
        }

        let is_running = self.is_running.clone();
        is_running.store(true, Ordering::SeqCst);

        let mic_queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let sys_queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));

        let host = cpal::default_host();

        // 1. Inicia stream de microfone se solicitado
        if record_mic {
            if let Some(device) = host.default_input_device() {
                if let Ok(supported_config) = device.default_input_config() {
                    let sample_rate: u32 = supported_config.sample_rate();
                    let channels = supported_config.channels() as usize;
                    let sample_format = supported_config.sample_format();
                    let stream_config: cpal::StreamConfig = supported_config.into();
                    let queue_clone = mic_queue.clone();

                    let err_fn = |err| eprintln!("Erro no stream do microfone: {:?}", err);

                    let stream_res = match sample_format {
                        cpal::SampleFormat::F32 => device.build_input_stream(
                            stream_config,
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        cpal::SampleFormat::I16 => device.build_input_stream(
                            stream_config,
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                let f32_data: Vec<f32> = data
                                    .iter()
                                    .map(|&s| s as f32 / 32768.0)
                                    .collect();
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(&f32_data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        cpal::SampleFormat::U16 => device.build_input_stream(
                            stream_config,
                            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                                let f32_data: Vec<f32> = data
                                    .iter()
                                    .map(|&s| (s as f32 - 32768.0) / 32768.0)
                                    .collect();
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(&f32_data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        _ => None,
                    };

                    if let Some(stream) = stream_res {
                        if stream.play().is_ok() {
                            self._mic_stream = Some(stream);
                        }
                    }
                }
            }
        }

        // 2. Inicia stream de áudio do sistema (Loopback no Windows) se solicitado
        if record_system_audio {
            // No Windows WASAPI, usar o default_output_device em build_input_stream ativa o modo Loopback
            if let Some(device) = host.default_output_device() {
                if let Ok(supported_config) = device.default_output_config() {
                    let sample_rate: u32 = supported_config.sample_rate();
                    let channels = supported_config.channels() as usize;
                    let sample_format = supported_config.sample_format();
                    let stream_config: cpal::StreamConfig = supported_config.into();
                    let queue_clone = sys_queue.clone();

                    let err_fn = |err| eprintln!("Erro no stream do sistema: {:?}", err);

                    let stream_res = match sample_format {
                        cpal::SampleFormat::F32 => device.build_input_stream(
                            stream_config,
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        cpal::SampleFormat::I16 => device.build_input_stream(
                            stream_config,
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                let f32_data: Vec<f32> = data
                                    .iter()
                                    .map(|&s| s as f32 / 32768.0)
                                    .collect();
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(&f32_data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        cpal::SampleFormat::U16 => device.build_input_stream(
                            stream_config,
                            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                                let f32_data: Vec<f32> = data
                                    .iter()
                                    .map(|&s| (s as f32 - 32768.0) / 32768.0)
                                    .collect();
                                let mut resampled = Vec::new();
                                resample_to_stereo_48k(&f32_data, channels, sample_rate, &mut resampled);
                                if let Ok(mut q) = queue_clone.lock() {
                                    q.extend(resampled);
                                }
                            },
                            err_fn,
                            None,
                        ).ok(),
                        _ => None,
                    };

                    if let Some(stream) = stream_res {
                        if stream.play().is_ok() {
                            self._system_stream = Some(stream);
                        }
                    }
                }
            }
        }

        // 3. Thread de mixagem e codificação AAC
        let (tx, rx) = bounded::<AudioPacket>(128);
        self.packet_rx = Some(rx.clone());

        let running_clone = is_running.clone();
        std::thread::spawn(move || {
            let mut enc = match Encoder::builder()
                .aot(Aot::AacLc)
                .sample_rate(48_000)
                .channel_mode(ChannelMode::Stereo)
                .bitrate(Bitrate::Cbr(128_000))
                .transport(Transport::Raw)
                .build()
            {
                Ok(e) => e,
                Err(err) => {
                    eprintln!("Falha ao inicializar o codificador AAC: {:?}", err);
                    return;
                }
            };

            let info = match enc.info() {
                Ok(i) => i,
                Err(_) => return,
            };

            // 1 frame AAC = 1024 amostras por canal = 2048 amostras estéreo i16
            let samples_per_frame = (info.frame_length() as usize) * 2;
            let mut out_buf = vec![0u8; info.max_out_buf_bytes() as usize];

            let start_time = Instant::now();
            let mut pause_start: Option<Instant> = None;
            let mut total_paused = Duration::ZERO;

            while running_clone.load(Ordering::SeqCst) {
                let is_paused = pause_signal.load(Ordering::SeqCst);
                if is_paused {
                    if pause_start.is_none() {
                        pause_start = Some(Instant::now());
                    }
                    std::thread::sleep(Duration::from_millis(15));
                    continue;
                } else if let Some(p) = pause_start.take() {
                    total_paused += p.elapsed();
                }

                let mic_available = mic_queue.lock().map(|q| q.len()).unwrap_or(0);
                let sys_available = sys_queue.lock().map(|q| q.len()).unwrap_or(0);

                let can_process = match (record_mic, record_system_audio) {
                    (true, true) => {
                        // Sincronização entre microfone e áudio do sistema:
                        // Aguarda até que ambas as filas tenham amostras suficientes.
                        // Caso uma fonte fique sem áudio por mais tempo (drift excessivo > 4 quadros),
                        // processa a fila adiantada para evitar acúmulo de latência.
                        (mic_available >= samples_per_frame && sys_available >= samples_per_frame)
                            || (mic_available >= samples_per_frame * 4 || sys_available >= samples_per_frame * 4)
                    }
                    (true, false) => mic_available >= samples_per_frame,
                    (false, true) => sys_available >= samples_per_frame,
                    _ => false,
                };

                if !can_process {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }

                // Extrai o bloco de áudio de cada canal mantendo a integridade sem cliques
                let mut mic_chunk = Vec::with_capacity(samples_per_frame);
                if record_mic {
                    if let Ok(mut q) = mic_queue.lock() {
                        let to_drain = samples_per_frame.min(q.len());
                        for _ in 0..to_drain {
                            if let Some(s) = q.pop_front() {
                                mic_chunk.push(s);
                            }
                        }
                    }
                }
                while mic_chunk.len() < samples_per_frame {
                    mic_chunk.push(0.0);
                }

                let mut sys_chunk = Vec::with_capacity(samples_per_frame);
                if record_system_audio {
                    if let Ok(mut q) = sys_queue.lock() {
                        let to_drain = samples_per_frame.min(q.len());
                        for _ in 0..to_drain {
                            if let Some(s) = q.pop_front() {
                                sys_chunk.push(s);
                            }
                        }
                    }
                }
                while sys_chunk.len() < samples_per_frame {
                    sys_chunk.push(0.0);
                }

                // Mixa as amostras
                let mixed_f32 = match (record_mic, record_system_audio) {
                    (true, true) => mix_stereo_signals(&mic_chunk, &sys_chunk),
                    (true, false) => mic_chunk,
                    (false, true) => sys_chunk,
                    _ => vec![0.0; samples_per_frame],
                };

                // Converte para i16
                let pcm_i16: Vec<i16> = mixed_f32
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                    .collect();

                let effective_elapsed = start_time.elapsed().saturating_sub(total_paused);
                let sample_time_ms = effective_elapsed.as_millis() as u64;

                if let Ok(encode_output) = enc.encode(&pcm_i16, &mut out_buf) {
                    let out_bytes = encode_output.out_bytes();
                    if out_bytes > 0 {
                        let packet = AudioPacket {
                            sample_time_ms,
                            duration_samples: info.frame_length(),
                            data: out_buf[..out_bytes].to_vec(),
                        };
                        let _ = tx.send(packet);
                    }
                }
            }
        });

        Ok(rx)
    }

    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self._mic_stream = None;
        self._system_stream = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_stereo_48k_passthrough() {
        let input = vec![0.1f32, -0.1f32, 0.2f32, -0.2f32];
        let mut out = Vec::new();
        resample_to_stereo_48k(&input, 2, 48_000, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn test_resample_mono_44100_to_48k() {
        let input = vec![0.5f32; 4410]; // 0.1s at 44.1kHz mono
        let mut out = Vec::new();
        resample_to_stereo_48k(&input, 1, 44_100, &mut out);
        // 0.1s at 48kHz stereo = 4800 frames = 9600 samples
        let frames = out.len() / 2;
        assert!(frames >= 4790 && frames <= 4810);
    }

    #[test]
    fn test_mix_stereo_signals_clamping() {
        let sig_a = vec![0.8f32, -0.7f32];
        let sig_b = vec![0.5f32, -0.5f32];
        let mixed = mix_stereo_signals(&sig_a, &sig_b);
        assert_eq!(mixed.len(), 2);
        assert_eq!(mixed[0], 1.0f32); // 0.8 + 0.5 clamped to 1.0
        assert_eq!(mixed[1], -1.0f32); // -0.7 + -0.5 clamped to -1.0
    }

    #[test]
    fn test_aac_encoder_initialization_and_encoding() {
        let mut enc = Encoder::builder()
            .aot(Aot::AacLc)
            .sample_rate(48_000)
            .channel_mode(ChannelMode::Stereo)
            .bitrate(Bitrate::Cbr(128_000))
            .transport(Transport::Raw)
            .build()
            .unwrap();

        let info = enc.info().unwrap();
        assert_eq!(info.frame_length(), 1024);

        let pcm = vec![0i16; 2048];
        let mut out = vec![0u8; info.max_out_buf_bytes() as usize];
        let res = enc.encode(&pcm, &mut out).unwrap();
        assert_eq!(res.in_samples(), 2048);
        assert!(res.out_bytes() > 0);
    }
}
