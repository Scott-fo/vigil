use std::collections::HashSet;

use crate::git::FileEntry;

use self::search::{SearchState, resolve_search_state};
use self::tree::{FileTreeNode, build_file_tree, sorted_directories, sorted_files};

mod search;
mod sort;
mod tree;
mod viewport;

pub use self::viewport::{FileTreeRange, FileTreeViewportMetrics, compute_window_range};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeInitialExpansion {
    Closed,
    Open,
    Depth(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeSearchMode {
    ExpandMatches,
    CollapseNonMatches,
    HideNonMatches,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeOptions {
    pub flatten_empty_directories: bool,
    pub initial_expansion: FileTreeInitialExpansion,
    pub initial_expanded_paths: HashSet<String>,
    pub collapsed_paths: HashSet<String>,
    pub search: Option<FileTreeSearch>,
}

impl Default for FileTreeOptions {
    fn default() -> Self {
        Self {
            flatten_empty_directories: true,
            initial_expansion: FileTreeInitialExpansion::Open,
            initial_expanded_paths: HashSet::new(),
            collapsed_paths: HashSet::new(),
            search: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeSearch {
    pub query: String,
    pub mode: FileTreeSearchMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenedSegment {
    pub name: String,
    pub path: String,
    pub is_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarItem {
    Header {
        path: String,
        label: String,
        depth: usize,
        collapsed: bool,
        flattened_segments: Vec<FlattenedSegment>,
        pos_in_set: usize,
        set_size: usize,
        contains_change: bool,
        matches_search: bool,
    },
    File {
        file: FileEntry,
        label: String,
        depth: usize,
        pos_in_set: usize,
        set_size: usize,
        matches_search: bool,
    },
}

impl SidebarItem {
    pub fn path(&self) -> &str {
        match self {
            SidebarItem::Header { path, .. } => path,
            SidebarItem::File { file, .. } => file.path.as_str(),
        }
    }

    pub fn is_directory(&self) -> bool {
        matches!(self, SidebarItem::Header { .. })
    }

    pub fn file(&self) -> Option<&FileEntry> {
        match self {
            SidebarItem::File { file, .. } => Some(file),
            SidebarItem::Header { .. } => None,
        }
    }
}

pub fn canonical_directory_path(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

pub fn get_ancestor_directory_paths(path: &str) -> Vec<String> {
    let normalized = path.trim_end_matches('/');
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut ancestors = Vec::new();
    let mut search_index = 0;
    while let Some(slash_index) = normalized[search_index..].find('/') {
        let end = search_index + slash_index + 1;
        ancestors.push(normalized[..end].to_string());
        search_index = end;
    }
    ancestors
}

pub fn build_sidebar_items(
    files: &[FileEntry],
    collapsed_directories: &HashSet<String>,
) -> Vec<SidebarItem> {
    let options = FileTreeOptions {
        collapsed_paths: collapsed_directories
            .iter()
            .map(|path| canonical_directory_path(path))
            .collect(),
        ..FileTreeOptions::default()
    };
    build_sidebar_items_with_options(files, &options)
}

pub fn build_sidebar_items_with_options(
    files: &[FileEntry],
    options: &FileTreeOptions,
) -> Vec<SidebarItem> {
    let root = build_file_tree(files);
    let search_state = options
        .search
        .as_ref()
        .map(|search| resolve_search_state(&root, search));
    let mut items = Vec::with_capacity(files.len());
    visit_directory(&root, 0, options, search_state.as_ref(), &mut items);
    items
}

#[inline]
fn visit_directory(
    node: &FileTreeNode,
    depth: usize,
    options: &FileTreeOptions,
    search_state: Option<&SearchState>,
    items: &mut Vec<SidebarItem>,
) {
    let child_count = node.directories.len() + node.files.len();
    let directories = sorted_directories(node);
    for (directory_index, directory) in directories.iter().enumerate() {
        let (terminal, flattened_segments) = resolve_flattened_directory(directory, options);
        if search_state.is_some_and(|state| !state.visible_paths.contains(&terminal.path)) {
            continue;
        }

        let collapsed = !is_directory_expanded(terminal, depth, options);
        let label = if flattened_segments.len() <= 1 {
            terminal.name.clone()
        } else {
            flattened_segments
                .iter()
                .map(|segment| segment.name.as_str())
                .collect::<Vec<_>>()
                .join("/")
        };
        items.push(SidebarItem::Header {
            path: terminal.path.clone(),
            label,
            depth,
            collapsed,
            flattened_segments,
            pos_in_set: directory_index,
            set_size: child_count,
            contains_change: terminal.contains_change,
            matches_search: search_state
                .is_some_and(|state| state.matching_paths.contains(&terminal.path)),
        });

        if !collapsed {
            visit_directory(terminal, depth + 1, options, search_state, items);
        }
    }

    let files = sorted_files(node);
    for (file_offset, file) in files.iter().enumerate() {
        if search_state.is_some_and(|state| !state.visible_paths.contains(&file.file.path)) {
            continue;
        }

        items.push(SidebarItem::File {
            file: file.file.clone(),
            label: file.label.clone(),
            depth,
            pos_in_set: node.directories.len() + file_offset,
            set_size: child_count,
            matches_search: search_state
                .is_some_and(|state| state.matching_paths.contains(&file.file.path)),
        });
    }
}

#[inline]
fn resolve_flattened_directory<'a>(
    start: &'a FileTreeNode,
    options: &FileTreeOptions,
) -> (&'a FileTreeNode, Vec<FlattenedSegment>) {
    let mut node = start;
    let mut segments = vec![FlattenedSegment {
        name: node.name.clone(),
        path: node.path.clone(),
        is_terminal: false,
    }];

    while options.flatten_empty_directories && node.files.is_empty() && node.directories.len() == 1
    {
        let Some(next) = node.directories.values().next() else {
            break;
        };
        node = next;
        segments.push(FlattenedSegment {
            name: node.name.clone(),
            path: node.path.clone(),
            is_terminal: false,
        });
    }

    if let Some(last) = segments.last_mut() {
        last.is_terminal = true;
    }
    (node, segments)
}

#[inline]
fn is_directory_expanded(
    directory: &FileTreeNode,
    depth: usize,
    options: &FileTreeOptions,
) -> bool {
    if options.collapsed_paths.contains(&directory.path) {
        return false;
    }
    if options.initial_expanded_paths.contains(&directory.path) {
        return true;
    }

    match options.initial_expansion {
        FileTreeInitialExpansion::Closed => false,
        FileTreeInitialExpansion::Open => true,
        FileTreeInitialExpansion::Depth(max_depth) => depth < max_depth,
    }
}

#[cfg(test)]
mod tests;
