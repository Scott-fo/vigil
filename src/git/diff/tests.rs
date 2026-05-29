use std::collections::HashMap;

use ratatui::text::{Line, Span};

use super::super::highlight::HighlightRegistry;
use super::full_file::{FullDiffOp, compute_full_diff_ops, split_file_contents_owned};
use super::preview::create_untracked_file_diff;
use super::rendering::expand_tabs_in_spans;
use super::resolution::{NormalizedDiffResolution, normalize_diff_resolution};
use super::view::build_merge_conflict_diff_view;
use super::*;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Text,
    widgets::{Paragraph, Widget},
};

fn render_lines_to_strings(lines: Vec<Line<'static>>, width: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, lines.len() as u16);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(Text::from(lines)).render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn verify_file_hunk_values(file: &FileDiffMetadata) -> Result<(), String> {
    let mut current_split_line_total = 0usize;
    let mut current_unified_line_total = 0usize;
    let mut last_hunk_addition_end = 0usize;

    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        let mut context_lines = 0usize;
        let mut addition_lines = 0usize;
        let mut deletion_lines = 0usize;
        let mut expected_split_line_count = 0usize;
        let mut expected_unified_line_count = 0usize;

        for content in &hunk.hunk_content {
            match content {
                HunkContent::Context { lines, .. } => {
                    context_lines += *lines;
                    expected_split_line_count += *lines;
                    expected_unified_line_count += *lines;
                }
                HunkContent::Change {
                    additions,
                    deletions,
                    ..
                } => {
                    addition_lines += *additions;
                    deletion_lines += *deletions;
                    expected_split_line_count += (*additions).max(*deletions);
                    expected_unified_line_count += *additions + *deletions;
                }
            }
        }

        let prefix = format!("hunks[{hunk_index}]");
        if hunk.addition_count != addition_lines + context_lines {
            return Err(format!(
                "{prefix}: addition_count {} != additions + context {}",
                hunk.addition_count,
                addition_lines + context_lines
            ));
        }
        if hunk.deletion_count != deletion_lines + context_lines {
            return Err(format!(
                "{prefix}: deletion_count {} != deletions + context {}",
                hunk.deletion_count,
                deletion_lines + context_lines
            ));
        }
        if hunk.addition_lines != addition_lines {
            return Err(format!(
                "{prefix}: addition_lines {} != counted additions {}",
                hunk.addition_lines, addition_lines
            ));
        }
        if hunk.deletion_lines != deletion_lines {
            return Err(format!(
                "{prefix}: deletion_lines {} != counted deletions {}",
                hunk.deletion_lines, deletion_lines
            ));
        }
        if hunk.split_line_count != expected_split_line_count {
            return Err(format!(
                "{prefix}: split_line_count {} != expected {}",
                hunk.split_line_count, expected_split_line_count
            ));
        }
        if hunk.unified_line_count != expected_unified_line_count {
            return Err(format!(
                "{prefix}: unified_line_count {} != expected {}",
                hunk.unified_line_count, expected_unified_line_count
            ));
        }

        let expected_collapsed_before = hunk
            .addition_start
            .saturating_sub(1 + last_hunk_addition_end);
        if hunk.collapsed_before != expected_collapsed_before {
            return Err(format!(
                "{prefix}: collapsed_before {} != expected {}",
                hunk.collapsed_before, expected_collapsed_before
            ));
        }
        if hunk.split_line_start != current_split_line_total + hunk.collapsed_before {
            return Err(format!(
                "{prefix}: split_line_start {} != expected {}",
                hunk.split_line_start,
                current_split_line_total + hunk.collapsed_before
            ));
        }
        if hunk.unified_line_start != current_unified_line_total + hunk.collapsed_before {
            return Err(format!(
                "{prefix}: unified_line_start {} != expected {}",
                hunk.unified_line_start,
                current_unified_line_total + hunk.collapsed_before
            ));
        }

        current_split_line_total = hunk.split_line_start + hunk.split_line_count;
        current_unified_line_total = hunk.unified_line_start + hunk.unified_line_count;
        last_hunk_addition_end = hunk
            .addition_start
            .saturating_add(hunk.addition_count)
            .saturating_sub(1);
    }

    Ok(())
}

fn apply_full_diff_ops(old_lines: &[String], new_lines: &[String], ops: &[FullDiffOp]) {
    let mut reconstructed_old = Vec::new();
    let mut reconstructed_new = Vec::new();

    for op in ops {
        match *op {
            FullDiffOp::Equal {
                old_index,
                new_index,
            } => {
                reconstructed_old.push(old_lines[old_index].clone());
                reconstructed_new.push(new_lines[new_index].clone());
            }
            FullDiffOp::Delete { old_index, .. } => {
                reconstructed_old.push(old_lines[old_index].clone());
            }
            FullDiffOp::Insert { new_index, .. } => {
                reconstructed_new.push(new_lines[new_index].clone());
            }
        }
    }

    assert_eq!(reconstructed_old, old_lines);
    assert_eq!(reconstructed_new, new_lines);
}

#[test]
fn full_diff_ops_reconstruct_repeated_lines_and_boundaries() {
    let old_lines = ["alpha\n", "repeat\n", "repeat\n", "remove\n", "tail\n"]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let new_lines = [
        "start\n", "alpha\n", "repeat\n", "insert\n", "repeat\n", "tail\n",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();

    let ops = compute_full_diff_ops(&old_lines, &new_lines, false);

    apply_full_diff_ops(&old_lines, &new_lines, &ops);
    assert_eq!(
        ops.iter()
            .filter(|op| !matches!(op, FullDiffOp::Equal { .. }))
            .count(),
        3
    );
    assert!(matches!(ops.first(), Some(FullDiffOp::Insert { .. })));
    assert!(
        ops.iter()
            .any(|op| matches!(op, FullDiffOp::Delete { old_index: 3, .. }))
    );
}

#[test]
fn parse_patch_files_matches_pierre_hunk_metadata_shape() {
    let patch = "\
From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001
Subject: parser fixture

diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@ fn main
 context
-old
+new
+added
 tail
\\ No newline at end of file
";

    let parsed = parse_patch_files(patch, Some("fixture"), true).unwrap();
    assert_eq!(parsed.len(), 1);
    assert!(
        parsed[0]
            .patch_metadata
            .as_deref()
            .unwrap()
            .contains("Subject: parser fixture")
    );

    let file = &parsed[0].files[0];
    assert_eq!(file.name, "src/main.rs");
    assert_eq!(file.change_type, ChangeType::Change);
    assert_eq!(file.prev_object_id.as_deref(), Some("1111111"));
    assert_eq!(file.new_object_id.as_deref(), Some("2222222"));
    assert_eq!(file.mode.as_deref(), Some("100644"));
    assert_eq!(file.cache_key.as_deref(), Some("fixture-0-0"));
    assert_eq!(file.deletion_lines, vec!["context\n", "old\n", "tail"]);
    assert_eq!(
        file.addition_lines,
        vec!["context\n", "new\n", "added\n", "tail"]
    );

    let hunk = &file.hunks[0];
    assert_eq!(hunk.hunk_context.as_deref(), Some("fn main"));
    assert_eq!(hunk.addition_start, 1);
    assert_eq!(hunk.addition_count, 4);
    assert_eq!(hunk.deletion_start, 1);
    assert_eq!(hunk.deletion_count, 3);
    assert_eq!(hunk.addition_lines, 2);
    assert_eq!(hunk.deletion_lines, 1);
    assert_eq!(hunk.split_line_count, 4);
    assert_eq!(hunk.unified_line_count, 5);
    assert!(hunk.no_eof_cr_additions);
    assert!(hunk.no_eof_cr_deletions);
    assert_eq!(
        hunk.hunk_content,
        vec![
            HunkContent::Context {
                lines: 1,
                addition_line_index: 0,
                deletion_line_index: 0,
            },
            HunkContent::Change {
                deletions: 1,
                deletion_line_index: 1,
                additions: 2,
                addition_line_index: 1,
            },
            HunkContent::Context {
                lines: 1,
                addition_line_index: 3,
                deletion_line_index: 2,
            },
        ]
    );
}

#[test]
fn parse_patch_files_preserves_pure_rename_metadata() {
    let patch = "\
diff --git \"a/old name.txt\" \"b/new name.txt\"
similarity index 100%
rename from old name.txt
rename to new name.txt
";

    let parsed = parse_patch_files(patch, None, true).unwrap();
    let file = &parsed[0].files[0];

    assert_eq!(file.name, "new name.txt");
    assert_eq!(file.prev_name.as_deref(), Some("old name.txt"));
    assert_eq!(file.change_type, ChangeType::RenamePure);
    assert!(file.hunks.is_empty());
}

#[test]
fn parse_patch_files_matches_pierre_file_patch_fixture_summary() {
    let patch = "\
diff --git a//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml b//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
--- a//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
+++ b//Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml
@@ -3720,32 +3720 @@
 
-
-# Codex 2025 holiday campaign
-- airflow:
-    environment: prod
-    dag:
-      start_date: \"2025-12-25T00:00:00Z\"
-      schedule: \"@daily\"
-      audience: INTERNAL_APPLIED
-      urgency: MEDIUM
-      notification:
-        email: shijie.rao@openai.com
-        pagerduty: pagerduty-chatgpt-growth-retention-oncall
-      airflow_dataset_sensors:
-        - fully_qualified_table_name: analytics.scratch.shijie_codex_2025_holiday_campaign_user_id
-  databricks_source:
-    spark_sql: |
-      SELECT DISTINCT
-        user_id
-      FROM
-        analytics.scratch.shijie_codex_2025_holiday_campaign_user_id
-  azure_blob_storage_stage:
-    storage_account: oailodestoneprod
-    container: notifications
-  rockset_sink:
-    workspace: campaigns
-    collection_alias: codex_2025_holiday
-    deployments:
-      - deployment_rrn: rrn:rsd:rs6:c74bab26-bcfd-4e9b-82f8-1417bea02b8d
-        assumed_role_rrn: rrn:role:rs6:68fb7059-b1d7-46f6-bd4e-0d11088735f9
-    shard_count_minimum: 4
-  owner: growth
";

    let parsed = parse_patch_files(patch, Some("file-patch"), true).unwrap();
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].patch_metadata.is_none());

    let file = &parsed[0].files[0];
    assert_eq!(
        file.name,
        "/Users/shijie.rao/code/openai/project/oai-airflow-shared/oai_airflow_shared/applied_data_platform/lodestone/lodestone_config_notifications.yaml"
    );
    assert_eq!(file.cache_key.as_deref(), Some("file-patch-0-0"));
    assert_eq!(file.change_type, ChangeType::Change);
    assert_eq!(file.addition_lines, vec!["\n"]);
    assert_eq!(file.deletion_lines.len(), 32);
    assert_eq!(file.deletion_lines[0], "\n");
    assert_eq!(file.deletion_lines[1], "\n");
    assert_eq!(file.deletion_lines[2], "# Codex 2025 holiday campaign\n");
    assert_eq!(file.deletion_lines[31], "  owner: growth\n");
    assert_eq!(file.split_line_count, 3751);
    assert_eq!(file.unified_line_count, 3751);

    let hunk = &file.hunks[0];
    assert_eq!(hunk.addition_start, 3720);
    assert_eq!(hunk.addition_count, 1);
    assert_eq!(hunk.addition_lines, 0);
    assert_eq!(hunk.deletion_start, 3720);
    assert_eq!(hunk.deletion_count, 32);
    assert_eq!(hunk.deletion_lines, 31);
    assert_eq!(hunk.collapsed_before, 3719);
    assert_eq!(hunk.split_line_start, 3719);
    assert_eq!(hunk.unified_line_start, 3719);
    assert_eq!(hunk.split_line_count, 32);
    assert_eq!(hunk.unified_line_count, 32);
    assert_eq!(
        hunk.hunk_content,
        vec![
            HunkContent::Context {
                lines: 1,
                addition_line_index: 0,
                deletion_line_index: 0,
            },
            HunkContent::Change {
                deletions: 31,
                deletion_line_index: 1,
                additions: 0,
                addition_line_index: 1,
            },
        ]
    );
    verify_file_hunk_values(file).unwrap();
}

#[test]
fn parse_patch_files_ignores_format_patch_version_trailer() {
    let patch = "\
From 02a2e4e6806f7e8f3adf685fde57cc773196f206 Mon Sep 17 00:00:00 2001
From: \"Patch Fixture\" <patch.fixture@example.invalid>
Date: Tue, 5 May 2026 15:45:50 -0600
Subject: [PATCH] example patch with version trailer

---
 file.txt | 1 +
 1 file changed, 1 insertion(+)

diff --git a/file.txt b/file.txt
index 626799f..8c1202a 100644
--- a/file.txt
+++ b/file.txt
@@ -1,2 +1,3 @@
 line one
+line two
 line three
-- 
2.52.0

";

    let parsed = parse_patch_files(patch, None, true).unwrap();
    let file = &parsed[0].files[0];
    let hunk = &file.hunks[0];

    assert_eq!(hunk.addition_lines, 1);
    assert_eq!(hunk.deletion_lines, 0);
    assert_eq!(
        file.addition_lines,
        vec!["line one\n", "line two\n", "line three\n"]
    );
    assert_eq!(file.deletion_lines, vec!["line one\n", "line three\n"]);
    verify_file_hunk_values(file).unwrap();
}

#[test]
fn parse_patch_files_preserves_final_blank_context_line() {
    let patch = "\
--- a/example.js
+++ b/example.js
@@ -1,4 +1,3 @@
 keep
-remove a
-remove b
+add
 
";

    let parsed = parse_patch_files(patch, None, true).unwrap();
    let file = &parsed[0].files[0];

    assert_eq!(file.addition_lines, vec!["keep\n", "add\n", "\n"]);
    assert_eq!(
        file.deletion_lines,
        vec!["keep\n", "remove a\n", "remove b\n", "\n"]
    );
    verify_file_hunk_values(file).unwrap();
}

#[test]
fn parse_patch_files_salvages_malformed_bare_newline_in_hunk() {
    let patch = "\
diff --git a/malformed.txt b/malformed.txt
index 1111111..2222222 100644
--- a/malformed.txt
+++ b/malformed.txt
@@ -1,3 +1,2 @@
-old one

 old two
+new two
";

    let parsed = parse_patch_files(patch, None, false).unwrap();
    let hunk = &parsed[0].files[0].hunks[0];

    assert_eq!(hunk.deletion_count, 3);
    assert_eq!(hunk.deletion_lines, 1);
    assert_eq!(hunk.addition_count, 2);
    assert_eq!(hunk.addition_lines, 1);
}

#[test]
fn parse_patch_files_preserves_bom_characters_in_hunk_lines() {
    let patch = [
        "diff --git a/bom.txt b/bom.txt\n",
        "index 1111111..2222222 100644\n",
        "--- a/bom.txt\n",
        "+++ b/bom.txt\n",
        "@@ -1 +1 @@\n",
        "-\u{FEFF}old\n",
        "+\u{FEFF}new\n",
    ]
    .join("");

    let parsed = parse_patch_files(&patch, None, true).unwrap();
    let file = &parsed[0].files[0];

    assert_eq!(file.deletion_lines[0], "\u{FEFF}old\n");
    assert_eq!(file.addition_lines[0], "\u{FEFF}new\n");
}

#[test]
fn parse_patch_files_preserves_quoted_git_header_backslash_escapes() {
    let old_name = "test/integration/image-optimizer/app/public/\\303\\244\\303\\266\\303\\274.png";
    let new_name = "test/e2e/image-optimizer/app/public/\\303\\244\\303\\266\\303\\274.png";
    let patch = format!(
        "diff --git \"a/{old_name}\" \"b/{new_name}\"\n\
similarity index 100%\n"
    );

    let file = process_file(&patch, None, Some(true), true)
        .unwrap()
        .unwrap();

    assert_eq!(file.name, new_name);
    assert_eq!(file.prev_name.as_deref(), Some(old_name));
    assert_eq!(file.change_type, ChangeType::RenamePure);
}

#[test]
fn parse_diff_from_file_returns_full_file_metadata_and_valid_hunks() {
    let old_file = FileContents {
        name: "example.ts".to_string(),
        contents: "one\nold\nshared\n".to_string(),
        lang: None,
        header: None,
        cache_key: Some("old-key".to_string()),
    };
    let new_file = FileContents {
        name: "example.ts".to_string(),
        contents: "one\nnew\nshared\nadded\n".to_string(),
        lang: None,
        header: None,
        cache_key: Some("new-key".to_string()),
    };

    let file = parse_diff_from_file(&old_file, &new_file, ParseDiffOptions::default());

    assert_eq!(file.name, "example.ts");
    assert_eq!(file.change_type, ChangeType::Change);
    assert!(!file.is_partial);
    assert_eq!(file.cache_key.as_deref(), Some("old-key:new-key"));
    assert_eq!(file.deletion_lines, vec!["one\n", "old\n", "shared\n"]);
    assert_eq!(
        file.addition_lines,
        vec!["one\n", "new\n", "shared\n", "added\n"]
    );
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(
        file.hunks[0].hunk_content,
        vec![
            HunkContent::Context {
                lines: 1,
                addition_line_index: 0,
                deletion_line_index: 0,
            },
            HunkContent::Change {
                deletions: 1,
                deletion_line_index: 1,
                additions: 1,
                addition_line_index: 1,
            },
            HunkContent::Context {
                lines: 1,
                addition_line_index: 2,
                deletion_line_index: 2,
            },
            HunkContent::Change {
                deletions: 0,
                deletion_line_index: 3,
                additions: 1,
                addition_line_index: 3,
            },
        ]
    );
    verify_file_hunk_values(&file).unwrap();
}

#[test]
fn parse_diff_from_file_can_ignore_whitespace_only_changes() {
    let old_file = FileContents {
        name: "test.txt".to_string(),
        contents: "hello world\nfoo bar\n".to_string(),
        lang: None,
        header: None,
        cache_key: None,
    };
    let new_file = FileContents {
        name: "test.txt".to_string(),
        contents: "  hello world\nfoo bar\n".to_string(),
        lang: None,
        header: None,
        cache_key: None,
    };

    let with_whitespace = parse_diff_from_file(&old_file, &new_file, ParseDiffOptions::default());
    assert!(!with_whitespace.hunks.is_empty());

    let without_whitespace = parse_diff_from_file(
        &old_file,
        &new_file,
        ParseDiffOptions {
            ignore_whitespace: true,
            ..ParseDiffOptions::default()
        },
    );
    assert!(without_whitespace.hunks.is_empty());
    assert_eq!(without_whitespace.change_type, ChangeType::Change);
}

#[test]
fn parse_diff_from_file_handles_unchanged_and_empty_files() {
    let unchanged = FileContents {
        name: "same.txt".to_string(),
        contents: "abc".to_string(),
        lang: None,
        header: None,
        cache_key: None,
    };

    let file = parse_diff_from_file(&unchanged, &unchanged, ParseDiffOptions::default());
    assert_eq!(file.change_type, ChangeType::Change);
    assert!(file.hunks.is_empty());
    assert_eq!(file.deletion_lines, vec!["abc"]);
    assert_eq!(file.addition_lines, vec!["abc"]);

    let empty = FileContents {
        name: "empty.txt".to_string(),
        contents: String::new(),
        lang: None,
        header: None,
        cache_key: None,
    };
    let empty_diff = parse_diff_from_file(&empty, &empty, ParseDiffOptions::default());
    assert_eq!(empty_diff.change_type, ChangeType::Change);
    assert!(empty_diff.hunks.is_empty());
    assert!(empty_diff.deletion_lines.is_empty());
    assert!(empty_diff.addition_lines.is_empty());
}

fn build_context(count: usize, label: &str) -> Vec<String> {
    (1..=count)
        .map(|index| format!(" {label}-{index}"))
        .collect()
}

fn create_resolution_fixture() -> FileDiffMetadata {
    let old_contents = [
        "line 01 stable",
        "line 02 add anchor",
        "line 03 stable",
        "line 04 stable",
        "line 05 stable",
        "line 06 delete me",
        "line 07 stable",
        "line 08 stable",
        "line 09 stable",
        "line 10 replace old",
        "line 11 stable",
        "line 12 stable",
        "line 13 stable",
        "line 14 mix old a",
        "line 15 mix shared",
        "line 16 mix old b",
        "line 17 stable",
        "",
    ]
    .join("\n");
    let new_contents = [
        "line 01 stable",
        "line 02 add anchor",
        "line 02.1 add first",
        "line 02.2 add second",
        "line 03 stable",
        "line 04 stable",
        "line 05 stable",
        "line 07 stable",
        "line 08 stable",
        "line 09 stable",
        "line 10 replace new",
        "line 11 stable",
        "line 12 stable",
        "line 13 stable",
        "line 14 mix new a",
        "line 15 mix shared",
        "line 16 mix new b",
        "line 17 stable",
        "",
    ]
    .join("\n");

    parse_diff_from_file(
        &FileContents {
            name: "example.ts".to_string(),
            contents: old_contents,
            lang: None,
            header: None,
            cache_key: Some("old-key".to_string()),
        },
        &FileContents {
            name: "example.ts".to_string(),
            contents: new_contents,
            lang: None,
            header: None,
            cache_key: Some("new-key".to_string()),
        },
        ParseDiffOptions {
            context_lines: 1,
            ..ParseDiffOptions::default()
        },
    )
}

fn hunk_lines(file: &FileDiffMetadata, hunk_index: usize) -> Vec<String> {
    let hunk = &file.hunks[hunk_index];
    file.addition_lines[hunk.addition_line_index..hunk.addition_line_index + hunk.addition_count]
        .to_vec()
}

fn expected_resolved_hunk_lines(
    file: &FileDiffMetadata,
    hunk_index: usize,
    resolution: DiffHunkResolution,
) -> Vec<String> {
    let hunk = &file.hunks[hunk_index];
    let mut lines = Vec::new();
    for content in &hunk.hunk_content {
        match *content {
            HunkContent::Context {
                lines: count,
                addition_line_index,
                ..
            } => lines.extend_from_slice(
                &file.addition_lines[addition_line_index..addition_line_index + count],
            ),
            HunkContent::Change {
                deletions,
                deletion_line_index,
                additions,
                addition_line_index,
            } => match normalize_diff_resolution(resolution) {
                NormalizedDiffResolution::Deletions => lines.extend_from_slice(
                    &file.deletion_lines[deletion_line_index..deletion_line_index + deletions],
                ),
                NormalizedDiffResolution::Additions => lines.extend_from_slice(
                    &file.addition_lines[addition_line_index..addition_line_index + additions],
                ),
                NormalizedDiffResolution::Both => {
                    lines.extend_from_slice(
                        &file.deletion_lines[deletion_line_index..deletion_line_index + deletions],
                    );
                    lines.extend_from_slice(
                        &file.addition_lines[addition_line_index..addition_line_index + additions],
                    );
                }
            },
        }
    }
    lines
}

fn assert_resolved_hunk(file: &FileDiffMetadata, hunk_index: usize, expected_lines: &[String]) {
    let hunk = &file.hunks[hunk_index];
    assert!(
        hunk.hunk_content
            .iter()
            .all(|content| matches!(content, HunkContent::Context { .. }))
    );
    assert_eq!(hunk.addition_lines, 0);
    assert_eq!(hunk.deletion_lines, 0);
    assert_eq!(hunk.addition_count, expected_lines.len());
    assert_eq!(hunk.deletion_count, expected_lines.len());
    assert_eq!(
        &file.addition_lines
            [hunk.addition_line_index..hunk.addition_line_index + expected_lines.len()],
        expected_lines
    );
    assert_eq!(
        &file.deletion_lines
            [hunk.deletion_line_index..hunk.deletion_line_index + expected_lines.len()],
        expected_lines
    );
    verify_file_hunk_values(file).unwrap();
}

fn virtual_metrics_fixture() -> VirtualFileMetrics {
    VirtualFileMetrics {
        hunk_line_count: 2,
        line_height: 10,
        diff_header_height: 30,
        spacing: 4,
        padding_top: None,
        padding_bottom: None,
        hunk_separator_height: None,
    }
}

fn create_two_hunk_diff() -> FileDiffMetadata {
    let old_lines = (1..=140).map(|index| index.to_string()).collect::<Vec<_>>();
    let new_lines = old_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 39 {
                "changed-40".to_string()
            } else if index == 99 {
                "changed-100".to_string()
            } else {
                line.clone()
            }
        })
        .collect::<Vec<_>>();

    parse_diff_from_file(
        &FileContents {
            name: "two-hunks.ts".to_string(),
            contents: format!("{}\n", old_lines.join("\n")),
            lang: None,
            header: None,
            cache_key: None,
        },
        &FileContents {
            name: "two-hunks.ts".to_string(),
            contents: format!("{}\n", new_lines.join("\n")),
            lang: None,
            header: None,
            cache_key: None,
        },
        ParseDiffOptions::default(),
    )
}

fn compute_height_for_test(
    file_diff: &FileDiffMetadata,
    options: EstimatedDiffHeightOptions<'_>,
) -> EstimatedDiffHeights {
    compute_estimated_diff_heights(file_diff, options).unwrap()
}

#[test]
fn trim_patch_context_matches_pierre_large_context_split() {
    let hunk1_before = build_context(40, "h1-before");
    let hunk1_after = build_context(40, "h1-after");
    let hunk2_before = build_context(40, "h2-before");
    let hunk2_middle = build_context(36, "h2-middle");
    let hunk2_after = build_context(40, "h2-after");

    let patch = [
        vec![
            "diff --git a/file.txt b/file.txt".to_string(),
            "--- a/file.txt".to_string(),
            "+++ b/file.txt".to_string(),
            "@@ -1,82 +1,84 @@".to_string(),
        ],
        hunk1_before.clone(),
        vec![
            "-old-1".to_string(),
            "-old-2".to_string(),
            "+new-1".to_string(),
            "+new-2".to_string(),
            "+new-3".to_string(),
            "+new-4".to_string(),
        ],
        hunk1_after.clone(),
        vec!["@@ -200,118 +200,117 @@".to_string()],
        hunk2_before.clone(),
        vec!["+only-add".to_string()],
        hunk2_middle.clone(),
        vec!["-old-3".to_string(), "-old-4".to_string()],
        hunk2_after.clone(),
    ]
    .concat()
    .join("\n");

    let expected = [
        vec![
            "diff --git a/file.txt b/file.txt".to_string(),
            "--- a/file.txt".to_string(),
            "+++ b/file.txt".to_string(),
            "@@ -31,22 +31,24 @@".to_string(),
        ],
        hunk1_before[30..].to_vec(),
        vec![
            "-old-1".to_string(),
            "-old-2".to_string(),
            "+new-1".to_string(),
            "+new-2".to_string(),
            "+new-3".to_string(),
            "+new-4".to_string(),
        ],
        hunk1_after[..10].to_vec(),
        vec!["@@ -230,20 +230,21 @@".to_string()],
        hunk2_before[30..].to_vec(),
        vec!["+only-add".to_string()],
        hunk2_middle[..10].to_vec(),
        vec!["@@ -266,22 +267,20 @@".to_string()],
        hunk2_middle[26..].to_vec(),
        vec!["-old-3".to_string(), "-old-4".to_string()],
        hunk2_after[..10].to_vec(),
    ]
    .concat()
    .join("\n");

    assert_eq!(trim_patch_context(&patch, 10), expected);
}

#[test]
fn trim_patch_context_omits_single_line_counts_and_drops_context_only_hunks() {
    let patch = [
        "diff --git a/a.txt b/a.txt",
        "--- a/a.txt",
        "+++ b/a.txt",
        "@@ -1,0 +1,1 @@",
        "+hello",
    ]
    .join("\n");

    assert_eq!(
        trim_patch_context(&patch, 0),
        [
            "diff --git a/a.txt b/a.txt",
            "--- a/a.txt",
            "+++ b/a.txt",
            "@@ -1,0 +1 @@",
            "+hello",
        ]
        .join("\n")
    );

    let context_only = [
        "diff --git a/empty.txt b/empty.txt",
        "--- a/empty.txt",
        "+++ b/empty.txt",
        "@@ -1,4 +1,4 @@",
        " one",
        " two",
        " three",
        " four",
    ]
    .join("\n");

    assert_eq!(
        trim_patch_context(&context_only, 10),
        [
            "diff --git a/empty.txt b/empty.txt",
            "--- a/empty.txt",
            "+++ b/empty.txt",
        ]
        .join("\n")
    );
}

#[test]
fn simple_diff_utils_match_pierre_edge_cases() {
    assert_eq!(clean_last_newline("alpha\n"), "alpha");
    assert_eq!(clean_last_newline("alpha\r\n"), "alpha");
    assert_eq!(clean_last_newline("alpha\r"), "alpha\r");
    assert_eq!(clean_last_newline("alpha\n\n"), "alpha\n");

    assert_eq!(get_line_ending_type("a\r\nb\n"), LineEndingType::CRLF);
    assert_eq!(get_line_ending_type("a\rb"), LineEndingType::CR);
    assert_eq!(get_line_ending_type("a\nb"), LineEndingType::LF);
    assert_eq!(get_line_ending_type("ab"), LineEndingType::None);

    assert_eq!(
        parse_line_type("+added"),
        Some(ParsedLine {
            line: "added".to_string(),
            line_type: HunkLineType::Addition,
        })
    );
    assert_eq!(
        parse_line_type("-"),
        Some(ParsedLine {
            line: "\n".to_string(),
            line_type: HunkLineType::Deletion,
        })
    );
    assert_eq!(
        parse_line_type("\\ No newline at end of file"),
        Some(ParsedLine {
            line: " No newline at end of file".to_string(),
            line_type: HunkLineType::Metadata,
        })
    );
    assert_eq!(parse_line_type("x invalid"), None);
    assert_eq!(parse_line_type(""), None);

    assert_eq!(
        get_icon_for_type(DiffIconType::File),
        "diffs-icon-file-code"
    );
    assert_eq!(
        get_icon_for_type(DiffIconType::from(ChangeType::Change)),
        "diffs-icon-symbol-modified"
    );
    assert_eq!(
        get_icon_for_type(DiffIconType::from(ChangeType::New)),
        "diffs-icon-symbol-added"
    );
    assert_eq!(
        get_icon_for_type(DiffIconType::from(ChangeType::Deleted)),
        "diffs-icon-symbol-deleted"
    );
    assert_eq!(
        get_icon_for_type(DiffIconType::from(ChangeType::RenamePure)),
        "diffs-icon-symbol-moved"
    );
    assert_eq!(
        get_icon_for_type(DiffIconType::from(ChangeType::RenameChanged)),
        "diffs-icon-symbol-moved"
    );
}

#[test]
fn untracked_file_diff_preserves_missing_final_newline_metadata() {
    let diff = create_untracked_file_diff("new/no-newline.rs", "fn added() {}");

    assert!(diff.contains("\\ No newline at end of file"));
    let parsed = parse_patch_files(&diff, None, true).unwrap();
    let file = &parsed[0].files[0];
    assert_eq!(file.name, "new/no-newline.rs");
    assert_eq!(file.change_type, ChangeType::New);
    assert_eq!(file.addition_lines, vec!["fn added() {}".to_string()]);
    assert!(file.hunks[0].no_eof_cr_additions);
    assert!(!file.hunks[0].no_eof_cr_deletions);
}

#[test]
fn untracked_file_diff_omits_missing_final_newline_metadata_when_present() {
    let diff = create_untracked_file_diff("new/with-newline.rs", "fn added() {}\n");

    assert!(!diff.contains("\\ No newline at end of file"));
    let parsed = parse_patch_files(&diff, None, true).unwrap();
    let file = &parsed[0].files[0];
    assert_eq!(file.name, "new/with-newline.rs");
    assert_eq!(file.change_type, ChangeType::New);
    assert_eq!(file.addition_lines, vec!["fn added() {}\n".to_string()]);
    assert!(!file.hunks[0].no_eof_cr_additions);
    assert!(!file.hunks[0].no_eof_cr_deletions);
}

#[test]
fn are_files_equal_matches_pierre_cache_identity() {
    let base = FileContents {
        name: "src/main.rs".to_string(),
        contents: "fn main() {}\n".to_string(),
        lang: Some("rust".to_string()),
        header: Some("header-a".to_string()),
        cache_key: Some("cache-a".to_string()),
    };
    let different_header = FileContents {
        header: Some("header-b".to_string()),
        ..base.clone()
    };
    let different_cache_key = FileContents {
        cache_key: Some("cache-b".to_string()),
        ..base.clone()
    };
    let different_lang = FileContents {
        lang: Some("typescript".to_string()),
        ..base.clone()
    };

    assert!(are_files_equal(None, None));
    assert!(!are_files_equal(Some(&base), None));
    assert!(are_files_equal(Some(&base), Some(&different_header)));
    assert!(!are_files_equal(Some(&base), Some(&different_cache_key)));
    assert!(!are_files_equal(Some(&base), Some(&different_lang)));
}

#[test]
fn data_equality_helpers_match_pierre_cache_semantics() {
    let diff_without_cache = parse_diff_from_file(
        &FileContents {
            name: "a.txt".to_string(),
            contents: "old\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        &FileContents {
            name: "a.txt".to_string(),
            contents: "new\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        ParseDiffOptions::default(),
    );
    let diff_clone_without_cache = diff_without_cache.clone();
    assert!(are_diff_targets_equal(
        Some(&diff_without_cache),
        Some(&diff_without_cache)
    ));
    assert!(!are_diff_targets_equal(
        Some(&diff_without_cache),
        Some(&diff_clone_without_cache)
    ));

    let mut diff_with_cache = diff_without_cache.clone();
    diff_with_cache.cache_key = Some("same-cache".to_string());
    let mut matching_cache = diff_clone_without_cache.clone();
    matching_cache.cache_key = Some("same-cache".to_string());
    let mut different_cache = diff_clone_without_cache.clone();
    different_cache.cache_key = Some("different-cache".to_string());

    assert!(are_diff_targets_equal(
        Some(&diff_with_cache),
        Some(&matching_cache)
    ));
    assert!(!are_diff_targets_equal(
        Some(&diff_with_cache),
        Some(&different_cache)
    ));
    assert!(are_diff_targets_equal(None, None));
    assert!(!are_diff_targets_equal(Some(&diff_with_cache), None));

    let selection = SelectedLineRange {
        start: 3,
        side: Some(SelectionSide::Deletions),
        end: 8,
        end_side: Some(SelectionSide::Additions),
    };
    let same_selection = selection;
    let shifted_selection = SelectedLineRange {
        start: 4,
        ..selection
    };
    assert!(are_selections_equal(None, None));
    assert!(are_selections_equal(
        Some(&selection),
        Some(&same_selection)
    ));
    assert!(!are_selections_equal(
        Some(&selection),
        Some(&shifted_selection)
    ));
    assert!(!are_selections_equal(Some(&selection), None));

    let hunk = HunkData {
        slot_name: "hunk-1".to_string(),
        hunk_index: 1,
        lines: 20,
        column_type: CodeColumnType::Unified,
        expandable: Some(HunkDataExpandable {
            chunked: true,
            up: false,
            down: true,
        }),
    };
    let same_hunk = hunk.clone();
    let different_expandable = HunkData {
        expandable: Some(HunkDataExpandable {
            chunked: true,
            up: true,
            down: true,
        }),
        ..hunk.clone()
    };
    assert!(are_hunk_data_equal(&hunk, &same_hunk));
    assert!(!are_hunk_data_equal(&hunk, &different_expandable));

    let conflict_result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "conflict.rs".to_string(),
            contents: [
                "before",
                "<<<<<<< HEAD",
                "ours",
                "=======",
                "theirs",
                ">>>>>>> topic",
                "after",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();
    let action = conflict_result.actions[0].as_ref().unwrap();
    let mut same_action_with_different_marker_text = action.clone();
    same_action_with_different_marker_text.marker_lines.start = "<<<<<<< other\n".to_string();
    assert!(are_merge_conflict_actions_equal(
        action,
        &same_action_with_different_marker_text
    ));

    let mut different_action = action.clone();
    different_action.conflict_data.end_content_index += 1;
    assert!(!are_merge_conflict_actions_equal(action, &different_action));
}

#[test]
fn utility_equality_helpers_match_pierre_non_dom_semantics() {
    let line_annotation = LineAnnotation {
        line_number: 12,
        metadata: Some(serde_json::json!({ "id": "a" })),
    };
    let same_line_annotation = line_annotation.clone();
    let different_line_annotation = LineAnnotation {
        metadata: Some(serde_json::json!({ "id": "b" })),
        ..line_annotation.clone()
    };
    assert!(are_line_annotations_equal(
        &line_annotation,
        &same_line_annotation
    ));
    assert!(!are_line_annotations_equal(
        &line_annotation,
        &different_line_annotation
    ));
    assert_eq!(get_line_annotation_name(&line_annotation), "annotation-12");

    let diff_annotation = DiffLineAnnotation {
        side: SelectionSide::Additions,
        line_number: 7,
        metadata: Some(serde_json::json!("meta")),
    };
    let same_diff_annotation = diff_annotation.clone();
    let different_side_annotation = DiffLineAnnotation {
        side: SelectionSide::Deletions,
        ..diff_annotation.clone()
    };
    assert!(are_diff_line_annotations_equal(
        &diff_annotation,
        &same_diff_annotation
    ));
    assert!(!are_diff_line_annotations_equal(
        &diff_annotation,
        &different_side_annotation
    ));
    assert_eq!(
        get_line_annotation_name(&diff_annotation),
        "annotation-additions-7"
    );

    let object_a = serde_json::json!({
        "theme": "ignored",
        "same": 1,
        "flag": true
    })
    .as_object()
    .unwrap()
    .clone();
    let object_b = serde_json::json!({
        "theme": "also-ignored",
        "same": 1,
        "flag": true
    })
    .as_object()
    .unwrap()
    .clone();
    let object_with_extra_key = serde_json::json!({
        "theme": "ignored",
        "same": 1,
        "flag": true,
        "extra": "nope"
    })
    .as_object()
    .unwrap()
    .clone();
    assert!(are_objects_equal(
        Some(&object_a),
        Some(&object_b),
        &["theme"]
    ));
    assert!(!are_objects_equal(
        Some(&object_a),
        Some(&object_with_extra_key),
        &["theme"]
    ));
    assert!(are_objects_equal(None, None, &[]));
    assert!(!are_objects_equal(Some(&object_a), None, &[]));

    let theme = ThemeSpec::Pair {
        dark: "pierre-dark".to_string(),
        light: "pierre-light".to_string(),
    };
    let same_theme = theme.clone();
    let different_theme = ThemeSpec::Name("pierre-dark".to_string());
    assert!(are_themes_equal(Some(&theme), Some(&same_theme)));
    assert!(!are_themes_equal(Some(&theme), Some(&different_theme)));
    assert!(are_themes_equal(None, None));

    let file_options = RenderFileOptions {
        theme: theme.clone(),
        use_token_transformer: true,
        tokenize_max_line_length: 1_000,
    };
    let same_file_options = file_options.clone();
    let different_file_options = RenderFileOptions {
        tokenize_max_line_length: 2_000,
        ..file_options.clone()
    };
    assert!(are_file_render_options_equal(
        &file_options,
        &same_file_options
    ));
    assert!(!are_file_render_options_equal(
        &file_options,
        &different_file_options
    ));

    let diff_options = RenderDiffOptions {
        theme: theme.clone(),
        use_token_transformer: true,
        tokenize_max_line_length: 1_000,
        line_diff_type: LineDiffType::WordAlt,
        max_line_diff_length: 500,
    };
    let same_diff_options = diff_options.clone();
    let different_diff_options = RenderDiffOptions {
        line_diff_type: LineDiffType::Char,
        ..diff_options.clone()
    };
    assert!(are_diff_render_options_equal(
        &diff_options,
        &same_diff_options
    ));
    assert!(!are_diff_render_options_equal(
        &diff_options,
        &different_diff_options
    ));

    let pre_props = PrePropertiesConfig {
        node_type: PreNodeType::Diff,
        diff_indicators: DiffIndicators::Bars,
        disable_background: false,
        disable_line_numbers: false,
        overflow: CodeOverflow::Scroll,
        split: true,
        total_lines: 42,
        custom_properties: Some(object_a.clone()),
    };
    let same_pre_props = pre_props.clone();
    let different_pre_props = PrePropertiesConfig {
        split: false,
        ..pre_props.clone()
    };
    assert!(are_pre_properties_equal(
        Some(&pre_props),
        Some(&same_pre_props)
    ));
    assert!(!are_pre_properties_equal(
        Some(&pre_props),
        Some(&different_pre_props)
    ));
    assert!(are_pre_properties_equal(None, None));

    let stats = WorkerStats {
        busy_workers: 1,
        diff_cache_size: 2,
        file_cache_size: 3,
        manager_state: "ready".to_string(),
        active_tasks: 4,
        queued_tasks: 5,
        theme_subscribers: 6,
        total_workers: 7,
        workers_failed: 8,
    };
    let same_stats = stats.clone();
    let different_stats = WorkerStats {
        queued_tasks: 9,
        ..stats.clone()
    };
    assert!(are_worker_stats_equal(Some(&stats), Some(&same_stats)));
    assert!(!are_worker_stats_equal(
        Some(&stats),
        Some(&different_stats)
    ));
    assert!(are_worker_stats_equal(None, None));
}

#[test]
fn merge_conflict_line_types_match_pierre_stack_parser() {
    let lines = split_file_contents_owned("const a = 1;\nconst b = 2;\n");
    assert_eq!(
        get_merge_conflict_line_types(&lines),
        vec![MergeConflictLineType::None, MergeConflictLineType::None]
    );

    let lines = split_file_contents_owned(
        &[
            "before",
            "<<<<<<< HEAD",
            "ours",
            "||||||| base",
            "base",
            "=======",
            "theirs",
            ">>>>>>> feature",
            "after",
        ]
        .join("\n"),
    );
    let result = get_merge_conflict_parse_result(&lines);
    assert_eq!(
        result.line_types,
        vec![
            MergeConflictLineType::None,
            MergeConflictLineType::MarkerStart,
            MergeConflictLineType::Current,
            MergeConflictLineType::MarkerBase,
            MergeConflictLineType::Base,
            MergeConflictLineType::MarkerSeparator,
            MergeConflictLineType::Incoming,
            MergeConflictLineType::MarkerEnd,
            MergeConflictLineType::None,
        ]
    );
    assert_eq!(
        result.regions,
        vec![MergeConflictRegion {
            conflict_index: 0,
            start_line_index: 1,
            start_line_number: 2,
            separator_line_index: 5,
            separator_line_number: 6,
            end_line_index: 7,
            end_line_number: 8,
            base_marker_line_index: Some(3),
            base_marker_line_number: Some(4),
        }]
    );
    assert_eq!(get_merge_conflict_action_line_number(&result.regions[0]), 1);

    let nested = split_file_contents_owned(
        &[
            "<<<<<<< HEAD",
            "outer ours",
            "<<<<<<< HEAD",
            "inner ours",
            "=======",
            "inner theirs",
            ">>>>>>> topic",
            "=======",
            "outer theirs",
            ">>>>>>> main",
        ]
        .join("\n"),
    );
    assert_eq!(
        get_merge_conflict_line_types(&nested),
        vec![
            MergeConflictLineType::MarkerStart,
            MergeConflictLineType::Current,
            MergeConflictLineType::MarkerStart,
            MergeConflictLineType::Current,
            MergeConflictLineType::MarkerSeparator,
            MergeConflictLineType::Incoming,
            MergeConflictLineType::MarkerEnd,
            MergeConflictLineType::MarkerSeparator,
            MergeConflictLineType::Incoming,
            MergeConflictLineType::MarkerEnd,
        ]
    );
}

#[test]
fn merge_conflict_marker_helpers_match_pierre_edges() {
    let lines = vec![
        "<<<<<<<HEAD\n".to_string(),
        "<<<<<<< HEAD\r\n".to_string(),
        "======= trailing label".to_string(),
        "=======".to_string(),
        ">>>>>>> branch\r".to_string(),
    ];
    assert_eq!(
        get_merge_conflict_line_types(&lines),
        vec![
            MergeConflictLineType::None,
            MergeConflictLineType::MarkerStart,
            MergeConflictLineType::Current,
            MergeConflictLineType::MarkerSeparator,
            MergeConflictLineType::MarkerEnd,
        ]
    );

    assert_eq!(
        get_merge_conflict_action_slot_name(MergeConflictActionSlotInput {
            hunk_index: 2,
            line_index: 17,
            conflict_index: 4,
        }),
        "merge-conflict-action-2-17-4"
    );
    assert_eq!(
        get_merge_conflict_action_line_number(&MergeConflictRegion {
            conflict_index: 0,
            start_line_index: 0,
            start_line_number: 1,
            separator_line_index: 2,
            separator_line_number: 3,
            end_line_index: 4,
            end_line_number: 5,
            base_marker_line_index: None,
            base_marker_line_number: None,
        }),
        1
    );
    assert_eq!(
        get_hunk_separator_slot_name(CodeColumnType::Unified, 3),
        "hunk-separator-unified-3"
    );
    assert_eq!(
        get_hunk_separator_slot_name(CodeColumnType::Additions, 4),
        "hunk-separator-additions-4"
    );
    assert_eq!(
        get_hunk_separator_slot_name(CodeColumnType::Deletions, 5),
        "hunk-separator-deletions-5"
    );
}

fn create_merge_conflict_resolution_fixture() -> (FileDiffMetadata, ProcessFileConflictData) {
    let hunk = Hunk {
        collapsed_before: 0,
        split_line_count: 3,
        split_line_start: 0,
        unified_line_count: 3,
        unified_line_start: 0,
        addition_count: 2,
        addition_start: 1,
        addition_lines: 2,
        addition_line_index: 0,
        deletion_count: 2,
        deletion_start: 1,
        deletion_lines: 2,
        deletion_line_index: 0,
        hunk_content: vec![
            HunkContent::Change {
                deletions: 1,
                deletion_line_index: 0,
                additions: 0,
                addition_line_index: 0,
            },
            HunkContent::Context {
                lines: 1,
                addition_line_index: 0,
                deletion_line_index: 1,
            },
            HunkContent::Change {
                deletions: 0,
                deletion_line_index: 2,
                additions: 1,
                addition_line_index: 1,
            },
        ],
        hunk_context: None,
        hunk_specs: "@@ -1,2 +1,2 @@\n".to_string(),
        no_eof_cr_additions: false,
        no_eof_cr_deletions: false,
    };

    (
        FileDiffMetadata {
            name: "conflict.txt".to_string(),
            prev_name: None,
            new_object_id: None,
            prev_object_id: None,
            mode: None,
            prev_mode: None,
            change_type: ChangeType::Change,
            hunks: vec![hunk],
            split_line_count: 3,
            unified_line_count: 3,
            is_partial: false,
            deletion_lines: vec!["ours\n".to_string(), "base\n".to_string()],
            addition_lines: vec!["base\n".to_string(), "theirs\n".to_string()],
            cache_key: Some("conflict-key".to_string()),
        },
        ProcessFileConflictData {
            hunk_index: 0,
            start_content_index: 0,
            end_content_index: 2,
            current_content_index: Some(0),
            base_content_index: Some(1),
            incoming_content_index: Some(2),
            end_marker_content_index: 2,
        },
    )
}

#[test]
fn resolve_conflict_strips_base_context_and_resolves_selected_side() {
    let (diff, conflict) = create_merge_conflict_resolution_fixture();

    let incoming = resolve_conflict(&diff, &conflict, MergeConflictResolution::Incoming)
        .expect("incoming conflict should resolve");
    assert_eq!(incoming.cache_key.as_deref(), Some("conflict-key:a-0:0-2"));
    assert_eq!(incoming.deletion_lines, vec!["theirs\n".to_string()]);
    assert_eq!(incoming.addition_lines, vec!["theirs\n".to_string()]);
    assert_eq!(
        incoming.hunks[0].hunk_content,
        vec![
            HunkContent::Context {
                lines: 0,
                deletion_line_index: 0,
                addition_line_index: 0,
            },
            HunkContent::Context {
                lines: 0,
                deletion_line_index: 0,
                addition_line_index: 0,
            },
            HunkContent::Context {
                lines: 1,
                deletion_line_index: 0,
                addition_line_index: 0,
            },
        ]
    );
    assert_eq!(incoming.hunks[0].deletion_count, 1);
    assert_eq!(incoming.hunks[0].addition_count, 1);
    assert_eq!(incoming.hunks[0].split_line_count, 1);
    assert_eq!(incoming.hunks[0].unified_line_count, 1);

    let current = resolve_conflict(&diff, &conflict, MergeConflictResolution::Current)
        .expect("current conflict should resolve");
    assert_eq!(current.cache_key.as_deref(), Some("conflict-key:d-0:0-2"));
    assert_eq!(current.deletion_lines, vec!["ours\n".to_string()]);
    assert_eq!(current.addition_lines, vec!["ours\n".to_string()]);

    let both = resolve_conflict(&diff, &conflict, MergeConflictResolution::Both)
        .expect("both conflict should resolve");
    assert_eq!(both.cache_key.as_deref(), Some("conflict-key:b-0:0-2"));
    assert_eq!(
        both.deletion_lines,
        vec!["ours\n".to_string(), "theirs\n".to_string()]
    );
    assert_eq!(
        both.addition_lines,
        vec!["ours\n".to_string(), "theirs\n".to_string()]
    );
    assert_eq!(both.hunks[0].deletion_count, 2);
    assert_eq!(both.hunks[0].addition_count, 2);
    verify_file_hunk_values(&both).unwrap();
}

#[test]
fn resolve_merge_conflict_contents_replaces_only_selected_marker_block() {
    let contents = [
        "before\n",
        "<<<<<<< HEAD\n",
        "ours\n",
        "||||||| base\n",
        "base\n",
        "=======\n",
        "theirs\n",
        ">>>>>>> feature\n",
        "middle\n",
        "<<<<<<< HEAD\n",
        "second ours\n",
        "=======\n",
        "second theirs\n",
        ">>>>>>> feature\n",
        "after\n",
    ]
    .concat();
    let parsed = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "conflict.txt".to_string(),
            contents: contents.clone(),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();
    let first = parsed.actions[0].as_ref().unwrap();
    let incoming = resolve_merge_conflict_contents(
        &contents,
        &first.conflict,
        MergeConflictResolution::Incoming,
    );

    assert_eq!(
        incoming,
        [
            "before\n",
            "theirs\n",
            "middle\n",
            "<<<<<<< HEAD\n",
            "second ours\n",
            "=======\n",
            "second theirs\n",
            ">>>>>>> feature\n",
            "after\n",
        ]
        .concat()
    );
}

#[test]
fn merge_conflict_diff_view_maps_selected_rows_to_conflict_index() {
    let parsed = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "conflict.txt".to_string(),
            contents: [
                "before\n",
                "<<<<<<< HEAD\n",
                "ours\n",
                "=======\n",
                "theirs\n",
                ">>>>>>> feature\n",
                "after\n",
            ]
            .concat(),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();
    let mut view = build_merge_conflict_diff_view(&parsed, None, None);
    let rendered = view.rendered_lines(DiffViewMode::Unified, 120).to_vec();
    let rows = render_lines_to_strings(rendered, 120);

    assert!(
        rows.iter()
            .any(|row| row.contains("1 Accept current change"))
    );
    assert!(rows.iter().any(|row| row.contains("<<<<<<< HEAD")));
    assert!(rows.iter().any(|row| row.contains("Current Change")));
    assert!(rows.iter().any(|row| row.contains("Incoming Change")));
    assert_eq!(
        view.selected_conflict_index(DiffViewMode::Unified, 120, 2),
        Some(0)
    );
    assert_eq!(
        view.selected_conflict_index(DiffViewMode::Unified, 120, 4),
        Some(0)
    );
}

#[test]
fn parse_merge_conflict_diff_from_file_creates_current_incoming_diff() {
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "session.ts".to_string(),
            contents: [
                "const start = true;",
                "<<<<<<< HEAD",
                "const ttl = 12;",
                "=======",
                "const ttl = 24;",
                ">>>>>>> feature",
                "const end = true;",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: Some("session-cache".to_string()),
        },
        6,
    )
    .unwrap();

    assert!(result.current_file.contents.contains("const ttl = 12;\n"));
    assert!(!result.current_file.contents.contains("<<<<<<< HEAD\n"));
    assert!(!result.current_file.contents.contains("const ttl = 24;\n"));
    assert!(result.incoming_file.contents.contains("const ttl = 24;\n"));
    assert!(!result.incoming_file.contents.contains("const ttl = 12;\n"));
    assert_eq!(
        result.current_file.cache_key.as_deref(),
        Some("session-cache:merge-conflict-current")
    );
    assert_eq!(
        result.incoming_file.cache_key.as_deref(),
        Some("session-cache:merge-conflict-incoming")
    );
    assert_eq!(
        result.file_diff.cache_key.as_deref(),
        Some("session-cache:merge-conflict-diff")
    );
    assert_eq!(
        result.file_diff.deletion_lines,
        split_file_contents_owned(&result.current_file.contents)
    );
    assert_eq!(
        result.file_diff.addition_lines,
        split_file_contents_owned(&result.incoming_file.contents)
    );

    let action = result.actions[0].as_ref().unwrap();
    assert_eq!(action.conflict_index, 0);
    assert_eq!(action.conflict_data.hunk_index, 0);
    assert_eq!(action.conflict_data.start_content_index, 1);
    assert_eq!(action.conflict_data.current_content_index, Some(1));
    assert_eq!(action.conflict_data.incoming_content_index, Some(1));
    assert_eq!(action.conflict_data.end_marker_content_index, 1);
    assert_eq!(action.marker_lines.start, "<<<<<<< HEAD\n");
    assert_eq!(action.marker_lines.separator, "=======\n");
    assert_eq!(action.marker_lines.end, ">>>>>>> feature\n");
    assert_eq!(
        action.conflict,
        MergeConflictRegion {
            conflict_index: 0,
            start_line_index: 1,
            start_line_number: 2,
            separator_line_index: 3,
            separator_line_number: 4,
            end_line_index: 5,
            end_line_number: 6,
            base_marker_line_index: None,
            base_marker_line_number: None,
        }
    );
    assert_eq!(result.marker_rows.len(), 3);
}

#[test]
fn parse_merge_conflict_diff_from_file_preserves_diff3_base_context() {
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "merge.ts".to_string(),
            contents: [
                "before",
                "<<<<<<< HEAD",
                "ours",
                "||||||| base",
                "base value",
                "=======",
                "theirs",
                ">>>>>>> topic",
                "after",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();

    assert!(result.current_file.contents.contains("ours\n"));
    assert!(result.current_file.contents.contains("base value\n"));
    assert!(!result.current_file.contents.contains("theirs\n"));
    assert!(result.incoming_file.contents.contains("theirs\n"));
    assert!(result.incoming_file.contents.contains("base value\n"));
    assert!(!result.incoming_file.contents.contains("ours\n"));

    let action = result.actions[0].as_ref().unwrap();
    assert_eq!(action.conflict_data.start_content_index, 1);
    assert_eq!(action.conflict_data.current_content_index, Some(1));
    assert_eq!(action.conflict_data.base_content_index, Some(2));
    assert_eq!(action.conflict_data.incoming_content_index, Some(3));
    assert_eq!(action.conflict_data.end_marker_content_index, 3);
    assert_eq!(action.marker_lines.base.as_deref(), Some("||||||| base\n"));
    assert_eq!(
        action.conflict,
        MergeConflictRegion {
            conflict_index: 0,
            start_line_index: 1,
            start_line_number: 2,
            separator_line_index: 5,
            separator_line_number: 6,
            end_line_index: 7,
            end_line_number: 8,
            base_marker_line_index: Some(3),
            base_marker_line_number: Some(4),
        }
    );
    assert_eq!(
        result
            .marker_rows
            .iter()
            .map(|row| row.row_type)
            .collect::<Vec<_>>(),
        vec![
            MergeConflictMarkerRowType::MarkerStart,
            MergeConflictMarkerRowType::MarkerBase,
            MergeConflictMarkerRowType::MarkerSeparator,
            MergeConflictMarkerRowType::MarkerEnd,
        ]
    );
}

#[test]
fn parse_merge_conflict_diff_from_file_leaves_plain_files_without_hunks() {
    let contents = "one\ntwo\nthree\n".to_string();
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "plain.ts".to_string(),
            contents: contents.clone(),
            lang: None,
            header: None,
            cache_key: Some("plain-cache".to_string()),
        },
        6,
    )
    .unwrap();

    assert_eq!(result.current_file.contents, contents);
    assert_eq!(result.incoming_file.contents, contents);
    assert!(result.file_diff.hunks.is_empty());
    assert!(result.actions.is_empty());
    assert!(result.marker_rows.is_empty());
    assert_eq!(result.file_diff.split_line_count, 0);
    assert_eq!(result.file_diff.unified_line_count, 0);
}

#[test]
fn parse_merge_conflict_diff_from_file_splits_large_context_gaps() {
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "split.ts".to_string(),
            contents: [
                "pre-0",
                "pre-1",
                "<<<<<<< A",
                "ours-1",
                "=======",
                "theirs-1",
                ">>>>>>> B",
                "gap-0",
                "gap-1",
                "gap-2",
                "gap-3",
                "<<<<<<< A",
                "ours-2",
                "=======",
                "theirs-2",
                ">>>>>>> B",
                "post-0",
                "post-1",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: None,
        },
        1,
    )
    .unwrap();

    assert_eq!(result.file_diff.hunks.len(), 2);
    assert_eq!(result.actions.len(), 2);
    assert_eq!(
        result.actions[0].as_ref().unwrap().conflict_data.hunk_index,
        0
    );
    assert_eq!(
        result.actions[1].as_ref().unwrap().conflict_data.hunk_index,
        1
    );
    assert_eq!(result.file_diff.hunks[0].collapsed_before, 1);
    assert_eq!(result.file_diff.hunks[1].collapsed_before, 2);
    assert_eq!(result.file_diff.hunks[0].addition_start, 2);
    assert_eq!(result.file_diff.hunks[1].addition_start, 7);
    assert_eq!(result.file_diff.hunks[0].hunk_content.len(), 3);
    assert_eq!(result.file_diff.hunks[1].hunk_content.len(), 3);
    assert_eq!(result.marker_rows.len(), 6);

    let first_anchor =
        get_merge_conflict_action_anchor(result.actions[0].as_ref().unwrap(), &result.file_diff);
    assert_eq!(
        first_anchor,
        Some(MergeConflictActionAnchor {
            hunk_index: 0,
            line_index: 2,
        })
    );
    let second_anchor =
        get_merge_conflict_action_anchor(result.actions[1].as_ref().unwrap(), &result.file_diff);
    assert_eq!(
        second_anchor,
        Some(MergeConflictActionAnchor {
            hunk_index: 1,
            line_index: 8,
        })
    );

    let mut missing_hunk_action = result.actions[0].as_ref().unwrap().clone();
    missing_hunk_action.conflict_data.hunk_index = 99;
    assert_eq!(
        get_merge_conflict_action_anchor(&missing_hunk_action, &result.file_diff),
        None
    );
}

#[test]
fn parse_merge_conflict_diff_from_file_anchors_empty_current_side() {
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "empty-current.ts".to_string(),
            contents: [
                "before",
                "<<<<<<< HEAD",
                "=======",
                "incoming only",
                ">>>>>>> topic",
                "after",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();

    assert_eq!(result.current_file.contents, "before\nafter\n");
    assert_eq!(
        result.incoming_file.contents,
        "before\nincoming only\nafter\n"
    );
    let action = result.actions[0].as_ref().unwrap();
    assert_eq!(action.conflict_data.start_content_index, 1);
    assert_eq!(action.conflict_data.current_content_index, Some(1));
    assert_eq!(action.conflict_data.incoming_content_index, Some(1));
    assert_eq!(action.conflict_data.end_marker_content_index, 1);

    let current = resolve_conflict(
        &result.file_diff,
        &action.conflict_data,
        MergeConflictResolution::Current,
    )
    .unwrap();
    assert_eq!(current.deletion_lines, vec!["before\n", "after\n"]);
    assert_eq!(current.addition_lines, vec!["before\n", "after\n"]);

    let incoming = resolve_conflict(
        &result.file_diff,
        &action.conflict_data,
        MergeConflictResolution::Incoming,
    )
    .unwrap();
    assert_eq!(
        incoming.deletion_lines,
        vec!["before\n", "incoming only\n", "after\n"]
    );
    assert_eq!(
        incoming.addition_lines,
        vec!["before\n", "incoming only\n", "after\n"]
    );
    assert_eq!(
        result
            .marker_rows
            .iter()
            .map(|row| row.row_type)
            .collect::<Vec<_>>(),
        vec![
            MergeConflictMarkerRowType::MarkerStart,
            MergeConflictMarkerRowType::MarkerSeparator,
            MergeConflictMarkerRowType::MarkerEnd,
        ]
    );
}

#[test]
fn parse_merge_conflict_diff_from_file_anchors_empty_incoming_side() {
    let result = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: "empty-incoming.ts".to_string(),
            contents: [
                "before",
                "<<<<<<< HEAD",
                "current only",
                "=======",
                ">>>>>>> topic",
                "after",
                "",
            ]
            .join("\n"),
            lang: None,
            header: None,
            cache_key: None,
        },
        6,
    )
    .unwrap();

    assert_eq!(
        result.current_file.contents,
        "before\ncurrent only\nafter\n"
    );
    assert_eq!(result.incoming_file.contents, "before\nafter\n");
    let action = result.actions[0].as_ref().unwrap();
    assert_eq!(action.conflict_data.start_content_index, 1);
    assert_eq!(action.conflict_data.current_content_index, Some(1));
    assert_eq!(action.conflict_data.incoming_content_index, Some(1));
    assert_eq!(action.conflict_data.end_marker_content_index, 1);

    let current = resolve_conflict(
        &result.file_diff,
        &action.conflict_data,
        MergeConflictResolution::Current,
    )
    .unwrap();
    assert_eq!(
        current.deletion_lines,
        vec!["before\n", "current only\n", "after\n"]
    );
    assert_eq!(
        current.addition_lines,
        vec!["before\n", "current only\n", "after\n"]
    );

    let incoming = resolve_conflict(
        &result.file_diff,
        &action.conflict_data,
        MergeConflictResolution::Incoming,
    )
    .unwrap();
    assert_eq!(incoming.deletion_lines, vec!["before\n", "after\n"]);
    assert_eq!(incoming.addition_lines, vec!["before\n", "after\n"]);
    assert_eq!(
        result
            .marker_rows
            .iter()
            .map(|row| row.row_type)
            .collect::<Vec<_>>(),
        vec![
            MergeConflictMarkerRowType::MarkerStart,
            MergeConflictMarkerRowType::MarkerSeparator,
            MergeConflictMarkerRowType::MarkerEnd,
        ]
    );
}

#[test]
fn get_singular_patch_requires_one_patch_with_one_file() {
    let one_file = [
        "diff --git a/a.txt b/a.txt",
        "--- a/a.txt",
        "+++ b/a.txt",
        "@@ -1 +1 @@",
        "-old",
        "+new",
    ]
    .join("\n");

    let file = get_singular_patch(&one_file).unwrap();
    assert_eq!(file.name, "a.txt");
    assert_eq!(file.hunks.len(), 1);

    let two_files = [
        one_file.as_str(),
        "diff --git a/b.txt b/b.txt",
        "--- a/b.txt",
        "+++ b/b.txt",
        "@@ -1 +1 @@",
        "-old",
        "+new",
    ]
    .join("\n");
    assert!(get_singular_patch(&two_files).is_err());

    let two_patches = [
        "From abc Mon Sep 17 00:00:00 2001",
        "diff --git a/a.txt b/a.txt",
        "--- a/a.txt",
        "+++ b/a.txt",
        "@@ -1 +1 @@",
        "-old",
        "+new",
        "From def Mon Sep 17 00:00:00 2001",
        "diff --git a/b.txt b/b.txt",
        "--- a/b.txt",
        "+++ b/b.txt",
        "@@ -1 +1 @@",
        "-old",
        "+new",
    ]
    .join("\n");
    assert!(get_singular_patch(&two_patches).is_err());
}

#[test]
fn diff_accept_reject_hunk_resolves_whole_hunks_and_reindexes_later_hunks() {
    let diff = create_resolution_fixture();
    let trailing_before = hunk_lines(&diff, 3);
    let expected_accept = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Accept);
    let expected_reject = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Reject);
    let expected_both = expected_resolved_hunk_lines(&diff, 3, DiffHunkResolution::Both);

    let accepted = diff_accept_reject_hunk(&diff, 2, DiffHunkResolution::Accept).unwrap();
    assert_eq!(
        accepted.cache_key.as_deref(),
        Some("old-key:new-key:a-2:0-2")
    );
    assert_resolved_hunk(&accepted, 2, &expected_accept);
    assert_eq!(hunk_lines(&accepted, 3), trailing_before);

    let rejected = diff_accept_reject_hunk(&diff, 2, DiffHunkResolution::Reject).unwrap();
    assert_eq!(
        rejected.cache_key.as_deref(),
        Some("old-key:new-key:d-2:0-2")
    );
    assert_resolved_hunk(&rejected, 2, &expected_reject);
    assert_eq!(hunk_lines(&rejected, 3), trailing_before);

    let both = diff_accept_reject_hunk(&diff, 3, DiffHunkResolution::Both).unwrap();
    assert_eq!(both.cache_key.as_deref(), Some("old-key:new-key:b-3:0-4"));
    assert_resolved_hunk(&both, 3, &expected_both);
}

#[test]
fn diff_accept_reject_content_resolves_one_change_block_and_updates_cache_key() {
    let diff = create_resolution_fixture();
    let expected = expected_resolved_hunk_lines(&diff, 2, DiffHunkResolution::Accept);
    let result = diff_accept_reject_content(&diff, 2, 1, DiffHunkResolution::Accept).unwrap();

    assert_eq!(result.cache_key.as_deref(), Some("old-key:new-key:a-2:1-1"));
    let hunk = &result.hunks[2];
    assert!(matches!(hunk.hunk_content[1], HunkContent::Context { .. }));
    assert_eq!(
        &result.addition_lines
            [hunk.addition_line_index..hunk.addition_line_index + hunk.addition_count],
        expected.as_slice()
    );
    verify_file_hunk_values(&result).unwrap();
}

#[test]
fn diff_accept_reject_hunk_resolves_partial_patches_without_materializing_omitted_context() {
    let patch = "\
diff --git a/index.html b/index.html
index 36c553c..711c67c 100644
--- a/index.html
+++ b/index.html
@@ -6,8 +6,9 @@
 </head>
 <body>
 <header>
-  <h1>Welcome</h1>
-  <p>Thanks for visiting</p>
+  <h1>Welcome to Our Site</h1>
+  <p>We're glad you're here</p>
+  <a href=\"/about\" class=\"btn\">Learn More</a>
 </header>
 <footer>
   <p>&copy; Acme Inc.</p>";
    let diff = parse_patch_files(patch, None, true).unwrap()[0].files[0].clone();
    let expected = expected_resolved_hunk_lines(&diff, 0, DiffHunkResolution::Accept);

    let result = diff_accept_reject_hunk(&diff, 0, DiffHunkResolution::Accept).unwrap();
    let hunk = &result.hunks[0];

    assert!(result.is_partial);
    assert_eq!(result.deletion_lines, expected);
    assert_eq!(result.addition_lines, expected);
    assert_eq!(hunk.collapsed_before, 5);
    assert_eq!(hunk.addition_start, 6);
    assert_eq!(hunk.deletion_start, 6);
    assert_eq!(hunk.addition_line_index, 0);
    assert_eq!(hunk.deletion_line_index, 0);
    assert_eq!(result.split_line_count, 14);
    assert_eq!(result.unified_line_count, 14);
    verify_file_hunk_values(&result).unwrap();
}

#[test]
fn diff_accept_reject_hunk_both_inherits_no_eof_cr_from_additions() {
    let diff = parse_diff_from_file(
        &FileContents {
            name: "example.ts".to_string(),
            contents: "start\nold\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        &FileContents {
            name: "example.ts".to_string(),
            contents: "start\nnew".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        ParseDiffOptions::default(),
    );
    let expected_lines = vec![
        "start\n".to_string(),
        "old\n".to_string(),
        "new".to_string(),
    ];

    let result = diff_accept_reject_hunk(&diff, 0, DiffHunkResolution::Both).unwrap();
    let hunk = &result.hunks[0];

    assert!(hunk.no_eof_cr_additions);
    assert!(hunk.no_eof_cr_deletions);
    assert_eq!(result.deletion_lines, expected_lines);
    assert_eq!(result.addition_lines, expected_lines);
}

#[test]
fn render_range_and_virtual_window_helpers_match_pierre() {
    let default_range = RenderRange::default();
    let bounded_range = RenderRange {
        starting_line: 12,
        total_lines: Some(30),
        buffer_before: 4,
        buffer_after: 8,
    };
    let same_bounded_range = RenderRange {
        starting_line: 12,
        total_lines: Some(30),
        buffer_before: 4,
        buffer_after: 8,
    };

    assert!(is_default_render_range(&default_range));
    assert!(!is_default_render_range(&bounded_range));
    assert!(are_render_ranges_equal(None, None));
    assert!(!are_render_ranges_equal(Some(&default_range), None));
    assert!(are_render_ranges_equal(
        Some(&bounded_range),
        Some(&same_bounded_range)
    ));
    assert!(!are_render_ranges_equal(
        Some(&default_range),
        Some(&bounded_range)
    ));

    let window = VirtualWindowSpecs {
        top: 10.0,
        bottom: 120.0,
    };
    let same_window = VirtualWindowSpecs {
        top: 10.0,
        bottom: 120.0,
    };
    let shifted_window = VirtualWindowSpecs {
        top: 11.0,
        bottom: 120.0,
    };

    assert!(are_virtual_window_specs_equal(None, None));
    assert!(!are_virtual_window_specs_equal(Some(&window), None));
    assert!(are_virtual_window_specs_equal(
        Some(&window),
        Some(&same_window)
    ));
    assert!(!are_virtual_window_specs_equal(
        Some(&window),
        Some(&shifted_window)
    ));
}

#[test]
fn create_window_from_scroll_position_matches_pierre_edge_cases() {
    assert_eq!(
        create_window_from_scroll_position(WindowFromScrollPositionOptions {
            scroll_top: 0.0,
            height: 100.0,
            scroll_height: 1000.0,
            fit_perfectly: false,
            fit_perfectly_overscroll: 0.0,
            overscroll_size: 25.0,
        }),
        VirtualWindowSpecs {
            top: 0.0,
            bottom: 125.0,
        }
    );
    assert_eq!(
        create_window_from_scroll_position(WindowFromScrollPositionOptions {
            scroll_top: 475.25,
            height: 100.0,
            scroll_height: 1000.0,
            fit_perfectly: false,
            fit_perfectly_overscroll: 0.0,
            overscroll_size: 30.0,
        }),
        VirtualWindowSpecs {
            top: 445.0,
            bottom: 606.0,
        }
    );
    assert_eq!(
        create_window_from_scroll_position(WindowFromScrollPositionOptions {
            scroll_top: 930.0,
            height: 100.0,
            scroll_height: 1000.0,
            fit_perfectly: false,
            fit_perfectly_overscroll: 0.0,
            overscroll_size: 40.0,
        }),
        VirtualWindowSpecs {
            top: 890.0,
            bottom: 1000.0,
        }
    );
    assert_eq!(
        create_window_from_scroll_position(WindowFromScrollPositionOptions {
            scroll_top: 12.5,
            height: 100.0,
            scroll_height: 90.0,
            fit_perfectly: false,
            fit_perfectly_overscroll: 0.0,
            overscroll_size: 20.0,
        }),
        VirtualWindowSpecs {
            top: 12.5,
            bottom: 140.0,
        }
    );
    assert_eq!(
        create_window_from_scroll_position(WindowFromScrollPositionOptions {
            scroll_top: 150.0,
            height: 100.0,
            scroll_height: 1000.0,
            fit_perfectly: true,
            fit_perfectly_overscroll: 15.0,
            overscroll_size: 50.0,
        }),
        VirtualWindowSpecs {
            top: 135.0,
            bottom: 280.0,
        }
    );
}

#[test]
fn get_total_line_count_from_hunks_matches_pierre() {
    assert_eq!(get_total_line_count_from_hunks(&[]), 0);

    let diff = create_two_hunk_diff();
    let last_hunk = diff.hunks.last().unwrap();
    assert_eq!(
        get_total_line_count_from_hunks(&diff.hunks),
        (last_hunk.addition_start + last_hunk.addition_count)
            .max(last_hunk.deletion_start + last_hunk.deletion_count)
    );
}

#[test]
fn virtual_diff_layout_helpers_match_pierre_separator_and_expansion_rules() {
    let metrics = virtual_metrics_fixture();
    assert_eq!(
        get_expanded_region_public(false, 10, None, 1, 1),
        ExpandedRegion {
            from_start: 0,
            from_end: 0,
            range_size: 10,
            collapsed_lines: 10,
            render_all: false,
        }
    );
    assert_eq!(
        get_expanded_region_public(false, 10, Some(ExpandedHunks::All), 1, 1),
        ExpandedRegion {
            from_start: 10,
            from_end: 0,
            range_size: 10,
            collapsed_lines: 0,
            render_all: true,
        }
    );
    assert_eq!(
        get_expanded_region_public(false, 1, None, 1, 1),
        ExpandedRegion {
            from_start: 1,
            from_end: 0,
            range_size: 1,
            collapsed_lines: 0,
            render_all: true,
        }
    );
    let mut expanded_hunks = HashMap::new();
    expanded_hunks.insert(
        1,
        HunkExpansionRegion {
            from_start: 3,
            from_end: 20,
        },
    );
    assert_eq!(
        get_expanded_region_public(
            false,
            10,
            Some(ExpandedHunks::Regions(&expanded_hunks)),
            1,
            1
        ),
        ExpandedRegion {
            from_start: 10,
            from_end: 0,
            range_size: 10,
            collapsed_lines: 0,
            render_all: true,
        }
    );
    assert_eq!(
        get_expanded_region_public(true, 10, Some(ExpandedHunks::All), 1, 1),
        ExpandedRegion {
            from_start: 0,
            from_end: 0,
            range_size: 10,
            collapsed_lines: 10,
            render_all: false,
        }
    );

    let leading_cases = [
        (HunkSeparatorKind::Simple, 0, Some("@@ -1 +1 @@"), None),
        (HunkSeparatorKind::Simple, 1, Some("@@ -1 +1 @@"), Some(4)),
        (HunkSeparatorKind::Metadata, 0, None, None),
        (
            HunkSeparatorKind::Metadata,
            0,
            Some("@@ -1 +1 @@"),
            Some(32),
        ),
        (
            HunkSeparatorKind::LineInfo,
            0,
            Some("@@ -1 +1 @@"),
            Some(36),
        ),
        (
            HunkSeparatorKind::LineInfo,
            1,
            Some("@@ -1 +1 @@"),
            Some(40),
        ),
        (
            HunkSeparatorKind::LineInfoBasic,
            0,
            Some("@@ -1 +1 @@"),
            Some(32),
        ),
        (HunkSeparatorKind::Custom, 0, Some("@@ -1 +1 @@"), Some(36)),
        (HunkSeparatorKind::Custom, 1, Some("@@ -1 +1 @@"), Some(40)),
    ];
    for (kind, hunk_index, hunk_specs, total_height) in leading_cases {
        assert_eq!(
            get_leading_hunk_separator_layout(kind, &metrics, hunk_index, hunk_specs)
                .map(|layout| layout.total_height),
            total_height
        );
    }

    let trailing_cases = [
        (HunkSeparatorKind::Simple, None),
        (HunkSeparatorKind::Metadata, None),
        (HunkSeparatorKind::LineInfo, Some(36)),
        (HunkSeparatorKind::LineInfoBasic, Some(32)),
        (HunkSeparatorKind::Custom, Some(36)),
    ];
    for (kind, total_height) in trailing_cases {
        assert_eq!(
            get_trailing_hunk_separator_layout(kind, &metrics).map(|layout| layout.total_height),
            total_height
        );
    }

    let custom_metrics = VirtualFileMetrics {
        hunk_separator_height: Some(12),
        ..metrics
    };
    assert_eq!(
        get_leading_hunk_separator_layout(
            HunkSeparatorKind::LineInfo,
            &custom_metrics,
            1,
            Some("@@ -1 +1 @@")
        )
        .map(|layout| layout.total_height),
        Some(20)
    );
}

#[test]
fn compute_estimated_diff_heights_matches_pierre_cases() {
    let metrics = virtual_metrics_fixture();
    let base_options = EstimatedDiffHeightOptions {
        metrics,
        disable_file_header: false,
        hunk_separator_kind: HunkSeparatorKind::LineInfo,
        expand_unchanged: false,
        expanded_hunks: None,
        collapsed_context_threshold: 1,
    };

    let same = parse_diff_from_file(
        &FileContents {
            name: "same.ts".to_string(),
            contents: "one\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        &FileContents {
            name: "same.ts".to_string(),
            contents: "one\n".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        ParseDiffOptions::default(),
    );
    assert_eq!(
        compute_height_for_test(
            &same,
            EstimatedDiffHeightOptions {
                metrics: VirtualFileMetrics {
                    padding_top: Some(6),
                    padding_bottom: Some(13),
                    ..metrics
                },
                ..base_options
            }
        ),
        EstimatedDiffHeights {
            split_height: 36,
            unified_height: 36,
        }
    );

    let no_newline = parse_diff_from_file(
        &FileContents {
            name: "no-newline.ts".to_string(),
            contents: "one\ntwo".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        &FileContents {
            name: "no-newline.ts".to_string(),
            contents: "one\nTWO".to_string(),
            lang: None,
            header: None,
            cache_key: None,
        },
        ParseDiffOptions::default(),
    );
    assert_eq!(
        compute_height_for_test(&no_newline, base_options),
        EstimatedDiffHeights {
            split_height: 64,
            unified_height: 84,
        }
    );

    let two_hunk = create_two_hunk_diff();
    assert_eq!(
        compute_height_for_test(&two_hunk, base_options),
        EstimatedDiffHeights {
            split_height: 326,
            unified_height: 346,
        }
    );
    assert_eq!(
        compute_height_for_test(
            &two_hunk,
            EstimatedDiffHeightOptions {
                hunk_separator_kind: HunkSeparatorKind::Simple,
                ..base_options
            }
        ),
        EstimatedDiffHeights {
            split_height: 218,
            unified_height: 238,
        }
    );
    assert_eq!(
        compute_height_for_test(
            &two_hunk,
            EstimatedDiffHeightOptions {
                expand_unchanged: true,
                ..base_options
            }
        ),
        EstimatedDiffHeights {
            split_height: 1434,
            unified_height: 1454,
        }
    );
    let mut expanded_hunks = HashMap::new();
    expanded_hunks.insert(
        0,
        HunkExpansionRegion {
            from_start: 2,
            from_end: 3,
        },
    );
    assert_eq!(
        compute_height_for_test(
            &two_hunk,
            EstimatedDiffHeightOptions {
                expanded_hunks: Some(ExpandedHunks::Regions(&expanded_hunks)),
                ..base_options
            }
        ),
        EstimatedDiffHeights {
            split_height: 376,
            unified_height: 396,
        }
    );
    let partial = FileDiffMetadata {
        is_partial: true,
        ..two_hunk.clone()
    };
    assert_eq!(
        compute_height_for_test(&partial, base_options),
        EstimatedDiffHeights {
            split_height: 290,
            unified_height: 310,
        }
    );
    assert_eq!(
        compute_height_for_test(
            &two_hunk,
            EstimatedDiffHeightOptions {
                hunk_separator_kind: HunkSeparatorKind::Metadata,
                ..base_options
            }
        ),
        EstimatedDiffHeights {
            split_height: 278,
            unified_height: 298,
        }
    );
}

#[test]
fn iterate_over_file_matches_pierre_windowing_and_last_line_behavior() {
    let lines = split_file_contents_owned("line1\nline2\nline3\n\n\n");
    let mut contents = Vec::new();
    iterate_over_file(&lines, FileIterationOptions::default(), |line| {
        contents.push((
            line.line_index,
            line.line_number,
            line.content.to_string(),
            line.is_last_line,
        ));
        false
    });

    assert_eq!(
        contents,
        vec![
            (0, 1, "line1\n".to_string(), false),
            (1, 2, "line2\n".to_string(), false),
            (2, 3, "line3\n".to_string(), false),
            (3, 4, "\n".to_string(), true),
        ]
    );

    let lines = split_file_contents_owned(
        &(0..10)
            .map(|index| format!("line{index}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let mut window = Vec::new();
    iterate_over_file(
        &lines,
        FileIterationOptions {
            starting_line: 5,
            total_lines: Some(3),
        },
        |line| {
            window.push((line.line_index, line.is_last_line));
            false
        },
    );
    assert_eq!(window, vec![(5, false), (6, false), (7, false)]);

    let mut early = Vec::new();
    iterate_over_file(&lines, FileIterationOptions::default(), |line| {
        early.push(line.line_index);
        line.line_index == 4
    });
    assert_eq!(early, vec![0, 1, 2, 3, 4]);
}

#[test]
fn collect_diff_lines_matches_pierre_iteration_shapes() {
    let old_file = FileContents {
        name: "sample.txt".to_string(),
        contents: (1..=20)
            .map(|index| {
                if index == 10 || index == 18 {
                    format!("old {index}\n")
                } else {
                    format!("line {index}\n")
                }
            })
            .collect(),
        lang: None,
        header: None,
        cache_key: None,
    };
    let new_file = FileContents {
        name: "sample.txt".to_string(),
        contents: (1..=20)
            .map(|index| {
                if index == 10 || index == 18 {
                    format!("new {index}\n")
                } else {
                    format!("line {index}\n")
                }
            })
            .collect(),
        lang: None,
        header: None,
        cache_key: None,
    };
    let diff = parse_diff_from_file(
        &old_file,
        &new_file,
        ParseDiffOptions {
            context_lines: 1,
            ..ParseDiffOptions::default()
        },
    );

    assert_eq!(diff.hunks.len(), 2);
    assert_eq!(diff.hunks[0].collapsed_before, 8);
    assert_eq!(diff.hunks[1].collapsed_before, 5);

    let unified = collect_diff_lines(
        &diff,
        DiffIterationOptions {
            diff_style: DiffStyle::Unified,
            collapsed_context_threshold: 0,
            ..DiffIterationOptions::default()
        },
    )
    .unwrap();
    assert_eq!(unified.len(), 8);
    assert_eq!(unified[0].line_type, DiffLineType::Context);
    assert_eq!(unified[0].collapsed_before, 8);
    assert_eq!(unified[0].addition_line.unwrap().line_number, 9);
    assert_eq!(unified[1].deletion_line.unwrap().line_number, 10);
    assert!(unified[1].addition_line.is_none());
    assert_eq!(unified[2].addition_line.unwrap().line_number, 10);
    assert!(unified[2].deletion_line.is_none());
    assert_eq!(unified[4].collapsed_before, 5);

    let split = collect_diff_lines(
        &diff,
        DiffIterationOptions {
            diff_style: DiffStyle::Split,
            collapsed_context_threshold: 0,
            ..DiffIterationOptions::default()
        },
    )
    .unwrap();
    assert_eq!(split.len(), 6);
    assert_eq!(split[1].deletion_line.unwrap().line_number, 10);
    assert_eq!(split[1].addition_line.unwrap().line_number, 10);
    assert_eq!(split[3].collapsed_before, 5);

    let window = collect_diff_lines(
        &diff,
        DiffIterationOptions {
            diff_style: DiffStyle::Unified,
            starting_line: 1,
            total_lines: Some(3),
            collapsed_context_threshold: 0,
            ..DiffIterationOptions::default()
        },
    )
    .unwrap();
    assert_eq!(window.len(), 3);
    assert_eq!(
        window
            .iter()
            .map(|line| line
                .addition_line
                .or(line.deletion_line)
                .unwrap()
                .unified_line_index)
            .collect::<Vec<_>>(),
        vec![9, 10, 11]
    );
}

#[test]
fn collect_diff_lines_expands_full_file_context_like_pierre() {
    let old_file = FileContents {
        name: "sample.txt".to_string(),
        contents: (1..=12).map(|index| format!("line {index}\n")).collect(),
        lang: None,
        header: None,
        cache_key: None,
    };
    let mut new_contents = (1..=12)
        .map(|index| format!("line {index}\n"))
        .collect::<String>();
    new_contents = new_contents.replace("line 8\n", "changed 8\n");
    let new_file = FileContents {
        name: "sample.txt".to_string(),
        contents: new_contents,
        lang: None,
        header: None,
        cache_key: None,
    };
    let diff = parse_diff_from_file(
        &old_file,
        &new_file,
        ParseDiffOptions {
            context_lines: 1,
            ..ParseDiffOptions::default()
        },
    );
    let mut expanded_hunks = HashMap::new();
    expanded_hunks.insert(
        0,
        HunkExpansionRegion {
            from_start: 2,
            from_end: 1,
        },
    );

    let lines = collect_diff_lines(
        &diff,
        DiffIterationOptions {
            diff_style: DiffStyle::Unified,
            expanded_hunks: Some(ExpandedHunks::Regions(&expanded_hunks)),
            collapsed_context_threshold: 1,
            ..DiffIterationOptions::default()
        },
    )
    .unwrap();

    assert_eq!(lines[0].line_type, DiffLineType::ContextExpanded);
    assert_eq!(lines[1].line_type, DiffLineType::ContextExpanded);
    assert_eq!(lines[2].line_type, DiffLineType::ContextExpanded);
    assert_eq!(lines[2].collapsed_before, 3);
    assert_eq!(lines[3].line_type, DiffLineType::Context);
    assert_eq!(lines[3].addition_line.unwrap().line_number, 7);
}

#[test]
fn unified_render_expands_tabs_before_rendering() {
    let diff = "@@ -1 +1 @@\n-\told\n+\tnew";
    let mut view = build_diff_view_from_diff_text(diff, Some("go"));
    let rendered = view.rendered_lines(DiffViewMode::Unified, 24).to_vec();
    let rows = render_lines_to_strings(rendered, 24);

    assert_eq!(rows[0], "   1 -     old          ");
    assert_eq!(rows[1], "   1 +     new          ");
}

#[test]
fn split_render_expands_tabs_on_both_sides() {
    let diff = "@@ -1 +1 @@\n-\told\n+\tnew";
    let mut view = build_diff_view_from_diff_text(diff, Some("go"));
    let rendered = view.rendered_lines(DiffViewMode::Split, 29).to_vec();
    let rows = render_lines_to_strings(rendered, 29);

    assert_eq!(rows, vec!["   1     old      1     new  "]);
}

#[test]
fn split_render_wraps_sides_and_keeps_columns_aligned() {
    let diff = "@@ -1 +1 @@\n-abcdefghijklmnop\n+xy";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));
    let rendered = view.rendered_lines(DiffViewMode::Split, 29).to_vec();
    let rows = render_lines_to_strings(rendered, 29);

    assert_eq!(
        rows,
        vec![
            "   1 abcdefg      1 xy       ",
            "     hijklmn                 ",
            "     op                      ",
        ]
    );
}

#[test]
fn split_wrapped_rows_keep_line_navigation_targets() {
    let diff = "@@ -1 +1 @@\n-abcdefghijklmnop\n+xy";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

    assert_eq!(view.display_line_count(DiffViewMode::Split, 29), 3);
    assert_eq!(
        view.selected_line_number(DiffViewMode::Split, 29, 0),
        Some(1)
    );
    assert_eq!(
        view.selected_line_number(DiffViewMode::Split, 29, 1),
        Some(1)
    );
    assert_eq!(
        view.selected_line_number(DiffViewMode::Split, 29, 2),
        Some(1)
    );
}

#[test]
fn unified_render_wraps_long_lines_with_indented_continuations() {
    let diff = "@@ -1 +1 @@\n+abcdefghijklmnop";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));
    let rendered = view.rendered_lines(DiffViewMode::Unified, 16).to_vec();
    let rows = render_lines_to_strings(rendered, 16);

    assert_eq!(rows, vec!["   1 + abcdefgh ", "       ijklmnop "]);
}

#[test]
fn unified_wrapped_rows_keep_line_navigation_targets() {
    let diff = "@@ -1 +1 @@\n+abcdefghijklmnop";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

    assert_eq!(view.display_line_count(DiffViewMode::Unified, 16), 2);
    assert_eq!(
        view.selected_line_number(DiffViewMode::Unified, 16, 0),
        Some(1)
    );
    assert_eq!(
        view.selected_line_number(DiffViewMode::Unified, 16, 1),
        Some(1)
    );
}

#[test]
fn compare_source_line_navigation_ignores_removed_only_rows() {
    let diff = "@@ -1,2 +1,2 @@\n-old\n+new\n context";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

    assert_eq!(
        view.selected_new_line_number(DiffViewMode::Unified, 80, 0),
        None
    );
    assert_eq!(
        view.selected_new_line_number(DiffViewMode::Unified, 80, 1),
        Some(1)
    );
    assert_eq!(
        view.selected_new_line_number(DiffViewMode::Split, 80, 0),
        Some(1)
    );

    let deleted_diff = "@@ -1 +0,0 @@\n-old";
    let mut deleted_view = build_diff_view_from_diff_text(deleted_diff, Some("rust"));
    assert_eq!(
        deleted_view.selected_new_line_number(DiffViewMode::Split, 80, 0),
        None
    );
}

#[test]
fn selection_hit_testing_ignores_prefix_and_targets_split_panes() {
    let diff = "@@ -1 +1 @@\n-old\n+new";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

    assert!(
        view.selection_point_at(DiffViewMode::Unified, 24, 0, 3)
            .is_none()
    );
    assert_eq!(
        view.selection_point_at(DiffViewMode::Unified, 24, 0, 8),
        Some(DiffSelectionPoint {
            display_index: 0,
            pane: DiffSelectionPane::Unified,
            column: 1,
        })
    );
    assert_eq!(
        view.selection_point_at(DiffViewMode::Split, 29, 0, 21),
        Some(DiffSelectionPoint {
            display_index: 0,
            pane: DiffSelectionPane::Right,
            column: 1,
        })
    );
}

#[test]
fn split_selection_extracts_only_selected_pane_text() {
    let diff = "\
@@ -1,2 +1,2 @@
-old_one
+new_one
-old_two
+new_two
";
    let mut view = build_diff_view_from_diff_text(diff, Some("rust"));

    let selected = view.selected_text(
        DiffViewMode::Split,
        40,
        DiffSelectionPoint {
            display_index: 0,
            pane: DiffSelectionPane::Right,
            column: 0,
        },
        DiffSelectionPoint {
            display_index: 1,
            pane: DiffSelectionPane::Right,
            column: 2,
        },
    );

    assert_eq!(selected.as_deref(), Some("new_one\nnew"));
}

#[test]
fn expanded_context_lines_render_with_exact_syntax_highlighting() {
    let mut old_file_lines = (1..=40)
        .map(|index| format!("let gap_value_{index} = old_call_{index}();"))
        .collect::<Vec<_>>();
    let mut new_file_lines = (1..=40)
        .map(|index| format!("let gap_value_{index} = new_call_{index}();"))
        .collect::<Vec<_>>();
    old_file_lines[0] = "let old_start = 0;".to_string();
    new_file_lines[0] = "let new_start = 1;".to_string();
    old_file_lines[39] = "let old_end = 0;".to_string();
    new_file_lines[39] = "let new_end = 1;".to_string();

    let diff = "\
@@ -1 +1 @@
-let old_start = 0;
+let new_start = 1;
@@ -40 +40 @@
-let old_end = 0;
+let new_end = 1;
";
    let mut view = build_diff_view_from_diff_text_with_context(
        diff,
        Some("rust"),
        Some(old_file_lines),
        Some(new_file_lines.clone()),
    );
    let registry = HighlightRegistry::new_for_filetypes(["rust"])
        .expect("highlight registry should initialize");
    view.apply_exact_syntax_highlighting(Some("rust"), &registry);

    let gap_index = (0..view.display_line_count(DiffViewMode::Unified, 120))
        .find(|index| {
            matches!(
                view.selected_gap_action(DiffViewMode::Unified, 120, *index),
                Some((_, GapExpandDirection::Up))
            )
        })
        .expect("expected expandable gap");
    let expanded_gap_index = view.expand_selected_gap(DiffViewMode::Unified, 120, gap_index, 1);
    assert!(
        expanded_gap_index > 0,
        "expanded line should precede the gap control"
    );

    let rendered = view.rendered_lines(DiffViewMode::Unified, 120);
    let expanded_line = &rendered[expanded_gap_index - 1];
    let target_text = new_file_lines[1].as_str();

    assert!(
        expanded_line
            .spans
            .iter()
            .skip(2)
            .any(|span| span.content.as_ref() == "let"),
        "expected tokenized syntax spans for expanded context line `{target_text}`, got {expanded_line:?}"
    );
    assert!(
        expanded_line
            .spans
            .iter()
            .skip(2)
            .all(|span| span.content.as_ref() != target_text),
        "expanded context line should not render as a single fallback span: {expanded_line:?}"
    );
}

#[test]
fn tab_expansion_tracks_columns_across_spans() {
    let spans = expand_tabs_in_spans(vec![Span::raw("ab"), Span::raw("\t"), Span::raw("cd")]);

    let contents = spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>();

    assert_eq!(contents, vec!["ab", "  ", "cd"]);
}
