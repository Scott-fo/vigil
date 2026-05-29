use crate::theme;

pub fn status_color(status: &str) -> ratatui::style::Color {
    let palette = theme::active_palette();
    if status == "??" {
        return palette.success;
    }
    if status.contains('D') {
        return palette.error;
    }
    if status.contains('R') || status.contains('C') {
        return palette.secondary;
    }
    if status.contains('A') {
        return palette.success;
    }
    if status.contains('M') {
        return palette.warning;
    }
    palette.text_muted
}

#[cfg(test)]
mod tests {
    use crate::theme;

    use super::status_color;

    #[test]
    fn status_color_treats_added_files_as_success() {
        assert_eq!(status_color("A "), theme::active_palette().success);
        assert_eq!(status_color("??"), theme::active_palette().success);
    }
}
