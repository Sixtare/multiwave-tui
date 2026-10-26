//! Linux capture through the sound server's CLI tools: `pactl` lists the sources (the
//! "monitors" of the outputs are what the user hears) and `parec` delivers the samples.
//! Works with PulseAudio and PipeWire (pipewire-pulse), and unlike the ALSA plugin list it
//! only shows real outputs, named the way the desktop names them.

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

/// An output's monitor source, the thing `parec` records to hear what that output plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorSource {
    /// Source name to pass to `parec -d`, e.g. `alsa_output.pci-....analog-stereo.monitor`.
    pub name: String,
    /// Friendly output name, e.g. `Built-in Audio Analog Stereo`.
    pub description: String,
    pub rate: u32,
    pub channels: u16,
}

/// An application currently playing audio (a PulseAudio "sink input"), together with the monitor
/// of the output it plays on, which is where `parec` can record just that application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSource {
    /// Sink input index, which `parec --monitor-stream` takes.
    pub sink_input: u32,
    /// Name shown in the list, unique among the listed applications.
    pub label: String,
    pub monitor: MonitorSource,
}

fn run(args: &[&str]) -> Option<String> {
    // `LC_ALL=C` keeps the output format independent of the user's locale.
    let output = Command::new("pactl")
        .args(args)
        .env("LC_ALL", "C")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Name of the sink the system plays on by default.
pub fn default_sink() -> Option<String> {
    let name = run(&["get-default-sink"])?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Every output monitor on the sound server (microphones and other inputs are left out).
/// `None` when `pactl` is missing or the server does not answer.
pub fn list_monitors() -> Option<Vec<MonitorSource>> {
    run(&["list", "sources"]).map(|text| parse_monitors(&text))
}

/// Every application playing audio right now. `None` when `pactl` is missing or the server does
/// not answer.
pub fn list_apps() -> Option<Vec<AppSource>> {
    let inputs = parse_sink_inputs(&run(&["list", "sink-inputs"])?);
    let sinks = parse_sinks(&run(&["list", "sinks", "short"])?);
    let monitors = list_monitors()?;

    let mut apps = Vec::new();
    for input in inputs {
        let monitor = sinks
            .iter()
            .find(|(index, _)| *index == input.sink)
            .map(|(_, name)| format!("{name}.monitor"))
            .and_then(|name| monitors.iter().find(|monitor| monitor.name == name));
        if let Some(monitor) = monitor {
            apps.push(AppSource {
                sink_input: input.index,
                label: input.label,
                monitor: monitor.clone(),
            });
        }
    }
    unique_labels(&mut apps);
    Some(apps)
}

/// Gives applications that share a name a ` (#index)` suffix, so each row is distinguishable.
fn unique_labels(apps: &mut [AppSource]) {
    let labels: Vec<String> = apps.iter().map(|app| app.label.clone()).collect();
    for app in apps.iter_mut() {
        if labels.iter().filter(|label| **label == app.label).count() > 1 {
            app.label = format!("{} (#{})", app.label, app.sink_input);
        }
    }
}

struct SinkInput {
    index: u32,
    sink: u32,
    label: String,
}

/// Parses the text of `pactl list sink-inputs`.
fn parse_sink_inputs(text: &str) -> Vec<SinkInput> {
    struct Partial {
        index: u32,
        sink: Option<u32>,
        application: Option<String>,
        media: Option<String>,
    }
    let mut inputs = Vec::new();
    let mut current: Option<Partial> = None;

    let mut flush = |partial: Option<Partial>| {
        if let Some(partial) = partial
            && let Some(sink) = partial.sink
        {
            let label = partial
                .application
                .or(partial.media)
                .unwrap_or_else(|| format!("Stream #{}", partial.index));
            inputs.push(SinkInput {
                index: partial.index,
                sink,
                label,
            });
        }
    };

    for line in text.lines() {
        if let Some(index) = line.strip_prefix("Sink Input #") {
            flush(current.take());
            current = index.trim().parse().ok().map(|index| Partial {
                index,
                sink: None,
                application: None,
                media: None,
            });
            continue;
        }
        let Some(partial) = current.as_mut() else {
            continue;
        };
        let line = line.trim();
        if let Some(sink) = line.strip_prefix("Sink: ") {
            partial.sink = sink.trim().parse().ok();
        } else if let Some(name) = property(line, "application.name") {
            partial.application = Some(name);
        } else if let Some(name) = property(line, "media.name") {
            partial.media = Some(name);
        }
    }
    flush(current.take());
    inputs
}

/// The non-empty value of `key = "value"`, if `line` is that property.
fn property(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    (!value.is_empty()).then(|| value.to_string())
}

/// Parses `pactl list sinks short` into `(index, name)` pairs.
fn parse_sinks(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let index = fields.next()?.trim().parse().ok()?;
            Some((index, fields.next()?.trim().to_string()))
        })
        .collect()
}

/// Parses the text of `pactl list sources`.
fn parse_monitors(text: &str) -> Vec<MonitorSource> {
    let mut monitors = Vec::new();
    let mut current: Option<MonitorSource> = None;

    let mut flush = |source: Option<MonitorSource>| {
        if let Some(source) = source
            && source.name.ends_with(".monitor")
        {
            monitors.push(source);
        }
    };

    for line in text.lines() {
        if line.starts_with("Source #") {
            flush(current.take());
            current = Some(MonitorSource {
                name: String::new(),
                description: String::new(),
                rate: 48_000,
                channels: 2,
            });
            continue;
        }
        let Some(source) = current.as_mut() else {
            continue;
        };
        let line = line.trim();
        if let Some(name) = line.strip_prefix("Name: ") {
            source.name = name.to_string();
        } else if let Some(description) = line.strip_prefix("Description: ") {
            source.description = description
                .strip_prefix("Monitor of ")
                .unwrap_or(description)
                .to_string();
        } else if let Some(spec) = line.strip_prefix("Sample Specification: ")
            && let Some((channels, rate)) = parse_spec(spec)
        {
            source.channels = channels;
            source.rate = rate;
        }
    }
    flush(current.take());
    monitors
}

/// `s16le 2ch 48000Hz` -> `(2, 48000)`.
fn parse_spec(spec: &str) -> Option<(u16, u32)> {
    let mut channels = None;
    let mut rate = None;
    for part in spec.split_whitespace() {
        if let Some(value) = part.strip_suffix("ch") {
            channels = value.parse().ok();
        } else if let Some(value) = part.strip_suffix("Hz") {
            rate = value.parse().ok();
        }
    }
    Some((channels.filter(|c| *c > 0)?, rate.filter(|r| *r > 0)?))
}

/// A running `parec` process and the thread reading its samples. Dropping it stops both.
pub struct PulseCapture {
    child: Child,
    stopped: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl PulseCapture {
    /// Starts recording `source`, or only the application playing on it as `sink_input` when
    /// given. `on_samples` gets interleaved f32 samples; `on_end` runs
    /// if `parec` ends on its own (device unplugged, server restarted), not when dropped.
    pub fn start(
        source: &MonitorSource,
        sink_input: Option<u32>,
        mut on_samples: impl FnMut(&[f32]) + Send + 'static,
        on_end: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        let mut command = Command::new("parec");
        command.args(["-d", &source.name]);
        if let Some(sink_input) = sink_input {
            command.arg(format!("--monitor-stream={sink_input}"));
        }
        let mut child = command
            .args(["--format=float32le", "--latency-msec=20"])
            .arg(format!("--rate={}", source.rate))
            .arg(format!("--channels={}", source.channels))
            .arg("--client-name=multiwave-tui")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout is piped");

        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stopped = Arc::clone(&stopped);
        let frame_bytes = 4 * source.channels as usize;
        let reader = std::thread::spawn(move || {
            let mut raw = [0u8; 8192];
            let mut pending: Vec<u8> = Vec::with_capacity(raw.len() * 2);
            let mut samples: Vec<f32> = Vec::with_capacity(raw.len() / 4);
            loop {
                match stdout.read(&mut raw) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => pending.extend_from_slice(&raw[..n]),
                }
                // Only whole frames go to the processor; the rest waits for the next read.
                let usable = pending.len() / frame_bytes * frame_bytes;
                samples.clear();
                samples.extend(
                    pending[..usable]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
                pending.drain(..usable);
                if !samples.is_empty() {
                    on_samples(&samples);
                }
            }
            if !reader_stopped.load(Ordering::Relaxed) {
                on_end();
            }
        });

        Ok(Self {
            child,
            stopped,
            reader: Some(reader),
        })
    }
}

impl Drop for PulseCapture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
Source #53
\tState: SUSPENDED
\tName: alsa_input.pci-0000_0a_00.6.analog-stereo
\tDescription: Family 17h HD Audio Analog Stereo
\tSample Specification: s16le 2ch 48000Hz
\tProperties:
\t\tdevice.class = \"sound\"

Source #54
\tState: RUNNING
\tName: alsa_output.usb-HyperX.analog-stereo.monitor
\tDescription: Monitor of HyperX Cloud Alpha S Analog Stereo
\tSample Specification: s16le 2ch 44100Hz
\tProperties:
\t\tdevice.class = \"monitor\"

Source #60
\tName: alsa_output.hdmi-stereo.monitor
\tDescription: Monitor of HDMI / DisplayPort
\tSample Specification: float32le 6ch 96000Hz
";

    #[test]
    fn test_only_monitors_are_listed() {
        let monitors = parse_monitors(SAMPLE);
        let names: Vec<_> = monitors.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "alsa_output.usb-HyperX.analog-stereo.monitor",
                "alsa_output.hdmi-stereo.monitor"
            ]
        );
    }

    #[test]
    fn test_monitor_description_and_format() {
        let monitors = parse_monitors(SAMPLE);
        assert_eq!(
            monitors[0].description,
            "HyperX Cloud Alpha S Analog Stereo"
        );
        assert_eq!((monitors[0].rate, monitors[0].channels), (44_100, 2));
        assert_eq!((monitors[1].rate, monitors[1].channels), (96_000, 6));
    }

    const INPUTS: &str = "\
Sink Input #2378
\tDriver: PipeWire
\tSink: 55
\tCorked: yes
\tProperties:
\t\tmedia.name = \"Playback\"
\t\tapplication.name = \"Spotify\"

Sink Input #2400
\tSink: 10718
\tProperties:
\t\tmedia.name = \"Some stream\"

Sink Input #2401
\tSink: 55
\tProperties:
\t\tapplication.name = \"\"
";

    #[test]
    fn test_sink_inputs_are_named_after_their_application() {
        let inputs = parse_sink_inputs(INPUTS);
        let found: Vec<_> = inputs
            .iter()
            .map(|i| (i.index, i.sink, i.label.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (2378, 55, "Spotify"),
                (2400, 10718, "Some stream"),
                (2401, 55, "Stream #2401")
            ]
        );
    }

    #[test]
    fn test_sinks_short_gives_index_and_name() {
        let sinks = parse_sinks("55\talsa_output.usb\tPipeWire\ts16le 2ch 48000Hz\tRUNNING\n");
        assert_eq!(sinks, [(55, "alsa_output.usb".to_string())]);
    }

    #[test]
    fn test_applications_with_the_same_name_get_an_index() {
        let monitor = MonitorSource {
            name: "x.monitor".into(),
            description: "x".into(),
            rate: 48_000,
            channels: 2,
        };
        let app = |sink_input, label: &str| AppSource {
            sink_input,
            label: label.into(),
            monitor: monitor.clone(),
        };
        let mut apps = [app(1, "Firefox"), app(2, "Firefox"), app(3, "Spotify")];
        unique_labels(&mut apps);
        let labels: Vec<_> = apps.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["Firefox (#1)", "Firefox (#2)", "Spotify"]);
    }

    #[test]
    fn test_empty_output_has_no_monitors() {
        assert!(parse_monitors("").is_empty());
    }
}
