use super::{SyntaxToken, push_syntax_token};

pub(super) fn highlight_markdown_line_tokens(line: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let indent_len = line.len() - line.trim_start().len();
    let (_, trimmed) = line.split_at(indent_len);
    push_syntax_token(&mut tokens, 0, indent_len, None);

    if trimmed.is_empty() {
        return tokens;
    }

    let bare = trimmed.trim();
    if bare.len() >= 3 && bare.chars().all(|ch| matches!(ch, '-' | '*' | '_')) {
        push_syntax_token(&mut tokens, indent_len, line.len(), Some("operator"));
        return tokens;
    }

    for fence in ["```", "~~~"] {
        if let Some(rest) = trimmed.strip_prefix(fence) {
            push_syntax_token(
                &mut tokens,
                indent_len,
                indent_len + fence.len(),
                Some("markup.raw"),
            );
            let ws_len = rest.len() - rest.trim_start().len();
            let info_start = indent_len + fence.len() + ws_len;
            push_syntax_token(&mut tokens, indent_len + fence.len(), info_start, None);
            push_syntax_token(&mut tokens, info_start, line.len(), Some("label"));
            return tokens;
        }
    }

    if let Some(rest) = trimmed.strip_prefix("> ") {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + 2,
            Some("markup.quote"),
        );
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + indent_len + 2,
                    end: token.end + indent_len + 2,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    let heading_marker_len = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&heading_marker_len) && trimmed[heading_marker_len..].starts_with(' ') {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + heading_marker_len,
            Some("markup.heading"),
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len,
            indent_len + heading_marker_len + 1,
            None,
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len + 1,
            line.len(),
            Some("markup.heading"),
        );
        return tokens;
    }

    if let Some(prefix_len) = markdown_list_prefix_len(trimmed) {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + prefix_len,
            Some("markup.list"),
        );
        let rest = &trimmed[prefix_len..];
        let rest_start = indent_len + prefix_len;
        if let Some(task_rest) = rest.strip_prefix("[ ] ") {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.unchecked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        if let Some(task_rest) = rest
            .strip_prefix("[x] ")
            .or_else(|| rest.strip_prefix("[X] "))
        {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.checked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + rest_start,
                    end: token.end + rest_start,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    tokens.extend(
        highlight_markdown_inline_tokens(trimmed)
            .into_iter()
            .map(|token| SyntaxToken {
                start: token.start + indent_len,
                end: token.end + indent_len,
                highlight_name: token.highlight_name,
            }),
    );
    tokens
}

fn highlight_markdown_inline_tokens(text: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < text.len() {
        let remainder = &text[index..];

        if let Some(rest) = remainder.strip_prefix('`')
            && let Some(end) = rest.find('`')
        {
            let code_end = index + 1 + end + 1;
            push_syntax_token(&mut tokens, index, code_end, Some("markup.raw"));
            index = code_end;
            continue;
        }

        if let Some(label_end) = remainder.find("](")
            && remainder.starts_with('[')
            && let Some(url_end) = remainder[label_end + 2..].find(')')
        {
            let label_text_end = index + label_end + 1;
            let url_start = index + label_end + 2;
            let url_end = url_start + url_end;
            push_syntax_token(&mut tokens, index, index + 1, None);
            push_syntax_token(
                &mut tokens,
                index + 1,
                label_text_end,
                Some("markup.link.label"),
            );
            push_syntax_token(&mut tokens, label_text_end, label_text_end + 2, None);
            push_syntax_token(&mut tokens, url_start, url_end, Some("markup.link.url"));
            push_syntax_token(&mut tokens, url_end, url_end + 1, None);
            index = url_end + 1;
            continue;
        }

        let mut next_break = remainder.len();
        for needle in ["`", "["] {
            if let Some(found) = remainder.find(needle) {
                next_break = next_break.min(found);
            }
        }
        if next_break == 0 {
            next_break = remainder.chars().next().map(char::len_utf8).unwrap_or(1);
        }
        push_syntax_token(&mut tokens, index, index + next_break, None);
        index += next_break;
    }

    tokens
}

fn markdown_list_prefix_len(text: &str) -> Option<usize> {
    for marker in ["- ", "* ", "+ "] {
        if text.starts_with(marker) {
            return Some(marker.len());
        }
    }

    let digit_count = text.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count > 0 {
        let remainder = &text[digit_count..];
        if remainder.starts_with(". ") || remainder.starts_with(") ") {
            return Some(digit_count + 2);
        }
    }

    None
}
