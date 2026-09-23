use crate::sidebar::{self, SidebarItem};

use super::super::App;

impl App {
    /// Collapses or expands the focused section heading or directory. Returns
    /// false when the focused row is a file.
    pub(in crate::app) fn toggle_focused_sidebar_directory(&mut self) -> bool {
        match self.focused_sidebar_item().cloned() {
            Some(SidebarItem::Section { section, .. }) => {
                if !self.collapsed_sections.insert(section) {
                    self.collapsed_sections.remove(&section);
                }
                self.rebuild_sidebar_items();
                self.focus_sidebar_path_or_nearest(Some(section), "");
                true
            }
            Some(item @ SidebarItem::Header { .. }) => {
                let Some(key) = item.directory_key() else {
                    return false;
                };
                if !self.collapsed_directories.insert(key.clone()) {
                    self.collapsed_directories.remove(&key);
                }
                self.rebuild_sidebar_items();
                self.focus_sidebar_path_or_nearest(key.section, &key.path);
                true
            }
            Some(SidebarItem::File { .. }) | None => false,
        }
    }

    pub(in crate::app) fn expand_focused_sidebar_directory(&mut self) {
        match self.focused_sidebar_item().cloned() {
            Some(SidebarItem::Section { section, .. }) => {
                if self.collapsed_sections.remove(&section) {
                    self.rebuild_sidebar_items();
                    self.focus_sidebar_path_or_nearest(Some(section), "");
                }
            }
            Some(item @ SidebarItem::Header { .. }) => {
                let Some(key) = item.directory_key() else {
                    return;
                };
                if self.collapsed_directories.remove(&key) {
                    self.rebuild_sidebar_items();
                    self.focus_sidebar_path_or_nearest(key.section, &key.path);
                }
            }
            Some(SidebarItem::File { .. }) | None => {}
        }
    }

    pub(in crate::app) fn collapse_focused_sidebar_directory_or_focus_parent(&mut self) {
        let Some(item) = self.focused_sidebar_item().cloned() else {
            return;
        };

        match item {
            SidebarItem::Section { section, .. } => {
                if self.collapsed_sections.insert(section) {
                    self.rebuild_sidebar_items();
                    self.focus_sidebar_path_or_nearest(Some(section), "");
                }
            }
            SidebarItem::Header { .. } => {
                let Some(key) = item.directory_key() else {
                    return;
                };
                if self.collapsed_directories.insert(key.clone()) {
                    self.rebuild_sidebar_items();
                    self.focus_sidebar_path_or_nearest(key.section, &key.path);
                }
            }
            SidebarItem::File { section, .. } => {
                match sidebar::get_ancestor_directory_paths(item.path()).pop() {
                    Some(parent_path) => self.focus_sidebar_path_or_nearest(section, &parent_path),
                    // Top-level files step out to their section heading.
                    None if section.is_some() => self.focus_sidebar_path_or_nearest(section, ""),
                    None => {}
                }
            }
        }
    }
}
