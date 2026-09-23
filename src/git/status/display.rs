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

/// One-letter summary of a porcelain status pair, such as `M` for `" M"` or
/// `?` for untracked files. Returns an empty string for unknown codes.
pub fn status_label(status: &str) -> &'static str {
    if status == "??" {
        return "?";
    }

    for (marker, label) in [
        ('D', "D"),
        ('A', "A"),
        ('M', "M"),
        ('R', "R"),
        ('C', "C"),
        ('U', "U"),
    ] {
        if status.contains(marker) {
            return label;
        }
    }

    ""
}

#[cfg(test)]
mod tests {
    use crate::theme;

    use super::{status_color, status_label};

    #[test]
    fn status_label_normalizes_porcelain_columns() {
        assert_eq!(status_label(" M"), "M");
        assert_eq!(status_label("M "), "M");
        assert_eq!(status_label("MM"), "M");
        assert_eq!(status_label("A "), "A");
        assert_eq!(status_label("??"), "?");
        assert_eq!(status_label(" D"), "D");
        assert_eq!(status_label("  "), "");
    }

    #[test]
    fn status_color_treats_added_files_as_success() {
        assert_eq!(status_color("A "), theme::active_palette().success);
        assert_eq!(status_color("??"), theme::active_palette().success);
    }
}
