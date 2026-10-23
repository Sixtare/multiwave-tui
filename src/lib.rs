pub mod app;
pub mod audio_processor;
pub mod color_picker;
pub mod event;
pub mod palette;
pub mod sound_device;
pub mod tui;
pub mod ui;
pub mod update;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use cpal::traits::DeviceTrait;

    #[test]
    fn test_app_creation() {
        let app = app::App::new();
        assert_eq!(app.audio_bars.len(), 32);
        assert_eq!(app.gain, 1.0);
    }

    #[test]
    fn test_sound_device_filtered_list() {
        let devices = sound_device::SoundDevice::list_output_devices();
        assert!(
            !devices.is_empty(),
            "At least one output device should be detected"
        );

        // Verify that internal dummy devices like "Discard all samples" were filtered out
        // and, on Linux, that every listed device except the auto-following default is a
        // physical card-scoped PCM rather than a global ALSA plugin alias.
        for (idx, (name, dev)) in devices.iter().enumerate() {
            let lower = name.to_lowercase();
            assert!(!lower.contains("discard all samples"));
            assert!(!lower.contains("default alsa output"));

            #[cfg(target_os = "linux")]
            {
                if idx == 0 {
                    continue;
                }

                let driver = dev
                    .description()
                    .ok()
                    .and_then(|desc| desc.driver().map(str::to_string))
                    .unwrap_or_default();
                assert!(
                    sound_device::is_card_scoped_pcm(&driver),
                    "unexpected global ALSA device '{name}' with PCM id '{driver}'"
                );
            }

            #[cfg(not(target_os = "linux"))]
            let _ = (idx, dev);
        }
    }
}
