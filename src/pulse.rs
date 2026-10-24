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
    /// Starts recording `source`. `on_samples` gets interleaved f32 samples; `on_end` runs
    /// if `parec` ends on its own (device unplugged, server restarted), not when dropped.
    pub fn start(
        source: &MonitorSource,
        mut on_samples: impl FnMut(&[f32]) + Send + 'static,
        on_end: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        let mut child = Command::new("parec")
            .args(["-d", &source.name])
            .args(["--format=float32le", "--latency-msec=20"])
            .arg(format!("--rate={}", source.rate))
            .arg(format!("--channels={}", source.channels))
            .arg("--client-name=ratatui-audio-visualizer")
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

    #[test]
    fn test_empty_output_has_no_monitors() {
        assert!(parse_monitors("").is_empty());
    }
}
