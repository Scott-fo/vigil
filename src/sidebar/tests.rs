use super::*;

fn file(path: &str, status: &str) -> FileEntry {
    FileEntry {
        status: status.to_string(),
        path: path.to_string(),
        label: path.rsplit('/').next().unwrap_or(path).to_string(),
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
