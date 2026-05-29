use super::super::FileEntry;
use super::filetype::resolve_diff_filetype;

#[derive(Debug, Clone)]
pub(crate) struct StatusEntry {
    pub(crate) status: String,
    pub(crate) path: String,
    pub(crate) original_path: Option<String>,
}

pub(crate) fn parse_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut fields = raw.split('\0');

    while let Some(field) = fields.next() {
        let field = field.trim_end();

        if field.len() < 4 {
            continue;
        }

        let x = field.chars().next().unwrap_or(' ');
        let y = field.chars().nth(1).unwrap_or(' ');
        let status = to_status_pair(x, y);
        let path = &field[3..];

        if path.is_empty() {
            continue;
        }

        entries.push(StatusEntry {
            status,
            path: path.to_string(),
            original_path: matches!(x, 'R' | 'C')
                .then(|| fields.next().unwrap_or_default())
                .filter(|original_path| !original_path.is_empty())
                .map(str::to_owned),
        });
    }

    entries
}

pub(crate) fn parse_diff_name_status_entries(raw: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut fields = raw.split('\0');

    while let Some(status_field) = fields.next() {
        let status_field = status_field.trim();

        if status_field.is_empty() {
            continue;
        }

        let status_code = status_field.chars().next().unwrap_or(' ');
        match status_code {
            'R' | 'C' => {
                let original_path = fields.next().unwrap_or_default();
                let path = fields.next().unwrap_or_default();

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path: path.to_string(),
                    original_path: (!original_path.is_empty()).then_some(original_path.to_string()),
                });
            }
            _ => {
                let path = fields.next().unwrap_or_default();

                if path.is_empty() {
                    continue;
                }

                entries.push(StatusEntry {
                    status: status_code.to_string(),
                    path: path.to_string(),
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

#[cfg(test)]
mod tests {
    use super::{parse_diff_name_status_entries, parse_status_entries};

    #[test]
    fn status_parser_keeps_destination_path_for_renames() {
        let entries = parse_status_entries("R  after.rs\0before.rs\0");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "R ");
        assert_eq!(entries[0].path, "after.rs");
        assert_eq!(entries[0].original_path.as_deref(), Some("before.rs"));
    }

    #[test]
    fn diff_name_status_parser_keeps_destination_path_for_renames() {
        let entries = parse_diff_name_status_entries("R100\0before.rs\0after.rs\0");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "R");
        assert_eq!(entries[0].path, "after.rs");
        assert_eq!(entries[0].original_path.as_deref(), Some("before.rs"));
    }
}
