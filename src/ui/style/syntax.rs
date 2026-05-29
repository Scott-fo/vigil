use ratatui::style::{Modifier, Style};

use super::{palette, text_color};

#[inline]
pub fn syntax_style(name: &str, fallback: Style) -> Style {
    let palette = palette();
    let style = match name {
        "comment" | "comment.documentation" => Style::new().fg(palette.syntax_comment),
        "markup.quote" => Style::new().fg(palette.syntax_comment),
        "keyword"
        | "keyword.conditional"
        | "keyword.conditional.ternary"
        | "keyword.coroutine"
        | "keyword.debug"
        | "keyword.directive"
        | "keyword.exception"
        | "keyword.function"
        | "keyword.import"
        | "keyword.modifier"
        | "keyword.operator"
        | "keyword.repeat"
        | "keyword.return"
        | "keyword.type"
        | "conditional"
        | "exception"
        | "repeat" => Style::new()
            .fg(palette.syntax_keyword)
            .add_modifier(Modifier::BOLD),
        "function"
        | "function.builtin"
        | "function.call"
        | "function.method"
        | "function.method.call"
        | "function.method.builtin"
        | "function.macro"
        | "function.special"
        | "constructor"
        | "constructor.builtin"
        | "method"
        | "method.call" => Style::new().fg(palette.syntax_function),
        "label"
        | "module"
        | "module.builtin"
        | "namespace"
        | "variable.parameter"
        | "property"
        | "property.definition"
        | "parameter"
        | "field" => Style::new().fg(palette.syntax_variable),
        "constant" | "constant.builtin" => Style::new().fg(palette.syntax_number),
        "variable" | "variable.member" => Style::new(),
        "variable.builtin" => Style::new().fg(palette.syntax_variable),
        "string"
        | "character"
        | "character.special"
        | "markup.link.url"
        | "markup.raw"
        | "markup.raw.block"
        | "string.escape"
        | "string.regexp"
        | "string.special"
        | "string.special.url"
        | "string.special.key"
        | "string.special.path"
        | "string.special.regex"
        | "string.special.symbol"
        | "string.special.uri" => Style::new().fg(palette.syntax_string),
        "number" | "number.float" | "boolean" => Style::new().fg(palette.syntax_number),
        "type" | "type.builtin" | "type.definition" | "type.qualifier" | "attribute"
        | "attribute.builtin" | "tag.attribute" | "markup.heading" | "markup.heading.1"
        | "markup.heading.2" | "markup.heading.3" | "markup.heading.4" | "markup.heading.5"
        | "markup.heading.6" => Style::new().fg(palette.syntax_type),
        "markup.link" | "markup.link.label" => Style::new().fg(palette.syntax_function),
        "markup.list" | "markup.list.checked" | "markup.list.unchecked" => {
            Style::new().fg(palette.syntax_keyword)
        }
        "operator" | "delimiter" => Style::new().fg(palette.syntax_operator),
        "punctuation"
        | "punctuation.delimiter"
        | "punctuation.bracket"
        | "punctuation.special"
        | "tag.delimiter"
        | "embedded" => Style::new().fg(text_color()),
        "property.builtin" | "tag" | "tag.builtin" | "tag.error" => {
            Style::new().fg(palette.syntax_function)
        }
        _ => fallback,
    };
    fallback.patch(style)
}
