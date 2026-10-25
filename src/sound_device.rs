use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use color_eyre::Result;
use cpal::{
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::audio_processor::AudioProcessor;
use crate::capture_settings::CaptureSettings;
use crate::event::Event;
use crate::pulse::{self, AppSource, MonitorSource, PulseCapture};

const DEFAULT_DEVICE_KEY: &str = "__default_output__";
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Where an entry's audio comes from.
enum Source {
    /// A cpal device (WASAPI render endpoint on Windows, ALSA PCM on Linux without `pactl`).
    Cpal(Device),
    /// The monitor of an output on a PulseAudio/PipeWire server, recorded with `parec`.
    Pulse(MonitorSource),
}

/// A running capture; dropping it stops the audio, which is all the fields are kept for.
#[allow(dead_code)]
enum Capture {
    Cpal(Stream),
    Pulse(PulseCapture),
}

struct DeviceEntry {
    name: String,
    key: String,
    is_default: bool,
    source: Source,
}

struct DeviceList {
    entries: Vec<DeviceEntry>,
    default_key: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct DeviceResolution {
    active_idx: usize,
    selected_idx: usize,
    active_lost: bool,
    active_changed: bool,
}

/// The applications that can be captured, as the device popup lists them.
#[derive(Debug, PartialEq, Eq)]
pub struct AppListUpdate {
    pub names: Vec<String>,
    /// Row of the application being captured, if it is in the list.
    pub active: Option<usize>,
}

/// Changes produced after re-enumerating output devices or recovering from a lost stream.
#[derive(Debug)]
pub struct DeviceListUpdate {
    pub names: Vec<String>,
    pub active_idx: usize,
    pub selected_idx: usize,
    pub active_changed: bool,
    pub changed: bool,
    pub message: Option<String>,
}

pub struct SoundDevice {
    pub name: String,
    pub info: String,
    devices: Vec<DeviceEntry>,
    pub active_idx: usize,
    _capture: Option<Capture>,
    default_key: Option<String>,
    last_device_check: Instant,
    last_app_check: Instant,
    last_captured_check: Instant,
    /// Applications listed by the last [`SoundDevice::list_apps`], in the order of the popup rows.
    apps: Vec<AppSource>,
    /// The application being captured instead of a whole output, if any.
    captured_app: Option<AppSource>,
    /// Settings the capture callback reads on every block, so they can change without
    /// rebuilding the stream.
    settings: Arc<CaptureSettings>,
}

impl SoundDevice {
    /// Configures environment variables required to capture audio output (Sink Monitor)
    /// in PipeWire and PulseAudio before opening audio streams.
    ///
    /// Windows uses WASAPI loopback through cpal and does not need any environment setup.
    pub fn init_capture_env() {
        #[cfg(target_os = "linux")]
        {
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
    }

    /// Enumerates available output audio devices on the host system.
    ///
    /// On Linux, internal ALSA aliases and raw `hw:` duplicates are filtered out. On Windows,
    /// active render endpoints are listed and deduplicated by their stable cpal device id,
    /// because two different endpoints may share the same friendly name.
    pub fn list_output_devices() -> Vec<String> {
        Self::enumerate_output_devices()
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    fn enumerate_output_devices() -> DeviceList {
        Self::init_capture_env();
        if cfg!(target_os = "linux")
            && let Some(list) = Self::enumerate_pulse_devices()
        {
            return list;
        }
        Self::enumerate_cpal_devices()
    }

    /// Lists the outputs of a PulseAudio/PipeWire server through their monitor sources: one
    /// row per real output, named like the desktop names it. `None` when `pactl` is missing,
    /// the server does not answer or there is no output, so cpal can take over.
    fn enumerate_pulse_devices() -> Option<DeviceList> {
        let monitors = pulse::list_monitors().filter(|monitors| !monitors.is_empty())?;
        let default_sink = pulse::default_sink();
        let default_monitor = default_sink.as_ref().map(|sink| format!("{sink}.monitor"));
        let default = monitors
            .iter()
            .find(|monitor| Some(&monitor.name) == default_monitor.as_ref())
            .unwrap_or(&monitors[0])
            .clone();

        // The default output is the first row and follows the system; its own monitor is not
        // listed a second time.
        let mut entries = vec![DeviceEntry {
            name: format!("Default Output ({})", default.description),
            key: DEFAULT_DEVICE_KEY.to_string(),
            is_default: true,
            source: Source::Pulse(default.clone()),
        }];
        entries.extend(
            monitors
                .into_iter()
                .filter(|monitor| monitor.name != default.name)
                .map(|monitor| DeviceEntry {
                    name: monitor.description.clone(),
                    key: monitor.name.clone(),
                    is_default: false,
                    source: Source::Pulse(monitor),
                }),
        );

        Some(DeviceList {
            entries,
            default_key: Some(default_sink.unwrap_or(default.name)),
        })
    }

    fn enumerate_cpal_devices() -> DeviceList {
        let host = cpal::default_host();
        let mut entries = Vec::new();
        let mut default_key = None;

        // 1. First option: Default output device (auto-follows active system sink).
        if let Some(default_dev) = host.default_output_device() {
            let default_name = default_dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "Default System Output".to_string());
            default_key = Some(device_key(&default_dev, &default_name));
            entries.push(DeviceEntry {
                name: format!("Default Output ({})", default_name),
                key: DEFAULT_DEVICE_KEY.to_string(),
                is_default: true,
                source: Source::Cpal(default_dev),
            });
        }

        // 2. Enumerate specific output devices.
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                // Read metadata first: unlike config probing, this does not open the PCM and
                // therefore does not trigger ALSA plugin errors (e.g. JACK/OSS connections).
                let Ok(desc) = dev.description() else {
                    continue;
                };
                let name = desc.name().to_string();

                // ALSA exposes global plugin aliases (lavrate, speex, upmix, jack, oss,
                // pipewire, pulse, ...) next to the real card PCMs. They share the
                // `snd_device_name_hint` list but have no `:CARD=` in their PCM id.
                // Filter them before probing so they are not shown as outputs and their
                // backends are not opened (JACK/OSS would emit connection errors).
                // Also drop raw 'hw:' entries to avoid duplicates of 'plughw:' converters.
                if cfg!(target_os = "linux") {
                    let driver = desc.driver().unwrap_or("");
                    if !is_card_scoped_pcm(driver) || driver.starts_with("hw:") {
                        continue;
                    }

                    // Filter out ALSA internal null devices and redundant sound server loopbacks.
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

                    // Only probe configs for plausible physical devices. Must have a valid
                    // supported config to avoid unconfigured hardware ports.
                    if dev.default_input_config().is_err() && dev.default_output_config().is_err() {
                        continue;
                    }
                }

                let key = device_key(&dev, &name);
                let duplicate = if cfg!(target_os = "linux") {
                    // ALSA aliases often expose the same physical device under several PCM ids.
                    entries
                        .iter()
                        .any(|entry| !entry.is_default && entry.name == name)
                } else {
                    // Windows labels can collide (e.g. two identical monitors), so identity
                    // must come from the endpoint id instead of the friendly name.
                    entries
                        .iter()
                        .any(|entry| !entry.is_default && entry.key == key)
                };

                if !duplicate {
                    entries.push(DeviceEntry {
                        name,
                        key,
                        is_default: false,
                        source: Source::Cpal(dev),
                    });
                }
            }
        }

        DeviceList {
            entries,
            default_key,
        }
    }

    /// Returns a list of user-friendly device names for display in the UI.
    pub fn device_names(&self) -> Vec<String> {
        self.devices
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// Initializes and detects output audio devices.
    pub fn new(settings: Arc<CaptureSettings>) -> Result<Self> {
        let list = Self::enumerate_output_devices();
        let name = list
            .entries
            .first()
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());
        let info = format!("Available output devices: {}", list.entries.len());

        Ok(Self {
            name,
            info,
            devices: list.entries,
            active_idx: 0,
            _capture: None,
            default_key: list.default_key,
            last_device_check: Instant::now(),
            last_app_check: Instant::now(),
            last_captured_check: Instant::now(),
            apps: Vec::new(),
            captured_app: None,
            settings,
        })
    }

    /// Starts audio capture on the currently active device.
    pub fn start_capture(&mut self, sender: mpsc::Sender<Event>) -> Result<()> {
        self.switch_device(self.active_idx, sender)
    }

    /// Switches the capture stream to the device at the specified index.
    pub fn switch_device(&mut self, index: usize, sender: mpsc::Sender<Event>) -> Result<()> {
        Self::init_capture_env();

        if index >= self.devices.len() {
            return Err(color_eyre::eyre::eyre!(
                "Device index out of range: {}",
                index
            ));
        }

        let entry = &self.devices[index];
        let device_name = entry.name.clone();
        let settings = Arc::clone(&self.settings);

        let (capture, device_info) = match &entry.source {
            Source::Cpal(device) => {
                Self::start_cpal_capture(device, &device_name, settings, sender)?
            }
            Source::Pulse(monitor) => {
                Self::start_pulse_capture(monitor, None, &device_name, settings, sender)?
            }
        };

        // Replace the old capture only after the new one is running. If the switch fails, the
        // previous capture keeps running instead of leaving the app silent.
        self._capture = Some(capture);

        // Only commit the new device state after the capture starts successfully, so a failed
        // switch does not leave the UI pointing at a device that is not actually capturing.
        self.name = device_name;
        self.info = device_info;
        self.active_idx = index;
        self.captured_app = None;

        Ok(())
    }

    /// Lists the applications playing audio (Linux with `pactl`; empty elsewhere), remembering
    /// them so [`SoundDevice::switch_to_app`] can take a row of this list.
    pub fn list_apps(&mut self) -> AppListUpdate {
        self.last_app_check = Instant::now();
        self.apps = pulse::list_apps().unwrap_or_default();
        // The captured application stays listed even when it is paused and has no stream.
        AppListUpdate {
            names: self.apps.iter().map(|app| app.label.clone()).collect(),
            active: self.captured_app.as_ref().and_then(|captured| {
                self.apps
                    .iter()
                    .position(|app| app.sink_input == captured.sink_input)
            }),
        }
    }

    /// The list of applications again, at most once per second, so an open popup follows apps
    /// that start or stop playing.
    pub fn poll_apps(&mut self) -> Option<AppListUpdate> {
        (self.last_app_check.elapsed() >= DEVICE_CHECK_INTERVAL).then(|| self.list_apps())
    }

    /// Captures only the application at `index` of the last application list.
    pub fn switch_to_app(&mut self, index: usize, sender: mpsc::Sender<Event>) -> Result<()> {
        let app =
            self.apps.get(index).cloned().ok_or_else(|| {
                color_eyre::eyre::eyre!("Application index out of range: {index}")
            })?;
        let (capture, device_info) = Self::start_pulse_capture(
            &app.monitor,
            Some(app.sink_input),
            &app.label,
            Arc::clone(&self.settings),
            sender,
        )?;
        self._capture = Some(capture);
        self.name = app.label.clone();
        self.info = device_info;
        self.captured_app = Some(app);
        Ok(())
    }

    /// Whether an application is being captured instead of a whole output.
    pub fn is_capturing_app(&self) -> bool {
        self.captured_app.is_some()
    }

    /// Goes back to the default output when the captured application has stopped playing
    /// (checked at most once per second). `None` while it is still there or no app is captured.
    pub fn poll_captured_app(&mut self, sender: &mpsc::Sender<Event>) -> Option<DeviceListUpdate> {
        let captured = self.captured_app.as_ref()?;
        if self.last_captured_check.elapsed() < DEVICE_CHECK_INTERVAL {
            return None;
        }
        self.last_captured_check = Instant::now();
        let still_playing = pulse::list_apps()
            .map(|apps| apps.iter().any(|app| app.sink_input == captured.sink_input))
            .unwrap_or(true);
        if still_playing {
            return None;
        }
        Some(self.fall_back_to_default_output(sender))
    }

    /// Stops capturing the application and captures the default output (row 0) again.
    fn fall_back_to_default_output(&mut self, sender: &mpsc::Sender<Event>) -> DeviceListUpdate {
        self.captured_app = None;
        let message = match self.switch_device(0, sender.clone()) {
            Ok(()) => "Application closed; switched to the default output".to_string(),
            Err(error) => format!("Failed to switch audio device: {error}"),
        };
        DeviceListUpdate {
            names: self.device_names(),
            active_idx: self.active_idx,
            selected_idx: self.active_idx,
            active_changed: true,
            changed: true,
            message: Some(message),
        }
    }

    /// Polls for default-output changes and endpoint hot-plug events.
    ///
    /// Windows requires polling because cpal does not expose `IMMNotificationClient`; the
    /// full list is re-enumerated at most once per second. On Linux the list is only refreshed
    /// when the default output identity changes, avoiding repeated ALSA PCM probing.
    pub fn poll_device_changes(
        &mut self,
        selected_idx: usize,
        sender: &mpsc::Sender<Event>,
    ) -> Option<DeviceListUpdate> {
        // Rebuilding the output capture would drop the application capture.
        if self.captured_app.is_some() || self.last_device_check.elapsed() < DEVICE_CHECK_INTERVAL {
            return None;
        }
        self.last_device_check = Instant::now();

        let default_changed = Self::current_default_key() != self.default_key;
        if !cfg!(target_os = "windows") && !default_changed {
            return None;
        }

        let update = self.refresh_devices(selected_idx, sender, false);
        update.changed.then_some(update)
    }

    /// Re-enumerates devices after the audio callback reports an invalidated/lost stream.
    pub fn recover_from_device_loss(
        &mut self,
        selected_idx: usize,
        sender: &mpsc::Sender<Event>,
    ) -> DeviceListUpdate {
        self.last_device_check = Instant::now();
        if self.captured_app.is_some() {
            return self.fall_back_to_default_output(sender);
        }
        self.refresh_devices(selected_idx, sender, true)
    }

    fn current_default_key() -> Option<String> {
        if cfg!(target_os = "linux")
            && let Some(sink) = pulse::default_sink()
        {
            return Some(sink);
        }
        let host = cpal::default_host();
        host.default_output_device().map(|device| {
            let name = device
                .description()
                .map(|desc| desc.name().to_string())
                .unwrap_or_default();
            device_key(&device, &name)
        })
    }

    fn refresh_devices(
        &mut self,
        selected_idx: usize,
        sender: &mpsc::Sender<Event>,
        force_rebuild: bool,
    ) -> DeviceListUpdate {
        let list = Self::enumerate_output_devices();
        let default_changed = list.default_key != self.default_key;
        let new_devices = list.entries;

        let list_changed = new_devices.len() != self.devices.len()
            || new_devices
                .iter()
                .zip(self.devices.iter())
                .any(|(new, old)| new.key != old.key || new.name != old.name);

        let resolution = {
            let old_slots: Vec<(&str, bool)> = self
                .devices
                .iter()
                .map(|entry| (entry.key.as_str(), entry.is_default))
                .collect();
            let new_slots: Vec<(&str, bool)> = new_devices
                .iter()
                .map(|entry| (entry.key.as_str(), entry.is_default))
                .collect();

            resolve_device_positions(
                &old_slots,
                &new_slots,
                self.active_idx,
                selected_idx,
                default_changed,
                force_rebuild,
            )
        };

        let new_active_idx = resolution.active_idx;
        let new_selected_idx = resolution.selected_idx;
        let active_lost = resolution.active_lost;
        let active_changed = resolution.active_changed;

        self.devices = new_devices;
        self.default_key = list.default_key;

        let changed = list_changed || active_changed;
        if !changed {
            self.active_idx = new_active_idx;
            return DeviceListUpdate {
                names: self.device_names(),
                active_idx: self.active_idx,
                selected_idx: new_selected_idx,
                active_changed,
                changed,
                message: None,
            };
        }

        let mut message = None;
        if active_changed {
            if self.devices.is_empty() {
                self.active_idx = 0;
                message = Some("No audio output devices available".to_string());
            } else {
                match self.switch_device(new_active_idx, sender.clone()) {
                    Ok(()) => {
                        if active_lost {
                            message = Some(
                                "Audio device disconnected; switched to the default output"
                                    .to_string(),
                            );
                        }
                    }
                    Err(error) => {
                        self.active_idx = new_active_idx;
                        message = Some(format!("Failed to switch audio device: {error}"));
                    }
                }
            }
        } else {
            self.active_idx = new_active_idx;
        }

        DeviceListUpdate {
            names: self.device_names(),
            active_idx: self.active_idx,
            selected_idx: new_selected_idx,
            active_changed,
            changed,
            message,
        }
    }

    fn start_cpal_capture(
        device: &Device,
        device_name: &str,
        settings: Arc<CaptureSettings>,
        sender: mpsc::Sender<Event>,
    ) -> Result<(Capture, String)> {
        let (config, sample_format) = Self::get_device_config(device)?;

        let device_info = format!(
            "Device: {}, Sample Rate: {}Hz, Channels: {}, Format: {:?}",
            device_name, config.sample_rate, config.channels, sample_format
        );

        let channels = config.channels as usize;
        let processor = settings.new_processor(channels, config.sample_rate);

        // Each sample format needs its own monomorphized stream; only the matching arm runs, so
        // the arguments are moved, not cloned.
        macro_rules! stream {
            ($sample:ty) => {
                build_capture_stream::<$sample>(device, &config, processor, sender, settings)?
            };
        }
        let stream = match sample_format {
            SampleFormat::I8 => stream!(i8),
            SampleFormat::I16 => stream!(i16),
            SampleFormat::I24 => stream!(cpal::I24),
            SampleFormat::I32 => stream!(i32),
            SampleFormat::I64 => stream!(i64),
            SampleFormat::U8 => stream!(u8),
            SampleFormat::U16 => stream!(u16),
            SampleFormat::U24 => stream!(cpal::U24),
            SampleFormat::U32 => stream!(u32),
            SampleFormat::U64 => stream!(u64),
            SampleFormat::F32 => stream!(f32),
            SampleFormat::F64 => stream!(f64),
            sample_fmt => {
                return Err(color_eyre::eyre::eyre!(
                    "Unsupported sample format: {:?}",
                    sample_fmt
                ));
            }
        };

        stream.play()?;
        Ok((Capture::Cpal(stream), device_info))
    }

    /// Records an output's monitor with `parec`, feeding the same processor the cpal path uses.
    fn start_pulse_capture(
        monitor: &MonitorSource,
        sink_input: Option<u32>,
        device_name: &str,
        settings: Arc<CaptureSettings>,
        sender: mpsc::Sender<Event>,
    ) -> Result<(Capture, String)> {
        let device_info = format!(
            "Device: {}, Sample Rate: {}Hz, Channels: {}, Format: F32 (parec)",
            device_name, monitor.rate, monitor.channels
        );
        let mut processor = settings.new_processor(monitor.channels as usize, monitor.rate);

        let bars_sender = sender.clone();
        let capture = PulseCapture::start(
            monitor,
            sink_input,
            move |samples| {
                settings.apply_to(&mut processor);
                for bars in processor.process_interleaved_samples(samples) {
                    let _ = bars_sender.send(Event::AudioBars(bars));
                }
            },
            move || {
                let _ = sender.send(Event::AudioDeviceLost);
            },
        )
        .map_err(|error| {
            color_eyre::eyre::eyre!("Failed to start parec (install pulseaudio-utils): {error}")
        })?;
        Ok((Capture::Pulse(capture), device_info))
    }

    fn get_device_config(device: &Device) -> Result<(StreamConfig, SampleFormat)> {
        // Try default_input_config first (works directly on loopback/monitor sinks).
        if let Ok(supported) = device.default_input_config() {
            let sample_format = supported.sample_format();
            return Ok((supported.into(), sample_format));
        }

        // WASAPI render endpoints reject default_input_config; their output mix format is
        // used instead and cpal automatically enables loopback when build_input_stream runs.
        if let Ok(supported) = device.default_output_config() {
            let sample_format = supported.sample_format();
            return Ok((supported.into(), sample_format));
        }

        Err(color_eyre::eyre::eyre!(
            "Failed to find compatible stream configuration for device"
        ))
    }
}

/// Resolves the active and selected rows after a device list refresh.
///
/// Identity is the stable device key, not the row index: hot-plugging an endpoint can shift
/// every index, while a friendly-name collision on Windows must not confuse two endpoints.
fn resolve_device_positions(
    old_devices: &[(&str, bool)],
    new_devices: &[(&str, bool)],
    active_idx: usize,
    selected_idx: usize,
    default_changed: bool,
    force_rebuild: bool,
) -> DeviceResolution {
    let active_was_default = old_devices
        .get(active_idx)
        .is_some_and(|(_, is_default)| *is_default);
    let old_active_key = old_devices.get(active_idx).map(|(key, _)| *key);
    let old_selected_key = old_devices.get(selected_idx).map(|(key, _)| *key);

    let new_active_idx = if new_devices.is_empty() {
        0
    } else if active_was_default {
        new_devices
            .iter()
            .position(|(_, is_default)| *is_default)
            .unwrap_or(0)
    } else if let Some(key) = old_active_key
        && let Some(index) = new_devices
            .iter()
            .position(|(device_key, _)| *device_key == key)
    {
        index
    } else {
        new_devices
            .iter()
            .position(|(_, is_default)| *is_default)
            .unwrap_or(0)
    };

    let active_lost = !active_was_default
        && old_active_key
            .is_some_and(|key| !new_devices.iter().any(|(device_key, _)| *device_key == key));
    let active_changed = force_rebuild
        || (active_was_default && default_changed)
        || active_lost
        || (old_devices.is_empty() && !new_devices.is_empty());

    let selected_idx = old_selected_key
        .and_then(|key| {
            new_devices
                .iter()
                .position(|(device_key, _)| *device_key == key)
        })
        .unwrap_or(new_active_idx);

    DeviceResolution {
        active_idx: new_active_idx,
        selected_idx,
        active_lost,
        active_changed,
    }
}

fn build_capture_stream<T>(
    device: &Device,
    config: &StreamConfig,
    processor: AudioProcessor,
    sender: mpsc::Sender<Event>,
    settings: Arc<CaptureSettings>,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut processor = processor;
    let error_sender = sender.clone();
    let mut scratch = Vec::<f32>::with_capacity(1024);

    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            settings.apply_to(&mut processor);
            scratch.clear();
            scratch.extend(data.iter().map(|&sample| f32::from_sample(sample)));

            let bars_batch = processor.process_interleaved_samples(&scratch);
            for bars in bars_batch {
                let _ = sender.send(Event::AudioBars(bars));
            }
        },
        stream_error_callback(error_sender),
        None,
    )
}

fn device_key(device: &Device, fallback_name: &str) -> String {
    device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| format!("name:{fallback_name}"))
}

/// Returns true when an ALSA PCM id points at a specific sound card, as opposed to a global
/// plugin alias such as `lavrate`, `speex`, `upmix`, `jack`, `pipewire` or `default`.
pub(crate) fn is_card_scoped_pcm(driver: &str) -> bool {
    driver.contains(":CARD=")
}

/// Reports stream errors through the event channel instead of writing to stderr, which would
/// corrupt the TUI (the terminal backend also uses stderr).
fn stream_error_callback(
    sender: mpsc::Sender<Event>,
) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |err| {
        let event = match err {
            cpal::StreamError::DeviceNotAvailable | cpal::StreamError::StreamInvalidated => {
                Event::AudioDeviceLost
            }
            // Input overflow/underrun is transient and would otherwise leave a permanent
            // error banner while the visualizer keeps working.
            cpal::StreamError::BufferUnderrun => return,
            other => Event::AudioError(format!("Audio stream error: {other}")),
        };
        let _ = sender.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{is_card_scoped_pcm, resolve_device_positions};

    #[test]
    fn test_global_alsa_plugins_are_not_card_scoped() {
        for driver in [
            "null",
            "default",
            "sysdefault",
            "lavrate",
            "samplerate",
            "speexrate",
            "speex",
            "upmix",
            "vdownmix",
            "jack",
            "oss",
            "pipewire",
            "pulse",
        ] {
            assert!(
                !is_card_scoped_pcm(driver),
                "global ALSA plugin '{driver}' should be filtered out"
            );
        }
    }

    #[test]
    fn test_physical_alsa_pcms_are_card_scoped() {
        for driver in [
            "sysdefault:CARD=S",
            "front:CARD=S,DEV=0",
            "surround71:CARD=Generic_1,DEV=0",
            "iec958:CARD=S,DEV=0",
            "hdmi:CARD=HDMI,DEV=1",
            "plughw:CARD=0,DEV=0",
        ] {
            assert!(
                is_card_scoped_pcm(driver),
                "physical ALSA PCM '{driver}' should be kept"
            );
        }
    }

    #[test]
    fn test_default_output_change_keeps_default_active() {
        let old = [("default", true), ("speakers", false)];
        let new = [("default", true), ("speakers", false)];

        let resolution = resolve_device_positions(&old, &new, 0, 1, true, false);

        assert_eq!(resolution.active_idx, 0);
        assert_eq!(resolution.selected_idx, 1);
        assert!(resolution.active_changed);
        assert!(!resolution.active_lost);
    }

    #[test]
    fn test_default_change_does_not_switch_pinned_device() {
        let old = [("default", true), ("speakers", false)];
        let new = [("default", true), ("speakers", false)];

        let resolution = resolve_device_positions(&old, &new, 1, 1, true, false);

        assert_eq!(resolution.active_idx, 1);
        assert_eq!(resolution.selected_idx, 1);
        assert!(!resolution.active_changed);
        assert!(!resolution.active_lost);
    }

    #[test]
    fn test_explicit_device_survives_hotplug_without_rebuild() {
        let old = [("default", true), ("speakers", false)];
        let new = [
            ("default", true),
            ("headphones", false),
            ("speakers", false),
        ];

        let resolution = resolve_device_positions(&old, &new, 1, 1, false, false);

        assert_eq!(resolution.active_idx, 2);
        assert_eq!(resolution.selected_idx, 2);
        assert!(!resolution.active_changed);
        assert!(!resolution.active_lost);
    }

    #[test]
    fn test_lost_explicit_device_falls_back_to_default() {
        let old = [("default", true), ("speakers", false), ("headset", false)];
        let new = [("default", true), ("headset", false)];

        let resolution = resolve_device_positions(&old, &new, 1, 1, false, false);

        assert_eq!(resolution.active_idx, 0);
        assert_eq!(resolution.selected_idx, 0);
        assert!(resolution.active_changed);
        assert!(resolution.active_lost);
    }

    #[test]
    fn test_lost_selected_device_preserves_active_row() {
        let old = [("default", true), ("speakers", false), ("headset", false)];
        let new = [("default", true), ("speakers", false)];

        let resolution = resolve_device_positions(&old, &new, 1, 2, false, false);

        assert_eq!(resolution.active_idx, 1);
        assert_eq!(resolution.selected_idx, 1);
        assert!(!resolution.active_changed);
    }

    #[test]
    fn test_device_list_appearing_starts_default_capture() {
        let old = [];
        let new = [("default", true), ("speakers", false)];

        let resolution = resolve_device_positions(&old, &new, 0, 0, false, false);

        assert_eq!(resolution.active_idx, 0);
        assert!(resolution.active_changed);
    }

    #[test]
    fn test_stream_recovery_forces_rebuild_of_same_device() {
        let old = [("default", true), ("speakers", false)];
        let new = [("default", true), ("speakers", false)];

        let resolution = resolve_device_positions(&old, &new, 1, 1, false, true);

        assert_eq!(resolution.active_idx, 1);
        assert!(resolution.active_changed);
        assert!(!resolution.active_lost);
    }
}
