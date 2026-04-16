use super::*;

struct FileSearchCandidate {
    index: usize,
    haystack: String,
}

impl AsRef<str> for FileSearchCandidate {
    fn as_ref(&self) -> &str {
        &self.haystack
    }
}

impl App {
    pub(super) async fn open_file_search_modal(&mut self) -> color_eyre::Result<()> {
        if self.file_search_modal_open {
            return Ok(());
        }

        self.file_search_modal_open = true;
        self.file_search_query.clear();
        self.file_search_selected_index = 0;
        self.file_search_initial_path = self.selected_file().map(|file| file.path.clone());
        self.sync_file_search_selection_after_query_change().await
    }

    pub(super) async fn cancel_file_search_modal(&mut self) -> color_eyre::Result<()> {
        let initial_path = self.file_search_initial_path.clone();
        self.close_file_search_modal();
        if let Some(path) = initial_path {
            self.select_file_by_path(&path).await?;
        }
        Ok(())
    }

    pub(super) fn confirm_file_search_modal(&mut self) {
        if self.selected_file_search_path().is_none() {
            return;
        }
        self.close_file_search_modal();
    }

    fn close_file_search_modal(&mut self) {
        self.file_search_modal_open = false;
        self.file_search_query.clear();
        self.file_search_selected_index = 0;
        self.file_search_initial_path = None;
    }

    pub fn filtered_file_search_indices(&mut self) -> Vec<usize> {
        let query = self.file_search_query.trim();
        if query.is_empty() {
            return (0..self.files.len()).collect();
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let candidates = self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| FileSearchCandidate {
                index,
                haystack: format!("{} {} {}", file.path, file.label, file.status),
            })
            .collect::<Vec<_>>();

        pattern
            .match_list(candidates, &mut self.file_search_matcher)
            .into_iter()
            .map(|(candidate, _score)| candidate.index)
            .collect()
    }

    pub(super) async fn sync_file_search_selection_after_query_change(
        &mut self,
    ) -> color_eyre::Result<()> {
        let filtered = self.filtered_file_search_indices();
        if filtered.is_empty() {
            self.file_search_selected_index = 0;
            return Ok(());
        }

        let selected_path = self.selected_file().map(|file| file.path.clone());
        if let Some(selected_path) = selected_path
            && let Some(index) = filtered
                .iter()
                .position(|file_index| self.files[*file_index].path == selected_path)
        {
            self.file_search_selected_index = index;
            return Ok(());
        }

        self.file_search_selected_index = 0;
        self.preview_file_search_selection().await
    }

    pub(super) async fn move_file_search_selection(
        &mut self,
        delta: i32,
    ) -> color_eyre::Result<()> {
        let filtered_len = self.filtered_file_search_indices().len();
        if filtered_len == 0 {
            self.file_search_selected_index = 0;
            return Ok(());
        }

        let current = self.file_search_selected_index.min(filtered_len - 1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            current.saturating_add(delta as usize)
        }
        .min(filtered_len - 1);
        self.file_search_selected_index = next;
        self.preview_file_search_selection().await
    }

    pub(crate) fn selected_file_search_path(&mut self) -> Option<String> {
        self.filtered_file_search_indices()
            .get(self.file_search_selected_index)
            .and_then(|index| self.files.get(*index))
            .map(|file| file.path.clone())
    }

    async fn preview_file_search_selection(&mut self) -> color_eyre::Result<()> {
        let Some(path) = self.selected_file_search_path() else {
            return Ok(());
        };

        self.select_file_by_path(&path).await
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn build_test_app() -> App {
        let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"));
        app.files = vec![
            FileEntry {
                status: "M ".to_string(),
                path: "src/app/mod.rs".to_string(),
                label: "mod.rs".to_string(),
                filetype: Some("rust"),
            },
            FileEntry {
                status: "A ".to_string(),
                path: "src/ui/sidebar.rs".to_string(),
                label: "sidebar.rs".to_string(),
                filetype: Some("rust"),
            },
        ];
        app.rebuild_sidebar_items();
        app
    }

    #[test]
    fn file_search_filters_by_path_fragments() {
        let mut app = build_test_app();
        app.file_search_query = "side".to_string();

        assert_eq!(app.filtered_file_search_indices(), vec![1]);
    }

    #[tokio::test]
    async fn cancelling_file_search_restores_initial_selection() {
        let mut app = build_test_app();
        app.open_file_search_modal()
            .await
            .expect("modal should open");
        app.move_file_search_selection(1)
            .await
            .expect("selection should preview");

        assert_eq!(
            app.selected_file().map(|file| file.path.as_str()),
            Some("src/ui/sidebar.rs")
        );

        app.cancel_file_search_modal()
            .await
            .expect("cancel should restore");

        assert_eq!(
            app.selected_file().map(|file| file.path.as_str()),
            Some("src/app/mod.rs")
        );
        assert!(!app.file_search_modal_open);
    }
}
