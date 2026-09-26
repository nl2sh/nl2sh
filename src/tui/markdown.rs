use super::theme::Theme;
use ratatui::{
    style::{Color, Modifier},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) fn render(source: &str, width: usize, theme: Theme, ascii: bool) -> Vec<Line<'static>> {
    let source_lines: Vec<&str> = source.lines().collect();
    let mut rendered = Vec::new();
    let mut index = 0;
    let mut code_language: Option<&str> = None;
    while index < source_lines.len() {
        let line = source_lines[index];
        if line.trim_start().starts_with("```") {
            code_language = if code_language.is_some() {
                None
            } else {
                Some(line.trim_start().trim_start_matches('`').trim())
            };
            index += 1;
            continue;
        }
        if let Some(language) = code_language {
            rendered.push(Line::from(code_spans(line, language, theme)));
            index += 1;
            continue;
        }
        if index + 1 < source_lines.len()
            && parse_table_row(line).is_some()
            && is_table_separator(source_lines[index + 1])
        {
            let start = index;
            index += 2;
            while index < source_lines.len() && parse_table_row(source_lines[index]).is_some() {
                index += 1;
            }
            rendered.extend(render_table(
                &source_lines[start..index],
                width,
                theme,
                ascii,
            ));
            continue;
        }
        rendered.push(render_line(line, theme, ascii));
        index += 1;
    }
    if source.is_empty() {
        rendered.push(Line::default());
    }
    rendered
}

fn code_spans(line: &str, language: &str, theme: Theme) -> Vec<Span<'static>> {
    let language = language.split_whitespace().next().unwrap_or("");
    let language = match language.to_ascii_lowercase().as_str() {
        "sh" | "shell" | "bash" | "zsh" => "shell",
        "rs" | "rust" => "rust",
        "py" | "python" => "python",
        "js" | "jsx" | "javascript" | "ts" | "tsx" | "typescript" => "javascript",
        "json" => "json",
        _ => "",
    };
    let code_style = |color| theme.style(color).bg(theme.background_alt);
    if language.is_empty() {
        return vec![Span::styled(
            line.to_owned(),
            code_style(theme.text_primary),
        )];
    }
    let mut spans = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        let first = rest.chars().next().unwrap_or_default();
        if first == '#' && (language == "shell" || language == "python")
            || rest.starts_with("//") && (language == "rust" || language == "javascript")
        {
            spans.push(Span::styled(rest.to_owned(), code_style(theme.text_muted)));
            break;
        }
        if first == '"' || first == '\'' && language != "json" {
            let quote = first;
            let mut escaped = false;
            let mut end = rest.len();
            for (index, character) in rest.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == quote {
                    end = index + character.len_utf8();
                    break;
                }
            }
            let color = if language == "json" && rest[end..].trim_start().starts_with(':') {
                theme.cyan
            } else {
                theme.success
            };
            spans.push(Span::styled(rest[..end].to_owned(), code_style(color)));
            rest = &rest[end..];
            continue;
        }
        if first == '$' && language == "shell" {
            let end = rest[1..]
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .map_or(rest.len(), |index| index + 1);
            spans.push(Span::styled(
                rest[..end].to_owned(),
                code_style(theme.accent),
            ));
            rest = &rest[end..];
            continue;
        }
        if first.is_ascii_alphanumeric() || first == '_' {
            let end = rest
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .unwrap_or(rest.len());
            let token = &rest[..end];
            let color = if is_code_keyword(token, language) {
                theme.special
            } else if token
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
            {
                theme.warning
            } else {
                theme.text_primary
            };
            spans.push(Span::styled(token.to_owned(), code_style(color)));
            rest = &rest[end..];
            continue;
        }
        let end = first.len_utf8();
        spans.push(Span::styled(
            rest[..end].to_owned(),
            code_style(theme.text_primary),
        ));
        rest = &rest[end..];
    }
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), code_style(theme.text_primary)));
    }
    spans
}

fn is_code_keyword(token: &str, language: &str) -> bool {
    match language {
        "shell" => matches!(
            token,
            "if" | "then"
                | "else"
                | "elif"
                | "fi"
                | "for"
                | "in"
                | "do"
                | "done"
                | "case"
                | "esac"
                | "while"
                | "function"
        ),
        "rust" => matches!(
            token,
            "fn" | "let"
                | "mut"
                | "pub"
                | "use"
                | "mod"
                | "impl"
                | "struct"
                | "enum"
                | "trait"
                | "match"
                | "if"
                | "else"
                | "for"
                | "while"
                | "loop"
                | "return"
                | "async"
                | "await"
                | "true"
                | "false"
        ),
        "python" => matches!(
            token,
            "def"
                | "class"
                | "import"
                | "from"
                | "as"
                | "if"
                | "elif"
                | "else"
                | "for"
                | "while"
                | "in"
                | "return"
                | "yield"
                | "async"
                | "await"
                | "True"
                | "False"
                | "None"
        ),
        "javascript" => matches!(
            token,
            "const"
                | "let"
                | "var"
                | "function"
                | "class"
                | "import"
                | "export"
                | "from"
                | "if"
                | "else"
                | "for"
                | "while"
                | "return"
                | "async"
                | "await"
                | "true"
                | "false"
                | "null"
                | "undefined"
                | "interface"
                | "type"
        ),
        "json" => matches!(token, "true" | "false" | "null"),
        _ => false,
    }
}

fn render_line(line: &str, theme: Theme, ascii: bool) -> Line<'static> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return Line::default();
    }
    if let Some((level, content)) = heading_content(trimmed) {
        let color = if level == 1 { theme.accent } else { theme.cyan };
        return Line::from(inline_spans(content, color, theme)).style(theme.bold(color));
    }
    if let Some((label, content, color)) = semantic_label(trimmed, theme) {
        return Line::from(vec![
            Span::styled(label.to_owned(), theme.bold(color)),
            Span::styled(content.to_owned(), theme.style(theme.text_primary)),
        ]);
    }
    if let Some(content) = trimmed.strip_prefix("> ") {
        let marker = if ascii { "| " } else { "│ " };
        let mut spans = vec![Span::styled(
            marker.to_owned(),
            theme.style(theme.text_muted),
        )];
        spans.extend(inline_spans(content, theme.text_primary, theme));
        return Line::from(spans);
    }
    if let Some(content) = list_content(trimmed) {
        let marker = if ascii { "- " } else { "• " };
        let mut spans = vec![Span::styled(marker.to_owned(), theme.style(theme.cyan))];
        spans.extend(inline_spans(content, theme.text_primary, theme));
        return Line::from(spans);
    }
    if trimmed == "---" || trimmed == "***" {
        return Line::styled(
            if ascii { "-" } else { "─" }.repeat(40),
            theme.style(theme.border),
        );
    }
    Line::from(inline_spans(line, theme.text_primary, theme))
}

fn semantic_label(line: &str, theme: Theme) -> Option<(&str, &str, Color)> {
    for (marker, color) in [
        ("✔ 优点：", theme.success),
        ("✔ 优点:", theme.success),
        ("⚠ 短板：", theme.warning),
        ("⚠ 短板:", theme.warning),
        ("✘ 问题：", theme.error),
        ("✘ 问题:", theme.error),
        ("结论：", theme.accent),
        ("结论:", theme.accent),
    ] {
        if let Some(content) = line.strip_prefix(marker) {
            return Some((marker, content, color));
        }
    }
    None
}

fn heading_content(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
        Some((hashes, line[hashes + 1..].trim()))
    } else {
        None
    }
}

fn list_content(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
}

fn inline_spans(mut text: &str, base: Color, theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    while !text.is_empty() {
        let Some((index, marker)) = next_marker(text) else {
            spans.push(styled_text(text, base, Modifier::empty(), None, theme));
            break;
        };
        if index > 0 {
            spans.push(styled_text(
                &text[..index],
                base,
                Modifier::empty(),
                None,
                theme,
            ));
            text = &text[index..];
        }
        match marker {
            "**" => {
                if let Some(end) = text[2..].find("**") {
                    spans.push(styled_text(
                        &text[2..end + 2],
                        base,
                        Modifier::BOLD,
                        None,
                        theme,
                    ));
                    text = &text[end + 4..];
                } else {
                    push_first_char(&mut spans, &mut text, base, theme);
                }
            }
            "`" => {
                if let Some(end) = text[1..].find('`') {
                    spans.push(styled_text(
                        &text[1..end + 1],
                        theme.text_primary,
                        Modifier::empty(),
                        Some(theme.background_alt),
                        theme,
                    ));
                    text = &text[end + 2..];
                } else {
                    push_first_char(&mut spans, &mut text, base, theme);
                }
            }
            "[" => {
                if let Some((consumed, label, url)) = parse_link(text) {
                    spans.push(styled_text(
                        label,
                        theme.accent,
                        Modifier::UNDERLINED,
                        None,
                        theme,
                    ));
                    spans.push(styled_text(
                        &format!(" ({url})"),
                        theme.text_muted,
                        Modifier::empty(),
                        None,
                        theme,
                    ));
                    text = &text[consumed..];
                } else {
                    push_first_char(&mut spans, &mut text, base, theme);
                }
            }
            "*" => {
                if let Some(end) = text[1..].find('*') {
                    spans.push(styled_text(
                        &text[1..end + 1],
                        base,
                        Modifier::ITALIC,
                        None,
                        theme,
                    ));
                    text = &text[end + 2..];
                } else {
                    push_first_char(&mut spans, &mut text, base, theme);
                }
            }
            _ => push_first_char(&mut spans, &mut text, base, theme),
        }
    }
    spans
}

fn next_marker(text: &str) -> Option<(usize, &'static str)> {
    ["**", "`", "[", "*"]
        .into_iter()
        .filter_map(|marker| text.find(marker).map(|index| (index, marker)))
        .min_by_key(|(index, marker)| (*index, std::cmp::Reverse(marker.len())))
}

fn parse_link(text: &str) -> Option<(usize, &str, &str)> {
    let label_end = text.find("](")?;
    let url_end = text[label_end + 2..].find(')')? + label_end + 2;
    Some((
        url_end + 1,
        &text[1..label_end],
        &text[label_end + 2..url_end],
    ))
}

fn push_first_char(spans: &mut Vec<Span<'static>>, text: &mut &str, base: Color, theme: Theme) {
    if let Some(character) = text.chars().next() {
        spans.push(styled_text(
            &character.to_string(),
            base,
            Modifier::empty(),
            None,
            theme,
        ));
        *text = &text[character.len_utf8()..];
    }
}

fn styled_text(
    text: &str,
    foreground: Color,
    modifier: Modifier,
    background: Option<Color>,
    theme: Theme,
) -> Span<'static> {
    let mut style = theme.style(foreground).add_modifier(modifier);
    if let Some(background) = background {
        style = style.bg(background);
    }
    Span::styled(text.to_owned(), style)
}

fn render_table(lines: &[&str], width: usize, theme: Theme, ascii: bool) -> Vec<Line<'static>> {
    let Some(headers) = parse_table_row(lines[0]) else {
        return lines
            .iter()
            .map(|line| Line::styled((*line).to_owned(), theme.style(theme.text_primary)))
            .collect();
    };
    let rows: Vec<Vec<String>> = lines[2..]
        .iter()
        .filter_map(|line| parse_table_row(line))
        .collect();
    let columns = headers.len();
    if columns == 0 || rows.iter().any(|row| row.len() != columns) {
        return lines
            .iter()
            .map(|line| Line::styled((*line).to_owned(), theme.style(theme.text_primary)))
            .collect();
    }
    let mut widths = vec![1; columns];
    for row in std::iter::once(&headers).chain(rows.iter()) {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }
    let minimum_total = columns.saturating_mul(7).saturating_add(1);
    if width < minimum_total {
        return render_table_as_list(&headers, &rows, theme);
    }
    while table_width(&widths) > width {
        let Some((index, _)) = widths.iter().enumerate().max_by_key(|(_, value)| **value) else {
            break;
        };
        if widths[index] <= 4 {
            return render_table_as_list(&headers, &rows, theme);
        }
        widths[index] -= 1;
    }
    render_bordered_table(&headers, &rows, &widths, theme, ascii)
}

fn parse_table_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let body = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let body = body.strip_suffix('|').unwrap_or(body);
    let cells: Vec<String> = body
        .split('|')
        .map(|cell| plain_inline(cell.trim()))
        .collect();
    (!cells.is_empty()).then_some(cells)
}

fn is_table_separator(line: &str) -> bool {
    parse_table_row(line).is_some_and(|cells| {
        cells.iter().all(|cell| {
            let cell = cell.trim_matches(':').trim();
            cell.len() >= 3 && cell.chars().all(|c| c == '-')
        })
    })
}

fn plain_inline(text: &str) -> String {
    text.replace(['`', '*'], "")
}

fn table_width(widths: &[usize]) -> usize {
    widths.iter().sum::<usize>() + widths.len().saturating_mul(3) + 1
}

fn render_bordered_table(
    headers: &[String],
    rows: &[Vec<String>],
    widths: &[usize],
    theme: Theme,
    ascii: bool,
) -> Vec<Line<'static>> {
    let chars = if ascii {
        ('+', '+', '+', '+', '+', '+', '-', '|')
    } else {
        ('┌', '┬', '┐', '├', '┼', '┤', '─', '│')
    };
    let mut output = vec![styled_table_line(
        border(widths, chars.0, chars.1, chars.2, chars.6),
        theme.border,
        false,
        theme,
    )];
    output.extend(render_table_row(headers, widths, chars.7, true, theme));
    output.push(styled_table_line(
        border(widths, chars.3, chars.4, chars.5, chars.6),
        theme.border,
        false,
        theme,
    ));
    for row in rows {
        output.extend(render_table_row(row, widths, chars.7, false, theme));
    }
    let bottom = if ascii {
        border(widths, '+', '+', '+', '-')
    } else {
        border(widths, '└', '┴', '┘', '─')
    };
    output.push(styled_table_line(bottom, theme.border, false, theme));
    output
}

fn render_table_row(
    row: &[String],
    widths: &[usize],
    vertical: char,
    header: bool,
    theme: Theme,
) -> Vec<Line<'static>> {
    let wrapped: Vec<Vec<String>> = row
        .iter()
        .zip(widths)
        .map(|(cell, width)| wrap_display(cell, *width))
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|line_index| {
            let mut spans = vec![Span::styled(
                vertical.to_string(),
                theme.style(theme.border),
            )];
            for (index, width) in widths.iter().enumerate() {
                let cell = wrapped[index]
                    .get(line_index)
                    .map(String::as_str)
                    .unwrap_or("");
                let padding = format!(
                    " {cell}{}",
                    " ".repeat(width.saturating_sub(UnicodeWidthStr::width(cell)) + 1)
                );
                let color = if header {
                    theme.cyan
                } else if index == 0 {
                    theme.text_secondary
                } else {
                    theme.text_primary
                };
                let style = if header {
                    theme.bold(color)
                } else {
                    theme.style(color)
                };
                spans.push(Span::styled(padding, style));
                spans.push(Span::styled(
                    vertical.to_string(),
                    theme.style(theme.border),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

fn render_table_as_list(
    headers: &[String],
    rows: &[Vec<String>],
    theme: Theme,
) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return vec![Line::styled(headers.join(" | "), theme.bold(theme.cyan))];
    }
    let mut output = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 {
            output.push(Line::default());
        }
        for (header, value) in headers.iter().zip(row) {
            output.push(Line::from(vec![
                styled_text(
                    &format!("{header}: "),
                    theme.text_secondary,
                    Modifier::BOLD,
                    None,
                    theme,
                ),
                styled_text(value, theme.text_primary, Modifier::empty(), None, theme),
            ]));
        }
    }
    output
}

fn styled_table_line(text: String, color: Color, bold: bool, theme: Theme) -> Line<'static> {
    let modifier = if bold {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };
    Line::styled(text, theme.style(color).add_modifier(modifier))
}

fn border(widths: &[usize], left: char, middle: char, right: char, fill: char) -> String {
    let mut line = left.to_string();
    for (index, width) in widths.iter().enumerate() {
        line.push_str(&fill.to_string().repeat(width + 2));
        line.push(if index + 1 == widths.len() {
            right
        } else {
            middle
        });
    }
    line
}

fn wrap_display(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = vec![String::new()];
    let mut current_width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + character_width > width {
            lines.push(String::new());
            current_width = 0;
        }
        if let Some(line) = lines.last_mut() {
            line.push(character);
        }
        current_width += character_width;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_inline_styles_lists_quotes_and_code() {
        let source = "## 标题\n- **粗体** 和 `code`\n> 引用\n```sh\necho hello\n```";
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let lines = render(source, 80, theme, false);
        assert_eq!(lines[0].to_string(), "标题");
        assert_eq!(lines[1].to_string(), "• 粗体 和 code");
        assert_eq!(lines[2].to_string(), "│ 引用");
        assert_eq!(lines[3].to_string(), "echo hello");
        assert!(lines[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[0].style.fg, Some(theme.cyan));
        assert_eq!(lines[3].spans[0].style.fg, Some(theme.text_primary));
        assert_eq!(lines[3].spans[0].style.bg, Some(theme.background_alt));
    }

    #[test]
    fn highlights_fenced_code_without_changing_text_or_unknown_languages() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let lines = render(
            "```rust\nlet answer = 42; // result\n```\n```unknown\n<raw>\n```",
            80,
            theme,
            false,
        );
        assert_eq!(lines[0].to_string(), "let answer = 42; // result");
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content == "let" && span.style.fg == Some(theme.special)));
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content == "42" && span.style.fg == Some(theme.warning)));
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content == "// result" && span.style.fg == Some(theme.text_muted)));
        assert_eq!(lines[1].to_string(), "<raw>");
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.text_primary));
    }

    #[test]
    fn aligns_chinese_table_and_degrades_on_narrow_windows() {
        let source = "| 应用 | RSS |\n|---|---|\n| 前台应用 | 288 MB |\n| system | 271 MB |";
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let table = render(source, 60, theme, false);
        assert!(table[0].to_string().starts_with('┌'));
        let row_widths: Vec<usize> = table
            .iter()
            .map(|line| UnicodeWidthStr::width(line.to_string().as_str()))
            .collect();
        assert!(row_widths.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(table[1].spans[0].style.fg, Some(theme.border));
        assert_eq!(table[1].spans[1].style.fg, Some(theme.cyan));
        assert_eq!(table[3].spans[1].style.fg, Some(theme.text_secondary));
        assert_eq!(table[3].spans[3].style.fg, Some(theme.text_primary));

        let narrow = render(source, 10, theme, false);
        assert!(narrow.iter().any(|line| line.to_string().contains("应用:")));
        assert!(!narrow.iter().any(|line| line.to_string().contains('┌')));
    }

    #[test]
    fn malformed_markdown_falls_back_to_visible_text() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let lines = render("未关闭的 **粗体 和 [链接", 80, theme, false);
        assert_eq!(lines[0].to_string(), "未关闭的 **粗体 和 [链接");
    }

    #[test]
    fn semantic_labels_color_only_the_label() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let lines = render(
            "✔ 优点：Cortex-A55\n⚠ 短板：能效核心\n结论：适合电视",
            80,
            theme,
            false,
        );
        assert_eq!(lines[0].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[0].spans[1].style.fg, Some(theme.text_primary));
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.warning));
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.accent));
        assert_eq!(lines[2].spans[1].style.fg, Some(theme.text_primary));
    }
}
