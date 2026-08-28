use crate::sidebar::{self, SidebarItem};

use super::App;

mod directory;
mod file;
mod focus;
mod row;
mod viewport;
mod visibility;

impl App {
    pub(in crate::app) fn rebuild_sidebar_items(&mut self) {
        self.sidebar_items = sidebar::build_sidebar_items(&self.files, &self.collapsed_directories);
    }

    pub(super) fn focused_sidebar_item(&self) -> Option<&SidebarItem> {
        self.sidebar_items.get(self.selected_sidebar_row)
    }
}
