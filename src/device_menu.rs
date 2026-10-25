//! The output devices listed in the device popup, with the cursor, the active device and the
//! scroll position.

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
}

impl DeviceMenu {
    /// A list with the cursor on the active device.
    pub fn new(names: Vec<String>, active: usize) -> Self {
        Self {
            names,
            selected: active,
            active,
            scroll_offset: 0,
        }
    }

    /// First device row visible.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Takes a refreshed list, keeping both indices inside it.
    pub fn replace(&mut self, names: Vec<String>, active: usize, selected: usize) {
        let last = names.len().saturating_sub(1);
        self.names = names;
        self.active = active.min(last);
        self.selected = selected.min(last);
    }

    /// Moves the cursor down, wrapping around.
    pub fn select_next(&mut self) {
        if !self.names.is_empty() {
            self.selected = (self.selected + 1) % self.names.len();
        }
    }

    /// Moves the cursor up, wrapping around.
    pub fn select_prev(&mut self) {
        if !self.names.is_empty() {
            self.selected = (self.selected + self.names.len() - 1) % self.names.len();
        }
    }

    /// Scrolls so the selected row stays within `visible_rows`.
    pub fn ensure_visible(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.names.is_empty() {
            return;
        }

        let selected = self.selected.min(self.names.len() - 1);
        let max_offset = self.names.len().saturating_sub(visible_rows);

        if selected < self.scroll_offset {
            self.scroll_offset = selected;
        } else if selected >= self.scroll_offset.saturating_add(visible_rows) {
            self.scroll_offset = selected + 1 - visible_rows;
        }

        self.scroll_offset = self.scroll_offset.min(max_offset);
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
    fn test_cursor_wraps_in_both_directions() {
        let mut menu = DeviceMenu::new(names(3), 0);

        menu.select_prev();
        assert_eq!(menu.selected, 2);
        menu.select_next();
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn test_navigation_on_an_empty_list_does_nothing() {
        let mut menu = DeviceMenu::default();
        menu.select_next();
        menu.select_prev();
        menu.ensure_visible(4);
        assert_eq!((menu.selected, menu.scroll_offset()), (0, 0));
    }
}
