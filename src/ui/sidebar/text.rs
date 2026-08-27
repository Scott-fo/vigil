use ratatui::style::Color;
use unicode_width::UnicodeWidthChar;

use super::super::text_muted_color;

pub(super) fn devicon_for_path(path: &str) -> Option<(char, Color)> {
    let icon = devicons::icon_for_file(path, &Some(devicons::Theme::Dark));
    if icon.icon == '*' {
        return None;
    }

    Some((
        icon.icon,
        hex_color(icon.color).unwrap_or_else(text_muted_color),
    ))
}

fn hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }

    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(red, green, blue))
}

pub(super) fn sidebar_status_label(status: &str) -> String {
    if status == "??" {
        return "?".to_string();
    }

    for marker in ['D', 'A', 'M', 'R', 'C', 'U'] {
        if status.contains(marker) {
            return marker.to_string();
        }
    }

    status.trim().to_string()
}

pub(super) fn file_label_width(
    width: u16,
    indent: &str,
    indicator: &str,
    review_marker: &str,
    status: &str,
) -> usize {
    let reserved_width = display_width(indent)
        .saturating_add(display_width(indicator))
        .saturating_add(display_width(review_marker))
        .saturating_add(display_width(status))
        .saturating_add(if review_marker.is_empty() { 2 } else { 3 });
    (width as usize).saturating_sub(reserved_width).max(1)
}

pub(super) fn truncate_middle(value: &str, max_width: usize) -> String {
    if display_width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let left_width = max_width.saturating_sub(1) / 2;
    let right_width = max_width.saturating_sub(1).saturating_sub(left_width);
    format!(
        "{}…{}",
        take_width_prefix(value, left_width),
        take_width_suffix(value, right_width)
    )
}

fn take_width_prefix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut result = String::new();
    for char in value.chars() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        result.push(char);
    }
    result
}

fn take_width_suffix(value: &str, max_width: usize) -> String {
    let mut width = 0usize;
    let mut chars = Vec::new();
    for char in value.chars().rev() {
        let char_width = char.width().unwrap_or(0);
        if width.saturating_add(char_width) > max_width {
            break;
        }
        width = width.saturating_add(char_width);
        chars.push(char);
    }
    chars.into_iter().rev().collect()
}

pub(super) fn status_gap(
    width: u16,
    indent: &str,
    indicator: &str,
    label: &str,
    review_marker: &str,
    status: &str,
) -> String {
    if status.is_empty() && review_marker.is_empty() {
        return String::new();
    }

    let occupied_width = display_width(indent)
        .saturating_add(display_width(indicator))
        .saturating_add(display_width(label))
        .saturating_add(display_width(review_marker))
        .saturating_add(display_width(status))
        .saturating_add(if review_marker.is_empty() { 1 } else { 2 });
    let row_width = width as usize;
    let gap_width = row_width.saturating_sub(occupied_width).max(1);
    " ".repeat(gap_width)
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|char| char.width().unwrap_or(0))
        .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devicon_resolves_known_types_and_skips_default_icon() {
        let tsx = devicon_for_path("src/components/Button.tsx").expect("tsx should have a devicon");
        assert_ne!(tsx.0, '*');

        assert!(devicon_for_path("src/components/Button.unknown-vigil-type").is_none());
    }

    #[test]
    fn parses_devicon_hex_colors() {
        assert_eq!(hex_color("#1354BF"), Some(Color::Rgb(19, 84, 191)));
        assert_eq!(hex_color("1354BF"), None);
        assert_eq!(hex_color("#1354"), None);
    }

    #[test]
    fn sidebar_status_label_normalizes_git_status_columns() {
        assert_eq!(sidebar_status_label(" M"), "M");
        assert_eq!(sidebar_status_label("M "), "M");
        assert_eq!(sidebar_status_label("MM"), "M");
        assert_eq!(sidebar_status_label("A "), "A");
        assert_eq!(sidebar_status_label("??"), "?");
        assert_eq!(sidebar_status_label(" D"), "D");
    }

    #[test]
    fn truncate_middle_keeps_extension_visible() {
        assert_eq!(
            truncate_middle("JavaScriptSyntaxHighlighter.tsx", 18),
            "JavaScri…ghter.tsx"
        );
        assert_eq!(truncate_middle("short.ts", 18), "short.ts");
        assert_eq!(truncate_middle("short.ts", 1), "…");
    }

    #[test]
    fn status_gap_keeps_status_visible() {
        let (tsx_icon, _) =
            devicon_for_path("file.tsx").expect("tsx should have a devicon for width tests");
        let indicator = format!("{tsx_icon} ");

        assert!(!status_gap(12, "", &indicator, "file.tsx", "", "M").is_empty());
        assert_eq!(status_gap(4, "", &indicator, "file.tsx", "", "M"), " ");
        assert_eq!(status_gap(12, "", &indicator, "file.tsx", "", ""), "");
    }

    #[test]
    fn file_label_width_reserves_status_gap_and_trailing_space() {
        let (tsx_icon, _) =
            devicon_for_path("file.tsx").expect("tsx should have a devicon for width tests");
        let indicator = format!("{tsx_icon} ");
        let label_width = file_label_width(24, "", &indicator, "●", "M");
        let label = truncate_middle("JavaScriptSyntaxHighlighter.tsx", label_width);
        let gap = status_gap(24, "", &indicator, &label, "●", "M");
        let rendered_width = display_width(&indicator)
            + display_width(&label)
            + display_width(&gap)
            + display_width("●")
            + 1
            + display_width("M")
            + 1;

        assert!(display_width(&label) <= label_width);
        assert_eq!(rendered_width, 24);
    }
}
