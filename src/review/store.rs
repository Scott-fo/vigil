use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;
use rusqlite::{Connection, OptionalExtension, params};

use super::{ReviewFinding, ReviewReport, ReviewScope, ReviewSnapshot, ReviewSummary};

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone)]
pub struct PersistedReview {
    pub id: String,
    pub provider_session_id: Option<String>,
    pub snapshot: ReviewSnapshot,
    pub report: ReviewReport,
}

#[derive(Debug, Clone)]
pub struct ReviewStore {
    path: PathBuf,
}

impl ReviewStore {
    pub fn open_default() -> color_eyre::Result<Self> {
        Self::open(default_database_path())
    }

    pub fn open(path: PathBuf) -> color_eyre::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "failed to create review database directory {}",
                    parent.display()
                )
            })?;
        }
        let store = Self { path };
        store.initialize()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save_report(
        &self,
        snapshot: &ReviewSnapshot,
        report: &ReviewReport,
        provider: &str,
        provider_session_id: Option<&str>,
    ) -> color_eyre::Result<PersistedReview> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let completed_at_ms = now_ms_i64();
        let id = format!("{}-{completed_at_ms}", snapshot.id);
        let scope_json = serde_json::to_string(&snapshot.scope)?;
        let summary_json = serde_json::to_string(&report.summary)?;

        transaction.execute(
            "insert or replace into review_runs (
                id,
                provider,
                provider_session_id,
                repo_root,
                worktree_root,
                review_mode,
                scope_json,
                branch,
                head_sha,
                snapshot_id,
                patch_hash,
                status,
                summary_json,
                created_at_ms,
                completed_at_ms
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'completed', ?12, ?13, ?14)",
            params![
                id.as_str(),
                provider,
                provider_session_id,
                snapshot.repo_root.display().to_string(),
                snapshot.worktree_root.display().to_string(),
                review_mode_name(&snapshot.scope),
                scope_json,
                snapshot.branch.as_deref(),
                snapshot.head_sha.as_deref(),
                snapshot.id.as_str(),
                patch_hash(&snapshot.patch),
                summary_json,
                to_i64(snapshot.created_at_ms),
                completed_at_ms,
            ],
        )?;

        transaction.execute(
            "delete from review_findings where review_id = ?1",
            params![id.as_str()],
        )?;

        for (index, finding) in report.findings.iter().enumerate() {
            let fingerprint = if finding.fingerprint.is_empty() {
                finding_fingerprint(snapshot, finding)
            } else {
                finding.fingerprint.clone()
            };
            let finding_id = format!("{id}-{index}-{fingerprint}");
            transaction.execute(
                "insert into review_findings (
                    id,
                    review_id,
                    path,
                    side,
                    line,
                    end_line,
                    severity,
                    title,
                    body,
                    suggested_patch,
                    fingerprint,
                    state
                ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    finding_id.as_str(),
                    id.as_str(),
                    finding.path.as_str(),
                    enum_string(finding.side)?,
                    finding.line.map(i64::from),
                    finding.end_line.map(i64::from),
                    enum_string(finding.severity)?,
                    finding.title.as_str(),
                    finding.body.as_str(),
                    finding.suggested_patch.as_deref(),
                    fingerprint.as_str(),
                    enum_string(finding.state)?,
                ],
            )?;
        }

        transaction.commit()?;
        self.load_by_id(&id)?
            .ok_or_else(|| color_eyre::eyre::eyre!("saved review {id} could not be loaded"))
    }

    pub fn load_latest_for_snapshot(
        &self,
        snapshot_id: &str,
    ) -> color_eyre::Result<Option<PersistedReview>> {
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "select id from review_runs
                 where snapshot_id = ?1 and status = 'completed'
                 order by completed_at_ms desc
                 limit 1",
                params![snapshot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        drop(connection);

        match id {
            Some(id) => self.load_by_id(&id),
            None => Ok(None),
        }
    }

    fn initialize(&self) -> color_eyre::Result<()> {
        let connection = self.connection()?;
        connection.execute_batch(
            "
            pragma journal_mode = wal;
            pragma foreign_keys = on;

            create table if not exists review_runs (
                id text primary key,
                provider text not null,
                provider_session_id text,
                repo_root text not null,
                worktree_root text not null,
                review_mode text not null,
                scope_json text not null,
                branch text,
                head_sha text,
                snapshot_id text not null,
                patch_hash text not null,
                status text not null,
                summary_json text,
                created_at_ms integer not null,
                completed_at_ms integer
            );

            create index if not exists review_runs_snapshot_idx
                on review_runs(snapshot_id, completed_at_ms);

            create table if not exists review_findings (
                id text primary key,
                review_id text not null references review_runs(id) on delete cascade,
                path text not null,
                side text not null,
                line integer,
                end_line integer,
                severity text not null,
                title text not null,
                body text not null,
                suggested_patch text,
                fingerprint text not null,
                state text not null
            );

            create index if not exists review_findings_review_path_idx
                on review_findings(review_id, path, line);
            ",
        )?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    fn load_by_id(&self, id: &str) -> color_eyre::Result<Option<PersistedReview>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "select
                    id,
                    provider_session_id,
                    repo_root,
                    worktree_root,
                    scope_json,
                    branch,
                    head_sha,
                    snapshot_id,
                    summary_json,
                    created_at_ms
                 from review_runs
                 where id = ?1",
                params![id],
                |row| {
                    Ok(ReviewRunRow {
                        id: row.get(0)?,
                        provider_session_id: row.get(1)?,
                        repo_root: row.get::<_, String>(2)?.into(),
                        worktree_root: row.get::<_, String>(3)?.into(),
                        scope_json: row.get(4)?,
                        branch: row.get(5)?,
                        head_sha: row.get(6)?,
                        snapshot_id: row.get(7)?,
                        summary_json: row.get(8)?,
                        created_at_ms: row.get::<_, i64>(9)?,
                    })
                },
            )
            .optional()?;

        let Some(row) = row else {
            return Ok(None);
        };
        let findings = load_findings(&connection, &row.id)?;
        let summary: ReviewSummary = serde_json::from_str(&row.summary_json)?;
        let scope: ReviewScope = serde_json::from_str(&row.scope_json)?;

        Ok(Some(PersistedReview {
            id: row.id,
            provider_session_id: row.provider_session_id,
            snapshot: ReviewSnapshot {
                id: row.snapshot_id,
                repo_root: row.repo_root,
                worktree_root: row.worktree_root,
                head_sha: row.head_sha,
                branch: row.branch,
                scope,
                files: findings
                    .iter()
                    .map(|finding| finding.path.clone())
                    .collect(),
                patch: String::new(),
                created_at_ms: row.created_at_ms.max(0) as u128,
            },
            report: ReviewReport { summary, findings },
        }))
    }

    fn connection(&self) -> color_eyre::Result<Connection> {
        Connection::open(&self.path)
            .wrap_err_with(|| format!("failed to open review database {}", self.path.display()))
    }
}

#[derive(Debug)]
struct ReviewRunRow {
    id: String,
    provider_session_id: Option<String>,
    repo_root: PathBuf,
    worktree_root: PathBuf,
    scope_json: String,
    branch: Option<String>,
    head_sha: Option<String>,
    snapshot_id: String,
    summary_json: String,
    created_at_ms: i64,
}

fn load_findings(
    connection: &Connection,
    review_id: &str,
) -> color_eyre::Result<Vec<ReviewFinding>> {
    let mut statement = connection.prepare(
        "select
            path,
            side,
            line,
            end_line,
            severity,
            title,
            body,
            suggested_patch,
            fingerprint,
            state
         from review_findings
         where review_id = ?1
         order by
            case severity
                when 'critical' then 5
                when 'high' then 4
                when 'medium' then 3
                when 'low' then 2
                when 'info' then 1
                else 0
            end desc,
            path asc,
            line asc",
    )?;
    let rows = statement.query_map(params![review_id], |row| {
        Ok(ReviewFinding {
            path: row.get(0)?,
            side: enum_from_string(row.get::<_, String>(1)?).map_err(to_sql_error)?,
            line: row.get::<_, Option<i64>>(2)?.and_then(to_u32),
            end_line: row.get::<_, Option<i64>>(3)?.and_then(to_u32),
            severity: enum_from_string(row.get::<_, String>(4)?).map_err(to_sql_error)?,
            title: row.get(5)?,
            body: row.get(6)?,
            suggested_patch: row.get(7)?,
            fingerprint: row.get(8)?,
            state: enum_from_string(row.get::<_, String>(9)?).map_err(to_sql_error)?,
        })
    })?;

    let mut findings = Vec::new();
    for row in rows {
        findings.push(row?);
    }
    Ok(findings)
}

pub fn default_database_path() -> PathBuf {
    data_dir().join("reviews.sqlite3")
}

fn data_dir() -> PathBuf {
    if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
        let trimmed = xdg_data_home.trim();
        if !trimmed.is_empty() {
            return database_dir_from_data_home(Path::new(trimmed));
        }
    }

    database_dir_from_data_home(
        &home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".local")
            .join("share"),
    )
}

fn database_dir_from_data_home(data_home: &Path) -> PathBuf {
    data_home.join("vigil")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn review_mode_name(scope: &ReviewScope) -> &'static str {
    match scope {
        ReviewScope::WorkingTree => "working_tree",
        ReviewScope::CommitCompare { .. } => "commit_compare",
        ReviewScope::BranchCompare { .. } => "branch_compare",
    }
}

fn finding_fingerprint(snapshot: &ReviewSnapshot, finding: &ReviewFinding) -> String {
    let mut input = String::new();
    input.push_str(&snapshot.id);
    input.push('\n');
    input.push_str(&finding.path);
    input.push('\n');
    input.push_str(
        &finding
            .line
            .map(|line| line.to_string())
            .unwrap_or_default(),
    );
    input.push('\n');
    input.push_str(&finding.title);
    input.push('\n');
    input.push_str(&finding.body);
    format!("{:016x}", patch_hash_u64(input.as_bytes()))
}

fn patch_hash(patch: &str) -> String {
    format!("{:016x}", patch_hash_u64(patch.as_bytes()))
}

fn patch_hash_u64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn enum_string<T: serde::Serialize>(value: T) -> color_eyre::Result<String> {
    let value = serde_json::to_value(value)?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| color_eyre::eyre::eyre!("failed to encode enum value as sqlite string"))
}

fn enum_from_string<T: serde::de::DeserializeOwned>(value: String) -> color_eyre::Result<T> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(Into::into)
}

fn to_sql_error(error: color_eyre::Report) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

fn to_u32(value: i64) -> Option<u32> {
    u32::try_from(value).ok()
}

fn to_i64(value: u128) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn now_ms_i64() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| to_i64(duration.as_millis()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{ReviewFindingState, ReviewSeverity, ReviewSide, ReviewVerdict};

    fn temp_database_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("vigil-review-tests")
            .join(format!("{name}-{}.sqlite3", now_ms_i64()))
    }

    fn snapshot() -> ReviewSnapshot {
        ReviewSnapshot {
            id: "snapshot-1".to_string(),
            repo_root: PathBuf::from("/repo"),
            worktree_root: PathBuf::from("/repo"),
            head_sha: Some("abc".to_string()),
            branch: Some("main".to_string()),
            scope: ReviewScope::WorkingTree,
            files: vec!["src/lib.rs".to_string()],
            patch: "diff".to_string(),
            created_at_ms: 7,
        }
    }

    fn report() -> ReviewReport {
        ReviewReport {
            summary: ReviewSummary {
                headline: "Looks close".to_string(),
                verdict: ReviewVerdict::HasConcerns,
                body: "One issue.".to_string(),
                risk_areas: vec!["state".to_string()],
            },
            findings: vec![ReviewFinding {
                path: "src/lib.rs".to_string(),
                side: ReviewSide::New,
                line: Some(12),
                end_line: None,
                severity: ReviewSeverity::Medium,
                title: "Stale request".to_string(),
                body: "Old results can win.".to_string(),
                suggested_patch: None,
                state: ReviewFindingState::Open,
                fingerprint: String::new(),
            }],
        }
    }

    #[test]
    fn database_directory_uses_xdg_data_home_style_location() {
        let path = database_dir_from_data_home(Path::new("/tmp/vigil-xdg-data"));

        assert_eq!(path, PathBuf::from("/tmp/vigil-xdg-data").join("vigil"));
    }

    #[test]
    fn store_round_trips_review_comments() {
        let path = temp_database_path("round-trip");
        let store = ReviewStore::open(path.clone()).expect("store opens");
        let saved = store
            .save_report(&snapshot(), &report(), "codex-app", Some("thread-1"))
            .expect("save report");
        let loaded = store
            .load_latest_for_snapshot("snapshot-1")
            .expect("load latest")
            .expect("review");

        assert_eq!(loaded.id, saved.id);
        assert_eq!(loaded.provider_session_id.as_deref(), Some("thread-1"));
        assert_eq!(loaded.report.summary.headline, "Looks close");
        assert_eq!(loaded.report.findings[0].path, "src/lib.rs");
        assert!(!loaded.report.findings[0].fingerprint.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn loaded_findings_sort_by_severity_rank() {
        let path = temp_database_path("severity-order");
        let store = ReviewStore::open(path.clone()).expect("store opens");
        let mut report = report();
        let mut high = report.findings[0].clone();
        high.line = Some(8);
        high.severity = ReviewSeverity::High;
        high.title = "Higher priority".to_string();
        report.findings.push(high);

        let loaded = store
            .save_report(&snapshot(), &report, "codex-app", None)
            .expect("save report");

        assert_eq!(loaded.report.findings[0].severity, ReviewSeverity::High);
        assert_eq!(loaded.report.findings[1].severity, ReviewSeverity::Medium);

        let _ = fs::remove_file(path);
    }
}
