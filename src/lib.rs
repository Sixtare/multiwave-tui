pub mod app;
pub mod audio_processor;
pub mod color_picker;
pub mod event;
pub mod notices;
pub mod palette;
pub mod pulse;
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
        let names = sound_device::SoundDevice::list_output_devices();
        assert!(
            !names.is_empty(),
            "At least one output device should be detected"
        );

        // Internal dummy devices must be filtered out and no output may appear twice.
        for (idx, name) in names.iter().enumerate() {
            let lower = name.to_lowercase();
            assert!(!lower.contains("discard all samples"));
            assert!(!lower.contains("default alsa output"));
            if idx > 0 {
                assert!(
                    !names[1..idx].contains(name),
                    "output '{name}' is listed twice"
                );
            }
        }
    }
}
