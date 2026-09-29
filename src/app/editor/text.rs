//! Editing a modal's text in `$VISUAL` / `$EDITOR`.
//!
//! The text goes to a temporary Markdown file, the TUI is suspended while
//! the editor runs, and whatever the file holds afterwards replaces the
//! modal's text. An editor that exits with an error leaves the text as it
//! was.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::fs;

use super::{
    super::App,
    shell::{build_editor_shell_command, current_editor_command},
};

impl App {
    pub(super) async fn edit_modal_text_in_editor(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let Some(text) = self.pull_request_modal_text().map(|text| text.text()) else {
            return Ok(());
        };
        let Some(editor_command) = current_editor_command() else {
            self.status_message =
                Some("Set VISUAL or EDITOR to write comments in an editor.".to_string());
            return Ok(());
        };

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join("vigil");
        let path = directory.join(format!(
            "comment-{}-{}.md",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(error) = async {
            fs::create_dir_all(&directory).await?;
            fs::write(&path, format!("{text}\n")).await
        }
        .await
        {
            self.status_message = Some(format!("failed to prepare the editor file: {error}"));
            return Ok(());
        }

        let command = build_editor_shell_command(&editor_command, &path, None);
        let result = self.run_editor_command(command, terminal).await;
        let edited = fs::read_to_string(&path).await;
        let _ = fs::remove_file(&path).await;

        match (result, edited) {
            (Ok(Ok(status)), Ok(edited)) if status.success() => {
                if let Some(text) = self.pull_request_modal_text() {
                    text.set_text(edited.trim_end_matches(['\n', '\r']));
                }
            }
            (Ok(Ok(status)), Err(error)) if status.success() => {
                self.status_message = Some(format!("failed to read the edited text: {error}"));
            }
            (Ok(Ok(status)), _) => {
                self.status_message = Some(format!(
                    "editor exited with code {}; text unchanged",
                    status.code().unwrap_or(1)
                ));
            }
            (Ok(Err(error)), _) => {
                self.status_message = Some(format!("failed to launch editor: {error}"));
            }
            (Err(error), _) => {
                self.status_message = Some(format!("editor task failed: {error}"));
            }
        }
        Ok(())
    }
}
