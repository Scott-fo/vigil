//! "Viewed" marks for the files in the current review.
//!
//! Marks are loaded once per review target (repository plus working tree,
//! commit, or branch comparison) and saved in the background as they change.
//! A file counts as viewed only while its diff fingerprint in the loaded review
//! snapshot matches the fingerprint recorded with the mark, so marks clear on
//! their own when a file changes. Until the snapshot has loaded, no file is
//! viewed and toggling reports that the diff is still loading.

use tokio::task;

use super::{App, review::review_scope_from_mode};
use crate::{
    event::Event,
    git::DiffFingerprint,
    review::{ReviewStore, ViewedFiles, ViewedScope},
};

impl App {
    pub fn is_file_viewed(&self, path: &str) -> bool {
        self.viewed_files
            .is_viewed(path, self.current_diff_fingerprint(path))
    }

    /// Number of files in the current (filtered) file list marked viewed.
    pub fn viewed_file_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| self.is_file_viewed(&file.path))
            .count()
    }

    fn current_diff_fingerprint(&self, path: &str) -> Option<DiffFingerprint> {
        self.review_diff_snapshot
            .as_ref()?
            .fingerprint_for_file(path)
    }

    /// Loads marks for the current review target if it differs from the one
    /// already loaded. Refreshes within the same target keep in-memory marks.
    pub(in crate::app) fn queue_viewed_files_load(&mut self) {
        let scope = review_scope_from_mode(&self.review_mode)
            .map(|scope| ViewedScope::new(&self.repo_root, &scope));
        if scope == self.viewed_scope {
            return;
        }

        self.viewed_scope = scope.clone();
        self.viewed_files = ViewedFiles::default();
        self.viewed_request_id = self.viewed_request_id.saturating_add(1);
        let Some(scope) = scope else {
            return;
        };

        let request_id = self.viewed_request_id;
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = task::spawn_blocking(move || {
                ReviewStore::open_default()?.load_viewed_files(&scope)
            })
            .await
            .map_err(color_eyre::Report::from)
            .and_then(|result| result)
            .map_err(|error| error.to_string());
            let _ = sender.send(Event::ViewedFilesLoaded { request_id, result });
        }));
    }

    pub(in crate::app) fn handle_viewed_files_loaded(
        &mut self,
        request_id: u64,
        result: Result<ViewedFiles, String>,
    ) -> bool {
        if request_id != self.viewed_request_id {
            return false;
        }

        match result {
            Ok(viewed) => self.viewed_files = viewed,
            Err(error) => {
                self.status_message = Some(format!("failed to load viewed files: {error}"));
            }
        }
        true
    }

    pub(in crate::app) fn handle_viewed_file_saved(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => false,
            Err(error) => {
                self.status_message = Some(format!("failed to save viewed file: {error}"));
                true
            }
        }
    }

    pub(in crate::app) fn toggle_selected_file_viewed(&mut self) {
        let Some(path) = self.selected_file().map(|file| file.path.clone()) else {
            return;
        };
        let viewed = !self.is_file_viewed(&path);
        self.set_file_viewed(&path, viewed);
    }

    /// Marks the selected file viewed, then selects the next unviewed file in
    /// sidebar order, wrapping around.
    pub(in crate::app) async fn mark_viewed_and_select_next(&mut self) -> color_eyre::Result<()> {
        let Some(path) = self.selected_file().map(|file| file.path.clone()) else {
            return Ok(());
        };
        if !self.is_file_viewed(&path) && !self.set_file_viewed(&path, true) {
            return Ok(());
        }

        let visible = self.visible_file_paths();
        let start = visible
            .iter()
            .position(|candidate| *candidate == path)
            .map_or(0, |index| index + 1);
        let next = (0..visible.len())
            .map(|offset| &visible[(start + offset) % visible.len()])
            .find(|candidate| !self.is_file_viewed(candidate))
            .cloned();
        match next {
            Some(next) => self.select_file_by_path(&next).await?,
            None => self.status_message = Some("all files viewed".to_string()),
        }
        Ok(())
    }

    /// Updates the in-memory mark and persists it in the background. Returns
    /// false, with a status message, when the file's diff is not loaded yet.
    fn set_file_viewed(&mut self, path: &str, viewed: bool) -> bool {
        let (Some(scope), Some(fingerprint)) = (
            self.viewed_scope.clone(),
            self.current_diff_fingerprint(path),
        ) else {
            self.status_message = Some("diff still loading; try again in a moment".to_string());
            return false;
        };

        if viewed {
            self.viewed_files.mark(path, fingerprint);
        } else {
            self.viewed_files.unmark(path);
        }

        let path = path.to_string();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = task::spawn_blocking(move || {
                ReviewStore::open_default()?.set_file_viewed(
                    &scope,
                    &path,
                    viewed.then_some(fingerprint),
                )
            })
            .await
            .map_err(color_eyre::Report::from)
            .and_then(|result| result)
            .map_err(|error| error.to_string());
            let _ = sender.send(Event::ViewedFileSaved(result));
        }));
        true
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::*;
    use crate::git::{self, FileEntry};

    fn snapshot(body: &str) -> Arc<git::ReviewDiffSnapshot> {
        let diff = format!(
            "diff --git a/src/lib.rs b/src/lib.rs\n\
             --- a/src/lib.rs\n\
             +++ b/src/lib.rs\n\
             @@ -1,1 +1,2 @@\n \
             fn existing() {{}}\n\
             +{body}\n"
        );
        Arc::new(git::ReviewDiffSnapshot::from_diff_text(&diff, None).expect("diff parses"))
    }

    fn app_with_snapshot(body: &str) -> App {
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-viewed-tests"));
        app.files = vec![FileEntry {
            status: " M".to_string(),
            path: "src/lib.rs".to_string(),
            label: "lib.rs".to_string(),
            filetype: Some("rust"),
        }];
        app.review_diff_snapshot = Some(snapshot(body));
        app.viewed_scope = Some(ViewedScope::new(
            &app.repo_root,
            &crate::review::ReviewScope::WorkingTree,
        ));
        app
    }

    #[test]
    fn marking_a_file_counts_it_until_its_diff_changes() {
        let mut app = app_with_snapshot("fn added() {}");
        let fingerprint = app
            .current_diff_fingerprint("src/lib.rs")
            .expect("snapshot fingerprints the file");
        app.viewed_files.mark("src/lib.rs", fingerprint);

        assert!(app.is_file_viewed("src/lib.rs"));
        assert_eq!(app.viewed_file_count(), 1);

        app.review_diff_snapshot = Some(snapshot("fn changed() {}"));
        assert!(!app.is_file_viewed("src/lib.rs"));
        assert_eq!(app.viewed_file_count(), 0);
    }

    #[test]
    fn toggling_without_a_loaded_diff_reports_instead_of_marking() {
        let mut app = app_with_snapshot("fn added() {}");
        app.review_diff_snapshot = None;
        app.selected_file_index = 0;

        app.toggle_selected_file_viewed();

        assert!(!app.is_file_viewed("src/lib.rs"));
        assert_eq!(
            app.status_message.as_deref(),
            Some("diff still loading; try again in a moment")
        );
    }

    #[test]
    fn footer_reports_viewed_count() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = app_with_snapshot("fn added() {}");
        let fingerprint = app.current_diff_fingerprint("src/lib.rs").unwrap();
        app.viewed_files.mark("src/lib.rs", fingerprint);
        let mut terminal = Terminal::new(TestBackend::new(140, 10)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &mut app))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let footer: String = (0..140u16)
            .map(|column| buffer[(column, 9)].symbol())
            .collect();
        assert!(footer.contains("1/1 viewed"), "footer was: {footer}");
    }

    #[test]
    fn stale_load_results_are_ignored() {
        let mut app = app_with_snapshot("fn added() {}");
        app.viewed_request_id = 2;
        let mut loaded = ViewedFiles::default();
        loaded.mark(
            "src/lib.rs",
            app.current_diff_fingerprint("src/lib.rs").unwrap(),
        );

        assert!(!app.handle_viewed_files_loaded(1, Ok(loaded.clone())));
        assert!(!app.is_file_viewed("src/lib.rs"));
        assert!(app.handle_viewed_files_loaded(2, Ok(loaded)));
        assert!(app.is_file_viewed("src/lib.rs"));
    }
}
