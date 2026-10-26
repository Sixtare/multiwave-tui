//! The rows of the main menu.

/// A row of the main menu, in the order they are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuRow {
    #[default]
    Device,
    Color,
    Sensitivity,
    Bars,
    Smoothing,
    BarGap,
    Mirror,
    Fps,
    Render,
}

impl MenuRow {
    /// Every row, top to bottom.
    pub const ALL: [MenuRow; 9] = [
        MenuRow::Device,
        MenuRow::Color,
        MenuRow::Sensitivity,
        MenuRow::Bars,
        MenuRow::Smoothing,
        MenuRow::BarGap,
        MenuRow::Mirror,
        MenuRow::Fps,
        MenuRow::Render,
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|&row| row == self)
            .expect("every row is in ALL")
    }

    /// The row below, wrapping around to the first.
    pub fn next(self) -> MenuRow {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The row above, wrapping around to the last.
    pub fn previous(self) -> MenuRow {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuRow::Device => "Audio Output",
            MenuRow::Color => "Bar style",
            MenuRow::Sensitivity => "Sensitivity",
            MenuRow::Bars => "Bars",
            MenuRow::Smoothing => "Smoothing",
            MenuRow::BarGap => "Bar gap",
            MenuRow::Mirror => "Mirror",
            MenuRow::Fps => "FPS",
            MenuRow::Render => "Render",
        }
    }

    /// The key that reaches the row's option directly, shown beside the menu.
    pub fn shortcut(self) -> &'static str {
        match self {
            MenuRow::Device => "O",
            MenuRow::Color => "C",
            MenuRow::Sensitivity => "↑/↓",
            MenuRow::Bars => "←/→",
            MenuRow::Smoothing => "S",
            MenuRow::BarGap => "G",
            MenuRow::Mirror => "M",
            MenuRow::Fps => "F",
            MenuRow::Render => "R",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_wraps_around() {
        assert_eq!(MenuRow::Render.next(), MenuRow::Device);
        assert_eq!(MenuRow::Device.previous(), MenuRow::Render);
        assert_eq!(MenuRow::Device.next(), MenuRow::Color);
    }

    #[test]
    fn test_next_visits_every_row_once() {
        let mut row = MenuRow::Device;
        for expected in MenuRow::ALL {
            assert_eq!(row, expected);
            row = row.next();
        }
    }
}
