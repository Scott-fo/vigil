use std::collections::HashMap;

use crate::git::FileEntry;

use super::sort::{SegmentSortKey, compare_segment_sort_keys, create_segment_sort_key};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileTreeNode {
    pub(super) name: String,
    pub(super) path: String,
    sort_key: SegmentSortKey,
    pub(super) contains_change: bool,
    pub(super) directories: HashMap<String, FileTreeNode>,
    pub(super) files: Vec<FileTreeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileTreeFile {
    pub(super) file: FileEntry,
    pub(super) label: String,
    sort_key: SegmentSortKey,
}

#[inline]
pub(super) fn build_file_tree(files: &[FileEntry]) -> FileTreeNode {
    let mut root = create_tree_node(String::new(), String::new());

    for file in files {
        let leaf_name = file
            .path
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(file.path.as_str());
        let sidebar_label = sidebar_file_label(file, leaf_name);
        let file_has_change = file_has_change(file);

        if file_has_change {
            root.contains_change = true;
        }

        let mut parts = file
            .path
            .split('/')
            .filter(|part| !part.is_empty())
            .peekable();
        let Some(first_part) = parts.next() else {
            root.files.push(create_tree_file(file, sidebar_label));
            continue;
        };

        if parts.peek().is_none() {
            root.files.push(create_tree_file(file, sidebar_label));
            continue;
        }

        let mut current = &mut root;
        let mut directory_path = String::new();
        let mut part = first_part;
        loop {
            directory_path.push_str(part);
            directory_path.push('/');
            current = current
                .directories
                .entry(part.to_string())
                .or_insert_with(|| create_tree_node(part.to_string(), directory_path.clone()));

            if file_has_change {
                current.contains_change = true;
            }

            let Some(next_part) = parts.next() else {
                break;
            };
            if parts.peek().is_none() {
                break;
            }
            part = next_part;
        }

        current.files.push(create_tree_file(file, sidebar_label));
    }

    root
}

#[inline]
fn create_tree_node(name: String, path: String) -> FileTreeNode {
    let sort_key = create_segment_sort_key(&name);
    FileTreeNode {
        name,
        path,
        sort_key,
        contains_change: false,
        directories: HashMap::new(),
        files: Vec::new(),
    }
}

#[inline]
fn create_tree_file(file: &FileEntry, label: String) -> FileTreeFile {
    let sort_key = create_segment_sort_key(&label);
    FileTreeFile {
        file: file.clone(),
        label,
        sort_key,
    }
}

#[inline]
fn file_has_change(file: &FileEntry) -> bool {
    !file.status.trim().is_empty()
}

#[inline]
fn display_name_from_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[inline]
fn sidebar_file_label(file: &FileEntry, leaf_name: &str) -> String {
    if let Some((from, to)) = file.label.split_once(" -> ") {
        format!(
            "{} -> {}",
            display_name_from_path(from),
            display_name_from_path(to)
        )
    } else {
        leaf_name.to_string()
    }
}

#[inline]
pub(super) fn sorted_directories(node: &FileTreeNode) -> Vec<&FileTreeNode> {
    let mut directories = node.directories.values().collect::<Vec<_>>();
    directories.sort_by(|a, b| {
        compare_segment_sort_keys(&a.sort_key, &b.sort_key).then_with(|| a.name.cmp(&b.name))
    });
    directories
}

#[inline]
pub(super) fn sorted_files(node: &FileTreeNode) -> Vec<&FileTreeFile> {
    let mut files = node.files.iter().collect::<Vec<_>>();
    files.sort_by(|a, b| {
        compare_segment_sort_keys(&a.sort_key, &b.sort_key).then_with(|| a.label.cmp(&b.label))
    });
    files
}

#[cfg(test)]
mod tests {
    use crate::git::FileEntry;

    use super::build_file_tree;

    fn file(status: &str, path: &str, label: &str) -> FileEntry {
        FileEntry {
            status: status.to_string(),
            path: path.to_string(),
            label: label.to_string(),
            filetype: Some("rust"),
        }
    }

    #[test]
    fn build_file_tree_tracks_directory_changes_and_rename_labels() {
        let root = build_file_tree(&[
            file("M ", "src/main.rs", "src/main.rs"),
            file("R ", "src/new.rs", "src/old.rs -> src/new.rs"),
        ]);
        let src = root.directories.get("src").expect("src directory");

        assert!(root.contains_change);
        assert!(src.contains_change);
        assert_eq!(src.files.len(), 2);
        assert!(
            src.files
                .iter()
                .any(|file| file.label == "old.rs -> new.rs")
        );
    }
}
