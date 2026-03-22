use std::collections::{HashMap, HashSet};

use crate::git::FileEntry;

#[derive(Debug, Clone)]
struct FileTreeNode {
    name: String,
    path: String,
    directories: HashMap<String, FileTreeNode>,
    files: Vec<(FileEntry, String)>,
}

#[derive(Debug, Clone)]
pub enum SidebarItem {
    Header {
        path: String,
        label: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        file: FileEntry,
        label: String,
        depth: usize,
    },
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

fn get_single_child_directory(node: &FileTreeNode) -> Option<&FileTreeNode> {
    (node.directories.len() == 1)
        .then(|| node.directories.values().next())
        .flatten()
}

fn compress_directory_chain(start: &FileTreeNode) -> (&FileTreeNode, String) {
    let mut node = start;
    let mut labels = vec![node.name.clone()];

    while node.files.is_empty() {
        let Some(next) = get_single_child_directory(node) else {
            break;
        };
        node = next;
        labels.push(node.name.clone());
    }

    (node, labels.join("/"))
}

pub fn build_sidebar_items(
    files: &[FileEntry],
    collapsed_directories: &HashSet<String>,
) -> Vec<SidebarItem> {
    let mut root = create_tree_node(String::new(), String::new());

    for file in files {
        let parts: Vec<&str> = file
            .path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let leaf_name = parts.last().copied().unwrap_or(file.path.as_str());
        let sidebar_label = sidebar_file_label(file, leaf_name);

        if parts.len() <= 1 {
            root.files.push((file.clone(), sidebar_label));
            continue;
        }

        let mut current = &mut root;
        let mut current_path = String::new();
        for part in &parts[..parts.len() - 1] {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(part);

            current = current
                .directories
                .entry((*part).to_string())
                .or_insert_with(|| create_tree_node((*part).to_string(), current_path.clone()));
        }

        current.files.push((file.clone(), sidebar_label));
    }

    let mut items = Vec::new();
    visit_node(&root, 0, collapsed_directories, &mut items);
    items
}

fn visit_node(
    node: &FileTreeNode,
    depth: usize,
    collapsed_directories: &HashSet<String>,
    items: &mut Vec<SidebarItem>,
) {
    let mut directories: Vec<&FileTreeNode> = node.directories.values().collect();
    directories.sort_by(|a, b| a.name.cmp(&b.name));

    for directory in directories {
        let (compact_node, label) = compress_directory_chain(directory);
        let is_collapsed = collapsed_directories.contains(&compact_node.path);
        items.push(SidebarItem::Header {
            path: compact_node.path.clone(),
            label,
            depth,
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            visit_node(compact_node, depth + 1, collapsed_directories, items);
        }
    }

    let mut node_files = node.files.clone();
    node_files.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    for (file, label) in node_files {
        items.push(SidebarItem::File { file, label, depth });
    }
}
