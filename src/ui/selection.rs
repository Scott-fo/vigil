use ratatui::{
    style::Modifier,
    text::{Line, Span},
};

#[inline]
pub(super) fn highlight_line(line: &Line<'static>) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .cloned()
            .map(|span| Span::styled(span.content, span.style.add_modifier(Modifier::REVERSED)))
            .collect::<Vec<_>>(),
    )
    .style(line.style.add_modifier(Modifier::REVERSED))
}

#[inline]
pub(super) fn highlight_line_range(
    line: &Line<'static>,
    start: usize,
    end: usize,
) -> Line<'static> {
    if start >= end {
        return line.clone();
    }

    let mut highlighted = Vec::new();
    let mut column = 0usize;

    for span in &line.spans {
        let content = span.content.as_ref();
        let span_width = unicode_width::UnicodeWidthStr::width(content);
        if span_width == 0 {
            highlighted.push(span.clone());
            continue;
        }

        let span_start = column;
        let span_end = column + span_width;
        column = span_end;

        if span_end <= start || span_start >= end {
            highlighted.push(span.clone());
            continue;
        }

        let highlight_start = start.saturating_sub(span_start).min(span_width);
        let highlight_end = end.saturating_sub(span_start).min(span_width);

        let prefix = slice_text_by_width(content, 0, highlight_start);
        if !prefix.is_empty() {
            highlighted.push(Span::styled(prefix, span.style));
        }

        let selected = slice_text_by_width(content, highlight_start, highlight_end);
        if !selected.is_empty() {
            highlighted.push(Span::styled(
                selected,
                span.style.add_modifier(Modifier::REVERSED),
            ));
        }

        let suffix = slice_text_by_width(content, highlight_end, span_width);
        if !suffix.is_empty() {
            highlighted.push(Span::styled(suffix, span.style));
        }
    }

    Line::from(highlighted).style(line.style)
}

#[inline]
fn slice_text_by_width(content: &str, start: usize, end: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;

    for ch in content.chars() {
        let Some(ch_width) = unicode_width::UnicodeWidthChar::width(ch) else {
            continue;
        };
        if ch_width == 0 {
            continue;
        }

        let next_width = used + ch_width;
        if next_width <= start {
            used = next_width;
            continue;
        }
        if used >= end {
            break;
        }

        result.push(ch);
        used = next_width;
    }

    result
}
