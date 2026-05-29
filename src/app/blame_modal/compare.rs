use super::super::{App, ReviewMode};

impl App {
    pub(in crate::app) async fn open_blame_commit_compare(&mut self) -> color_eyre::Result<()> {
        let Some(details) = self.blame_details.clone() else {
            return Ok(());
        };

        let Some(selection) = details.compare_selection else {
            self.blame_error = Some("No committed change is available for this line.".to_string());
            return Ok(());
        };

        self.close_blame_modal();
        self.review_mode = ReviewMode::CommitCompare(selection);
        self.refresh().await
    }
}
