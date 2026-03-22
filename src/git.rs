use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use color_eyre::eyre::{WrapErr, eyre};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use tokio::{fs, process::Command};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};
use unicode_width::UnicodeWidthStr;

use crate::{app::DiffViewMode, ui};

pub type SharedHighlightRegistry = Arc<HighlightRegistry>;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub status: String,
    pub path: String,
    pub label: String,
    pub filetype: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct DiffView {
    rows: Vec<DiffRow>,
    pub note: Option<String>,
    render_cache: DiffRenderCache,
    nav_cache: DiffNavCache,
}

impl DiffView {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            rows: Vec::new(),
            note: Some(message.into()),
            render_cache: DiffRenderCache::default(),
            nav_cache: DiffNavCache::default(),
        }
    }

    pub fn rendered_lines(&mut self, mode: DiffViewMode, width: usize) -> &[Line<'static>] {
        let cache_is_stale = {
            let cache = self.render_cache.entry(mode);
            !cache.valid || cache.width != width
        };

        if cache_is_stale {
            let lines = if self.rows.is_empty() {
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))]
            } else {
                match mode {
                    DiffViewMode::Unified => render_unified_lines(&self.rows, width),
                    DiffViewMode::Split => render_split_lines(&self.rows, width),
                }
            };

            let cache = self.render_cache.entry_mut(mode);
            cache.width = width;
            cache.lines = lines;
            cache.valid = true;
        }

        &self.render_cache.entry(mode).lines
    }

    pub fn first_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_entries(mode)
            .iter()
            .position(Option::is_some)
            .unwrap_or(0)
    }

    pub fn last_selectable_index(&mut self, mode: DiffViewMode) -> usize {
        self.nav_entries(mode)
            .iter()
            .rposition(Option::is_some)
            .unwrap_or(0)
    }

    pub fn move_selection(&mut self, mode: DiffViewMode, current: usize, delta: i32) -> usize {
        let nav = self.nav_entries(mode);
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
        self.nav_entries(mode).get(index).copied().flatten()
    }

    pub fn display_line_count(&mut self, mode: DiffViewMode) -> usize {
        self.nav_entries(mode).len()
    }

    fn nav_entries(&mut self, mode: DiffViewMode) -> &[Option<usize>] {
        let cache = self.nav_cache.entry(mode);
        let needs_build = !cache.valid;
        if needs_build {
            let entries = match mode {
                DiffViewMode::Unified => build_unified_nav_entries(&self.rows),
                DiffViewMode::Split => build_split_nav_entries(&self.rows),
            };
            let cache = self.nav_cache.entry_mut(mode);
            cache.entries = entries;
            cache.valid = true;
        }
        &self.nav_cache.entry(mode).entries
    }
}

#[derive(Debug, Default)]
struct DiffRenderCache {
    unified: CachedLines,
    split: CachedLines,
}

impl DiffRenderCache {
    fn entry(&self, mode: DiffViewMode) -> &CachedLines {
        match mode {
            DiffViewMode::Unified => &self.unified,
            DiffViewMode::Split => &self.split,
        }
    }

    fn entry_mut(&mut self, mode: DiffViewMode) -> &mut CachedLines {
        match mode {
            DiffViewMode::Unified => &mut self.unified,
            DiffViewMode::Split => &mut self.split,
        }
    }
}

#[derive(Debug, Default)]
struct CachedLines {
    width: usize,
    lines: Vec<Line<'static>>,
    valid: bool,
}

#[derive(Debug, Default)]
struct DiffNavCache {
    unified: CachedNav,
    split: CachedNav,
}

impl DiffNavCache {
    fn entry(&self, mode: DiffViewMode) -> &CachedNav {
        match mode {
            DiffViewMode::Unified => &self.unified,
            DiffViewMode::Split => &self.split,
        }
    }

    fn entry_mut(&mut self, mode: DiffViewMode) -> &mut CachedNav {
        match mode {
            DiffViewMode::Unified => &mut self.unified,
            DiffViewMode::Split => &mut self.split,
        }
    }
}

#[derive(Debug, Default)]
struct CachedNav {
    entries: Vec<Option<usize>>,
    valid: bool,
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
    hunk_index: usize,
    kind: DiffLineKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: Vec<Span<'static>>,
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
    use ratatui::style::Color;

    if status == "??" || status.contains('A') {
        return Color::Rgb(166, 218, 149);
    }
    if status.contains('U') || status.contains('D') {
        return Color::Rgb(237, 135, 150);
    }
    if status.contains('R') || status.contains('C') {
        return Color::Rgb(198, 160, 246);
    }
    if status.contains('M') {
        return Color::Rgb(238, 212, 159);
    }
    Color::Rgb(184, 192, 224)
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

pub async fn resolve_repo_root() -> color_eyre::Result<PathBuf> {
    let output = Command::new("git")
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
    let entries = parse_status_entries(&output)
        .into_iter()
        .filter(|entry| entry.status != "!!")
        .map(to_file_entry)
        .collect();
    Ok(entries)
}

pub async fn load_diff_view(
    repo_root: &Path,
    file: &FileEntry,
    highlight_registry: Option<&HighlightRegistry>,
) -> color_eyre::Result<DiffView> {
    let preview = load_file_preview(repo_root, file).await?;
    if preview.diff.trim().is_empty() {
        let message = preview
            .note
            .unwrap_or_else(|| "No textual diff available.".to_string());
        return Ok(DiffView::empty(message));
    }

    let mut highlighter = SyntaxHighlighter::new(highlight_registry);
    Ok(DiffView {
        rows: build_diff_rows(&preview.diff, file.filetype, &mut highlighter),
        note: preview.note,
        render_cache: DiffRenderCache::default(),
        nav_cache: DiffNavCache::default(),
    })
}

struct FilePreview {
    diff: String,
    note: Option<String>,
}

async fn load_file_preview(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<FilePreview> {
    if file.status == "??" {
        load_untracked_preview(repo_root, &file.path).await
    } else {
        load_tracked_preview(repo_root, &file.path).await
    }
}

async fn load_tracked_preview(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<FilePreview> {
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
    Ok(FilePreview {
        diff: output,
        note: None,
    })
}

async fn load_untracked_preview(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<FilePreview> {
    let full_path = repo_root.join(file_path);
    let bytes = fs::read(&full_path)
        .await
        .wrap_err_with(|| format!("failed to read untracked file {}", full_path.display()))?;

    if bytes.contains(&0) {
        return Ok(FilePreview {
            diff: String::new(),
            note: Some("Binary or non-text file; no preview available.".to_string()),
        });
    }

    let content = String::from_utf8_lossy(&bytes);
    let diff = create_untracked_file_diff(file_path, &content);
    Ok(if diff.trim().is_empty() {
        FilePreview {
            diff,
            note: Some("Untracked empty file; no textual hunk to preview.".to_string()),
        }
    } else {
        FilePreview { diff, note: None }
    })
}

async fn git_output(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
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

fn build_diff_rows(
    diff: &str,
    filetype: Option<&'static str>,
    highlighter: &mut SyntaxHighlighter,
) -> Vec<DiffRow> {
    let normalized = diff.replace("\r\n", "\n");
    let mut rows = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut in_hunk = false;
    let mut hunk_index = 0usize;

    for raw_line in normalized.split('\n') {
        if raw_line.is_empty() && rows.is_empty() {
            continue;
        }

        if let Some((parsed_old_line, parsed_new_line)) = parse_hunk_header(raw_line) {
            old_line = parsed_old_line;
            new_line = parsed_new_line;
            in_hunk = true;
            hunk_index = hunk_index.saturating_add(1);
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
                    hunk_index,
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
                    hunk_index,
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
                    hunk_index,
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

    rows
}

fn parse_hunk_header(raw_line: &str) -> Option<(usize, usize)> {
    if !raw_line.starts_with("@@ -") {
        return None;
    }

    let remainder = raw_line.strip_prefix("@@ -")?;
    let (old_part, rest) = remainder.split_once(" +")?;
    let (new_part, _) = rest.split_once(" @@")?;

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part.split(',').next()?.parse::<usize>().ok()?;
    Some((old_start, new_start))
}

fn render_diff_row(
    hunk_index: usize,
    old_line: Option<usize>,
    new_line: Option<usize>,
    content: &str,
    kind: DiffLineKind,
    filetype: Option<&'static str>,
    highlighter: &mut SyntaxHighlighter,
) -> DiffRow {
    DiffRow {
        hunk_index,
        kind,
        old_line,
        new_line,
        content: highlighter.highlight_line(filetype, content, base_style(kind)),
    }
}

fn build_unified_nav_entries(rows: &[DiffRow]) -> Vec<Option<usize>> {
    let mut entries = Vec::with_capacity(rows.len());
    let mut last_hunk_index = None;

    for row in rows {
        if let Some(previous_hunk_index) = last_hunk_index {
            if row.hunk_index != previous_hunk_index {
                entries.push(None);
            }
        }

        entries.push(match row.kind {
            DiffLineKind::Added | DiffLineKind::Context => row.new_line,
            DiffLineKind::Removed => row.old_line,
        });
        last_hunk_index = Some(row.hunk_index);
    }

    entries
}

fn build_split_nav_entries(rows: &[DiffRow]) -> Vec<Option<usize>> {
    let mut entries = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<&DiffRow> = Vec::new();
    let mut pending_added: Vec<&DiffRow> = Vec::new();
    let mut last_hunk_index = None;

    let flush_pending = |entries: &mut Vec<Option<usize>>,
                         removed: &mut Vec<&DiffRow>,
                         added: &mut Vec<&DiffRow>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            entries.push(resolve_split_target_line(left, right));
        }
        removed.clear();
        added.clear();
    };

    for row in rows {
        if let Some(previous_hunk_index) = last_hunk_index {
            if row.hunk_index != previous_hunk_index {
                flush_pending(&mut entries, &mut pending_removed, &mut pending_added);
                entries.push(None);
            }
        }
        last_hunk_index = Some(row.hunk_index);

        match row.kind {
            DiffLineKind::Removed => pending_removed.push(row),
            DiffLineKind::Added => pending_added.push(row),
            DiffLineKind::Context => {
                flush_pending(&mut entries, &mut pending_removed, &mut pending_added);
                entries.push(resolve_split_target_line(Some(row), Some(row)));
            }
        }
    }

    flush_pending(&mut entries, &mut pending_removed, &mut pending_added);
    entries
}

fn resolve_split_target_line(left: Option<&DiffRow>, right: Option<&DiffRow>) -> Option<usize> {
    right.and_then(|row| row.new_line)
        .or_else(|| left.and_then(|row| row.old_line))
}

fn render_unified_lines(rows: &[DiffRow], width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::with_capacity(rows.len());
    let mut last_hunk_index = None;

    for row in rows {
        if let Some(previous_hunk_index) = last_hunk_index {
            if row.hunk_index != previous_hunk_index {
                rendered.push(render_hunk_separator(width));
            }
        }

        rendered.push(render_unified_code_line(row, width));
        last_hunk_index = Some(row.hunk_index);
    }

    rendered
}

fn render_split_lines(rows: &[DiffRow], width: usize) -> Vec<Line<'static>> {
    let total_width = width.saturating_sub(1);
    let gutter_width = 3;
    let side_width = total_width.saturating_sub(gutter_width) / 2;
    let mut rendered = Vec::with_capacity(rows.len());
    let mut pending_removed: Vec<&DiffRow> = Vec::new();
    let mut pending_added: Vec<&DiffRow> = Vec::new();
    let mut last_hunk_index = None;

    let flush_pending = |rendered: &mut Vec<Line<'static>>,
                         removed: &mut Vec<&DiffRow>,
                         added: &mut Vec<&DiffRow>| {
        let row_count = removed.len().max(added.len());
        for index in 0..row_count {
            let left = removed.get(index).copied();
            let right = added.get(index).copied();
            rendered.push(render_split_pair_line(left, right, side_width));
        }
        removed.clear();
        added.clear();
    };

    for row in rows {
        if let Some(previous_hunk_index) = last_hunk_index {
            if row.hunk_index != previous_hunk_index {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                rendered.push(render_hunk_separator(total_width));
            }
        }
        last_hunk_index = Some(row.hunk_index);

        match row.kind {
            DiffLineKind::Removed => pending_removed.push(row),
            DiffLineKind::Added => pending_added.push(row),
            DiffLineKind::Context => {
                flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
                rendered.push(render_split_pair_line(Some(row), Some(row), side_width));
            }
        }
    }

    flush_pending(&mut rendered, &mut pending_removed, &mut pending_added);
    rendered
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

fn render_hunk_separator(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        " ".repeat(width.max(1)),
        ui::diff_context_style(),
    ))
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
