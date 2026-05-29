use std::path::Path;

pub(super) fn current_editor_command() -> Option<String> {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

pub(super) fn build_editor_shell_command(
    editor_command: &str,
    full_path: &Path,
    line_number: Option<usize>,
) -> String {
    let quoted_path = quote_shell_arg(&full_path.to_string_lossy());
    match line_number {
        Some(line_number) if editor_supports_plus_line(editor_command) => {
            format!("{editor_command} +{line_number} {quoted_path}")
        }
        _ => format!("{editor_command} {quoted_path}"),
    }
}

fn quote_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn editor_supports_plus_line(editor_command: &str) -> bool {
    let editor = editor_command.split_whitespace().next().unwrap_or_default();
    let binary = editor.rsplit('/').next().unwrap_or(editor);
    matches!(binary, "nvim" | "vim" | "vi" | "vimdiff" | "nvim-qt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_command_quotes_paths_and_adds_line_for_vim_family() {
        let command =
            build_editor_shell_command("vim", Path::new("/tmp/project/file's name.rs"), Some(42));

        assert_eq!(command, "vim +42 '/tmp/project/file'\"'\"'s name.rs'");
    }

    #[test]
    fn shell_command_omits_line_for_unknown_editor() {
        let command =
            build_editor_shell_command("code --wait", Path::new("/tmp/project/main.rs"), Some(7));

        assert_eq!(command, "code --wait '/tmp/project/main.rs'");
    }
}
