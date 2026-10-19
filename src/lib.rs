pub mod app;
pub mod audio_processor;
pub mod event;
pub mod sound_device;
pub mod tui;
pub mod ui;
pub mod update;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = app::App::new();
        assert_eq!(app.audio_bars.len(), 32);
        assert_eq!(app.gain, 1.0);
    }

    #[test]
    fn test_sound_device_sink_monitor() {
        let mut dev = sound_device::SoundDevice::new().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        assert!(dev.start_capture(tx, 16, 1.0).is_ok());
    }
}