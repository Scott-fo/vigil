#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub status: String,
    pub path: String,
    pub label: String,
    pub filetype: Option<&'static str>,
}

/// Joins the original and current path in a renamed or copied file's label.
pub(crate) const RENAME_LABEL_SEPARATOR: &str = " -> ";

impl FileEntry {
    /// Path the file had before a rename or copy, when git reported one.
    pub fn original_path(&self) -> Option<&str> {
        self.label
            .strip_suffix(self.path.as_str())?
            .strip_suffix(RENAME_LABEL_SEPARATOR)
            .filter(|original| !original.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::FileEntry;

    fn entry(path: &str, label: &str) -> FileEntry {
        FileEntry {
            status: "R ".to_string(),
            path: path.to_string(),
            label: label.to_string(),
            filetype: None,
        }
    }

    #[test]
    fn original_path_is_reported_only_for_renames() {
        assert_eq!(
            entry("docs/new.md", "notes.md -> docs/new.md").original_path(),
            Some("notes.md")
        );
        assert_eq!(entry("src/lib.rs", "src/lib.rs").original_path(), None);
        assert_eq!(entry("src/lib.rs", "lib.rs").original_path(), None);
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchMergeRequest {
    pub source_ref: String,
    pub destination_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchMergeOutcome {
    Prepared {
        source_ref: String,
        destination_ref: String,
    },
    Conflicted {
        source_ref: String,
        destination_ref: String,
    },
    AlreadyUpToDate {
        source_ref: String,
        destination_ref: String,
    },
}

#[derive(Debug, Clone)]
pub struct BranchCompareRefs {
    pub refs: Vec<String>,
    pub current_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: std::path::PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub prunable: bool,
    pub dirty: bool,
    pub change_count: usize,
}
