//! Diff preview loading.
//!
//! This module owns the async git/filesystem work needed to build preview data
//! for working-tree, commit, and branch comparison flows. Rendering and parsing
//! remain in the parent diff module; this module only gathers the textual diff
//! and optional exact file context.

use std::{path::Path, sync::Arc};

use color_eyre::eyre::{WrapErr, eyre};
use tokio::fs;

use super::{
    DiffExactContext, DiffPreviewData, DiffView, FileContents, MergeConflictLabels,
    ParseMergeConflictDiffFromFileResult, build_diff_view_from_preview_data,
    parse_merge_conflict_diff_from_file,
};
use crate::git::{
    BranchCompareSelection, CommitCompareSelection, FileEntry, HighlightRegistry,
    command::{git_output, git_output_raw},
    parse::build_branch_diff_range,
    refs::resolve_current_branch_ref,
};

pub async fn load_diff_view(
    repo_root: &Path,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    load_diff_view_for_working_tree(repo_root, file, highlight_registry).await
}

pub async fn load_diff_view_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_working_tree(repo_root, file, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_commit_compare(repo_root, file, selection, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_branch_compare(repo_root, file, selection, true).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_preview_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_file_preview(repo_root, file, include_exact_context).await
}

pub async fn load_diff_preview_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_commit_preview(repo_root, file, selection, include_exact_context).await
}

pub async fn load_diff_preview_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_branch_preview(repo_root, file, selection, include_exact_context).await
}

pub async fn load_diff_exact_context_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
) -> color_eyre::Result<DiffExactContext> {
    if file.status == "??" {
        let new_file_lines = load_working_tree_file_lines(repo_root, &file.path).await?;
        return Ok(DiffExactContext::from_lines(None, new_file_lines));
    }

    load_exact_context(
        repo_root,
        Some(PreviewTarget::Revision("HEAD")),
        Some(PreviewTarget::WorkingTree),
        &file.path,
    )
    .await
}

pub async fn load_diff_exact_context_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<DiffExactContext> {
    load_exact_context(
        repo_root,
        Some(PreviewTarget::Revision(selection.base_ref.as_str())),
        Some(PreviewTarget::Revision(selection.commit_hash.as_str())),
        &file.path,
    )
    .await
}

pub async fn load_diff_exact_context_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<DiffExactContext> {
    let merge_base = resolve_branch_compare_base(repo_root, selection).await?;
    load_exact_context(
        repo_root,
        Some(PreviewTarget::Revision(merge_base.as_str())),
        Some(PreviewTarget::Revision(selection.source_ref.as_str())),
        &file.path,
    )
    .await
}

#[derive(Clone, Copy)]
enum PreviewTarget<'a> {
    Revision(&'a str),
    WorkingTree,
}

async fn load_file_preview(
    repo_root: &Path,
    file: &FileEntry,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    if let Some(preview) = load_merge_conflict_preview(repo_root, file).await? {
        return Ok(preview);
    }

    if file.status == "??" {
        load_untracked_preview(repo_root, &file.path, include_exact_context).await
    } else {
        load_tracked_preview(repo_root, &file.path, include_exact_context).await
    }
}

async fn load_merge_conflict_preview(
    repo_root: &Path,
    file: &FileEntry,
) -> color_eyre::Result<Option<DiffPreviewData>> {
    let full_path = repo_root.join(&file.path);
    let bytes = match fs::read(full_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.contains(&0) {
        return Ok(None);
    }
    let contents = String::from_utf8_lossy(&bytes).into_owned();
    if !(contents.contains("<<<<<<<")
        && contents.contains("=======")
        && contents.contains(">>>>>>>"))
    {
        return Ok(None);
    }

    let parsed = parse_merge_conflict_diff_from_file(
        &FileContents {
            name: file.path.clone(),
            contents,
            lang: file.filetype.map(str::to_string),
            header: None,
            cache_key: Some(format!("{}:{}:merge-conflict", file.path, file.status)),
        },
        6,
    )?;

    if parsed.actions.iter().all(Option::is_none) {
        return Ok(None);
    }

    let labels = resolve_merge_conflict_labels(repo_root, &parsed).await;

    Ok(Some(DiffPreviewData::from_merge_conflict(parsed, labels)))
}

async fn resolve_merge_conflict_labels(
    repo_root: &Path,
    parsed: &ParseMergeConflictDiffFromFileResult,
) -> MergeConflictLabels {
    let current_ref = resolve_current_branch_ref(repo_root).await.ok().flatten();
    merge_conflict_labels_from_markers(parsed, current_ref.as_deref())
}

fn merge_conflict_labels_from_markers(
    parsed: &ParseMergeConflictDiffFromFileResult,
    current_ref: Option<&str>,
) -> MergeConflictLabels {
    let marker_current = parsed
        .actions
        .iter()
        .flatten()
        .find_map(|action| marker_label(action.marker_lines.start.as_str(), "<<<<<<<"));
    let incoming = parsed
        .actions
        .iter()
        .flatten()
        .find_map(|action| marker_label(action.marker_lines.end.as_str(), ">>>>>>>"))
        .unwrap_or_else(|| "incoming".to_string());

    let current = match marker_current.as_deref() {
        Some("HEAD") => current_ref.unwrap_or("HEAD").to_string(),
        Some(label) => label.to_string(),
        None => current_ref.unwrap_or("current").to_string(),
    };

    MergeConflictLabels { current, incoming }
}

fn marker_label(line: &str, marker: &str) -> Option<String> {
    let label = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix(marker)?
        .trim();
    (!label.is_empty()).then(|| label.to_string())
}

async fn load_commit_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
            "--",
            file.path.as_str(),
        ],
        Some(PreviewTarget::Revision(selection.base_ref.as_str())),
        Some(PreviewTarget::Revision(selection.commit_hash.as_str())),
        file.path.as_str(),
        include_exact_context,
    )
    .await
}

async fn load_branch_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let diff_range = build_branch_diff_range(selection);
    let merge_base = resolve_branch_compare_base(repo_root, selection).await?;
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            diff_range.as_str(),
            "--",
            file.path.as_str(),
        ],
        Some(PreviewTarget::Revision(merge_base.as_str())),
        Some(PreviewTarget::Revision(selection.source_ref.as_str())),
        file.path.as_str(),
        include_exact_context,
    )
    .await
}

async fn resolve_branch_compare_base(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<String> {
    let merge_base = git_output(
        repo_root,
        &[
            "merge-base",
            selection.destination_ref.as_str(),
            selection.source_ref.as_str(),
        ],
    )
    .await?;
    let merge_base = merge_base.trim().to_string();
    if merge_base.is_empty() {
        return Err(eyre!(
            "failed to resolve merge base for {} and {}",
            selection.destination_ref,
            selection.source_ref
        ));
    }
    Ok(merge_base)
}

async fn load_tracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    load_revision_preview(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            "HEAD",
            "--",
            file_path,
        ],
        Some(PreviewTarget::Revision("HEAD")),
        Some(PreviewTarget::WorkingTree),
        file_path,
        include_exact_context,
    )
    .await
}

async fn load_revision_preview(
    repo_root: &Path,
    diff_args: &[&str],
    old_target: Option<PreviewTarget<'_>>,
    new_target: Option<PreviewTarget<'_>>,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let diff = git_output(repo_root, diff_args).await?;
    let old_file_lines = if include_exact_context {
        load_preview_target_lines(repo_root, old_target, file_path).await?
    } else {
        None
    };
    let new_file_lines = if include_exact_context || diff_needs_context_lines(&diff) {
        load_preview_target_lines(repo_root, new_target, file_path).await?
    } else {
        None
    };

    Ok(DiffPreviewData::from_sources(
        diff,
        None,
        old_file_lines,
        new_file_lines,
    ))
}

async fn load_exact_context(
    repo_root: &Path,
    old_target: Option<PreviewTarget<'_>>,
    new_target: Option<PreviewTarget<'_>>,
    file_path: &str,
) -> color_eyre::Result<DiffExactContext> {
    let (old_file_lines, new_file_lines) = tokio::try_join!(
        load_preview_target_lines(repo_root, old_target, file_path),
        load_preview_target_lines(repo_root, new_target, file_path)
    )?;
    Ok(DiffExactContext::from_lines(old_file_lines, new_file_lines))
}

async fn load_untracked_preview(
    repo_root: &Path,
    file_path: &str,
    include_exact_context: bool,
) -> color_eyre::Result<DiffPreviewData> {
    let full_path = repo_root.join(file_path);
    match fs::metadata(&full_path).await {
        Ok(metadata) if metadata.is_dir() => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Directory or symlinked directory; no preview available.".to_string()),
                None,
                None,
            ));
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Unable to read untracked file content.".to_string()),
                None,
                None,
            ));
        }
    };

    let bytes = match fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(DiffPreviewData::from_sources(
                String::new(),
                Some("Unable to read untracked file content.".to_string()),
                None,
                None,
            ));
        }
    };

    if bytes.contains(&0) {
        return Ok(DiffPreviewData::from_sources(
            String::new(),
            Some("Binary or non-text file; no preview available.".to_string()),
            None,
            None,
        ));
    }

    let content = String::from_utf8_lossy(&bytes);
    let diff = create_untracked_file_diff(file_path, &content);
    let needs_new_file_context = include_exact_context || diff_needs_context_lines(&diff);
    let normalized_content = Arc::<str>::from(content.replace("\r\n", "\n"));
    let new_file_lines = if needs_new_file_context {
        Some(split_lines_for_context(&content))
    } else {
        None
    };
    let new_file_source = needs_new_file_context.then_some(normalized_content.clone());
    Ok(if diff.trim().is_empty() {
        DiffPreviewData {
            diff,
            note: Some("Untracked empty file; no textual hunk to preview.".to_string()),
            old_file_source: None,
            new_file_lines: Some(split_lines_for_context(&content)),
            new_file_source: Some(normalized_content),
            merge_conflict: None,
            merge_conflict_labels: None,
        }
    } else {
        DiffPreviewData {
            diff,
            note: None,
            old_file_source: None,
            new_file_lines,
            new_file_source,
            merge_conflict: None,
            merge_conflict_labels: None,
        }
    })
}

async fn load_preview_target_lines(
    repo_root: &Path,
    target: Option<PreviewTarget<'_>>,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    match target {
        Some(PreviewTarget::Revision(revision)) => {
            load_revision_file_lines(repo_root, revision, file_path).await
        }
        Some(PreviewTarget::WorkingTree) => {
            load_working_tree_file_lines(repo_root, file_path).await
        }
        None => Ok(None),
    }
}

async fn load_working_tree_file_lines(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    let full_path = repo_root.join(file_path);
    let bytes = match fs::read(full_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    if bytes.contains(&0) {
        return Ok(None);
    }

    Ok(Some(split_lines_for_context(&String::from_utf8_lossy(
        &bytes,
    ))))
}

async fn load_revision_file_lines(
    repo_root: &Path,
    revision: &str,
    file_path: &str,
) -> color_eyre::Result<Option<Vec<String>>> {
    let spec = format!("{revision}:{file_path}");
    let output = git_output_raw(repo_root, &["show", spec.as_str()])
        .await
        .wrap_err_with(|| format!("failed to load {spec}"))?;

    if !output.status.success() {
        return Ok(None);
    }
    if output.stdout.contains(&0) {
        return Ok(None);
    }

    Ok(Some(split_lines_for_context(&String::from_utf8_lossy(
        &output.stdout,
    ))))
}

pub(super) fn split_lines_for_context(content: &str) -> Vec<String> {
    let normalized = content.replace("\r\n", "\n");
    let mut lines = normalized
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if normalized.ends_with('\n') {
        let _ = lines.pop();
    }
    lines
}

pub(super) fn diff_needs_context_lines(diff: &str) -> bool {
    let mut hunk_count = 0usize;
    for line in diff.lines() {
        if line.starts_with("@@ -") {
            hunk_count += 1;
            if hunk_count > 1 {
                return true;
            }
        }
    }
    false
}

pub(super) fn create_untracked_file_diff(input_path: &str, content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    if normalized.is_empty() {
        return String::new();
    }

    let has_trailing_newline = normalized.ends_with('\n');
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    if has_trailing_newline {
        let _ = lines.pop();
    }

    let line_count = lines.len();
    let hunk_header = format!("@@ -0,0 +1,{} @@", line_count);
    let mut diff_lines = vec![
        format!("diff --git a/{input_path} b/{input_path}"),
        "new file mode 100644".to_string(),
        "index 0000000..1111111".to_string(),
        "--- /dev/null".to_string(),
        format!("+++ b/{input_path}"),
        hunk_header,
    ];
    diff_lines.extend(lines.into_iter().map(|line| format!("+{}", line)));
    if !has_trailing_newline {
        diff_lines.push("\\ No newline at end of file".to_string());
    }
    diff_lines.push(String::new());
    diff_lines.join("\n")
}
