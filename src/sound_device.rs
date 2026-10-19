use std::sync::mpsc;
use color_eyre::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig,
};

use crate::audio_processor::AudioProcessor;
use crate::event::Event;

pub struct SoundDevice {
    pub name: String,
    pub info: String,
    pub devices: Vec<(String, Device)>,
    pub active_idx: usize,
    _stream: Option<Stream>,
}

impl SoundDevice {
    /// Configures environment variables required to capture audio output (Sink Monitor)
    /// in PipeWire and PulseAudio before opening audio streams.
    pub fn init_capture_env() {
        // On PipeWire (via pipewire-alsa), stream.capture.sink: true instructs WirePlumber
        // to link the recording stream directly to the audio output monitor (Sink).
        if std::env::var("PIPEWIRE_ALSA").is_err() {
            unsafe {
                std::env::set_var("PIPEWIRE_ALSA", r#"{"stream.capture.sink": true}"#);
            }
        }
        // On PulseAudio, directs recording to the default system sink monitor.
        if std::env::var("PULSE_SOURCE").is_err() {
            unsafe {
                std::env::set_var("PULSE_SOURCE", "@DEFAULT_SINK@.monitor");
            }
        }
    }

    /// Enumerates available output audio devices on the host system,
    /// filtering out internal ALSA virtual sinks and raw 'hw:' duplicates.
    pub fn list_output_devices() -> Vec<(String, Device)> {
        Self::init_capture_env();
        let host = cpal::default_host();
        let mut result = Vec::new();

        // 1. First option: Default output device (auto-follows active system sink)
        if let Some(default_dev) = host.default_output_device() {
            let default_name = default_dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "Default System Output".to_string());
            result.push((format!("Default Output ({})", default_name), default_dev));
        }

        // 2. Enumerate specific physical output devices
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                // Read metadata first: unlike config probing, this does not open the PCM and
                // therefore does not trigger ALSA plugin errors (e.g. JACK/OSS connections).
                let Ok(desc) = dev.description() else {
                    continue;
                };
                let name = desc.name().to_string();
                let driver = desc.driver().unwrap_or("");

                // Filter out raw 'hw:' to avoid technical duplicates of 'plughw:' software converters.
                // JACK and OSS are legacy plugins whose backends are usually not running/existent;
                // skip them before probing so ALSA doesn't emit connection errors for them.
                if driver.starts_with("hw:")
                    || driver.starts_with("jack")
                    || driver.starts_with("oss")
                {
                    continue;
                }

                // Filter out ALSA internal null devices and redundant sound server loopbacks
                let name_lower = name.to_lowercase();
                if name_lower.contains("discard all samples")
                    || name_lower.contains("null")
                    || name_lower.contains("default alsa output")
                    || name_lower == "pipewire sound server"
                    || name_lower == "pulseaudio sound server"
                    || name_lower == "default"
                {
                    continue;
                }

                // Only probe configs for plausible physical devices. Must have a valid supported
                // config to avoid unconfigured hardware ports.
                if dev.default_input_config().is_err() && dev.default_output_config().is_err() {
                    continue;
                }

                // Avoid duplicate device names in the list
                if !result.iter().any(|(n, _)| n == &name) {
                    result.push((name, dev));
                }
            }
        }

        result
    }

    /// Returns a list of user-friendly device names for display in the UI.
    pub fn device_names(&self) -> Vec<String> {
        self.devices.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Initializes and detects output audio devices.
    pub fn new() -> Result<Self> {
        Self::init_capture_env();
        let devices = Self::list_output_devices();
        let name = devices
            .first()
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let info = format!("Available output devices: {}", devices.len());

        Ok(Self {
            name,
            info,
            devices,
            active_idx: 0,
            _stream: None,
        })
    }

    /// Starts audio capture on the currently active device.
    pub fn start_capture(
        &mut self,
        sender: mpsc::Sender<Event>,
        num_bars: usize,
        gain: f32,
    ) -> Result<()> {
        self.switch_device(self.active_idx, sender, num_bars, gain)
    }

    /// Switches the capture stream to the device at the specified index.
    pub fn switch_device(
        &mut self,
        index: usize,
        sender: mpsc::Sender<Event>,
        num_bars: usize,
        gain: f32,
    ) -> Result<()> {
        Self::init_capture_env();
        // Drop current stream first to release the previous device
        self._stream = None;

        if index >= self.devices.len() {
            return Err(color_eyre::eyre::eyre!("Device index out of range: {}", index));
        }

        let (device_name, device) = &self.devices[index];
        let device_name = device_name.clone();

        let (config, sample_format) = Self::get_device_config(device)?;

        let device_info = format!(
            "Device: {}, Sample Rate: {}Hz, Channels: {}, Format: {:?}",
            device_name, config.sample_rate, config.channels, sample_format
        );

        let channels = config.channels as usize;
        let fft_size = 1024;
        let mut processor = AudioProcessor::new(channels, fft_size, num_bars, gain);

        let error_sender = sender.clone();
        let stream = match sample_format {
            SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let bars_batch = processor.process_interleaved_samples(data);
                    for bars in bars_batch {
                        let _ = sender.send(Event::AudioBars(bars));
                    }
                },
                stream_error_callback(error_sender),
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
                stream_error_callback(error_sender),
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
                stream_error_callback(error_sender),
                None,
            )?,
            sample_fmt => {
                return Err(color_eyre::eyre::eyre!(
                    "Unsupported sample format: {:?}",
                    sample_fmt
                ));
            }
        };

        stream.play()?;
        self._stream = Some(stream);

        // Only commit the new device state after the stream starts successfully, so a failed
        // switch does not leave the UI pointing at a device that is not actually capturing.
        self.name = device_name;
        self.info = device_info;
        self.active_idx = index;

        Ok(())
    }

    fn get_device_config(device: &Device) -> Result<(StreamConfig, SampleFormat)> {
        // Try default_input_config (works directly on loopback/monitor sinks)
        if let Ok(supported) = device.default_input_config() {
            let sample_format = supported.sample_format();
            return Ok((supported.into(), sample_format));
        }

        // Try default_output_config as stream config fallback
        if let Ok(supported) = device.default_output_config() {
            let sample_format = supported.sample_format();
            return Ok((supported.into(), sample_format));
        }

        // Fallback to default input device
        let host = cpal::default_host();
        if let Some(def_in) = host.default_input_device()
            && let Ok(supported) = def_in.default_input_config()
        {
            let sample_format = supported.sample_format();
            return Ok((supported.into(), sample_format));
        }

        Err(color_eyre::eyre::eyre!(
            "Failed to find compatible stream configuration for device"
        ))
    }
}

/// Reports stream errors through the event channel instead of writing to stderr, which would
/// corrupt the TUI (the terminal backend also uses stderr).
fn stream_error_callback(
    sender: mpsc::Sender<Event>,
) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |err| {
        let _ = sender.send(Event::AudioError(format!("Audio stream error: {err}")));
    }
}