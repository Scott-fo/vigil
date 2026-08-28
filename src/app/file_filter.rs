//! Exclude files from the review file list by path suffix.
//!
//! Git still returns every changed file. This module owns the view filter:
//! which suffixes hide files, how a `.ts` suffix also matches `.tsx` (and
//! `.js` matches `.jsx`), and the modal that edits the list. Callers keep a
//! loaded snapshot and a visible file list; changing suffixes re-filters
//! without another git round-trip.
//!
//! A suffix matches the end of a path when the character before it is not
//! alphanumeric, so `test.ts` hides `foo.test.ts` and `src/test.ts` but not
//! `latest.ts`. A leading `*` is ignored, so `*.test.ts` is the same as
//! `.test.ts`.

mod modal;

#[cfg(test)]
mod tests;

use crate::git::FileEntry;

use super::App;

/// Path suffixes that hide files from the review file list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExcludeSuffixes(Vec<String>);

impl ExcludeSuffixes {
    pub fn from_query(query: &str) -> Self {
        Self::from_parts(query.split(|ch: char| ch == ',' || ch.is_whitespace()))
    }

    pub fn from_parts(parts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut suffixes = Vec::new();
        for part in parts {
            let normalized = normalize_suffix(part.as_ref());
            if normalized.is_empty()
                || suffixes
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(&normalized))
            {
                continue;
            }
            suffixes.push(normalized);
        }
        Self(suffixes)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_query(&self) -> String {
        self.0.join(" ")
    }

    pub fn hides(&self, path: &str) -> bool {
        self.0
            .iter()
            .any(|suffix| path_matches_exclude_suffix(path, suffix))
    }

    pub fn filter_entries(&self, files: &[FileEntry]) -> Vec<FileEntry> {
        if self.is_empty() {
            return files.to_vec();
        }

        files
            .iter()
            .filter(|file| !self.hides(&file.path))
            .cloned()
            .collect()
    }
}

impl App {
    pub(in crate::app) fn rebuild_visible_file_list(&mut self, previously_selected: Option<&str>) {
        self.files = self
            .file_exclude_suffixes
            .filter_entries(&self.loaded_files);
        self.rebuild_sidebar_items();
        self.restore_selected_file(previously_selected);
    }

    pub(in crate::app) fn hidden_file_count(&self) -> usize {
        self.loaded_files.len().saturating_sub(self.files.len())
    }

    pub(in crate::app) fn preview_hidden_file_count(&self, suffixes: &ExcludeSuffixes) -> usize {
        self.loaded_files
            .iter()
            .filter(|file| suffixes.hides(&file.path))
            .count()
    }

    pub fn file_filter_preview_message(&self) -> String {
        let suffixes = ExcludeSuffixes::from_query(&self.file_filter_query);
        let hidden_count = self.preview_hidden_file_count(&suffixes);
        if suffixes.is_empty() {
            return format!(
                "No suffixes. All {} file{} stay visible.",
                self.loaded_files.len(),
                if self.loaded_files.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }

        let remaining = self.loaded_files.len().saturating_sub(hidden_count);
        format!("{hidden_count} hidden, {remaining} visible. test.ts also matches test.tsx.")
    }

    pub(in crate::app) fn apply_file_exclude_suffixes(&mut self, suffixes: ExcludeSuffixes) {
        if suffixes == self.file_exclude_suffixes {
            return;
        }

        let previously_selected = self.selected_file().map(|file| file.path.clone());
        self.file_exclude_suffixes = suffixes;
        self.invalidate_review_snapshot();
        self.clear_review_diff_snapshot();
        self.clear_review_diff_stats();
        self.diff_cache_generation = self.diff_cache_generation.saturating_add(1);
        self.diff_view_cache.clear();
        self.diff_prefetch_direction = Default::default();
        self.diff_prefetch_anchor_file_index = None;
        self.rebuild_visible_file_list(previously_selected.as_deref());
        self.queue_review_diff_stats_load();
        self.queue_review_diff_snapshot_load();
        self.queue_diff_search_index_load();
        self.queue_selected_diff_load(true, true);
        self.status_message = Some(self.current_status_message());
        self.queue_review_restore_for_current_snapshot();
    }
}

fn normalize_suffix(value: &str) -> String {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('*')
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn path_matches_exclude_suffix(path: &str, suffix: &str) -> bool {
    path_ends_with_suffix(path, suffix)
        || companion_jsx_suffix(suffix)
            .is_some_and(|companion| path_ends_with_suffix(path, &companion))
}

fn path_ends_with_suffix(path: &str, suffix: &str) -> bool {
    if suffix.is_empty() || suffix.len() > path.len() {
        return false;
    }

    let start = path.len() - suffix.len();
    if !path.is_char_boundary(start) {
        return false;
    }
    if !path[start..].eq_ignore_ascii_case(suffix) {
        return false;
    }

    start == 0
        || suffix.starts_with('.')
        || path[..start]
            .chars()
            .next_back()
            .is_none_or(|previous| !previous.is_ascii_alphanumeric())
}

fn companion_jsx_suffix(suffix: &str) -> Option<String> {
    if let Some(stem) = strip_ascii_suffix(suffix, ".ts") {
        return Some(format!("{stem}.tsx"));
    }
    if let Some(stem) = strip_ascii_suffix(suffix, ".js") {
        return Some(format!("{stem}.jsx"));
    }
    None
}

fn strip_ascii_suffix<'a>(value: &'a str, suffix: &str) -> Option<&'a str> {
    if suffix.len() > value.len() {
        return None;
    }

    let start = value.len() - suffix.len();
    if !value.is_char_boundary(start) {
        return None;
    }
    value[start..]
        .eq_ignore_ascii_case(suffix)
        .then_some(&value[..start])
}
