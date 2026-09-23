use crate::sidebar::{self, SidebarBuildOptions, SidebarGrouping, SidebarItem, SidebarSection};

use super::App;

mod directory;
mod file;
mod focus;
mod row;
mod viewport;
mod visibility;

impl App {
    pub(in crate::app) fn rebuild_sidebar_items(&mut self) {
        let grouping = if self.is_working_tree_mode() {
            SidebarGrouping::ByStageState
        } else {
            SidebarGrouping::Tree
        };
        self.sidebar_items = sidebar::build_sidebar(
            &self.files,
            SidebarBuildOptions {
                grouping,
                collapsed_directories: &self.collapsed_directories,
                collapsed_sections: &self.collapsed_sections,
            },
        );
    }

    /// Section of the sidebar row showing the selected file, if grouped.
    pub fn selected_file_section(&self) -> Option<SidebarSection> {
        let path = self.selected_file()?.path.as_str();
        self.sidebar_items
            .iter()
            .find(|item| item.file().is_some_and(|file| file.path == path))
            .and_then(SidebarItem::section)
    }

    pub(super) fn focused_sidebar_item(&self) -> Option<&SidebarItem> {
        self.sidebar_items.get(self.selected_sidebar_row)
    }
}
