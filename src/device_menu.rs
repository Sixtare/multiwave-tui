//! The output devices and applications listed in the device popup, with the cursor, the active
//! one and the scroll position of each list.

/// Which list the device popup shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceTab {
    /// Whole outputs: everything that device plays.
    #[default]
    Outputs,
    /// Single applications that are playing audio.
    Apps,
}

/// The applications playing audio, listed when the Apps tab is opened and then kept fresh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppList {
    pub names: Vec<String>,
    pub selected: usize,
    /// Row of the application being captured, if any.
    pub active: Option<usize>,
    scroll_offset: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceMenu {
    /// Device names as listed in the popup.
    pub names: Vec<String>,
    /// Index of the highlighted device.
    pub selected: usize,
    /// Index of the device being captured.
    pub active: usize,
    /// First device row visible when the list is scrollable.
    scroll_offset: usize,
    /// Which list the popup shows.
    pub tab: DeviceTab,
    /// Whether the cursor is on the mode row above the list instead of on a row of it.
    pub mode_focused: bool,
    pub apps: AppList,
}

impl DeviceTab {
    /// The name the mode row shows.
    pub fn label(self) -> &'static str {
        match self {
            DeviceTab::Outputs => "Devices",
            DeviceTab::Apps => "Apps",
        }
    }
}

impl DeviceMenu {
    /// A list with the cursor on the active device.
    pub fn new(names: Vec<String>, active: usize) -> Self {
        Self {
            names,
            selected: active,
            active,
            ..Self::default()
        }
    }

    /// Switches between the outputs and the applications.
    pub fn toggle_tab(&mut self) {
        self.tab = match self.tab {
            DeviceTab::Outputs => DeviceTab::Apps,
            DeviceTab::Apps => DeviceTab::Outputs,
        };
    }

    /// Names of the rows of the tab on show.
    pub fn shown_names(&self) -> &[String] {
        match self.tab {
            DeviceTab::Outputs => &self.names,
            DeviceTab::Apps => &self.apps.names,
        }
    }

    /// Highlighted row of the tab on show.
    pub fn shown_selected(&self) -> usize {
        match self.tab {
            DeviceTab::Outputs => self.selected,
            DeviceTab::Apps => self.apps.selected,
        }
    }

    /// Row of the tab on show that is being captured. While an application is captured no whole
    /// output is.
    pub fn shown_active(&self) -> Option<usize> {
        match self.tab {
            DeviceTab::Outputs => self.apps.active.is_none().then_some(self.active),
            DeviceTab::Apps => self.apps.active,
        }
    }

    /// Name of what is being captured: the application, or else the active output.
    pub fn active_name(&self) -> &str {
        self.apps
            .active
            .and_then(|row| self.apps.names.get(row))
            .or_else(|| self.names.get(self.active))
            .map_or("", String::as_str)
    }

    /// First row visible in the tab on show.
    pub fn scroll_offset(&self) -> usize {
        match self.tab {
            DeviceTab::Outputs => self.scroll_offset,
            DeviceTab::Apps => self.apps.scroll_offset,
        }
    }

    /// Takes a refreshed list, keeping both indices inside it.
    pub fn replace(&mut self, names: Vec<String>, active: usize, selected: usize) {
        let last = names.len().saturating_sub(1);
        self.names = names;
        self.active = active.min(last);
        self.selected = selected.min(last);
    }

    /// Takes a refreshed application list. The cursor stays on the same application when it is
    /// still listed, and otherwise on the same row.
    pub fn replace_apps(&mut self, names: Vec<String>, active: Option<usize>) {
        let kept = self
            .apps
            .names
            .get(self.apps.selected)
            .and_then(|name| names.iter().position(|candidate| candidate == name));
        self.apps.selected = kept
            .unwrap_or(self.apps.selected)
            .min(names.len().saturating_sub(1));
        self.apps.names = names;
        self.apps.active = active;
    }

    /// Moves the cursor down: through the list, then to the mode row, then back to the top. From
    /// the mode row it goes to the row the list cursor was left on.
    pub fn select_next(&mut self) {
        let focused = self.mode_focused;
        let (len, selected) = match self.tab {
            DeviceTab::Outputs => (self.names.len(), &mut self.selected),
            DeviceTab::Apps => (self.apps.names.len(), &mut self.apps.selected),
        };
        if focused {
            *selected = (*selected).min(len.saturating_sub(1));
            self.mode_focused = len == 0;
        } else if *selected + 1 >= len {
            *selected = 0;
            self.mode_focused = true;
        } else {
            *selected += 1;
        }
    }

    /// Moves the cursor up: through the list, then to the mode row, then back to the bottom.
    pub fn select_prev(&mut self) {
        let focused = self.mode_focused;
        let (len, selected) = match self.tab {
            DeviceTab::Outputs => (self.names.len(), &mut self.selected),
            DeviceTab::Apps => (self.apps.names.len(), &mut self.apps.selected),
        };
        if focused {
            *selected = len.saturating_sub(1);
            self.mode_focused = len == 0;
        } else if *selected == 0 {
            self.mode_focused = true;
        } else {
            *selected -= 1;
        }
    }

    /// Scrolls so the selected row stays within `visible_rows`.
    pub fn ensure_visible(&mut self, visible_rows: usize) {
        let (len, selected, scroll_offset) = match self.tab {
            DeviceTab::Outputs => (self.names.len(), self.selected, &mut self.scroll_offset),
            DeviceTab::Apps => (
                self.apps.names.len(),
                self.apps.selected,
                &mut self.apps.scroll_offset,
            ),
        };
        if visible_rows == 0 || len == 0 {
            return;
        }

        let selected = selected.min(len - 1);
        let max_offset = len.saturating_sub(visible_rows);

        if selected < *scroll_offset {
            *scroll_offset = selected;
        } else if selected >= scroll_offset.saturating_add(visible_rows) {
            *scroll_offset = selected + 1 - visible_rows;
        }

        *scroll_offset = (*scroll_offset).min(max_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("Device {i}")).collect()
    }

    #[test]
    fn test_replace_keeps_the_indices_inside_the_list() {
        let mut menu = DeviceMenu::new(names(5), 4);

        menu.replace(names(2), 4, 3);

        assert_eq!((menu.active, menu.selected), (1, 1));
    }

    #[test]
    fn test_replace_with_an_empty_list_resets_the_indices() {
        let mut menu = DeviceMenu::new(names(3), 2);

        menu.replace(Vec::new(), 2, 2);

        assert_eq!((menu.active, menu.selected), (0, 0));
    }

    #[test]
    fn test_cursor_passes_through_the_mode_row_when_wrapping() {
        let mut menu = DeviceMenu::new(names(3), 0);

        menu.select_prev();
        assert!(menu.mode_focused);
        menu.select_prev();
        assert_eq!((menu.mode_focused, menu.selected), (false, 2));
        menu.select_next();
        assert!(menu.mode_focused);
        menu.select_next();
        assert_eq!((menu.mode_focused, menu.selected), (false, 0));
    }

    #[test]
    fn test_navigation_on_an_empty_list_does_nothing() {
        let mut menu = DeviceMenu::default();
        menu.select_next();
        menu.select_prev();
        menu.ensure_visible(4);
        assert_eq!((menu.selected, menu.scroll_offset()), (0, 0));
    }

    #[test]
    fn test_apps_tab_has_its_own_cursor_and_active_row() {
        let mut menu = DeviceMenu::new(names(3), 1);
        menu.replace_apps(vec!["Spotify".into(), "Firefox".into()], None);
        menu.toggle_tab();

        menu.select_next();
        assert_eq!((menu.shown_selected(), menu.selected), (1, 1));
        assert_eq!(menu.shown_active(), None);

        menu.replace_apps(vec!["Spotify".into(), "Firefox".into()], Some(0));
        assert_eq!(menu.shown_active(), Some(0));
        assert_eq!(menu.active_name(), "Spotify");
        // No whole output is captured while an application is.
        menu.toggle_tab();
        assert_eq!(menu.shown_active(), None);
    }

    #[test]
    fn test_refreshed_app_list_keeps_the_cursor_on_the_same_app() {
        let mut menu = DeviceMenu::default();
        menu.replace_apps(vec!["A".into(), "B".into(), "C".into()], None);
        menu.apps.selected = 2;

        menu.replace_apps(vec!["C".into(), "A".into()], None);
        assert_eq!(menu.apps.selected, 0);

        menu.replace_apps(Vec::new(), None);
        assert_eq!(menu.apps.selected, 0);
    }
}
