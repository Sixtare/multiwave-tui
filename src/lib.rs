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
    fn test_sound_device_filtered_list() {
        let devices = sound_device::SoundDevice::list_output_devices();
        assert!(!devices.is_empty(), "At least one output device should be detected");

        // Verify that internal dummy devices like "Discard all samples" were filtered out
        for (name, _) in &devices {
            let lower = name.to_lowercase();
            assert!(!lower.contains("discard all samples"));
            assert!(!lower.contains("default alsa output"));
        }
    }
}