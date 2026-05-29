use super::super::types::WorktreeEntry;

pub(crate) fn parse_worktree_entries(raw: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for field in raw.split('\0') {
        if field.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }

        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(entry) = current.replace(WorktreeEntry {
                path: path.into(),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                prunable: false,
                dirty: false,
                change_count: 0,
            }) {
                entries.push(entry);
            }
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        if let Some(head) = field.strip_prefix("HEAD ") {
            entry.head = Some(head.to_string());
        } else if let Some(branch) = field.strip_prefix("branch ") {
            entry.branch = Some(
                branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string(),
            );
        } else if field == "detached" {
            entry.detached = true;
        } else if field == "bare" {
            entry.bare = true;
        } else if field.starts_with("prunable") {
            entry.prunable = true;
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::parse_worktree_entries;

    #[test]
    fn worktree_parser_reads_porcelain_records() {
        let entries = parse_worktree_entries(
            "worktree /repo/main\0HEAD abc123\0branch refs/heads/main\0\0\
             worktree /repo/feature\0HEAD def456\0detached\0\0",
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, std::path::PathBuf::from("/repo/main"));
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[0].detached);
        assert_eq!(entries[1].path, std::path::PathBuf::from("/repo/feature"));
        assert!(entries[1].detached);
    }
}
