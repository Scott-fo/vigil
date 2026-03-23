use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Widget},
};

const VIGIL_LOGO: [&str; 6] = [
    "██╗   ██╗ ██╗  ██████╗  ██╗ ██╗     ",
    "██║   ██║ ██║ ██╔════╝  ██║ ██║     ",
    "██║   ██║ ██║ ██║  ███╗ ██║ ██║     ",
    "╚██╗ ██╔╝ ██║ ██║   ██║ ██║ ██║     ",
    " ╚████╔╝  ██║ ╚██████╔╝  ██║ ███████╗",
    "  ╚═══╝   ╚═╝  ╚═════╝   ╚═╝ ╚══════╝",
];

pub struct Splash<'a> {
    error: Option<&'a str>,
    text_style: Style,
    text_muted_style: Style,
}

impl<'a> Splash<'a> {
    pub fn new(error: Option<&'a str>, text_style: Style, text_muted_style: Style) -> Self {
        Self {
            error,
            text_style,
            text_muted_style,
        }
    }

    fn subtitle(&self) -> Cow<'a, str> {
        match self.error {
            None => Cow::Borrowed("No changed files in working tree"),
            Some(message) if is_not_git_repository_error(message) => {
                Cow::Borrowed("Not a git repo, init to use vigil.")
            }
            Some(message) => Cow::Borrowed(message),
        }
    }

    fn show_init_hint(&self) -> bool {
        self.error.is_some_and(is_not_git_repository_error)
    }
}

impl Widget for Splash<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = VIGIL_LOGO
            .iter()
            .map(|line| {
                Line::from(Span::styled(
                    *line,
                    self.text_style.add_modifier(Modifier::BOLD),
                ))
            })
            .collect::<Vec<_>>();

        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            self.subtitle().into_owned(),
            self.text_muted_style,
        )));

        if self.show_init_hint() {
            lines.push(Line::from(Span::styled(
                "Press i to git init.",
                self.text_muted_style,
            )));
        }

        let content_height = lines.len() as u16;
        let y = area
            .y
            .saturating_add(area.height.saturating_sub(content_height) / 2);
        let content_area = Rect::new(area.x, y, area.width, content_height.min(area.height));

        Paragraph::new(Text::from(lines))
            .alignment(Alignment::Center)
            .render(content_area, buf);
    }
}

pub fn is_not_git_repository_error(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("not a git repository")
}

#[cfg(test)]
mod tests {
    use super::is_not_git_repository_error;

    #[test]
    fn detects_not_git_repository_errors() {
        assert!(is_not_git_repository_error(
            "fatal: not a git repository (or any of the parent directories): .git"
        ));
    }

    #[test]
    fn ignores_other_git_errors() {
        assert!(!is_not_git_repository_error(
            "fatal: ambiguous argument 'HEAD': unknown revision"
        ));
    }
}
