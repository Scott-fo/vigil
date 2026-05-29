use super::{CodeColumnType, HunkLineType, LineEndingType, ParsedLine};

pub fn clean_last_newline(contents: &str) -> String {
    if let Some(stripped) = contents.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = contents.strip_suffix('\n') {
        stripped.to_string()
    } else {
        contents.to_string()
    }
}

pub fn get_line_ending_type(content: &str) -> LineEndingType {
    if content.contains("\r\n") {
        LineEndingType::CRLF
    } else if content.contains('\r') {
        LineEndingType::CR
    } else if content.contains('\n') {
        LineEndingType::LF
    } else {
        LineEndingType::None
    }
}

pub fn parse_line_type(line: &str) -> Option<ParsedLine> {
    let first_char = line.chars().next()?;
    let line_type = match first_char {
        ' ' => HunkLineType::Context,
        '\\' => HunkLineType::Metadata,
        '+' => HunkLineType::Addition,
        '-' => HunkLineType::Deletion,
        _ => return None,
    };
    let processed_line = line.get(first_char.len_utf8()..).unwrap_or_default();
    Some(ParsedLine {
        line: if processed_line.is_empty() {
            "\n".to_string()
        } else {
            processed_line.to_string()
        },
        line_type,
    })
}

pub fn get_hunk_separator_slot_name(column_type: CodeColumnType, hunk_index: usize) -> String {
    let column_type = match column_type {
        CodeColumnType::Unified => "unified",
        CodeColumnType::Additions => "additions",
        CodeColumnType::Deletions => "deletions",
    };
    format!("hunk-separator-{column_type}-{hunk_index}")
}
fn trim_line_end(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

pub(super) fn line_without_ending(line: &str) -> &str {
    trim_line_end(line)
}
