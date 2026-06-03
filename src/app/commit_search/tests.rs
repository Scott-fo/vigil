use std::path::PathBuf;

use super::*;

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
}

fn build_commit_entry(hash: &str, short_hash: &str, subject: &str) -> CommitSearchEntry {
    CommitSearchEntry {
        hash: hash.to_string(),
        short_hash: short_hash.to_string(),
        parent_hashes: vec!["parent".to_string()],
        author: "Author".to_string(),
        date: "2026-03-24".to_string(),
        subject: subject.to_string(),
    }
}

#[test]
fn commit_search_filter_and_clamp_follow_filtered_entries() {
    let mut app = build_test_app();
    app.commit_search_entries = vec![
        build_commit_entry("aaaaaaaa", "aaaaaaa", "initial import"),
        build_commit_entry("bbbbbbbb", "bbbbbbb", "refactor parser"),
        build_commit_entry("cccccccc", "ccccccc", "fix renderer"),
    ];
    app.commit_search_query = "parser".to_string();

    assert_eq!(app.filtered_commit_search_indices(), vec![1]);

    app.commit_search_selected_index = 99;
    app.clamp_commit_search_selection();
    assert_eq!(app.commit_search_selected_index, 0);
    assert_eq!(
        app.selected_commit_search_entry()
            .map(|entry| entry.subject),
        Some("refactor parser".to_string())
    );
}

#[test]
fn commit_search_exact_query_syntax_does_not_fall_back_to_fuzzy() {
    let mut app = build_test_app();
    app.commit_search_entries = vec![
        build_commit_entry("aaaaaaaa", "aaaaaaa", "render pipeline"),
        build_commit_entry("bbbbbbbb", "bbbbbbb", "repair parser"),
    ];

    app.commit_search_query = "rp".to_string();
    assert!(!app.filtered_commit_search_indices().is_empty());

    app.commit_search_query = "'rp".to_string();
    assert_eq!(app.filtered_commit_search_indices(), Vec::<usize>::new());

    app.commit_search_query = "'render".to_string();
    assert_eq!(app.filtered_commit_search_indices(), vec![0]);
}
