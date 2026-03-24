use super::{
    CommitSearchEntry, FileEntry, LOG_FIELD_SEPARATOR, LOG_RECORD_SEPARATOR,
    types::BranchCompareSelection,
};

#[derive(Debug, Clone)]
pub(crate) struct StatusEntry {
    pub(crate) status: String,
    pub(crate) path: String,
    pub(crate) original_path: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ParsedBlameHeader {
    pub(crate) commit_hash: String,
    pub(crate) author: String,
    pub(crate) date: String,
    pub(crate) summary: String,
}

#[derive(Debug)]
pub(crate) struct ParsedCommitShow {
    pub(crate) commit_hash: String,
    pub(crate) short_hash: String,
    pub(crate) parent_hashes: Vec<String>,
    pub(crate) date: String,
    pub(crate) author: String,
    pub(crate) subject: String,
    pub(crate) description: String,
}

pub(crate) fn parse_blame_porcelain_header(raw: &str) -> Option<ParsedBlameHeader> {
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

pub(crate) fn parse_commit_show_output(raw: &str) -> Option<ParsedCommitShow> {
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

pub(crate) fn is_uncommitted_blame_hash(hash: &str) -> bool {
    hash.trim() == "0000000000000000000000000000000000000000"
}

fn format_unix_date(raw_seconds: &str) -> String {
    match raw_seconds.parse::<u64>() {
        Ok(seconds) if seconds > 0 => String::new(),
        _ => String::new(),
    }
}

pub(crate) fn parse_commit_log_entries(raw: &str) -> Vec<CommitSearchEntry> {
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

pub(crate) fn parse_status_entries(raw: &str) -> Vec<StatusEntry> {
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
            let original_path = fields.get(index).copied().unwrap_or_default().to_string();
            index += 1;
            entries.push(StatusEntry {
                status,
                path: first_path.clone(),
                original_path: (!original_path.is_empty()).then_some(original_path),
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

pub(crate) fn build_branch_diff_range(selection: &BranchCompareSelection) -> String {
    format!(
        "{}...{}",
        selection.destination_ref.trim(),
        selection.source_ref.trim()
    )
}

pub(crate) fn parse_diff_name_status_entries(raw: &str) -> Vec<StatusEntry> {
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

pub(crate) fn to_file_entry(entry: StatusEntry) -> FileEntry {
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

pub(crate) fn resolve_diff_filetype(path: &str) -> Option<&'static str> {
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
