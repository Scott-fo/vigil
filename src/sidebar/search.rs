use std::collections::HashSet;

use super::{
    FileTreeNode, FileTreeSearch, FileTreeSearchMode, get_ancestor_directory_paths,
    sorted_directories, sorted_files,
};

#[derive(Debug, Clone)]
pub(super) struct SearchState {
    pub(super) matching_paths: HashSet<String>,
    pub(super) visible_paths: HashSet<String>,
}

pub(super) fn resolve_search_state(root: &FileTreeNode, search: &FileTreeSearch) -> SearchState {
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
        .filter(|path| contains_case_insensitive(path, &query))
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

fn contains_case_insensitive(value: &str, query: &str) -> bool {
    if value.is_ascii() && query.is_ascii() {
        contains_case_insensitive_ascii(value.as_bytes(), query.as_bytes())
    } else {
        value.to_lowercase().contains(query)
    }
}

fn contains_case_insensitive_ascii(value: &[u8], query: &[u8]) -> bool {
    if query.is_empty() {
        return true;
    }
    if query.len() > value.len() {
        return false;
    }

    value.windows(query.len()).any(|window| {
        window
            .iter()
            .zip(query)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_case_insensitive_match_avoids_lowercase_allocation_path() {
        assert!(contains_case_insensitive("src/App/Button.tsx", "button"));
        assert!(contains_case_insensitive("src/App/Button.tsx", "APP"));
        assert!(!contains_case_insensitive("src/App/Button.tsx", "dialog"));
    }

    #[test]
    fn non_ascii_case_insensitive_match_uses_unicode_lowercase() {
        assert!(contains_case_insensitive("src/Café.rs", "café"));
    }
}
