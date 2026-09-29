//! The on-disk review database.
//!
//! [`ReviewStore`] opens (creating if needed) a SQLite database under the XDG
//! data directory and migrates it to the current schema. Feature modules such
//! as [`super::viewed`] add their own queries on top of [`ReviewStore`].

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;
use rusqlite::Connection;

/// Schema history:
///
/// - 2: `viewed_files`, alongside tables for a since-removed AI review
///   provider (`review_runs`, `review_findings`).
/// - 3: drops the AI review tables; only `viewed_files` remains.
const SCHEMA_VERSION: i64 = 3;

/// Handle to the review database. Cheap to clone; each call opens its own
/// blocking SQLite connection.
#[derive(Debug, Clone)]
pub struct ReviewStore {
    path: PathBuf,
}

impl ReviewStore {
    /// Opens the database at the default per-user location.
    pub fn open_default() -> color_eyre::Result<Self> {
        Self::open(default_database_path())
    }

    /// Opens the database at `path`, creating and migrating it as needed.
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

    fn initialize(&self) -> color_eyre::Result<()> {
        let connection = self.connection()?;
        connection.execute_batch(
            "
            pragma journal_mode = wal;
            pragma foreign_keys = on;

            drop table if exists review_findings;
            drop table if exists review_runs;

            create table if not exists viewed_files (
                scope text not null,
                path text not null,
                fingerprint text not null,
                viewed_at_ms integer not null,
                primary key (scope, path)
            );
            ",
        )?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    pub(super) fn connection(&self) -> color_eyre::Result<Connection> {
        Connection::open(&self.path)
            .wrap_err_with(|| format!("failed to open review database {}", self.path.display()))
    }
}

fn default_database_path() -> PathBuf {
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

pub(super) fn now_ms_i64() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::{
        git::DiffFingerprint,
        review::{ReviewScope, ViewedScope},
    };

    fn temp_database_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("vigil-review-tests")
            .join(format!("{name}-{}.sqlite3", now_ms_i64()))
    }

    fn table_names(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare("select name from sqlite_master where type = 'table' order by name")
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows")
    }

    #[test]
    fn database_directory_uses_xdg_data_home_style_location() {
        let path = database_dir_from_data_home(Path::new("/tmp/vigil-xdg-data"));

        assert_eq!(path, PathBuf::from("/tmp/vigil-xdg-data").join("vigil"));
    }

    /// Databases written before the AI review tables were removed must keep
    /// their viewed marks, keyed by the exact same scope strings.
    #[test]
    fn upgrading_a_version_2_database_drops_review_tables_and_keeps_viewed_marks() {
        let path = temp_database_path("upgrade-v2");
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        let fingerprint = DiffFingerprint::from_hex("aa").expect("fingerprint");
        {
            let connection = Connection::open(&path).expect("open legacy db");
            connection
                .execute_batch(
                    "
                    create table review_runs (
                        id text primary key,
                        snapshot_id text not null
                    );
                    create index review_runs_snapshot_idx on review_runs(snapshot_id);
                    create table review_findings (
                        id text primary key,
                        review_id text not null references review_runs(id) on delete cascade
                    );
                    create table viewed_files (
                        scope text not null,
                        path text not null,
                        fingerprint text not null,
                        viewed_at_ms integer not null,
                        primary key (scope, path)
                    );
                    insert into review_runs values ('run-1', 'snapshot-1');
                    insert into review_findings values ('finding-1', 'run-1');
                    pragma user_version = 2;
                    ",
                )
                .expect("create legacy schema");
            for (scope, file) in [
                ("/repo\0working-tree", "src/lib.rs"),
                ("/repo\0commit\0abc123", "src/commit.rs"),
                ("/repo\0branch\0feature\0main", "src/branch.rs"),
            ] {
                connection
                    .execute(
                        "insert into viewed_files values (?1, ?2, ?3, 1)",
                        params![scope, file, fingerprint.to_hex()],
                    )
                    .expect("insert viewed mark");
            }
        }

        let store = ReviewStore::open(path.clone()).expect("store upgrades");

        let connection = store.connection().expect("connection");
        assert_eq!(table_names(&connection), vec!["viewed_files".to_string()]);
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, SCHEMA_VERSION);

        let root = Path::new("/repo");
        for (scope, file) in [
            (ReviewScope::WorkingTree, "src/lib.rs"),
            (
                ReviewScope::CommitCompare {
                    commit_hash: "abc123".to_string(),
                },
                "src/commit.rs",
            ),
            (
                ReviewScope::BranchCompare {
                    source_ref: "feature".to_string(),
                    destination_ref: "main".to_string(),
                },
                "src/branch.rs",
            ),
        ] {
            let viewed = store
                .load_viewed_files(&ViewedScope::new(root, &scope))
                .expect("load viewed");
            assert!(
                viewed.is_viewed(file, Some(fingerprint)),
                "{scope:?} lost its viewed mark"
            );
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn reopening_a_current_database_is_idempotent() {
        let path = temp_database_path("reopen");
        let scope = ViewedScope::new(Path::new("/repo"), &ReviewScope::WorkingTree);
        let fingerprint = DiffFingerprint::from_hex("bb").expect("fingerprint");
        ReviewStore::open(path.clone())
            .expect("store opens")
            .set_file_viewed(&scope, "src/lib.rs", Some(fingerprint))
            .expect("mark");

        let reopened = ReviewStore::open(path.clone()).expect("store reopens");

        assert!(
            reopened
                .load_viewed_files(&scope)
                .expect("load")
                .is_viewed("src/lib.rs", Some(fingerprint))
        );
        let _ = fs::remove_file(path);
    }
}
