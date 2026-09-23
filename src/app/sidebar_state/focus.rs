use crate::sidebar::{self, SidebarItem, SidebarSection};

pub(super) fn visible_file_paths(items: &[SidebarItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.file().map(|file| file.path.clone()))
        .collect()
}

pub(super) fn selected_visible_file_index(
    items: &[SidebarItem],
    selected_path: &str,
) -> Option<usize> {
    items
        .iter()
        .filter_map(SidebarItem::file)
        .position(|file| file.path == selected_path)
}

pub(super) fn row_for_file_path(items: &[SidebarItem], path: &str) -> Option<usize> {
    items
        .iter()
        .position(|item| matches!(item, SidebarItem::File { file, .. } if file.path == path))
}

/// Row for `path` within `section`, else its nearest visible ancestor
/// directory in that section, else the section heading, else `fallback_row`.
pub(super) fn row_for_path_or_nearest(
    items: &[SidebarItem],
    section: Option<SidebarSection>,
    path: &str,
    fallback_row: usize,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    let row_for = |target: &str| {
        items
            .iter()
            .position(|item| item.section() == section && item.path() == target)
    };
    row_for(path)
        .or_else(|| {
            sidebar::get_ancestor_directory_paths(path)
                .into_iter()
                .rev()
                .find_map(|ancestor| row_for(&ancestor))
        })
        .or_else(|| {
            items.iter().position(|item| {
                matches!(item, SidebarItem::Section { section: heading, .. } if Some(*heading) == section)
            })
        })
        .or_else(|| Some(fallback_row.min(items.len() - 1)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{git::FileEntry, sidebar};

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
    fn row_for_path_prefers_exact_item() {
        let items = sidebar::build_sidebar_items(&[file("src/app/mod.rs")], &HashSet::new());

        assert_eq!(
            row_for_path_or_nearest(&items, None, "src/app/mod.rs", 99),
            Some(1)
        );
    }

    #[test]
    fn row_for_path_falls_back_to_nearest_visible_ancestor() {
        let collapsed = HashSet::from(["src/app/".to_string()]);
        let items = sidebar::build_sidebar_items(&[file("src/app/mod.rs")], &collapsed);

        assert_eq!(
            row_for_path_or_nearest(&items, None, "src/app/mod.rs", 99),
            Some(0)
        );
    }

    #[test]
    fn row_for_path_uses_bounded_fallback_when_no_related_row_exists() {
        let items = sidebar::build_sidebar_items(&[file("README.md")], &HashSet::new());

        assert_eq!(
            row_for_path_or_nearest(&items, None, "src/app/mod.rs", 99),
            Some(0)
        );
    }

    #[test]
    fn row_for_path_stays_within_its_section() {
        let staged = FileEntry {
            status: "M ".to_string(),
            ..file("src/lib.rs")
        };
        let unstaged = FileEntry {
            status: " M".to_string(),
            ..file("src/main.rs")
        };
        let items = sidebar::build_sidebar(
            &[staged, unstaged],
            sidebar::SidebarBuildOptions {
                grouping: sidebar::SidebarGrouping::ByStageState,
                collapsed_directories: &HashSet::new(),
                collapsed_sections: &HashSet::new(),
            },
        );

        let row = row_for_path_or_nearest(&items, Some(SidebarSection::Unstaged), "src/", 0)
            .expect("unstaged src/ row should exist");
        assert_eq!(items[row].section(), Some(SidebarSection::Unstaged));
        assert!(items[row].is_directory());
    }
}
