use std::sync::mpsc;
use color_eyre::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig,
};

use crate::audio_processor::AudioProcessor;
use crate::event::Event;

pub struct SoundDevice {
    pub device: Option<Device>,
    pub name: String,
    pub info: String,
    _stream: Option<Stream>,
}

impl SoundDevice {
        /// Cria um dispositivo simulado em memória (não toca em drivers nem no CPAL)
    pub fn dummy() -> Self {
        Self {
            device: None,
            name: "Dispositivo Simulado (Dummy)".to_string(),
            info: "Dispositivo de teste para desenvolvimento e CI".to_string(),
            _stream: None,
        }
    }
}

impl SoundDevice {
    /// Configura variáveis de ambiente necessárias para capturar a saída de som (Sink Monitor)
    /// no PipeWire e no PulseAudio antes de abrir os streams de áudio.
    
    pub fn init_capture_env() {
        // No PipeWire (via pipewire-alsa), stream.capture.sink: true instrui o WirePlumber
        // a vincular o stream de gravação diretamente ao monitor da saída de áudio (Sink).
        if std::env::var("PIPEWIRE_ALSA").is_err() {
            unsafe {
                std::env::set_var("PIPEWIRE_ALSA", r#"{"stream.capture.sink": true}"#);
            }
        }
        // No PulseAudio, orienta a gravação para o monitor do sink padrão do sistema.
        if std::env::var("PULSE_SOURCE").is_err() {
            unsafe {
                std::env::set_var("PULSE_SOURCE", "@DEFAULT_SINK@.monitor");
            }
        }
    }

    /// Inicializa e detecta o dispositivo de áudio para captura de saída.
    pub fn new() -> Result<Self> {
        Self::init_capture_env();
        let host = cpal::default_host();

        let (device, config, sample_format) = Self::find_capture_device_and_config(&host)?;

        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "Desconhecido".to_string());

        let info = format!(
            "Dispositivo: {}, Sample Rate: {}Hz, Canais: {}, Formato: {:?}",
            device_name, config.sample_rate, config.channels, sample_format
        );

        Ok(Self {
            device: Some(device),
            name: device_name,
            info,
            _stream: None,
        })
    }

    /// Inicia a captura de áudio contínua e envia as barras processadas pelo canal mpsc.
    pub fn start_capture(
        &mut self,
        sender: mpsc::Sender<Event>,
        num_bars: usize,
        gain: f32,
    ) -> Result<()> {
        Self::init_capture_env();
        let host = cpal::default_host();
        let (device, config, sample_format) = Self::find_capture_device_and_config(&host)?;

        let channels = config.channels as usize;
        let fft_size = 1024;
        let mut processor = AudioProcessor::new(channels, fft_size, num_bars, gain);

        let err_fn = |err| {
            eprintln!("Erro no fluxo de áudio cpal: {}", err);
        };

        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let bars_batch = processor.process_interleaved_samples(data);
                    for bars in bars_batch {
                        let _ = sender.send(Event::AudioBars(bars));
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let f32_samples: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32) / (i16::MAX as f32))
                        .collect();
                    let bars_batch = processor.process_interleaved_samples(&f32_samples);
                    for bars in bars_batch {
                        let _ = sender.send(Event::AudioBars(bars));
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let f32_samples: Vec<f32> = data
                        .iter()
                        .map(|&s| ((s as f32) / (u16::MAX as f32)) * 2.0 - 1.0)
                        .collect();
                    let bars_batch = processor.process_interleaved_samples(&f32_samples);
                    for bars in bars_batch {
                        let _ = sender.send(Event::AudioBars(bars));
                    }
                },
                err_fn,
                None,
            )?,
            sample_fmt => {
                return Err(color_eyre::eyre::eyre!(
                    "Formato de amostra não suportado: {:?}",
                    sample_fmt
                ));
            }
        };

        stream.play()?;
        self._stream = Some(stream);

        Ok(())
    }

    fn find_capture_device_and_config(
        host: &cpal::Host,
    ) -> Result<(Device, StreamConfig, SampleFormat)> {
        // Tenta obter dispositivo com suporte duplex/loopback
        if let Some(device) = host.default_output_device() {
            if let Ok(supported) = device.default_input_config() {
                let sample_format = supported.sample_format();
                return Ok((device, supported.into(), sample_format));
            }
        }

        if let Some(device) = host.default_input_device() {
            if let Ok(supported) = device.default_input_config() {
                let sample_format = supported.sample_format();
                return Ok((device, supported.into(), sample_format));
            }
        }

        Err(color_eyre::eyre::eyre!(
            "Nenhum dispositivo de áudio com configuração de entrada compatível foi encontrado."
        ))
    }
}