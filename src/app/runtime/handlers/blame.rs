use crate::git;

use super::super::super::App;

impl App {
    pub(in crate::app) fn handle_blame_loaded(
        &mut self,
        request_id: u64,
        result: Result<git::BlameCommitDetails, String>,
    ) -> bool {
        if request_id != self.blame_request_id || !self.blame_modal_open {
            return false;
        }

        self.blame_loading = false;
        self.blame_load_task = None;

        match result {
            Ok(details) => {
                self.blame_target = Some(details.target.clone());
                self.blame_details = Some(details);
                self.blame_error = None;
            }
            Err(error) => {
                self.blame_details = None;
                self.blame_error = Some(error);
            }
        }
        true
    }
}
