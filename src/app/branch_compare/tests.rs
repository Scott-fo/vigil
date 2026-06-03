use std::path::PathBuf;

use super::super::{App, BranchCompareField};

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
}

#[test]
fn seed_branch_compare_selection_prefers_current_branch_for_source() {
    let mut app = build_test_app();
    app.branch_compare_available_refs = vec![
        "feature/refactor".to_string(),
        "main".to_string(),
        "master".to_string(),
    ];

    app.seed_branch_compare_selection(Some("main"));

    assert_eq!(app.branch_compare_source_ref.as_deref(), Some("main"));
    assert_eq!(
        app.branch_compare_destination_ref.as_deref(),
        Some("master")
    );
    assert_eq!(app.branch_compare_selected_source_index, 1);
    assert_eq!(app.branch_compare_selected_destination_index, 0);
}

#[test]
fn seed_branch_compare_selection_falls_back_to_first_ref_without_current_branch() {
    let mut app = build_test_app();
    app.branch_compare_available_refs = vec![
        "feature/refactor".to_string(),
        "master".to_string(),
        "main".to_string(),
    ];

    app.seed_branch_compare_selection(None);

    assert_eq!(
        app.branch_compare_source_ref.as_deref(),
        Some("feature/refactor")
    );
    assert!(matches!(
        app.branch_compare_destination_ref.as_deref(),
        Some("main" | "master")
    ));
    assert_eq!(app.branch_compare_selected_source_index, 0);
    assert_eq!(app.branch_compare_selected_destination_index, 0);
}

#[test]
fn branch_compare_query_change_preserves_matching_selection() {
    let mut app = build_test_app();
    app.branch_compare_available_refs = vec![
        "feature/refactor".to_string(),
        "release/1.0".to_string(),
        "main".to_string(),
    ];
    app.branch_compare_active_field = BranchCompareField::Source;
    app.branch_compare_source_ref = Some("release/1.0".to_string());
    app.branch_compare_source_query = "release".to_string();

    app.sync_branch_compare_selection_after_query_change();

    assert_eq!(
        app.branch_compare_source_ref.as_deref(),
        Some("release/1.0")
    );
    assert_eq!(app.branch_compare_selected_source_index, 0);
}

#[test]
fn branch_compare_exact_query_syntax_does_not_fall_back_to_fuzzy() {
    let mut app = build_test_app();
    app.branch_compare_available_refs = vec![
        "feature/refactor".to_string(),
        "feature/render-pipeline".to_string(),
        "release/1.0".to_string(),
    ];
    app.branch_compare_active_field = BranchCompareField::Source;

    app.branch_compare_source_query = "fr".to_string();
    assert!(!app.filtered_branch_compare_refs().is_empty());

    app.branch_compare_source_query = "'fr".to_string();
    assert_eq!(app.filtered_branch_compare_refs(), Vec::<String>::new());

    app.branch_compare_source_query = "'release".to_string();
    assert_eq!(
        app.filtered_branch_compare_refs(),
        vec!["release/1.0".to_string()]
    );
}
