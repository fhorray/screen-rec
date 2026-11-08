//! Módulo de captura de áudio com suporte a microfone

#![allow(dead_code)]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct AudioRecorder {
    _stream: Option<Stream>,
    is_running: Arc<AtomicBool>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            _stream: None,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Lista os dispositivos de entrada de áudio disponíveis (microfones)
    pub fn list_input_devices() -> Vec<String> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => devices
                .filter_map(|d| d.description().ok().map(|desc| desc.name().to_string()))
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Inicia a captura de microfone
    pub fn start_input_capture(&mut self, record_mic: bool) -> Result<(), String> {
        if !record_mic {
            return Ok(());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "Nenhum dispositivo de microfone padrão encontrado".to_string())?;

        let supported_config = device
            .default_input_config()
            .map_err(|e| format!("Erro ao obter configuração de áudio: {:?}", e))?;

        let sample_format = supported_config.sample_format();
        let stream_config: cpal::StreamConfig = supported_config.into();

        let is_running = self.is_running.clone();
        is_running.store(true, Ordering::SeqCst);

        let err_fn = |err| eprintln!("Erro no stream de áudio: {:?}", err);

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |_data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Buffer de amostras
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |_data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Buffer de amostras
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                stream_config,
                move |_data: &[u16], _: &cpal::InputCallbackInfo| {
                    // Buffer de amostras
                },
                err_fn,
                None,
            ),
            _ => return Err("Formato de amostra de áudio não suportado".to_string()),
        }.map_err(|e| format!("Falha ao construir stream de áudio: {:?}", e))?;

        stream.play().map_err(|e| format!("Falha ao iniciar stream de áudio: {:?}", e))?;
        self._stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        self._stream = None;
    }
}
