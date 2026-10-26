//! Listing and capturing a single application's audio on Windows (WASAPI process loopback,
//! Windows 10 version 2004 or newer).
//!
//! The process-table logic is plain data code that compiles everywhere so its tests run on any
//! platform; only the WASAPI and `sysinfo` calls are Windows-only.
#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::{HashMap, HashSet};

#[cfg(windows)]
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

#[cfg(windows)]
use color_eyre::Result;

#[cfg(windows)]
use crate::{capture_settings::CaptureSettings, event::Event};

/// Format asked of the process loopback, which cannot report one of its own.
#[cfg(windows)]
const SAMPLE_RATE: usize = 48_000;
#[cfg(windows)]
const CHANNELS: usize = 2;
/// How often the capture thread checks whether it was asked to stop when no audio arrives.
#[cfg(windows)]
const WAIT_MS: u32 = 100;

/// An application that can be captured: the root of its process tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSource {
    pub pid: u32,
    /// Name shown in the list, unique among the listed applications.
    pub label: String,
    /// Executable name, to tell the application from a process that reused its id.
    pub exe: String,
}

/// What the process table says about one process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub name: String,
    pub parent: Option<u32>,
}

/// The process that owns `pid`'s whole application: browsers play audio from a child process, and
/// the loopback of the parent (with its tree) is what includes it. Climbs while the parent runs
/// the same executable.
fn app_root(pid: u32, processes: &HashMap<u32, ProcessInfo>) -> u32 {
    let mut current = pid;
    // The limit only guards against a cycle in a table that changed while it was read.
    for _ in 0..64 {
        let Some(info) = processes.get(&current) else {
            break;
        };
        match info
            .parent
            .and_then(|parent| processes.get(&parent).map(|p| (parent, p)))
        {
            Some((parent, parent_info)) if parent_info.name.eq_ignore_ascii_case(&info.name) => {
                current = parent;
            }
            _ => break,
        }
    }
    current
}

/// The executable name without its `.exe`.
fn label_of(exe: &str) -> String {
    let lower = exe.to_ascii_lowercase();
    match lower.strip_suffix(".exe") {
        Some(_) => exe[..exe.len() - 4].to_string(),
        None => exe.to_string(),
    }
}

/// The applications behind `session_pids` (the processes that own an audio session), one per
/// process tree, sorted by name. System sounds (pid 0) and processes that already exited are left
/// out.
fn build_apps(session_pids: &[u32], processes: &HashMap<u32, ProcessInfo>) -> Vec<AppSource> {
    let mut seen = HashSet::new();
    let mut apps: Vec<AppSource> = session_pids
        .iter()
        .filter(|&&pid| pid != 0 && processes.contains_key(&pid))
        .map(|&pid| app_root(pid, processes))
        .filter(|root| seen.insert(*root))
        .filter_map(|root| {
            let exe = processes.get(&root)?.name.clone();
            Some(AppSource {
                pid: root,
                label: label_of(&exe),
                exe,
            })
        })
        .collect();
    apps.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then(a.pid.cmp(&b.pid))
    });

    let labels: Vec<String> = apps.iter().map(|app| app.label.clone()).collect();
    for app in &mut apps {
        if labels.iter().filter(|label| **label == app.label).count() > 1 {
            app.label = format!("{} (#{})", app.label, app.pid);
        }
    }
    apps
}

/// Every running process, by id.
#[cfg(windows)]
fn process_table() -> HashMap<u32, ProcessInfo> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            (
                pid.as_u32(),
                ProcessInfo {
                    name: process.name().to_string_lossy().into_owned(),
                    parent: process.parent().map(|parent| parent.as_u32()),
                },
            )
        })
        .collect()
}

/// The ids of the processes that have an audio session playing on any active output.
#[cfg(windows)]
fn playing_pids() -> Option<Vec<u32>> {
    use wasapi::{DeviceEnumerator, Direction, SessionState};

    // COM state is per thread, and the caller's may already be set up in another mode.
    if wasapi::initialize_mta().is_err() {
        return None;
    }
    let enumerator = DeviceEnumerator::new().ok()?;
    let devices = enumerator.get_device_collection(&Direction::Render).ok()?;
    let mut pids = Vec::new();
    for device in &devices {
        let Ok(device) = device else { continue };
        let Ok(manager) = device.get_iaudiosessionmanager() else {
            continue;
        };
        let Ok(sessions) = manager.get_audiosessionenumerator() else {
            continue;
        };
        for index in 0..sessions.get_count().unwrap_or(0) {
            let Ok(control) = sessions.get_session(index) else {
                continue;
            };
            if matches!(control.get_state(), Ok(SessionState::Active))
                && let Ok(pid) = control.get_process_id()
            {
                pids.push(pid);
            }
        }
    }
    Some(pids)
}

/// Every application playing audio right now. `None` when the audio service does not answer.
#[cfg(windows)]
pub fn list_apps() -> Option<Vec<AppSource>> {
    // A short-lived thread keeps this call's COM initialization out of the UI thread.
    let pids = std::thread::spawn(playing_pids).join().ok().flatten()?;
    Some(build_apps(&pids, &process_table()))
}

/// Whether the application's process is still running.
#[cfg(windows)]
pub fn is_running(app: &AppSource) -> bool {
    process_table()
        .get(&app.pid)
        .is_some_and(|info| info.name == app.exe)
}

/// A running capture of one application; dropping it stops the thread.
#[cfg(windows)]
pub struct ProcessCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl ProcessCapture {
    /// Starts capturing `app` and its child processes, feeding the same processor the output
    /// capture uses. Returns the capture and a description of the stream.
    pub fn start(
        app: &AppSource,
        settings: Arc<CaptureSettings>,
        sender: mpsc::Sender<Event>,
    ) -> Result<(Self, String)> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = {
            let stop = Arc::clone(&stop);
            let pid = app.pid;
            std::thread::Builder::new()
                .name("app-capture".into())
                .spawn(move || capture_loop(pid, settings, sender, stop, ready_tx))?
        };
        let capture = Self {
            stop,
            thread: Some(thread),
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok((
                capture,
                format!(
                    "Application: {}, Sample Rate: {SAMPLE_RATE}Hz, Channels: {CHANNELS}, \
                     Format: F32 (WASAPI process loopback)",
                    app.label
                ),
            )),
            Ok(Err(message)) => Err(color_eyre::eyre::eyre!(
                "Failed to capture the application: {message}"
            )),
            Err(_) => Err(color_eyre::eyre::eyre!(
                "Failed to capture the application: the capture thread ended"
            )),
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Runs on the capture thread: opens the loopback, reports whether that worked through `ready`,
/// then reads audio until asked to stop.
#[cfg(windows)]
fn capture_loop(
    pid: u32,
    settings: Arc<CaptureSettings>,
    sender: mpsc::Sender<Event>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<(), String>>,
) {
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

    let open = || -> std::result::Result<_, String> {
        if wasapi::initialize_mta().is_err() {
            return Err("COM could not be initialized".to_string());
        }
        let format = WaveFormat::new(32, 32, &SampleType::Float, SAMPLE_RATE, CHANNELS, None);
        let mut client =
            AudioClient::new_application_loopback_client(pid, true).map_err(|e| e.to_string())?;
        client
            .initialize_client(
                &format,
                &Direction::Capture,
                &StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: 0,
                },
            )
            .map_err(|e| e.to_string())?;
        let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
        let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
        client.start_stream().map_err(|e| e.to_string())?;
        Ok((client, event, capture, format.get_blockalign() as usize))
    };
    let (client, event, capture, block_align) = match open() {
        Ok(opened) => {
            let _ = ready.send(Ok(()));
            opened
        }
        Err(message) => {
            let _ = ready.send(Err(message));
            return;
        }
    };

    let mut processor = settings.new_processor(CHANNELS, SAMPLE_RATE as u32);
    let mut bytes = Vec::<u8>::new();
    let mut samples = Vec::<f32>::new();
    'capture: while !stop.load(Ordering::Relaxed) {
        // No event for a while is normal: the application may be silent.
        let _ = event.wait_for_event(WAIT_MS);
        loop {
            let frames = match capture.get_next_packet_size() {
                Ok(Some(frames)) if frames > 0 => frames as usize,
                Ok(_) => break,
                Err(_) => {
                    let _ = sender.send(Event::AudioDeviceLost);
                    break 'capture;
                }
            };
            bytes.resize(frames * block_align, 0);
            let Ok((_, info)) = capture.read_from_device(&mut bytes) else {
                let _ = sender.send(Event::AudioDeviceLost);
                break 'capture;
            };
            samples.clear();
            if info.flags.silent {
                samples.resize(frames * CHANNELS, 0.0);
            } else {
                samples.extend(
                    bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|chunk| f32::from_le_bytes(*chunk)),
                );
            }
            settings.apply_to(&mut processor);
            for bars in processor.process_interleaved_samples(&samples) {
                let _ = sender.send(Event::AudioBars(bars));
            }
        }
    }
    let _ = client.stop_stream();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(u32, &str, Option<u32>)]) -> HashMap<u32, ProcessInfo> {
        rows.iter()
            .map(|&(pid, name, parent)| {
                (
                    pid,
                    ProcessInfo {
                        name: name.to_string(),
                        parent,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn test_a_child_playing_audio_resolves_to_the_application_root() {
        let processes = table(&[
            (1, "explorer.exe", None),
            (10, "firefox.exe", Some(1)),
            (11, "firefox.exe", Some(10)),
            (12, "firefox.exe", Some(11)),
        ]);
        assert_eq!(app_root(12, &processes), 10);
        assert_eq!(app_root(10, &processes), 10);
    }

    #[test]
    fn test_the_root_stops_at_a_parent_running_another_program() {
        let processes = table(&[
            (1, "cmd.exe", None),
            (2, "Spotify.exe", Some(1)),
            (3, "SPOTIFY.EXE", Some(2)),
        ]);
        assert_eq!(app_root(3, &processes), 2);
    }

    #[test]
    fn test_a_parent_cycle_does_not_hang() {
        let processes = table(&[(1, "a.exe", Some(2)), (2, "a.exe", Some(1))]);
        let root = app_root(1, &processes);
        assert!(root == 1 || root == 2);
    }

    #[test]
    fn test_apps_are_deduplicated_by_tree_sorted_and_named_without_exe() {
        let processes = table(&[
            (1, "explorer.exe", None),
            (20, "vlc.exe", Some(1)),
            (10, "firefox.exe", Some(1)),
            (11, "firefox.exe", Some(10)),
            (12, "firefox.exe", Some(10)),
        ]);
        let apps = build_apps(&[12, 20, 11, 10], &processes);
        let labels: Vec<_> = apps
            .iter()
            .map(|app| (app.pid, app.label.as_str()))
            .collect();
        assert_eq!(labels, [(10, "firefox"), (20, "vlc")]);
        assert_eq!(apps[0].exe, "firefox.exe");
    }

    #[test]
    fn test_system_sounds_and_exited_processes_are_not_listed() {
        let processes = table(&[(10, "vlc.exe", None)]);
        let apps = build_apps(&[0, 99, 10], &processes);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].pid, 10);
    }

    #[test]
    fn test_separate_instances_of_one_program_get_distinct_labels() {
        let processes = table(&[(10, "mpv.exe", None), (20, "mpv.exe", None)]);
        let apps = build_apps(&[10, 20], &processes);
        let labels: Vec<_> = apps.iter().map(|app| app.label.as_str()).collect();
        assert_eq!(labels, ["mpv (#10)", "mpv (#20)"]);
    }

    #[test]
    fn test_label_drops_only_a_trailing_exe() {
        assert_eq!(label_of("Spotify.exe"), "Spotify");
        assert_eq!(label_of("VLC.EXE"), "VLC");
        assert_eq!(label_of("exe-tool"), "exe-tool");
    }
}
