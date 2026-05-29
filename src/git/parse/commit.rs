use super::super::{CommitSearchEntry, LOG_FIELD_SEPARATOR, LOG_RECORD_SEPARATOR};

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
