use std::{
    collections::HashMap,
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use color_eyre::eyre::{WrapErr, eyre};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use tokio::{fs, process::Command};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};
use unicode_width::UnicodeWidthStr;

use crate::{app::DiffViewMode, ui};

pub type SharedHighlightRegistry = Arc<HighlightRegistry>;
const LOG_FIELD_SEPARATOR: char = '\u{001f}';
const LOG_RECORD_SEPARATOR: char = '\u{001e}';
pub const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub status: String,
    pub path: String,
    pub label: String,
    pub filetype: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CommitSearchEntry {
    pub hash: String,
    pub short_hash: String,
    pub parent_hashes: Vec<String>,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct CommitCompareSelection {
    pub base_ref: String,
    pub commit_hash: String,
    pub short_hash: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct BlameTarget {
    pub file_path: String,
    pub line_number: usize,
}

#[derive(Debug, Clone)]
pub struct BlameCommitDetails {
    pub target: BlameTarget,
    pub commit_hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
    pub description: String,
    pub is_uncommitted: bool,
    pub compare_selection: Option<CommitCompareSelection>,
}

#[derive(Debug, Clone)]
pub struct BranchCompareSelection {
    pub source_ref: String,
    pub destination_ref: String,
}

#[derive(Debug, Default)]
pub struct DiffView {
    rows: Vec<DiffRow>,
    pub note: Option<String>,
    hunks: Vec<DiffHunkBlock>,
    gaps: Vec<DiffHunkGap>,
    gap_expansions: HashMap<usize, DiffGapExpansion>,
    file_lines: Option<Vec<String>>,
    highlighted_file_lines: Option<Vec<Vec<Span<'static>>>>,
    display_cache: DiffDisplayCache,
}

impl DiffView {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            rows: Vec::new(),
            note: Some(message.into()),
            hunks: Vec::new(),
            gaps: Vec::new(),
            gap_expansions: HashMap::new(),
            file_lines: None,
            highlighted_file_lines: None,
            display_cache: DiffDisplayCache::default(),
        }
    }

    pub fn rendered_lines(&mut self, mode: DiffViewMode, width: usize) -> &[Line<'static>] {
        self.ensure_display_cache(mode, width);
        &self.display_cache.entry(mode).lines
    }

    pub fn first_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode)
            .iter()
            .position(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn last_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode)
            .iter()
            .rposition(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn move_selection(&mut self, mode: DiffViewMode, current: usize, delta: i32) -> usize {
        let nav = self.nav_targets(mode);
        if nav.is_empty() {
            return 0;
        }

        let mut index = current.min(nav.len().saturating_sub(1));
        if nav[index].is_none() {
            index = nav.iter().position(Option::is_some).unwrap_or(0);
        }

        if delta > 0 {
            for _ in 0..delta {
                let mut probe = index.saturating_add(1);
                while probe < nav.len() && nav[probe].is_none() {
                    probe += 1;
                }
                if probe < nav.len() {
                    index = probe;
                }
            }
        } else if delta < 0 {
            for _ in 0..delta.unsigned_abs() {
                let mut probe = index.saturating_sub(1);
                while probe > 0 && nav[probe].is_none() {
                    probe = probe.saturating_sub(1);
                }
                if nav[probe].is_some() {
                    index = probe;
                }
            }
        }

        index
    }

    pub fn selected_line_number(&mut self, mode: DiffViewMode, index: usize) -> Option<usize> {
        match self.nav_targets(mode).get(index).copied().flatten() {
            Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
            _ => None,
        }
    }

    pub fn selected_gap_index(&mut self, mode: DiffViewMode, index: usize) -> Option<usize> {
        self.selected_gap_action(mode, index)
            .map(|(gap_index, _)| gap_index)
    }

    pub fn selected_gap_action(
        &mut self,
        mode: DiffViewMode,
        index: usize,
    ) -> Option<(usize, GapExpandDirection)> {
        match self.nav_targets(mode).get(index).copied().flatten() {
            Some(DisplayNavTarget::Gap(gap_index, direction)) => Some((gap_index, direction)),
            _ => None,
        }
    }

    pub fn display_line_count(&mut self, mode: DiffViewMode) -> usize {
        self.nav_targets(mode).len()
    }

    pub fn expand_selected_gap(
        &mut self,
        mode: DiffViewMode,
        index: usize,
        amount: usize,
    ) -> usize {
        let Some((gap_index, direction)) = self.selected_gap_action(mode, index) else {
            return index;
        };
        let _ = self.expand_gap(gap_index, direction, amount);
        self.nav_targets(mode)
            .iter()
            .position(|target| {
                matches!(
                    target,
                    Some(DisplayNavTarget::Gap(candidate, candidate_direction))
                        if *candidate == gap_index && *candidate_direction == direction
                )
            })
            .unwrap_or(index.min(self.nav_targets(mode).len().saturating_sub(1)))
    }

    fn expand_gap(
        &mut self,
        gap_index: usize,
        direction: GapExpandDirection,
        amount: usize,
    ) -> bool {
        let Some(gap) = self.gaps.iter().find(|gap| gap.gap_index == gap_index) else {
            return false;
        };

        let expansion = self.gap_expansions.entry(gap_index).or_default();
        let remaining = gap
            .new_count
            .saturating_sub(expansion.from_previous + expansion.from_next);
        if remaining == 0 {
            return false;
        }

        let applied = amount.max(1).min(remaining);
        match direction {
            GapExpandDirection::Up => expansion.from_previous += applied,
            GapExpandDirection::Down => expansion.from_next += applied,
        }
        self.invalidate_display_cache();
        true
    }

    fn ensure_display_cache(&mut self, mode: DiffViewMode, width: usize) {
        let cache_is_stale = {
            let cache = self.display_cache.entry(mode);
            !cache.valid || cache.width != width
        };

        if !cache_is_stale {
            return;
        }

        let (lines, nav) = if self.rows.is_empty() {
            (
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))],
                vec![None],
            )
        } else {
            match mode {
                DiffViewMode::Unified => self.build_unified_display(width),
                DiffViewMode::Split => self.build_split_display(width),
            }
        };

        let cache = self.display_cache.entry_mut(mode);
        cache.width = width;
        cache.lines = lines;
        cache.nav = nav;
        cache.valid = true;
    }

    fn nav_targets(&mut self, mode: DiffViewMode) -> &[Option<DisplayNavTarget>] {
        self.ensure_display_cache(mode, 0);
        &self.display_cache.entry(mode).nav
    }

    fn build_unified_display(
        &self,
        width: usize,
    ) -> (Vec<Line<'static>>, Vec<Option<DisplayNavTarget>>) {
        let mut lines = Vec::new();
        let mut nav = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for row in &self.rows[hunk.row_start..hunk.row_end] {
                let line_number = match row.kind {
                    DiffLineKind::Added | DiffLineKind::Context => row.new_line,
                    DiffLineKind::Removed => row.old_line,
                };
                lines.push(render_unified_code_line(row, width));
                nav.push(line_number.map(DisplayNavTarget::Line));
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(&mut lines, &mut nav, gap, width, false);
            }
        }

        (lines, nav)
    }

    fn build_split_display(
        &self,
        width: usize,
    ) -> (Vec<Line<'static>>, Vec<Option<DisplayNavTarget>>) {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        let mut lines = Vec::new();
        let mut nav = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for (line, target_line) in
                render_split_hunk_rows(&self.rows[hunk.row_start..hunk.row_end], side_width)
            {
                lines.push(line);
                nav.push(target_line.map(DisplayNavTarget::Line));
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(&mut lines, &mut nav, gap, total_width, true);
            }
        }

        (lines, nav)
    }

    fn push_gap_display_rows(
        &self,
        lines: &mut Vec<Line<'static>>,
        nav: &mut Vec<Option<DisplayNavTarget>>,
        gap: &DiffHunkGap,
        width: usize,
        split: bool,
    ) {
        let expansion = self
            .gap_expansions
            .get(&gap.gap_index)
            .copied()
            .unwrap_or_default();
        let context_after_count = expansion.from_previous.min(gap.new_count);
        let remaining_after_previous = gap.new_count.saturating_sub(context_after_count);
        let context_before_count = expansion.from_next.min(remaining_after_previous);

        if let Some(file_lines) = self.file_lines.as_ref() {
            let start = gap.new_start.saturating_sub(1);
            for offset in 0..context_after_count {
                let line_number = gap.new_start + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                lines.push(render_expanded_context_line(
                    line_number,
                    &text,
                    self.highlighted_file_lines
                        .as_ref()
                        .and_then(|highlighted| highlighted.get(start + offset))
                        .cloned(),
                    width,
                    split,
                ));
                nav.push(Some(DisplayNavTarget::Line(line_number)));
            }
        }

        let remaining = gap
            .new_count
            .saturating_sub(context_after_count + context_before_count);
        if remaining > 0 {
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_previous > 0,
                GapExpandDirection::Up,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Up,
            )));
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_next > 0,
                GapExpandDirection::Down,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Down,
            )));
        }

        if let Some(file_lines) = self.file_lines.as_ref() {
            let start = gap
                .new_start
                .saturating_sub(1)
                .saturating_add(gap.new_count.saturating_sub(context_before_count));
            for offset in 0..context_before_count {
                let line_number = gap.new_start + gap.new_count - context_before_count + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                lines.push(render_expanded_context_line(
                    line_number,
                    &text,
                    self.highlighted_file_lines
                        .as_ref()
                        .and_then(|highlighted| highlighted.get(start + offset))
                        .cloned(),
                    width,
                    split,
                ));
                nav.push(Some(DisplayNavTarget::Line(line_number)));
            }
        }
    }

    fn invalidate_display_cache(&mut self) {
        self.display_cache = DiffDisplayCache::default();
    }
}

#[derive(Debug, Default)]
struct DiffDisplayCache {
    unified: CachedDisplay,
    split: CachedDisplay,
}

impl DiffDisplayCache {
    fn entry(&self, mode: DiffViewMode) -> &CachedDisplay {
        match mode {
            DiffViewMode::Unified => &self.unified,
            DiffViewMode::Split => &self.split,
        }
    }

    fn entry_mut(&mut self, mode: DiffViewMode) -> &mut CachedDisplay {
        match mode {
            DiffViewMode::Unified => &mut self.unified,
            DiffViewMode::Split => &mut self.split,
        }
    }
}

#[derive(Debug, Default)]
struct CachedDisplay {
    width: usize,
    lines: Vec<Line<'static>>,
    nav: Vec<Option<DisplayNavTarget>>,
    valid: bool,
}

#[derive(Debug, Clone, Copy)]
enum DisplayNavTarget {
    Line(usize),
    Gap(usize, GapExpandDirection),
}

#[derive(Debug, Clone)]
struct StatusEntry {
    status: String,
    path: String,
    original_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone)]
struct DiffRow {
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: Vec<Span<'static>>,
}

#[derive(Debug, Clone)]
struct DiffHunkBlock {
    new_start: usize,
    new_count: usize,
    row_start: usize,
    row_end: usize,
}

#[derive(Debug, Clone)]
struct DiffHunkGap {
    gap_index: usize,
    new_start: usize,
    new_count: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct DiffGapExpansion {
    from_previous: usize,
    from_next: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapExpandDirection {
    Up,
    Down,
}

static HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "comment.documentation",
    "constructor",
    "constructor.builtin",
    "function",
    "function.builtin",
    "keyword",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.escape",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

pub fn is_file_staged(status: &str) -> bool {
    if status == "??" {
        return false;
    }

    let index_status = status.chars().next().unwrap_or(' ');
    index_status != ' ' && index_status != '?'
}

pub fn status_color(status: &str) -> ratatui::style::Color {
    let palette = crate::theme::active_palette();

    if status == "??" || status.contains('A') {
        return palette.diff_highlight_added;
    }
    if status.contains('U') || status.contains('D') {
        return palette.diff_highlight_removed;
    }
    if status.contains('R') || status.contains('C') {
        return palette.secondary;
    }
    if status.contains('M') {
        return palette.warning;
    }
    palette.text_muted
}

pub async fn toggle_file_stage(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    let args: Vec<&str> = if is_file_staged(&file.status) {
        vec!["restore", "--staged", "--", file.path.as_str()]
    } else {
        vec!["add", "--", file.path.as_str()]
    };

    let _ = git_output(repo_root, &args).await?;
    Ok(())
}

pub async fn discard_file_changes(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    let args: Vec<&str> = if file.status == "??" {
        vec!["clean", "-f", "--", file.path.as_str()]
    } else {
        vec![
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
            file.path.as_str(),
        ]
    };

    let _ = git_output(repo_root, &args).await?;
    Ok(())
}

pub async fn commit_staged_changes(repo_root: &Path, message: &str) -> color_eyre::Result<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(eyre!("Commit message is required."));
    }

    let _ = git_output(repo_root, &["commit", "-m", trimmed]).await?;
    Ok(())
}

pub async fn push_to_remote(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["push"]).await?;
    Ok(())
}

pub async fn pull_from_remote(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["pull"]).await?;
    Ok(())
}

pub async fn init_repo(repo_root: &Path) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, &["init"]).await?;
    Ok(())
}

pub async fn list_searchable_commits(
    repo_root: &Path,
    limit: usize,
) -> color_eyre::Result<Vec<CommitSearchEntry>> {
    let output = git_output(
        repo_root,
        &[
            "log",
            &format!("--max-count={}", limit.max(1)),
            "--date=short",
            "--pretty=format:%H%x1f%P%x1f%h%x1f%ad%x1f%an%x1f%s%x1e",
        ],
    )
    .await?;

    Ok(parse_commit_log_entries(&output))
}

pub fn resolve_commit_base_ref(commit: &CommitSearchEntry) -> String {
    commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string())
}

pub async fn load_blame_commit_details(
    repo_root: &Path,
    target: &BlameTarget,
) -> color_eyre::Result<BlameCommitDetails> {
    let blame_output = git_output(
        repo_root,
        &[
            "blame",
            "--porcelain",
            "-L",
            &format!("{0},{0}", target.line_number),
            "--",
            target.file_path.as_str(),
        ],
    )
    .await?;

    let header = parse_blame_porcelain_header(&blame_output).ok_or_else(|| {
        eyre!(
            "unable to parse blame output for {}:{}",
            target.file_path,
            target.line_number
        )
    })?;

    if is_uncommitted_blame_hash(&header.commit_hash) {
        return Ok(BlameCommitDetails {
            target: target.clone(),
            commit_hash: header.commit_hash,
            short_hash: "working-tree".to_string(),
            author: if header.author.is_empty() {
                "Uncommitted".to_string()
            } else {
                header.author
            },
            date: header.date,
            subject: if header.summary.is_empty() {
                "Uncommitted line changes".to_string()
            } else {
                header.summary
            },
            description: "This line has uncommitted changes. Commit comparison is unavailable."
                .to_string(),
            is_uncommitted: true,
            compare_selection: None,
        });
    }

    let show_output = git_output(
        repo_root,
        &[
            "show",
            "-s",
            "--date=short",
            "--format=%H%x1f%h%x1f%P%x1f%ad%x1f%an%x1f%s%x1f%b",
            header.commit_hash.as_str(),
        ],
    )
    .await?;

    let commit = parse_commit_show_output(&show_output)
        .ok_or_else(|| eyre!("unable to parse commit metadata for {}", header.commit_hash))?;
    let subject = if commit.subject.is_empty() {
        header.summary
    } else {
        commit.subject
    };
    let description = if commit.description.trim().is_empty() {
        "No commit description.".to_string()
    } else {
        commit.description
    };
    let compare_base = commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string());
    let commit_hash = commit.commit_hash;
    let short_hash = commit.short_hash;

    Ok(BlameCommitDetails {
        target: target.clone(),
        commit_hash: commit_hash.clone(),
        short_hash: short_hash.clone(),
        author: if commit.author.is_empty() {
            header.author
        } else {
            commit.author
        },
        date: if commit.date.is_empty() {
            header.date
        } else {
            commit.date
        },
        description,
        is_uncommitted: false,
        compare_selection: Some(CommitCompareSelection {
            base_ref: compare_base,
            commit_hash: commit_hash.clone(),
            short_hash: short_hash.clone(),
            subject: subject.clone(),
        }),
        subject,
    })
}

pub async fn load_files_with_commit_diff(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await?;

    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}

pub async fn list_comparable_refs(repo_root: &Path) -> color_eyre::Result<Vec<String>> {
    let output = git_output(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)\t%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .await?;

    let mut refs = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (full_ref, short_ref) = line.split_once('\t')?;
            let short_ref = short_ref.trim();
            if short_ref.is_empty() || short_ref == "HEAD" {
                return None;
            }
            if full_ref.starts_with("refs/remotes/")
                && (!short_ref.contains('/') || short_ref.ends_with("/HEAD"))
            {
                return None;
            }
            Some(short_ref.to_string())
        })
        .collect::<Vec<_>>();

    refs.sort();
    refs.dedup();
    Ok(refs)
}

pub async fn load_files_with_branch_diff(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            build_branch_diff_range(selection).as_str(),
        ],
    )
    .await?;

    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}

pub async fn should_refresh_for_paths(
    repo_root: &Path,
    changed_paths: &[PathBuf],
) -> color_eyre::Result<bool> {
    if changed_paths.is_empty() {
        return Ok(true);
    }

    let mut candidate_paths = Vec::new();
    let mut seen_paths = HashSet::new();

    for path in changed_paths {
        let Ok(relative_path) = path.strip_prefix(repo_root) else {
            return Ok(true);
        };

        if relative_path.as_os_str().is_empty() {
            return Ok(true);
        }

        if relative_path
            .file_name()
            .is_some_and(|file_name| file_name == ".gitignore")
        {
            return Ok(true);
        }

        let relative = relative_path.to_string_lossy().replace('\\', "/");
        if seen_paths.insert(relative.clone()) {
            candidate_paths.push(relative);
        }
    }

    if candidate_paths.is_empty() {
        return Ok(false);
    }

    let ignored_paths = git_check_ignored(repo_root, &candidate_paths).await?;
    Ok(candidate_paths
        .iter()
        .any(|path| !ignored_paths.contains(path)))
}

pub async fn resolve_repo_root() -> color_eyre::Result<PathBuf> {
    resolve_repo_root_from(Path::new(".")).await
}

pub async fn resolve_repo_root_from(probe_path: &Path) -> color_eyre::Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(probe_path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .wrap_err("failed to resolve git repository root")?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

pub async fn load_files_with_status(repo_root: &Path) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut files = Vec::new();

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        files.push(to_file_entry(entry));
    }

    Ok(files)
}

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
    let preview = load_diff_preview_for_working_tree(repo_root, file).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_commit_compare(repo_root, file, selection).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_view_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_diff_preview_for_branch_compare(repo_root, file, selection).await?;
    build_diff_view_from_preview_data(&preview, file, highlight_registry)
}

pub async fn load_diff_preview_for_working_tree(
    repo_root: &Path,
    file: &FileEntry,
) -> color_eyre::Result<DiffPreviewData> {
    load_file_preview(repo_root, file).await
}

pub async fn load_diff_preview_for_commit_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<DiffPreviewData> {
    load_commit_preview(repo_root, file, selection).await
}

pub async fn load_diff_preview_for_branch_compare(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<DiffPreviewData> {
    load_branch_preview(repo_root, file, selection).await
}

pub fn build_diff_view_from_preview_data(
    preview: &DiffPreviewData,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    if preview.diff.trim().is_empty() {
        let message = preview
            .note
            .clone()
            .unwrap_or_else(|| "No textual diff available.".to_string());
        return Ok(DiffView::empty(message));
    }

    let mut highlighter = SyntaxHighlighter::new(highlight_registry);
    let (rows, hunks, gaps) = build_diff_rows(&preview.diff, file.filetype, &mut highlighter);
    let highlighted_file_lines = preview.context_lines.as_ref().map(|lines| {
        lines
            .iter()
            .map(|line| highlighter.highlight_line(file.filetype, line, ui::diff_context_style()))
            .collect::<Vec<_>>()
    });
    Ok(DiffView {
        rows,
        note: preview.note.clone(),
        hunks,
        gaps,
        gap_expansions: HashMap::new(),
        file_lines: preview.context_lines.clone(),
        highlighted_file_lines,
        display_cache: DiffDisplayCache::default(),
    })
}

#[derive(Debug, Clone)]
pub struct DiffPreviewData {
    diff: String,
    note: Option<String>,
    context_lines: Option<Vec<String>>,
}

async fn load_file_preview(
    repo_root: &Path,
    file: &FileEntry,
) -> color_eyre::Result<DiffPreviewData> {
    if file.status == "??" {
        load_untracked_preview(repo_root, &file.path).await
    } else {
        load_tracked_preview(repo_root, &file.path).await
    }
}

async fn load_commit_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<DiffPreviewData> {
    let output = git_output(
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
    )
    .await?;
    let context_lines = if diff_needs_context_lines(&output) {
        load_revision_file_lines(
            repo_root,
            selection.commit_hash.as_str(),
            file.path.as_str(),
        )
        .await?
    } else {
        None
    };

    Ok(DiffPreviewData {
        diff: output,
        note: None,
        context_lines,
    })
}

async fn load_branch_preview(
    repo_root: &Path,
    file: &FileEntry,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<DiffPreviewData> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            build_branch_diff_range(selection).as_str(),
            "--",
            file.path.as_str(),
        ],
    )
    .await?;
    let context_lines = if diff_needs_context_lines(&output) {
        load_revision_file_lines(repo_root, selection.source_ref.as_str(), file.path.as_str())
            .await?
    } else {
        None
    };

    Ok(DiffPreviewData {
        diff: output,
        note: None,
        context_lines,
    })
}

async fn load_tracked_preview(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<DiffPreviewData> {
    let output = git_output(
        repo_root,
        &[
            "diff",
            "--no-color",
            "--find-renames",
            "HEAD",
            "--",
            file_path,
        ],
    )
    .await?;
    let context_lines = if diff_needs_context_lines(&output) {
        load_working_tree_file_lines(repo_root, file_path).await?
    } else {
        None
    };
    Ok(DiffPreviewData {
        diff: output,
        note: None,
        context_lines,
    })
}

async fn load_untracked_preview(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<DiffPreviewData> {
    let full_path = repo_root.join(file_path);
    match fs::metadata(&full_path).await {
        Ok(metadata) if metadata.is_dir() => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Directory or symlinked directory; no preview available.".to_string()),
                context_lines: None,
            });
        }
        Ok(_) => {}
        Err(_) => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Unable to read untracked file content.".to_string()),
                context_lines: None,
            });
        }
    }

    let bytes = match fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(DiffPreviewData {
                diff: String::new(),
                note: Some("Unable to read untracked file content.".to_string()),
                context_lines: None,
            });
        }
    };

    if bytes.contains(&0) {
        return Ok(DiffPreviewData {
            diff: String::new(),
            note: Some("Binary or non-text file; no preview available.".to_string()),
            context_lines: None,
        });
    }

    let content = String::from_utf8_lossy(&bytes);
    let diff = create_untracked_file_diff(file_path, &content);
    let context_lines = if diff_needs_context_lines(&diff) {
        Some(split_lines_for_context(&content))
    } else {
        None
    };
    Ok(if diff.trim().is_empty() {
        DiffPreviewData {
            diff,
            note: Some("Untracked empty file; no textual hunk to preview.".to_string()),
            context_lines: Some(split_lines_for_context(&content)),
        }
    } else {
        DiffPreviewData {
            diff,
            note: None,
            context_lines,
        }
    })
}

async fn is_directory_status_entry(repo_root: &Path, path: &str) -> bool {
    match fs::metadata(repo_root.join(path)).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
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
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show", spec.as_str()])
        .output()
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

fn split_lines_for_context(content: &str) -> Vec<String> {
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

fn diff_needs_context_lines(diff: &str) -> bool {
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

pub async fn git_output(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .await
        .wrap_err_with(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git_check_ignored(
    repo_root: &Path,
    paths: &[String],
) -> color_eyre::Result<HashSet<String>> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root);
    command.args(["check-ignore", "-z", "--stdin"]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .wrap_err("failed to spawn git check-ignore")?;

    if let Some(mut stdin) = child.stdin.take() {
        let input = format!("{}\0", paths.join("\0"));
        tokio::io::AsyncWriteExt::write_all(&mut stdin, input.as_bytes())
            .await
            .wrap_err("failed to write git check-ignore stdin")?;
    }

    let output = child
        .wait_with_output()
        .await
        .wrap_err("failed to wait for git check-ignore")?;

    match output.status.code() {
        Some(0) | Some(1) => {}
        _ => {
            return Err(eyre!(
                "{}",
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            ));
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn create_untracked_file_diff(input_path: &str, content: &str) -> String {
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
    let mut body = lines
        .into_iter()
        .map(|line| format!("+{}", line))
        .collect::<Vec<_>>()
        .join("\n");

    if line_count > 0 && has_trailing_newline {
        body.push('\n');
    }

    [
        format!("diff --git a/{input_path} b/{input_path}"),
        "new file mode 100644".to_string(),
        "index 0000000..1111111".to_string(),
        "--- /dev/null".to_string(),
        format!("+++ b/{input_path}"),
        hunk_header,
        body,
        String::new(),
    ]
    .join("\n")
}

#[derive(Debug)]
struct ParsedBlameHeader {
    commit_hash: String,
    author: String,
    date: String,
    summary: String,
}

#[derive(Debug)]
struct ParsedCommitShow {
    commit_hash: String,
    short_hash: String,
    parent_hashes: Vec<String>,
    date: String,
    author: String,
    subject: String,
    description: String,
}

fn parse_blame_porcelain_header(raw: &str) -> Option<ParsedBlameHeader> {
    let mut lines = raw.lines();
    let first_line = lines.next()?.trim();
    let commit_hash = first_line.split_whitespace().next()?.trim();
    if commit_hash.len() != 40 {
        return None;
    }

    let mut author = String::new();
    let mut date = String::new();
    let mut summary = String::new();

    for line in lines {
        if line.starts_with('\t') {
            break;
        }
        if let Some(value) = line.strip_prefix("author ") {
            author = value.trim().to_string();
            continue;
        }
        if let Some(value) = line.strip_prefix("author-time ") {
            date = format_unix_date(value.trim());
            continue;
        }
        if let Some(value) = line.strip_prefix("summary ") {
            summary = value.trim().to_string();
        }
    }

    Some(ParsedBlameHeader {
        commit_hash: commit_hash.to_string(),
        author,
        date,
        summary,
    })
}

fn parse_commit_show_output(raw: &str) -> Option<ParsedCommitShow> {
    let mut fields = raw.split(LOG_FIELD_SEPARATOR);
    let commit_hash = fields.next()?.trim();
    let short_hash = fields.next()?.trim();
    let parents_raw = fields.next().unwrap_or("").trim();
    let date = fields.next().unwrap_or("").trim();
    let author = fields.next().unwrap_or("").trim();
    let subject = fields.next().unwrap_or("").trim();
    let description = fields.next().unwrap_or("").trim_end();

    if commit_hash.is_empty() || short_hash.is_empty() {
        return None;
    }

    Some(ParsedCommitShow {
        commit_hash: commit_hash.to_string(),
        short_hash: short_hash.to_string(),
        parent_hashes: parents_raw
            .split_whitespace()
            .map(str::trim)
            .filter(|parent| !parent.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        date: date.to_string(),
        author: author.to_string(),
        subject: subject.to_string(),
        description: description.to_string(),
    })
}

fn is_uncommitted_blame_hash(hash: &str) -> bool {
    hash.trim() == "0000000000000000000000000000000000000000"
}

fn format_unix_date(raw_seconds: &str) -> String {
    match raw_seconds.parse::<u64>() {
        Ok(seconds) if seconds > 0 => String::new(),
        _ => String::new(),
    }
}

fn parse_commit_log_entries(raw: &str) -> Vec<CommitSearchEntry> {
    raw.split(LOG_RECORD_SEPARATOR)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let mut fields = record.split(LOG_FIELD_SEPARATOR);
            let hash = fields.next()?.trim();
            let parents_raw = fields.next().unwrap_or("").trim();
            let short_hash = fields.next().unwrap_or("").trim();
            let date = fields.next().unwrap_or("").trim();
            let author = fields.next().unwrap_or("").trim();
            let subject = fields.next().unwrap_or("").trim();

            if hash.is_empty() || short_hash.is_empty() {
                return None;
            }

            Some(CommitSearchEntry {
                hash: hash.to_string(),
                short_hash: short_hash.to_string(),
                parent_hashes: parents_raw
                    .split_whitespace()
                    .map(str::trim)
                    .filter(|parent| !parent.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
                author: author.to_string(),
                date: date.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
}

fn parse_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let fields: Vec<&str> = raw.split('\0').collect();
    let mut index = 0;

    while index < fields.len() {
        let field = fields[index];
        index += 1;

        if field.len() < 4 {
            continue;
        }

        let x = field.chars().next().unwrap_or(' ');
        let y = field.chars().nth(1).unwrap_or(' ');
        let status = to_status_pair(x, y);
        let first_path = field[3..].to_string();

        if first_path.is_empty() {
            continue;
        }

        if matches!(x, 'R' | 'C') {
            let renamed_to = fields.get(index).copied().unwrap_or_default().to_string();
            index += 1;
            entries.push(StatusEntry {
                status,
                path: if renamed_to.is_empty() {
                    first_path.clone()
                } else {
                    renamed_to
                },
                original_path: Some(first_path),
            });
            continue;
        }

        entries.push(StatusEntry {
            status,
            path: first_path,
            original_path: None,
        });
    }

    entries
}

fn build_branch_diff_range(selection: &BranchCompareSelection) -> String {
    format!(
        "{}...{}",
        selection.destination_ref.trim(),
        selection.source_ref.trim()
    )
}

fn parse_diff_name_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let fields: Vec<&str> = raw.split('\0').collect();
    let mut index = 0;

    while index < fields.len() {
        let status_field = fields[index].trim();
        index += 1;

        if status_field.is_empty() {
            continue;
        }

        let status_code = status_field.chars().next().unwrap_or(' ');
        match status_code {
            'R' | 'C' => {
                let original_path = fields.get(index).copied().unwrap_or_default().to_string();
                let path = fields
                    .get(index + 1)
                    .copied()
                    .unwrap_or_default()
                    .to_string();
                index += 2;

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path,
                    original_path: (!original_path.is_empty()).then_some(original_path),
                });
            }
            _ => {
                let path = fields.get(index).copied().unwrap_or_default().to_string();
                index += 1;

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path,
                    original_path: None,
                });
            }
        }
    }

    entries
}

fn to_status_pair(index_code: char, worktree_code: char) -> String {
    if index_code == '?' && worktree_code == '?' {
        return "??".to_string();
    }
    if index_code == '!' && worktree_code == '!' {
        return "!!".to_string();
    }
    format!("{index_code}{worktree_code}")
}

fn to_file_entry(entry: StatusEntry) -> FileEntry {
    let label = entry
        .original_path
        .as_ref()
        .map(|from| format!("{from} -> {}", entry.path))
        .unwrap_or_else(|| entry.path.clone());

    FileEntry {
        status: entry.status,
        filetype: resolve_diff_filetype(&entry.path),
        path: entry.path,
        label,
    }
}

fn resolve_diff_filetype(path: &str) -> Option<&'static str> {
    let file_name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let extension = file_name.rsplit('.').next().unwrap_or("");

    match file_name.as_str() {
        "dockerfile" => None,
        "justfile" => Some("bash"),
        "cargo.toml" => Some("toml"),
        _ => match extension {
            "rs" => Some("rust"),
            "js" | "mjs" | "cjs" => Some("javascript"),
            "jsx" => Some("jsx"),
            "ts" | "mts" | "cts" => Some("typescript"),
            "tsx" => Some("tsx"),
            "py" => Some("python"),
            "go" => Some("go"),
            "c" | "h" => Some("c"),
            "cc" | "cp" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some("cpp"),
            "cs" => Some("csharp"),
            "sh" | "bash" | "zsh" | "ksh" => Some("bash"),
            "java" => Some("java"),
            "rb" => Some("ruby"),
            "php" | "php3" | "php4" | "php5" | "phtml" => Some("php"),
            "scala" | "sc" => Some("scala"),
            "html" | "htm" => Some("html"),
            "json" => Some("json"),
            "yaml" | "yml" => Some("yaml"),
            "hs" => Some("haskell"),
            "css" => Some("css"),
            "nix" => Some("nix"),
            "md" | "mdx" | "markdown" => Some("markdown"),
            _ => None,
        },
    }
}

#[derive(Debug, Clone)]
struct ParsedHunkHeader {
    old_start: usize,
    new_start: usize,
    new_count: usize,
}

fn build_diff_rows(
    diff: &str,
    filetype: Option<&'static str>,
    highlighter: &mut SyntaxHighlighter,
) -> (Vec<DiffRow>, Vec<DiffHunkBlock>, Vec<DiffHunkGap>) {
    let normalized = diff.replace("\r\n", "\n");
    let mut rows = Vec::new();
    let mut hunks = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut in_hunk = false;
    let mut hunk_index = 0usize;
    let mut current_hunk_header: Option<ParsedHunkHeader> = None;
    let mut current_row_start = 0usize;

    for raw_line in normalized.split('\n') {
        if raw_line.is_empty() && rows.is_empty() {
            continue;
        }

        if let Some(header) = parse_hunk_header(raw_line) {
            if let Some(previous_header) = current_hunk_header.take() {
                hunks.push(DiffHunkBlock {
                    new_start: previous_header.new_start,
                    new_count: previous_header.new_count,
                    row_start: current_row_start,
                    row_end: rows.len(),
                });
            }
            current_row_start = rows.len();
            old_line = header.old_start;
            new_line = header.new_start;
            in_hunk = true;
            hunk_index = hunk_index.saturating_add(1);
            current_hunk_header = Some(header);
            continue;
        }

        if !in_hunk {
            continue;
        }

        if raw_line.starts_with("\\ ") {
            continue;
        }

        let marker = raw_line.chars().next().unwrap_or(' ');
        let content = raw_line.get(1..).unwrap_or("");

        match marker {
            '+' => {
                rows.push(render_diff_row(
                    None,
                    Some(new_line),
                    content,
                    DiffLineKind::Added,
                    filetype,
                    highlighter,
                ));
                new_line += 1;
            }
            '-' => {
                rows.push(render_diff_row(
                    Some(old_line),
                    None,
                    content,
                    DiffLineKind::Removed,
                    filetype,
                    highlighter,
                ));
                old_line += 1;
            }
            ' ' => {
                rows.push(render_diff_row(
                    Some(old_line),
                    Some(new_line),
                    content,
                    DiffLineKind::Context,
                    filetype,
                    highlighter,
                ));
                old_line += 1;
                new_line += 1;
            }
            _ => {}
        }
    }

    if let Some(previous_header) = current_hunk_header.take() {
        hunks.push(DiffHunkBlock {
            new_start: previous_header.new_start,
            new_count: previous_header.new_count,
            row_start: current_row_start,
            row_end: rows.len(),
        });
    }

    let mut gaps = Vec::new();
    for (gap_index, pair) in hunks.windows(2).enumerate() {
        let previous = &pair[0];
        let next = &pair[1];
        let new_start = previous.new_start.saturating_add(previous.new_count);
        let new_count = next.new_start.saturating_sub(new_start);
        if new_count == 0 {
            continue;
        }
        gaps.push(DiffHunkGap {
            gap_index,
            new_start,
            new_count,
        });
    }

    (rows, hunks, gaps)
}

fn parse_hunk_header(raw_line: &str) -> Option<ParsedHunkHeader> {
    if !raw_line.starts_with("@@ -") {
        return None;
    }

    let remainder = raw_line.strip_prefix("@@ -")?;
    let (old_part, rest) = remainder.split_once(" +")?;
    let (new_part, _) = rest.split_once(" @@")?;

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part.split(',').next()?.parse::<usize>().ok()?;
    let new_count = new_part
        .split_once(',')
        .and_then(|(_, count)| count.parse::<usize>().ok())
        .unwrap_or(1);

    Some(ParsedHunkHeader {
        old_start,
        new_start,
        new_count,
    })
}

fn render_diff_row(
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: &str,
    kind: DiffLineKind,
    filetype: Option<&'static str>,
    highlighter: &mut SyntaxHighlighter,
) -> DiffRow {
    DiffRow {
        kind,
        old_line,
        new_line,
        content: highlighter.highlight_line(filetype, content, base_style(kind)),
    }
}

fn resolve_split_target_line(left: Option<&DiffRow>, right: Option<&DiffRow>) -> Option<usize> {
    right
        .and_then(|row| row.new_line)
        .or_else(|| left.and_then(|row| row.old_line))
}

fn render_unified_code_line(row: &DiffRow, width: usize) -> Line<'static> {
    let base_style = base_style(row.kind);
    let sign_style = match row.kind {
        DiffLineKind::Context => ui::context_sign_style(),
        DiffLineKind::Added => ui::added_sign_style(),
        DiffLineKind::Removed => ui::removed_sign_style(),
    };
    let marker = match row.kind {
        DiffLineKind::Context => ' ',
        DiffLineKind::Added => '+',
        DiffLineKind::Removed => '-',
    };
    let unified_line_number = match row.kind {
        DiffLineKind::Added | DiffLineKind::Context => row.new_line,
        DiffLineKind::Removed => row.old_line,
    };

    let mut spans = vec![
        Span::styled(
            format_line_number(unified_line_number),
            base_style.patch(ui::line_number_style()),
        ),
        Span::styled(format!("{marker} "), sign_style),
    ];
    spans.extend(row.content.clone());
    let padded = fit_spans_to_width(spans, width.saturating_sub(1), base_style);
    Line::from(padded).style(base_style)
}

fn render_split_pair_line(
    left: Option<&DiffRow>,
    right: Option<&DiffRow>,
    side_width: usize,
) -> Line<'static> {
    let gap = Span::styled("   ", ui::diff_context_style());
    let mut spans = Vec::new();
    spans.extend(render_split_side(left, true, side_width));
    spans.push(gap);
    spans.extend(render_split_side(right, false, side_width));
    Line::from(spans)
}

fn render_split_hunk_rows(
    rows: &[DiffRow],
    side_width: usize,
) -> Vec<(Line<'static>, Option<usize>)> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<&DiffRow> = Vec::new();
    let mut pending_added: Vec<&DiffRow> = Vec::new();

    let flush_pending = |rendered: &mut Vec<(Line<'static>, Option<usize>)>,
                         removed: &mut Vec<&DiffRow>,
                         added: &mut Vec<&DiffRow>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            rendered.push((
                render_split_pair_line(left, right, side_width),
                resolve_split_target_line(left, right),
            ));
        }
        removed.clear();
        added.clear();
    };

    for row in rows {
        match row.kind {
            DiffLineKind::Removed => pending_removed.push(row),
            DiffLineKind::Added => pending_added.push(row),
            DiffLineKind::Context => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                rendered.push((
                    render_split_pair_line(Some(row), Some(row), side_width),
                    resolve_split_target_line(Some(row), Some(row)),
                ));
            }
        }
    }

    flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
    rendered
}

fn render_expand_gap_line(
    width: usize,
    _remaining: usize,
    _has_expansion: bool,
    direction: GapExpandDirection,
) -> Line<'static> {
    let hint_style = ui::diff_hunk_style();
    let action_style = ui::diff_hunk_style().add_modifier(Modifier::BOLD);
    let label = match direction {
        GapExpandDirection::Down => "↑↑",
        GapExpandDirection::Up => "↓↓",
    };
    let side_padding = 1;
    let trailing_padding = width
        .saturating_sub(side_padding)
        .saturating_sub(label.width());
    let mut spans = vec![
        Span::styled(" ".repeat(side_padding), hint_style),
        Span::styled(label.to_string(), action_style),
        Span::styled(" ".repeat(trailing_padding), hint_style),
    ];
    spans = fit_spans_to_width(spans, width.max(1), hint_style);
    Line::from(spans).style(ui::diff_hunk_style())
}

fn render_expanded_context_line(
    line_number: usize,
    text: &str,
    highlighted_content: Option<Vec<Span<'static>>>,
    width: usize,
    split: bool,
) -> Line<'static> {
    let row = DiffRow {
        kind: DiffLineKind::Context,
        old_line: Some(line_number),
        new_line: Some(line_number),
        content: highlighted_content
            .unwrap_or_else(|| vec![Span::styled(text.to_string(), ui::diff_context_style())]),
    };
    if split {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        render_split_pair_line(Some(&row), Some(&row), side_width)
    } else {
        render_unified_code_line(&row, width)
    }
}

fn render_split_side(row: Option<&DiffRow>, left_side: bool, width: usize) -> Vec<Span<'static>> {
    let Some(row) = row else {
        return vec![Span::styled(" ".repeat(width), ui::diff_context_style())];
    };

    let line_number = if left_side {
        row.old_line
    } else {
        row.new_line
    };
    let base_style = base_style(row.kind);
    let mut spans = vec![Span::styled(
        format_line_number(line_number),
        base_style.patch(ui::line_number_style()),
    )];
    spans.extend(row.content.clone());
    fit_spans_to_width(spans, width, base_style)
}

fn fit_spans_to_width(
    spans: Vec<Span<'static>>,
    width: usize,
    pad_style: Style,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let mut fitted = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        if current_width >= width {
            break;
        }

        let content = span.content.as_ref();
        let remaining = width.saturating_sub(current_width);
        let content_width = UnicodeWidthStr::width(content);

        if content_width <= remaining {
            current_width += content_width;
            fitted.push(span);
            continue;
        }

        let truncated = truncate_to_width(content, remaining);
        current_width += UnicodeWidthStr::width(truncated.as_str());
        fitted.push(Span::styled(truncated, span.style));
        break;
    }

    if current_width < width {
        fitted.push(Span::styled(" ".repeat(width - current_width), pad_style));
    }

    fitted
}

fn truncate_to_width(content: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for ch in content.chars() {
        let ch_width = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        result.push(ch);
    }
    result
}

fn format_line_number(line: Option<usize>) -> String {
    format!(
        "{:>4} ",
        line.map_or(String::new(), |line| line.to_string())
    )
}

fn base_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Context => ui::diff_context_style(),
        DiffLineKind::Added => ui::diff_added_style(),
        DiffLineKind::Removed => ui::diff_removed_style(),
    }
}

pub struct HighlightRegistry {
    configs: HashMap<&'static str, HighlightConfiguration>,
}

impl std::fmt::Debug for HighlightRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightRegistry")
            .field("config_count", &self.configs.len())
            .finish()
    }
}

impl HighlightRegistry {
    pub fn new() -> color_eyre::Result<Self> {
        let mut configs = HashMap::new();

        register_highlight_config(
            &mut configs,
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )?;

        let jsx_highlights = format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        );
        register_highlight_config(
            &mut configs,
            "jsx",
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            &jsx_highlights,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            "tsx",
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "python",
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "go",
            tree_sitter_go::LANGUAGE.into(),
            "go",
            tree_sitter_go::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "c",
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            "cpp",
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            "c_sharp",
            "",
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "java",
            tree_sitter_java::LANGUAGE.into(),
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            "ruby",
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            "php",
            tree_sitter_php::HIGHLIGHTS_QUERY,
            tree_sitter_php::INJECTIONS_QUERY,
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "scala",
            tree_sitter_scala::LANGUAGE.into(),
            "scala",
            tree_sitter_scala::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_scala::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "html",
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "json",
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "yaml",
            tree_sitter_yaml::LANGUAGE.into(),
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "haskell",
            tree_sitter_haskell::LANGUAGE.into(),
            "haskell",
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
            tree_sitter_haskell::INJECTIONS_QUERY,
            tree_sitter_haskell::LOCALS_QUERY,
        )?;

        register_highlight_config(
            &mut configs,
            "css",
            tree_sitter_css::LANGUAGE.into(),
            "css",
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        )?;

        register_highlight_config(
            &mut configs,
            "nix",
            tree_sitter_nix::LANGUAGE.into(),
            "nix",
            tree_sitter_nix::HIGHLIGHTS_QUERY,
            tree_sitter_nix::INJECTIONS_QUERY,
            "",
        )?;

        Ok(Self { configs })
    }

    fn config(&self, filetype: &'static str) -> Option<&HighlightConfiguration> {
        self.configs.get(filetype)
    }
}

struct SyntaxHighlighter<'a> {
    registry: Option<&'a HighlightRegistry>,
    highlighter: Highlighter,
}

impl<'a> SyntaxHighlighter<'a> {
    fn new(registry: Option<&'a HighlightRegistry>) -> Self {
        Self {
            registry,
            highlighter: Highlighter::new(),
        }
    }

    fn highlight_line(
        &mut self,
        filetype: Option<&'static str>,
        line: &str,
        fallback: Style,
    ) -> Vec<Span<'static>> {
        let Some(filetype) = filetype else {
            return vec![Span::styled(line.to_string(), fallback)];
        };
        let Some(registry) = self.registry else {
            return vec![Span::styled(line.to_string(), fallback)];
        };
        let Some(config) = registry.config(filetype) else {
            return vec![Span::styled(line.to_string(), fallback)];
        };

        let Ok(events) = self
            .highlighter
            .highlight(config, line.as_bytes(), None, |_| None)
        else {
            return vec![Span::styled(line.to_string(), fallback)];
        };

        let mut style_stack = vec![fallback];
        let mut spans = Vec::new();

        for event in events {
            match event {
                Ok(HighlightEvent::HighlightStart(highlight)) => {
                    let name = HIGHLIGHT_NAMES.get(highlight.0).copied().unwrap_or("");
                    style_stack.push(ui::syntax_style(name, fallback));
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    if style_stack.len() > 1 {
                        let _ = style_stack.pop();
                    }
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    if start < end && end <= line.len() {
                        spans.push(Span::styled(
                            line[start..end].to_string(),
                            *style_stack.last().unwrap_or(&fallback),
                        ));
                    }
                }
                Err(_) => return vec![Span::styled(line.to_string(), fallback)],
            }
        }

        if spans.is_empty() {
            vec![Span::styled(line.to_string(), fallback)]
        } else {
            spans
        }
    }
}

fn register_highlight_config(
    configs: &mut HashMap<&'static str, HighlightConfiguration>,
    key: &'static str,
    language: tree_sitter::Language,
    language_name: &'static str,
    highlights: &str,
    injections: &str,
    locals: &str,
) -> color_eyre::Result<()> {
    let mut config =
        HighlightConfiguration::new(language, language_name, highlights, injections, locals)
            .wrap_err_with(|| format!("failed to build {key} highlight config"))?;
    config.configure(HIGHLIGHT_NAMES);
    configs.insert(key, config);
    Ok(())
}
