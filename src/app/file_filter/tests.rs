use std::path::PathBuf;

use crate::git::FileEntry;

use super::super::App;
use super::ExcludeSuffixes;

fn file(path: &str) -> FileEntry {
    FileEntry {
        status: "M ".to_string(),
        path: path.to_string(),
        label: path.rsplit('/').next().unwrap_or(path).to_string(),
        filetype: None,
    }
}

#[test]
fn test_ts_hides_test_ts_and_test_tsx() {
    let suffixes = ExcludeSuffixes::from_query("test.ts");

    assert!(suffixes.hides("src/foo.test.ts"));
    assert!(suffixes.hides("src/foo.test.tsx"));
    assert!(suffixes.hides("src/test.ts"));
    assert!(suffixes.hides("test.ts"));
    assert!(!suffixes.hides("src/foo.ts"));
    assert!(!suffixes.hides("src/latest.ts"));
    assert!(!suffixes.hides("src/contest.ts"));
}

#[test]
fn js_suffix_also_matches_jsx_companion() {
    let suffixes = ExcludeSuffixes::from_query(".js");

    assert!(suffixes.hides("src/widget.js"));
    assert!(suffixes.hides("src/widget.jsx"));
    assert!(!suffixes.hides("src/widget.ts"));
}

#[test]
fn dotted_ts_suffix_hides_tsx_files() {
    let suffixes = ExcludeSuffixes::from_query(".ts");

    assert!(suffixes.hides("src/foo.ts"));
    assert!(suffixes.hides("src/foo.tsx"));
    assert!(!suffixes.hides("src/foo.js"));
}

#[test]
fn explicit_tsx_suffix_does_not_hide_ts() {
    let suffixes = ExcludeSuffixes::from_query("test.tsx");

    assert!(suffixes.hides("src/foo.test.tsx"));
    assert!(!suffixes.hides("src/foo.test.ts"));
}

#[test]
fn glob_star_and_comma_separated_suffixes_normalize() {
    let suffixes = ExcludeSuffixes::from_query("*.test.ts, spec.ts  .TEST.ts");

    assert_eq!(suffixes.as_slice(), [".test.ts", "spec.ts"]);
    assert!(suffixes.hides("src/button.spec.ts"));
    assert!(suffixes.hides("src/button.test.tsx"));
    assert!(suffixes.hides("src/button.test.ts"));
    assert!(!suffixes.hides("src/test.ts"));
}

#[test]
fn filter_entries_keeps_visible_files_in_order() {
    let suffixes = ExcludeSuffixes::from_query("test.ts");
    let files = vec![
        file("src/app.ts"),
        file("src/app.test.ts"),
        file("src/app.test.tsx"),
        file("README.md"),
    ];

    let visible = suffixes.filter_entries(&files);
    assert_eq!(
        visible
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/app.ts", "README.md"]
    );
}

#[test]
fn empty_filter_keeps_every_file() {
    let suffixes = ExcludeSuffixes::from_query("  ,  ");
    assert!(suffixes.is_empty());
    assert!(!suffixes.hides("src/foo.test.ts"));
}

#[tokio::test]
async fn applying_suffixes_filters_the_visible_file_list() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-file-filter"));
    app.loaded_files = vec![
        file("src/app.ts"),
        file("src/app.test.ts"),
        file("src/app.test.tsx"),
        file("README.md"),
    ];
    app.rebuild_visible_file_list(Some("src/app.test.ts"));

    assert_eq!(app.files.len(), 4);
    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/app.test.ts")
    );

    app.apply_file_exclude_suffixes(ExcludeSuffixes::from_query("test.ts"));

    assert_eq!(
        app.files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/app.ts", "README.md"]
    );
    assert_eq!(
        app.selected_file().map(|file| file.path.as_str()),
        Some("src/app.ts")
    );
    assert_eq!(app.hidden_file_count(), 2);
    assert!(!app.show_splash());
    assert_eq!(app.default_status_message(), "2 changed files (2 hidden)");

    app.abort_background_tasks();
}
