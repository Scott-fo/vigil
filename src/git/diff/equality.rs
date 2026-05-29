//! Cache identity and equality helpers for diff data.
//!
//! These functions mirror Pierre-style cache comparisons. Keeping them together
//! makes the rules for "same enough to reuse" visible without mixing them into
//! parsing, rendering, or repository loading code.

use std::collections::HashSet;

use serde_json::{Map as JsonMap, Value as JsonValue};

use super::{
    DiffLineAnnotation, FileContents, FileDiffMetadata, HunkData, LineAnnotation,
    LineAnnotationName, MergeConflictDiffAction, PrePropertiesConfig, RenderDiffOptions,
    RenderFileOptions, SelectedLineRange, SelectionSide, ThemeSpec, WorkerStats,
};

pub fn are_files_equal(file_a: Option<&FileContents>, file_b: Option<&FileContents>) -> bool {
    match (file_a, file_b) {
        (None, None) => true,
        (Some(file_a), Some(file_b)) => {
            file_a.cache_key == file_b.cache_key
                && file_a.contents == file_b.contents
                && file_a.name == file_b.name
                && file_a.lang == file_b.lang
        }
        _ => false,
    }
}

pub fn are_diff_targets_equal(
    diff_a: Option<&FileDiffMetadata>,
    diff_b: Option<&FileDiffMetadata>,
) -> bool {
    match (diff_a, diff_b) {
        (None, None) => true,
        (Some(diff_a), Some(diff_b)) if std::ptr::eq(diff_a, diff_b) => true,
        (Some(diff_a), Some(diff_b)) => diff_a
            .cache_key
            .as_ref()
            .is_some_and(|cache_key| Some(cache_key) == diff_b.cache_key.as_ref()),
        _ => false,
    }
}

pub fn are_selections_equal(
    selection_a: Option<&SelectedLineRange>,
    selection_b: Option<&SelectedLineRange>,
) -> bool {
    selection_a == selection_b
}

pub fn are_hunk_data_equal(hunk_a: &HunkData, hunk_b: &HunkData) -> bool {
    hunk_a == hunk_b
}

pub fn are_line_annotations_equal<T: PartialEq>(
    annotation_a: &LineAnnotation<T>,
    annotation_b: &LineAnnotation<T>,
) -> bool {
    annotation_a == annotation_b
}

pub fn are_diff_line_annotations_equal<T: PartialEq>(
    annotation_a: &DiffLineAnnotation<T>,
    annotation_b: &DiffLineAnnotation<T>,
) -> bool {
    annotation_a == annotation_b
}

pub fn get_line_annotation_name(annotation: &impl LineAnnotationName) -> String {
    let side = match annotation.annotation_side() {
        Some(SelectionSide::Deletions) => "deletions-",
        Some(SelectionSide::Additions) => "additions-",
        None => "",
    };
    format!("annotation-{side}{}", annotation.annotation_line_number())
}

pub fn are_objects_equal(
    object_a: Option<&JsonMap<String, JsonValue>>,
    object_b: Option<&JsonMap<String, JsonValue>>,
    omit_keys: &[&str],
) -> bool {
    match (object_a, object_b) {
        (None, None) => true,
        (Some(object_a), Some(object_b)) => {
            let omit_keys: HashSet<&str> = omit_keys.iter().copied().collect();
            for (key, value_a) in object_a {
                if omit_keys.contains(key.as_str()) {
                    continue;
                }
                if object_b.get(key) != Some(value_a) {
                    return false;
                }
            }
            object_b
                .keys()
                .all(|key| omit_keys.contains(key.as_str()) || object_a.contains_key(key))
        }
        _ => false,
    }
}

pub fn are_themes_equal(theme_a: Option<&ThemeSpec>, theme_b: Option<&ThemeSpec>) -> bool {
    theme_a == theme_b
}

pub fn are_pre_properties_equal(
    props_a: Option<&PrePropertiesConfig>,
    props_b: Option<&PrePropertiesConfig>,
) -> bool {
    props_a == props_b
}

pub fn are_file_render_options_equal(
    options_a: &RenderFileOptions,
    options_b: &RenderFileOptions,
) -> bool {
    are_themes_equal(Some(&options_a.theme), Some(&options_b.theme))
        && options_a.use_token_transformer == options_b.use_token_transformer
        && options_a.tokenize_max_line_length == options_b.tokenize_max_line_length
}

pub fn are_diff_render_options_equal(
    options_a: &RenderDiffOptions,
    options_b: &RenderDiffOptions,
) -> bool {
    are_themes_equal(Some(&options_a.theme), Some(&options_b.theme))
        && options_a.use_token_transformer == options_b.use_token_transformer
        && options_a.tokenize_max_line_length == options_b.tokenize_max_line_length
        && options_a.line_diff_type == options_b.line_diff_type
        && options_a.max_line_diff_length == options_b.max_line_diff_length
}

pub fn are_worker_stats_equal(
    stats_a: Option<&WorkerStats>,
    stats_b: Option<&WorkerStats>,
) -> bool {
    stats_a == stats_b
}

pub fn are_merge_conflict_actions_equal(
    action_a: &MergeConflictDiffAction,
    action_b: &MergeConflictDiffAction,
) -> bool {
    action_a.conflict_data == action_b.conflict_data
        && action_a.conflict_index == action_b.conflict_index
        && action_a.conflict == action_b.conflict
}
