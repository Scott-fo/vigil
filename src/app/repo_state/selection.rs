#[cfg(test)]
use crate::sidebar;
use crate::{git::FileEntry, sidebar::SidebarItem};

pub(super) fn refreshed_file_index(
    previously_selected: Option<&str>,
    files: &[FileEntry],
    sidebar_items: &[SidebarItem],
) -> usize {
    previously_selected
        .and_then(|path| file_index_by_path(files, path))
        .or_else(|| {
            first_sidebar_file_path(sidebar_items).and_then(|path| file_index_by_path(files, path))
        })
        .unwrap_or(0)
}

fn file_index_by_path(files: &[FileEntry], path: &str) -> Option<usize> {
    files.iter().position(|file| file.path == path)
}

fn first_sidebar_file_path(sidebar_items: &[SidebarItem]) -> Option<&str> {
    sidebar_items
        .iter()
        .find_map(|item| item.file().map(|file| file.path.as_str()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn file(path: &str) -> FileEntry {
        FileEntry {
            status: "M ".to_string(),
            path: path.to_string(),
            label: path.rsplit('/').next().unwrap_or(path).to_string(),
            filetype: Some("rust"),
        }
    }

    #[test]
    fn preserves_previous_selection_when_file_survives_refresh() {
        let files = vec![file("src/lib.rs"), file("src/main.rs")];
        let sidebar_items = sidebar::build_sidebar_items(&files, &HashSet::new());

        assert_eq!(
            refreshed_file_index(Some("src/main.rs"), &files, &sidebar_items),
            1
        );
    }

    #[test]
    fn falls_back_to_first_visible_sidebar_file() {
        let files = vec![file("src/lib.rs"), file("README.md")];
        let sidebar_items = sidebar::build_sidebar_items(&files, &HashSet::new());

        assert_eq!(
            refreshed_file_index(Some("missing.rs"), &files, &sidebar_items),
            0
        );
    }

    #[test]
    fn returns_zero_when_no_file_is_available() {
        assert_eq!(refreshed_file_index(Some("missing.rs"), &[], &[]), 0);
    }
}
