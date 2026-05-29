use crate::sidebar;

use super::super::App;

impl App {
    pub(in crate::app) fn toggle_focused_sidebar_directory(&mut self) -> bool {
        let Some(path) = self.focused_sidebar_item().and_then(|item| {
            item.is_directory()
                .then(|| sidebar::canonical_directory_path(item.path()))
        }) else {
            return false;
        };

        if !self.collapsed_directories.insert(path.clone()) {
            self.collapsed_directories.remove(&path);
        }
        self.rebuild_sidebar_items();
        self.focus_sidebar_path_or_nearest(&path);
        true
    }

    pub(in crate::app) fn expand_focused_sidebar_directory(&mut self) {
        let Some(path) = self.focused_sidebar_item().and_then(|item| {
            item.is_directory()
                .then(|| sidebar::canonical_directory_path(item.path()))
        }) else {
            return;
        };
        if self.collapsed_directories.remove(&path) {
            self.rebuild_sidebar_items();
            self.focus_sidebar_path_or_nearest(&path);
        }
    }

    pub(in crate::app) fn collapse_focused_sidebar_directory_or_focus_parent(&mut self) {
        let Some(item_path) = self
            .focused_sidebar_item()
            .map(|item| item.path().to_string())
        else {
            return;
        };

        if self
            .focused_sidebar_item()
            .is_some_and(|item| item.is_directory())
        {
            let path = sidebar::canonical_directory_path(&item_path);
            if self.collapsed_directories.insert(path.clone()) {
                self.rebuild_sidebar_items();
                self.focus_sidebar_path_or_nearest(&path);
            }
            return;
        }

        if let Some(parent_path) = sidebar::get_ancestor_directory_paths(&item_path).pop() {
            self.focus_sidebar_path_or_nearest(&parent_path);
        }
    }
}
