use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use crate::git::FileEntry;

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
pub struct FileTreeRange {
    pub start: usize,
    pub end: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeViewportMetrics {
    pub item_count: usize,
    pub item_height: usize,
    pub scroll_top: usize,
    pub viewport_height: usize,
    pub overscan: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileTreeNode {
    name: String,
    path: String,
    directories: HashMap<String, FileTreeNode>,
    files: Vec<FileTreeFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileTreeFile {
    file: FileEntry,
    label: String,
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

pub fn compute_window_range(
    metrics: FileTreeViewportMetrics,
    current_range: Option<FileTreeRange>,
) -> FileTreeRange {
    let visible_range = compute_visible_range(&metrics);
    let normalized_current = current_range
        .and_then(|range| normalize_range(range, metrics.item_count))
        .unwrap_or(FileTreeRange {
            start: 0,
            end: None,
        });

    if let (Some(visible_end), Some(current_end)) = (visible_range.end, normalized_current.end)
        && visible_range.start >= normalized_current.start
        && visible_end <= current_end
    {
        return normalized_current;
    }

    expand_range(visible_range, metrics.item_count, metrics.overscan)
}

fn compute_visible_range(metrics: &FileTreeViewportMetrics) -> FileTreeRange {
    if metrics.item_count == 0 || metrics.item_height == 0 {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    let raw_start = metrics.scroll_top / metrics.item_height;
    let raw_end = metrics
        .scroll_top
        .saturating_add(metrics.viewport_height)
        .saturating_add(metrics.item_height.saturating_sub(1))
        / metrics.item_height;
    let raw_end = raw_end.saturating_sub(1);
    if raw_end < raw_start || raw_start >= metrics.item_count {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    FileTreeRange {
        start: raw_start,
        end: Some(raw_end.min(metrics.item_count - 1)),
    }
}

fn normalize_range(range: FileTreeRange, item_count: usize) -> Option<FileTreeRange> {
    let end = range.end?;
    if item_count == 0 || end < range.start {
        return None;
    }
    let start = range.start.min(item_count - 1);
    Some(FileTreeRange {
        start,
        end: Some(end.max(start).min(item_count - 1)),
    })
}

fn expand_range(range: FileTreeRange, item_count: usize, overscan: usize) -> FileTreeRange {
    let Some(end) = range.end else {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    };
    if item_count == 0 || end < range.start {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    FileTreeRange {
        start: range.start.saturating_sub(overscan),
        end: Some(end.saturating_add(overscan).min(item_count - 1)),
    }
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
    let mut items = Vec::new();
    visit_directory(&root, 0, options, search_state.as_ref(), &mut items);
    items
}

fn build_file_tree(files: &[FileEntry]) -> FileTreeNode {
    let mut root = create_tree_node(String::new(), String::new());

    for file in files {
        let parts = file
            .path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let leaf_name = parts.last().copied().unwrap_or(file.path.as_str());
        let sidebar_label = sidebar_file_label(file, leaf_name);

        if parts.len() <= 1 {
            root.files.push(FileTreeFile {
                file: file.clone(),
                label: sidebar_label,
            });
            continue;
        }

        let mut current = &mut root;
        let mut current_path = String::new();
        for part in &parts[..parts.len() - 1] {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);
            let directory_path = canonical_directory_path(&current_path);
            current = current
                .directories
                .entry((*part).to_string())
                .or_insert_with(|| create_tree_node((*part).to_string(), directory_path));
        }

        current.files.push(FileTreeFile {
            file: file.clone(),
            label: sidebar_label,
        });
    }

    root
}

fn create_tree_node(name: String, path: String) -> FileTreeNode {
    FileTreeNode {
        name,
        path,
        directories: HashMap::new(),
        files: Vec::new(),
    }
}

fn display_name_from_path(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

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
            contains_change: directory_contains_change(terminal),
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

fn sorted_directories(node: &FileTreeNode) -> Vec<&FileTreeNode> {
    let mut directories = node.directories.values().collect::<Vec<_>>();
    directories.sort_by(|a, b| compare_segment_values(&a.name, &b.name));
    directories
}

fn sorted_files(node: &FileTreeNode) -> Vec<&FileTreeFile> {
    let mut files = node.files.iter().collect::<Vec<_>>();
    files.sort_by(|a, b| compare_segment_values(&a.label, &b.label));
    files
}

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

fn directory_contains_change(directory: &FileTreeNode) -> bool {
    directory
        .files
        .iter()
        .any(|file| !file.file.status.trim().is_empty())
        || directory
            .directories
            .values()
            .any(directory_contains_change)
}

#[derive(Debug, Clone)]
struct SearchState {
    matching_paths: HashSet<String>,
    visible_paths: HashSet<String>,
}

fn resolve_search_state(root: &FileTreeNode, search: &FileTreeSearch) -> SearchState {
    let query = search.query.trim().to_lowercase();
    if query.is_empty() {
        let all_paths = collect_known_paths(root);
        return SearchState {
            matching_paths: HashSet::new(),
            visible_paths: all_paths.into_iter().collect(),
        };
    }

    let known_paths = collect_known_paths(root);
    let matching_paths = known_paths
        .iter()
        .filter(|path| path.to_lowercase().contains(&query))
        .cloned()
        .collect::<HashSet<_>>();

    let mut visible_paths = if matches!(search.mode, FileTreeSearchMode::HideNonMatches)
        && !matching_paths.is_empty()
    {
        HashSet::new()
    } else {
        known_paths.iter().cloned().collect()
    };

    for matching_path in &matching_paths {
        visible_paths.insert(matching_path.clone());
        for ancestor in get_ancestor_directory_paths(matching_path) {
            visible_paths.insert(ancestor);
        }
    }

    SearchState {
        matching_paths,
        visible_paths,
    }
}

fn collect_known_paths(root: &FileTreeNode) -> Vec<String> {
    let mut paths = Vec::new();
    collect_known_paths_from_directory(root, &mut paths);
    paths
}

fn collect_known_paths_from_directory(directory: &FileTreeNode, paths: &mut Vec<String>) {
    for child in sorted_directories(directory) {
        paths.push(child.path.clone());
        collect_known_paths_from_directory(child, paths);
    }
    for file in sorted_files(directory) {
        paths.push(file.file.path.clone());
    }
}

fn compare_segment_values(left: &str, right: &str) -> Ordering {
    let left_key = create_segment_sort_key(left);
    let right_key = create_segment_sort_key(right);
    let token_order = compare_natural_tokens(&left_key.tokens, &right_key.tokens);
    if token_order != Ordering::Equal {
        return token_order;
    }
    left_key
        .lower_value
        .cmp(&right_key.lower_value)
        .then_with(|| left.cmp(right))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SegmentSortKey {
    lower_value: String,
    tokens: Vec<NaturalToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NaturalToken {
    Number(u64),
    Text(String),
}

fn create_segment_sort_key(value: &str) -> SegmentSortKey {
    let lower_value = value.to_lowercase();
    SegmentSortKey {
        tokens: split_into_natural_tokens(&lower_value),
        lower_value,
    }
}

fn split_into_natural_tokens(value: &str) -> Vec<NaturalToken> {
    let mut tokens = Vec::new();
    let bytes = value.as_bytes();
    let mut token_start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        while index < bytes.len() && !bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        if index > token_start {
            tokens.push(NaturalToken::Text(value[token_start..index].to_string()));
        }

        let mut number = 0u64;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            number = number
                .saturating_mul(10)
                .saturating_add((bytes[index] - b'0') as u64);
            index += 1;
        }
        tokens.push(NaturalToken::Number(number));
        token_start = index;
    }

    if token_start < value.len() || tokens.is_empty() {
        tokens.push(NaturalToken::Text(value[token_start..].to_string()));
    }
    tokens
}

fn compare_natural_tokens(left: &[NaturalToken], right: &[NaturalToken]) -> Ordering {
    for (left_token, right_token) in left.iter().zip(right.iter()) {
        let order = match (left_token, right_token) {
            (NaturalToken::Number(left), NaturalToken::Number(right)) => left.cmp(right),
            (NaturalToken::Text(left), NaturalToken::Text(right)) => left.cmp(right),
            (NaturalToken::Number(left), NaturalToken::Text(right)) => left.to_string().cmp(right),
            (NaturalToken::Text(left), NaturalToken::Number(right)) => left.cmp(&right.to_string()),
        };
        if order != Ordering::Equal {
            return order;
        }
    }

    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: &str) -> FileEntry {
        FileEntry {
            status: status.to_string(),
            path: path.to_string(),
            label: display_name_from_path(path),
            filetype: None,
        }
    }

    fn paths(items: &[SidebarItem]) -> Vec<&str> {
        items.iter().map(SidebarItem::path).collect()
    }

    #[test]
    fn tree_rows_follow_pierre_directory_first_and_natural_sorting() {
        let files = vec![
            file("z10.ts", "M "),
            file("z2.ts", "M "),
            file("README.md", "M "),
            file("src/index.ts", "M "),
            file("src/lib/util.ts", "M "),
        ];

        let items = build_sidebar_items_with_options(
            &files,
            &FileTreeOptions {
                flatten_empty_directories: false,
                initial_expansion: FileTreeInitialExpansion::Depth(1),
                ..FileTreeOptions::default()
            },
        );

        assert_eq!(
            paths(&items),
            vec![
                "src/",
                "src/lib/",
                "src/index.ts",
                "README.md",
                "z2.ts",
                "z10.ts"
            ]
        );
        assert!(matches!(
            &items[0],
            SidebarItem::Header {
                pos_in_set: 0,
                set_size: 4,
                ..
            }
        ));
    }

    #[test]
    fn flattened_directories_use_terminal_path_and_joined_label() {
        let files = vec![
            file("config/project/local/settings.toml", "M "),
            file("src/main.rs", "M "),
        ];
        let items = build_sidebar_items(&files, &HashSet::new());

        assert_eq!(
            paths(&items),
            vec![
                "config/project/local/",
                "config/project/local/settings.toml",
                "src/",
                "src/main.rs"
            ]
        );
        assert!(matches!(
            &items[0],
            SidebarItem::Header {
                label,
                depth: 0,
                flattened_segments,
                ..
            } if label == "config/project/local" && flattened_segments.len() == 3
        ));
    }

    #[test]
    fn collapsed_terminal_directory_hides_descendants() {
        let files = vec![
            file("src/components/Button.tsx", "M "),
            file("src/index.ts", "M "),
            file("README.md", "M "),
        ];
        let collapsed = HashSet::from(["src/".to_string()]);
        let items = build_sidebar_items(&files, &collapsed);

        assert_eq!(paths(&items), vec!["src/", "README.md"]);
        assert!(matches!(
            &items[0],
            SidebarItem::Header {
                collapsed: true,
                contains_change: true,
                ..
            }
        ));
    }

    #[test]
    fn search_modes_match_pierre_visibility_shapes() {
        let files = vec![
            file("README.md", "M "),
            file("package.json", "M "),
            file("src/index.ts", "M "),
            file("src/components/Button.tsx", "M "),
            file("src/utils/worker.ts", "M "),
            file("src/utils/stream.ts", "M "),
        ];

        let hide = build_sidebar_items_with_options(
            &files,
            &FileTreeOptions {
                flatten_empty_directories: false,
                search: Some(FileTreeSearch {
                    query: "worker".to_string(),
                    mode: FileTreeSearchMode::HideNonMatches,
                }),
                ..FileTreeOptions::default()
            },
        );
        assert_eq!(
            paths(&hide),
            vec!["src/", "src/utils/", "src/utils/worker.ts"]
        );

        let keep_non_matches = build_sidebar_items_with_options(
            &files,
            &FileTreeOptions {
                flatten_empty_directories: false,
                search: Some(FileTreeSearch {
                    query: "worker".to_string(),
                    mode: FileTreeSearchMode::ExpandMatches,
                }),
                ..FileTreeOptions::default()
            },
        );
        assert!(paths(&keep_non_matches).contains(&"README.md"));
        assert!(paths(&keep_non_matches).contains(&"src/utils/worker.ts"));
    }

    #[test]
    fn viewport_window_range_matches_pierre_edges() {
        assert_eq!(
            compute_window_range(
                FileTreeViewportMetrics {
                    item_count: 120,
                    item_height: 30,
                    scroll_top: 1500,
                    viewport_height: 120,
                    overscan: 10,
                },
                None,
            ),
            FileTreeRange {
                start: 40,
                end: Some(63),
            }
        );

        let current = FileTreeRange {
            start: 40,
            end: Some(63),
        };
        assert_eq!(
            compute_window_range(
                FileTreeViewportMetrics {
                    item_count: 120,
                    item_height: 30,
                    scroll_top: 1530,
                    viewport_height: 120,
                    overscan: 10,
                },
                Some(current.clone()),
            ),
            current
        );
    }
}
