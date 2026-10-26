//! The user's options, saved between sessions as a small `key = value` text file.
//!
//! Unknown keys, missing keys and unparsable values are ignored, so a hand-edited or older file
//! never stops the app from starting; those options keep their defaults.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::palette::{BarColor, ColorMode};

/// Name of the directory holding the file, under `~/.local` on Linux and `%APPDATA%` on Windows.
const DIR_NAME: &str = "visualizer";
/// Name of the file inside that directory.
const FILE_NAME: &str = "config";

/// Every option the user can change, in the plain form it is saved in. [`App`](crate::app::App)
/// converts to and from it and clamps whatever it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Bar count the user chose, before the terminal width limits it.
    pub bars: usize,
    pub gain: f32,
    pub smoothing_level: u8,
    /// Level that the smoothing shortcut brings back after switching it off.
    pub last_smoothing_level: u8,
    pub bar_gap: bool,
    pub divider: bool,
    pub mirror: bool,
    pub fps: u32,
    pub color_mode: ColorMode,
    /// Bass, mids and treble colors.
    pub bands: [BarColor; 3],
    pub fade_level: u8,
}

impl Default for Config {
    fn default() -> Self {
        crate::app::App::default().config()
    }
}

impl Config {
    /// Where the file lives: `~/.local/visualizer/config` on Linux, `%APPDATA%\visualizer\config`
    /// on Windows. `None` when the home directory is unknown.
    pub fn path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?)
        } else {
            PathBuf::from(std::env::var_os("HOME")?).join(".local")
        };
        Some(base.join(DIR_NAME).join(FILE_NAME))
    }

    /// Reads the saved options; the defaults when there is no file or it cannot be read.
    pub fn load() -> Config {
        Self::path()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    /// Saves the options, creating the directory if needed.
    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::other("no home directory"))?;
        self.save_to(&path)
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.to_text())
    }

    pub fn to_text(&self) -> String {
        let mode = match self.color_mode {
            ColorMode::Single => "single",
            ColorMode::Gradient => "gradient",
            ColorMode::Vertical => "vertical",
            ColorMode::Rainbow => "rainbow",
        };
        format!(
            "bars = {}\ngain = {}\nsmoothing = {}\nlast_smoothing = {}\nbar_gap = {}\ndivider = {}\nmirror = {}\nfps = {}\n\
             color_mode = {}\ncolor_bass = {}\ncolor_mids = {}\ncolor_treble = {}\nfade = {}\n",
            self.bars,
            self.gain,
            self.smoothing_level,
            self.last_smoothing_level,
            self.bar_gap,
            self.divider,
            self.mirror,
            self.fps,
            mode,
            self.bands[0].hex(),
            self.bands[1].hex(),
            self.bands[2].hex(),
            self.fade_level,
        )
    }

    /// Reads the options in `text`; each one that is missing or invalid keeps its default.
    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "bars" => set(&mut config.bars, value.parse().ok()),
                "gain" => set(&mut config.gain, value.parse().ok()),
                "smoothing" => set(&mut config.smoothing_level, value.parse().ok()),
                "last_smoothing" => set(&mut config.last_smoothing_level, value.parse().ok()),
                "bar_gap" => set(&mut config.bar_gap, value.parse().ok()),
                "divider" => set(&mut config.divider, value.parse().ok()),
                "mirror" => set(&mut config.mirror, value.parse().ok()),
                "fps" => set(&mut config.fps, value.parse().ok()),
                "color_mode" => set(
                    &mut config.color_mode,
                    match value {
                        "single" => Some(ColorMode::Single),
                        "gradient" => Some(ColorMode::Gradient),
                        "vertical" => Some(ColorMode::Vertical),
                        "rainbow" => Some(ColorMode::Rainbow),
                        _ => None,
                    },
                ),
                "color_bass" => set(&mut config.bands[0], BarColor::parse_hex(value)),
                "color_mids" => set(&mut config.bands[1], BarColor::parse_hex(value)),
                "color_treble" => set(&mut config.bands[2], BarColor::parse_hex(value)),
                "fade" => set(&mut config.fade_level, value.parse().ok()),
                _ => {}
            }
        }
        config
    }
}

fn set<T>(target: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *target = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_text_round_trips() {
        let config = Config {
            bars: 120,
            gain: 1.7,
            smoothing_level: 4,
            last_smoothing_level: 7,
            bar_gap: false,
            divider: false,
            mirror: false,
            fps: 90,
            color_mode: ColorMode::Vertical,
            bands: [
                BarColor::rgb(1, 2, 3),
                BarColor::rgb(40, 50, 60),
                BarColor::rgb(255, 0, 128),
            ],
            fade_level: 8,
        };
        assert_eq!(Config::parse(&config.to_text()), config);
    }

    #[test]
    fn test_config_ignores_bad_and_unknown_lines() {
        let config = Config::parse("bars = lots\nfps = 90\nnonsense\ncolor_mode = neon\nfoo = 1\n");
        let default = Config::default();
        assert_eq!(config.bars, default.bars);
        assert_eq!(config.color_mode, default.color_mode);
        assert_eq!(config.fps, 90);
    }

    #[test]
    fn test_config_saves_and_loads_through_a_file() {
        let dir =
            std::env::temp_dir().join(format!("visualizer-config-test-{}", std::process::id()));
        let path = dir.join("nested").join(FILE_NAME);
        let config = Config {
            gain: 2.5,
            ..Config::default()
        };
        config.save_to(&path).unwrap();
        assert_eq!(Config::parse(&fs::read_to_string(&path).unwrap()), config);
        fs::remove_dir_all(dir).unwrap();
    }
}
