//! Per-file "viewed" marks, like the checkbox on a GitHub pull request.
//!
//! A mark belongs to one review target ([`ViewedScope`]) and one file, and
//! records the [`DiffFingerprint`] of the diff the reviewer saw. A file counts
//! as viewed only while its current fingerprint matches, so any later change to
//! that file's diff clears the mark without extra bookkeeping.
//!
//! Marks are persisted in the review database. [`ReviewStore`] calls are
//! blocking; interactive callers should run them off the UI thread.

use std::{collections::HashMap, path::Path};

use rusqlite::params;

use super::{ReviewScope, store::ReviewStore};
use crate::git::DiffFingerprint;

/// Identifies what is being reviewed: a repository plus the working tree, a
/// commit, or a branch comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewedScope(String);

impl ViewedScope {
    pub fn new(repo_root: &Path, scope: &ReviewScope) -> Self {
        let root = repo_root.display();
        Self(match scope {
            ReviewScope::WorkingTree => format!("{root}\0working-tree"),
            ReviewScope::CommitCompare { commit_hash, .. } => {
                format!("{root}\0commit\0{commit_hash}")
            }
            ReviewScope::BranchCompare {
                source_ref,
                destination_ref,
                ..
            } => format!("{root}\0branch\0{source_ref}\0{destination_ref}"),
        })
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Viewed marks for one [`ViewedScope`], keyed by file path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewedFiles {
    marks: HashMap<String, DiffFingerprint>,
}

impl ViewedFiles {
    /// Whether `path` was marked viewed at exactly its `current` diff. A file
    /// whose diff is not loaded yet (`None`) is never viewed.
    pub fn is_viewed(&self, path: &str, current: Option<DiffFingerprint>) -> bool {
        current.is_some_and(|current| self.marks.get(path) == Some(&current))
    }

    pub fn mark(&mut self, path: &str, fingerprint: DiffFingerprint) {
        self.marks.insert(path.to_string(), fingerprint);
    }

    pub fn unmark(&mut self, path: &str) {
        self.marks.remove(path);
    }
}

impl ReviewStore {
    pub fn load_viewed_files(&self, scope: &ViewedScope) -> color_eyre::Result<ViewedFiles> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare("select path, fingerprint from viewed_files where scope = ?1")?;
        let rows = statement.query_map(params![scope.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut viewed = ViewedFiles::default();
        for row in rows {
            let (path, fingerprint) = row?;
            // Rows with an unreadable fingerprint are treated as unviewed.
            if let Some(fingerprint) = DiffFingerprint::from_hex(&fingerprint) {
                viewed.mark(&path, fingerprint);
            }
        }
        Ok(viewed)
    }

    /// Records `path` as viewed at `fingerprint`, or clears the mark for `None`.
    pub fn set_file_viewed(
        &self,
        scope: &ViewedScope,
        path: &str,
        fingerprint: Option<DiffFingerprint>,
    ) -> color_eyre::Result<()> {
        let connection = self.connection()?;
        match fingerprint {
            Some(fingerprint) => connection.execute(
                "insert or replace into viewed_files (scope, path, fingerprint, viewed_at_ms)
                 values (?1, ?2, ?3, ?4)",
                params![
                    scope.as_str(),
                    path,
                    fingerprint.to_hex(),
                    super::store::now_ms_i64()
                ],
            )?,
            None => connection.execute(
                "delete from viewed_files where scope = ?1 and path = ?2",
                params![scope.as_str(), path],
            )?,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn fingerprint(value: &str) -> DiffFingerprint {
        DiffFingerprint::from_hex(value).expect("valid hex fingerprint")
    }

    fn temp_store(name: &str) -> (ReviewStore, PathBuf) {
        let path = std::env::temp_dir()
            .join("vigil-viewed-tests")
            .join(format!(
                "{name}-{}.sqlite3",
                super::super::store::now_ms_i64()
            ));
        (ReviewStore::open(path.clone()).expect("store opens"), path)
    }

    #[test]
    fn mark_only_counts_while_the_fingerprint_matches() {
        let mut viewed = ViewedFiles::default();
        viewed.mark("src/lib.rs", fingerprint("aa"));

        assert!(viewed.is_viewed("src/lib.rs", Some(fingerprint("aa"))));
        assert!(!viewed.is_viewed("src/lib.rs", Some(fingerprint("bb"))));
        assert!(!viewed.is_viewed("src/lib.rs", None));
        assert!(!viewed.is_viewed("src/main.rs", Some(fingerprint("aa"))));

        viewed.unmark("src/lib.rs");
        assert!(!viewed.is_viewed("src/lib.rs", Some(fingerprint("aa"))));
    }

    #[test]
    fn scopes_distinguish_review_targets() {
        let root = Path::new("/repo");
        let working_tree = ViewedScope::new(root, &ReviewScope::WorkingTree);
        let branch = ViewedScope::new(
            root,
            &ReviewScope::BranchCompare {
                source_ref: "main".to_string(),
                source_sha: None,
                destination_ref: "feature".to_string(),
                destination_sha: None,
                merge_base: None,
            },
        );

        assert_ne!(working_tree, branch);
        assert_ne!(
            working_tree,
            ViewedScope::new(Path::new("/other"), &ReviewScope::WorkingTree)
        );
    }

    #[test]
    fn store_round_trips_marks_per_scope() {
        let (store, path) = temp_store("round-trip");
        let scope = ViewedScope::new(Path::new("/repo"), &ReviewScope::WorkingTree);
        let other = ViewedScope::new(Path::new("/other"), &ReviewScope::WorkingTree);

        store
            .set_file_viewed(&scope, "src/lib.rs", Some(fingerprint("aa")))
            .expect("mark");
        store
            .set_file_viewed(&scope, "src/main.rs", Some(fingerprint("bb")))
            .expect("mark");
        store
            .set_file_viewed(&scope, "src/main.rs", None)
            .expect("unmark");

        let loaded = store.load_viewed_files(&scope).expect("load");
        assert!(loaded.is_viewed("src/lib.rs", Some(fingerprint("aa"))));
        assert!(!loaded.is_viewed("src/main.rs", Some(fingerprint("bb"))));
        assert_eq!(
            store.load_viewed_files(&other).expect("load other"),
            ViewedFiles::default()
        );

        let _ = fs::remove_file(path);
    }
}
