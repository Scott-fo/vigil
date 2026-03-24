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
