use super::{
    super::App,
    shell::{build_editor_shell_command, current_editor_command},
    target::EditorOpenTarget,
};

pub(in crate::app) enum AppCommand {
    OpenFileInEditor(String),
    OpenFileInEditorAtLine(String, usize),
}

impl App {
    pub(in crate::app) async fn run_command(
        &mut self,
        command: AppCommand,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let (path, line_number) = match command {
            AppCommand::OpenFileInEditor(path) => (path, None),
            AppCommand::OpenFileInEditorAtLine(path, line_number) => (path, Some(line_number)),
        };
        let target = match self.resolve_editor_open_target(&path, line_number).await {
            Ok(target) => target,
            Err(error) => {
                self.status_message = Some(format!("failed to prepare editor target: {error}"));
                return Ok(());
            }
        };

        if self.chooser_file_path.is_some() {
            self.write_chooser_selection_and_exit(&target).await?;
        } else {
            self.open_file_in_editor(&target, terminal).await?;
        }
        Ok(())
    }

    async fn open_file_in_editor(
        &mut self,
        target: &EditorOpenTarget,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> color_eyre::Result<()> {
        let Some(editor_command) = current_editor_command() else {
            self.status_message =
                Some("Set VISUAL or EDITOR to open files from vigil.".to_string());
            return Ok(());
        };

        let command =
            build_editor_shell_command(&editor_command, &target.full_path, target.line_number);
        let result = self.run_editor_command(command, terminal).await;

        match result {
            Ok(Ok(status)) if status.success() => {
                self.refresh().await?;
                self.status_message = Some(match target.line_number {
                    Some(line_number) => format!("opened {}:{}", target.display_path, line_number),
                    None => format!("opened {}", target.display_path),
                });
            }
            Ok(Ok(status)) => {
                self.status_message = Some(format!(
                    "editor exited with code {}",
                    status.code().unwrap_or(1)
                ));
            }
            Ok(Err(error)) => {
                self.status_message = Some(format!("failed to launch editor: {error}"));
            }
            Err(error) => {
                self.status_message = Some(format!("editor task failed: {error}"));
            }
        }

        Ok(())
    }
}
