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

fn grouped(
    files: &[FileEntry],
    collapsed_directories: &HashSet<DirectoryKey>,
    collapsed_sections: &HashSet<SidebarSection>,
) -> Vec<SidebarItem> {
    build_sidebar(
        files,
        SidebarBuildOptions {
            grouping: SidebarGrouping::ByStageState,
            collapsed_directories,
            collapsed_sections,
        },
    )
}

fn section_of(items: &[SidebarItem], path: &str) -> Option<SidebarSection> {
    items
        .iter()
        .find(|item| item.file().is_some_and(|file| file.path == path))
        .and_then(SidebarItem::section)
}

#[test]
fn grouped_sidebar_splits_files_by_stage_state() {
    let files = [
        file("src/staged.rs", "M "),
        file("src/added.rs", "A "),
        file("src/edited.rs", " M"),
        file("src/partial.rs", "MM"),
        file("src/new.rs", "??"),
        file("src/conflict.rs", "UU"),
    ];
    let items = grouped(&files, &HashSet::new(), &HashSet::new());

    for path in ["src/staged.rs", "src/added.rs"] {
        assert_eq!(
            section_of(&items, path),
            Some(SidebarSection::Staged),
            "{path}"
        );
    }
    for path in [
        "src/edited.rs",
        "src/partial.rs",
        "src/new.rs",
        "src/conflict.rs",
    ] {
        assert_eq!(
            section_of(&items, path),
            Some(SidebarSection::Unstaged),
            "{path}"
        );
    }
    let file_rows = items.iter().filter(|item| item.file().is_some()).count();
    assert_eq!(file_rows, files.len(), "each file appears exactly once");
}

#[test]
fn grouped_sidebar_omits_empty_sections_and_counts_files() {
    let items = grouped(
        &[file("a.rs", " M"), file("b.rs", "??")],
        &HashSet::new(),
        &HashSet::new(),
    );

    assert_eq!(
        items[0],
        SidebarItem::Section {
            section: SidebarSection::Unstaged,
            file_count: 2,
            collapsed: false,
        }
    );
    assert!(
        !items
            .iter()
            .any(|item| item.section() == Some(SidebarSection::Staged))
    );
}

#[test]
fn collapsing_a_directory_only_affects_its_own_section() {
    let files = [file("src/lib.rs", "M "), file("src/main.rs", " M")];
    let collapsed = HashSet::from([DirectoryKey::new(Some(SidebarSection::Staged), "src")]);
    let items = grouped(&files, &collapsed, &HashSet::new());

    assert_eq!(
        section_of(&items, "src/lib.rs"),
        None,
        "staged src/ is collapsed"
    );
    assert_eq!(
        section_of(&items, "src/main.rs"),
        Some(SidebarSection::Unstaged),
        "unstaged src/ stays open"
    );
}

#[test]
fn collapsed_section_keeps_its_heading_but_hides_files() {
    let files = [file("src/lib.rs", "M "), file("src/main.rs", " M")];
    let items = grouped(
        &files,
        &HashSet::new(),
        &HashSet::from([SidebarSection::Staged]),
    );

    assert_eq!(
        items[0],
        SidebarItem::Section {
            section: SidebarSection::Staged,
            file_count: 1,
            collapsed: true,
        }
    );
    assert!(matches!(
        items[1],
        SidebarItem::Section {
            section: SidebarSection::Unstaged,
            ..
        }
    ));
    assert_eq!(section_of(&items, "src/lib.rs"), None);
}

#[test]
fn tree_grouping_matches_the_ungrouped_builder() {
    let files = [file("src/lib.rs", "M "), file("README.md", " M")];
    let tree = build_sidebar(
        &files,
        SidebarBuildOptions {
            grouping: SidebarGrouping::Tree,
            collapsed_directories: &HashSet::new(),
            collapsed_sections: &HashSet::new(),
        },
    );

    assert_eq!(tree, build_sidebar_items(&files, &HashSet::new()));
}
