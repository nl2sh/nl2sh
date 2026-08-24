use super::{
    app::{App, WELCOME_TRAIN_SPEED, WELCOME_TRAIN_WIDTH},
    markdown,
    theme::Theme,
};
use crate::config::UiLanguage;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthChar;

fn title_line(app: &App, theme: Theme) -> Line<'static> {
    let separator = || Span::styled(" | ", theme.style(theme.text_muted));
    let mut spans = vec![
        Span::styled(
            format!("nl2sh v{}", env!("CARGO_PKG_VERSION")),
            theme.bold(theme.text_primary),
        ),
        separator(),
        Span::styled(app.mode.clone(), theme.style(theme.cyan)),
    ];
    if let Some(balance) = &app.provider_balance {
        spans.push(separator());
        spans.push(Span::styled(
            match app.language {
                UiLanguage::ZhCn => "余额 ",
                UiLanguage::En => "balance ",
            },
            theme.style(theme.text_secondary),
        ));
        spans.push(Span::styled(balance.clone(), theme.style(theme.success)));
    }
    spans.extend([
        separator(),
        Span::styled(
            app.root.clone(),
            theme.style(if app.root.eq_ignore_ascii_case("root") {
                theme.warning
            } else {
                theme.text_secondary
            }),
        ),
        separator(),
        Span::styled(app.model.clone(), theme.style(theme.special)),
        separator(),
        Span::styled(app.api_type.clone(), theme.style(theme.text_secondary)),
        separator(),
        Span::styled(
            if app.ascii { "ASCII" } else { "Unicode" },
            theme.style(theme.text_secondary),
        ),
    ]);
    spans.into()
}

fn shortcut_line(language: UiLanguage, theme: Theme) -> Line<'static> {
    let items = match language {
        UiLanguage::ZhCn => [
            ("Enter", " 发送"),
            ("F2", " 结果"),
            ("滚轮/PgUp/PgDn", " 滚动"),
            ("Shift+拖选/右键", " 复制"),
            ("Ctrl+Q", " 退出"),
        ],
        UiLanguage::En => [
            ("Enter", " send"),
            ("F2", " results"),
            ("Wheel/PgUp/PgDn", " scroll"),
            ("Shift+drag/right-click", " copy"),
            ("Ctrl+Q", " quit"),
        ],
    };
    let mut spans = Vec::new();
    for (index, (key, description)) in items.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" | ", theme.style(theme.text_muted)));
        }
        spans.push(Span::styled(key, theme.style(theme.cyan)));
        spans.push(Span::styled(description, theme.style(theme.text_muted)));
    }
    Line::from(spans)
}

fn status_line(app: &App, theme: Theme) -> Line<'static> {
    let remaining = app.max_context.saturating_sub(app.turn);
    let (status_label, turn_label, remaining_label) = match app.language {
        UiLanguage::ZhCn => ("状态：", "对话轮次 ", "剩余上下文 "),
        UiLanguage::En => ("status: ", "turn ", "context remaining "),
    };
    let (primary_status, status_detail) = app
        .status
        .split_once([';', '；'])
        .map_or((app.status.as_str(), None), |(status, detail)| {
            (status, Some(detail.trim()))
        });
    let mut spans = vec![
        Span::styled(
            status_label,
            theme.style(theme.text_secondary).bg(theme.background_alt),
        ),
        Span::styled(
            primary_status.to_owned(),
            theme
                .style(status_color(app, theme))
                .bg(theme.background_alt),
        ),
    ];
    if let Some(detail) = status_detail {
        spans.push(Span::styled(
            match app.language {
                UiLanguage::ZhCn => "；",
                UiLanguage::En => "; ",
            },
            theme.style(theme.text_muted).bg(theme.background_alt),
        ));
        spans.extend(status_detail_spans(detail, theme));
    }
    spans.extend([
        Span::styled(
            " | ",
            theme.style(theme.text_muted).bg(theme.background_alt),
        ),
        Span::styled(
            turn_label,
            theme.style(theme.text_muted).bg(theme.background_alt),
        ),
        Span::styled(
            app.turn.to_string(),
            theme.style(theme.text_primary).bg(theme.background_alt),
        ),
        Span::styled(
            " | ",
            theme.style(theme.text_muted).bg(theme.background_alt),
        ),
        Span::styled(
            remaining_label,
            theme.style(theme.text_muted).bg(theme.background_alt),
        ),
        Span::styled(
            remaining.to_string(),
            theme.style(theme.cyan).bg(theme.background_alt),
        ),
    ]);
    Line::from(spans)
}

fn status_detail_spans(detail: &str, theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut segment = String::new();
    let mut digits = false;
    for character in detail.chars() {
        let character_is_digit = character.is_ascii_digit();
        if character_is_digit != digits && !segment.is_empty() {
            let color = if digits {
                theme.text_primary
            } else {
                theme.text_muted
            };
            spans.push(Span::styled(
                std::mem::take(&mut segment),
                theme.style(color).bg(theme.background_alt),
            ));
        }
        digits = character_is_digit;
        segment.push(character);
    }
    if !segment.is_empty() {
        let color = if digits {
            theme.text_primary
        } else {
            theme.text_muted
        };
        spans.push(Span::styled(
            segment,
            theme.style(color).bg(theme.background_alt),
        ));
    }
    spans
}

fn status_color(app: &App, theme: Theme) -> Color {
    let status = app.status.to_ascii_lowercase();
    if status.contains("fail")
        || status.contains("error")
        || app.status.contains("失败")
        || app.status.contains("出错")
    {
        theme.error
    } else if status.contains("confirm") || app.status.contains("确认") {
        theme.warning
    } else if status.starts_with("idle") || app.status.starts_with("空闲") {
        theme.success
    } else {
        theme.cyan
    }
}

fn popup_color(dangerous: bool, informational: bool, theme: Theme) -> Color {
    if dangerous {
        theme.error
    } else if informational {
        theme.accent
    } else {
        theme.warning
    }
}

fn popup_style(theme: Theme, color: Color) -> Style {
    theme.style(color).bg(theme.background_alt)
}

fn popup_line(line: &str, dangerous: bool, theme: Theme) -> Line<'static> {
    if line.starts_with("> ") {
        return Line::styled(
            line.to_owned(),
            theme.bold(theme.accent).bg(theme.background_alt),
        );
    }
    if line.starts_with("  ") {
        let color = if line.contains("不可用") || line.contains("unavailable") {
            theme.text_muted
        } else {
            theme.text_primary
        };
        return Line::styled(line.to_owned(), popup_style(theme, color));
    }
    for label in ["命令：", "Command: "] {
        if let Some(command) = line.strip_prefix(label) {
            return Line::from(vec![
                Span::styled(label, popup_style(theme, theme.cyan)),
                Span::styled(command.to_owned(), popup_style(theme, theme.text_primary)),
            ]);
        }
    }
    for label in ["风险：", "Risk: "] {
        if let Some(value) = line.strip_prefix(label) {
            let mut spans = vec![
                Span::styled(label, popup_style(theme, theme.text_secondary)),
                Span::styled(
                    value.trim_end_matches(" | ROOT").to_owned(),
                    popup_style(
                        theme,
                        if dangerous {
                            theme.error
                        } else {
                            theme.warning
                        },
                    ),
                ),
            ];
            if value.ends_with(" | ROOT") {
                spans.push(Span::styled(
                    " | ROOT",
                    theme.bold(theme.warning).bg(theme.background_alt),
                ));
            }
            return Line::from(spans);
        }
    }
    let color = if line.contains("高风险") || line.to_ascii_lowercase().contains("high risk") {
        theme.error
    } else {
        theme.text_primary
    };
    Line::styled(line.to_owned(), popup_style(theme, color))
}

pub fn draw(f: &mut Frame, app: &App) {
    let theme = Theme::detect();
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        f.area(),
    );
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(f.area());
    let title = Paragraph::new(title_line(app, theme)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.style(theme.border)),
    );
    f.render_widget(title, areas[0]);
    let conversation_width = areas[1].width.saturating_sub(2) as usize;
    let lines = wrap_rendered_lines(
        conversation_lines(app, conversation_width, theme),
        conversation_width,
    );
    let visible_rows = areas[1].height.saturating_sub(2) as usize;
    let rendered_rows = lines.len();
    let bottom = rendered_rows.saturating_sub(visible_rows);
    let scroll = bottom
        .saturating_sub(app.conversation_scroll.min(bottom))
        .min(u16::MAX as usize) as u16;
    f.render_widget(
        Paragraph::new(Text::from(lines)).scroll((scroll, 0)).block(
            Block::default()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(theme.style(theme.border))
                .title(Line::styled(
                    match app.language {
                        UiLanguage::ZhCn => "对话历史",
                        UiLanguage::En => "Conversation",
                    },
                    theme.bold(theme.accent),
                )),
        ),
        areas[1],
    );
    let input_background = theme.background_alt;
    let input_row =
        ratatui::layout::Rect::new(areas[2].x, areas[2].y.saturating_add(1), areas[2].width, 1);
    let status_row =
        ratatui::layout::Rect::new(areas[2].x, areas[2].y.saturating_add(2), areas[2].width, 1);
    f.render_widget(
        Block::default().style(Style::default().bg(input_background)),
        input_row,
    );
    f.render_widget(
        Block::default().style(Style::default().bg(theme.background_alt)),
        status_row,
    );
    let input = Paragraph::new(Text::from(vec![
        input_editor_line(app, areas[2].width as usize, theme),
        status_line(app, theme),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(theme.style(if app.popup.is_none() {
                theme.border_focus
            } else {
                theme.border
            }))
            .title(shortcut_line(app.language, theme)),
    );
    f.render_widget(input, areas[2]);
    if app.popup.is_none() {
        render_command_menu(f, app, areas[2], theme);
    }
    if let Some(popup) = &app.popup {
        let popup_height = (popup.lines.len() as u16)
            .saturating_add(2)
            .max(11)
            .min(f.area().height.saturating_sub(2))
            .max(3);
        let area = bottom_left_rect(78, popup_height, areas[2].y, f.area());
        f.render_widget(Clear, area);
        f.render_widget(
            Paragraph::new(Text::from(
                popup
                    .lines
                    .iter()
                    .map(|line| popup_line(line, popup.dangerous, theme))
                    .collect::<Vec<_>>(),
            ))
            .style(popup_style(theme, theme.text_primary))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().bg(theme.background_alt))
                    .border_style(popup_style(
                        theme,
                        popup_color(popup.dangerous, popup.informational, theme),
                    ))
                    .title(Line::styled(
                        popup.title.as_str(),
                        theme
                            .bold(popup_color(popup.dangerous, popup.informational, theme))
                            .bg(theme.background_alt),
                    )),
            ),
            area,
        );
    }
}

fn render_command_menu(f: &mut Frame, app: &App, input_area: ratatui::layout::Rect, theme: Theme) {
    let suggestions = app.command_suggestions();
    if suggestions.is_empty() || input_area.y < 3 {
        return;
    }
    let height = (suggestions.len() as u16).saturating_add(2);
    let area = ratatui::layout::Rect::new(
        input_area.x,
        input_area.y.saturating_sub(height),
        input_area.width.min(52),
        height,
    );
    let selected = app.command_selection % suggestions.len();
    let lines = suggestions
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let description = match (app.language, *command) {
                (UiLanguage::ZhCn, "/balance") => "查询 Provider 余额",
                (UiLanguage::ZhCn, "/clear") => "清空当前会话",
                (UiLanguage::ZhCn, "/config") => "重新配置模型服务",
                (UiLanguage::ZhCn, "/setting") => "打开设置（/config 别名）",
                (UiLanguage::ZhCn, "/exit") => "安全退出",
                (UiLanguage::ZhCn, "/help") => "显示帮助",
                (UiLanguage::ZhCn, "/update") => "检查版本更新",
                (UiLanguage::En, "/balance") => "Query provider balance",
                (UiLanguage::En, "/clear") => "Clear the current session",
                (UiLanguage::En, "/config") => "Reconfigure the model provider",
                (UiLanguage::En, "/setting") => "Open Settings (/config alias)",
                (UiLanguage::En, "/exit") => "Quit safely",
                (UiLanguage::En, "/help") => "Show help",
                (UiLanguage::En, "/update") => "Check for updates",
                _ => "",
            };
            let style = if index == selected {
                theme.bold(theme.text_primary).bg(theme.background_alt)
            } else {
                theme.style(theme.text_secondary)
            };
            Line::from(vec![
                Span::styled(format!(" {command:<12}"), style.fg(theme.cyan)),
                Span::styled(format!(" {description}"), style),
            ])
        })
        .collect::<Vec<_>>();
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .style(theme.style(theme.text_primary))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.style(theme.border_focus))
                    .title(Line::styled(
                        match app.language {
                            UiLanguage::ZhCn => "命令",
                            UiLanguage::En => "Commands",
                        },
                        theme.bold(theme.accent),
                    )),
            ),
        area,
    );
}

fn input_editor_line(app: &App, width: usize, theme: Theme) -> Line<'static> {
    let available = width.saturating_sub(3);
    let cursor_character = app.input.text[..app.input.cursor()].chars().count();
    let characters = app.input.text.chars().collect::<Vec<_>>();
    let mut start = 0;
    while characters[start..cursor_character]
        .iter()
        .map(|character| UnicodeWidthChar::width(*character).unwrap_or(0))
        .sum::<usize>()
        >= available.max(1)
    {
        start += 1;
    }
    let mut before = String::new();
    let mut after = String::new();
    let mut used = 0;
    for (index, character) in characters.iter().enumerate().skip(start) {
        let character_width = UnicodeWidthChar::width(*character).unwrap_or(0);
        if used + character_width + 1 > available {
            break;
        }
        if index < cursor_character {
            before.push(*character);
        } else {
            after.push(*character);
        }
        used += character_width;
    }
    let base = theme.style(theme.text_primary).bg(theme.background_alt);
    let cursor = if app.cursor_visible && app.popup.is_none() {
        "│"
    } else {
        " "
    };
    Line::from(vec![
        Span::styled("> ", theme.style(theme.accent).bg(theme.background_alt)),
        Span::styled(before, base),
        ratatui::text::Span::styled(cursor, theme.style(theme.accent).bg(theme.background_alt)),
        Span::styled(after, base),
    ])
}

fn wrap_rendered_lines(lines: Vec<Line<'_>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let mut output = Vec::new();
    for line in lines {
        let mut current_spans = Vec::new();
        let mut current_width = 0;
        for span in line.spans {
            let style = line.style.patch(span.style);
            let mut segment = String::new();
            for character in span.content.chars() {
                let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
                if current_width > 0 && current_width + character_width > width {
                    if !segment.is_empty() {
                        current_spans.push(ratatui::text::Span::styled(
                            std::mem::take(&mut segment),
                            style,
                        ));
                    }
                    output.push(Line::from(std::mem::take(&mut current_spans)));
                    current_width = 0;
                }
                segment.push(character);
                current_width += character_width;
            }
            if !segment.is_empty() {
                current_spans.push(ratatui::text::Span::styled(segment, style));
            }
        }
        output.push(Line::from(current_spans));
    }
    output
}

fn conversation_lines(app: &App, width: usize, theme: Theme) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    for entry in &app.history {
        if let Some(art) = entry.strip_prefix(super::i18n::BUDDHA_ART_PREFIX) {
            lines.extend(buddha_art_lines(art, theme));
            if let Some(frame) = app.welcome_train_frame {
                lines.extend(welcome_train_lines(frame, width, theme));
            }
        } else if let Some(encoded) = entry.strip_prefix(super::output::TOOL_RESULT_PREFIX) {
            let (prefix, details) = encoded.split_once('\n').unwrap_or((encoded, ""));
            if app.tool_results_expanded {
                let label = match app.language {
                    UiLanguage::ZhCn => "工具结果：",
                    UiLanguage::En => "Tool result:",
                };
                lines.push(tool_result_heading(prefix, label, theme));
                lines.extend(tool_result_lines(details, theme));
            } else {
                let label = match app.language {
                    UiLanguage::ZhCn => "工具结果已折叠（F2 展开）",
                    UiLanguage::En => "Tool result collapsed (F2 to expand)",
                };
                lines.push(tool_result_heading(prefix, label, theme));
            }
        } else if let Some(visible) = entry.strip_prefix(super::output::LIVE_OUTPUT_PREFIX) {
            lines.push(conversation_line(visible, theme));
        } else if let Some(stream) = entry.strip_prefix(super::output::LLM_STREAM_PREFIX) {
            let (phase, text) = stream.split_once(':').unwrap_or(("0", stream));
            lines.extend(streaming_agent_lines(
                text,
                phase.parse().unwrap_or(0),
                theme,
                app.ascii,
            ));
        } else if starts_with_any(entry, &["[AGENT]", "🤖"]) {
            lines.extend(markdown::render(entry, width, theme, app.ascii));
        } else {
            append_multiline_entry(&mut lines, entry, theme);
        }
    }
    lines
}

fn streaming_agent_lines(
    text: &str,
    phase: usize,
    theme: Theme,
    ascii: bool,
) -> Vec<Line<'static>> {
    let prefix = if ascii { "[AGENT] " } else { "🤖 " };
    let characters = text.chars().collect::<Vec<_>>();
    let gradient_start = characters.len().saturating_sub(32);
    let mut lines = vec![Line::from(Span::styled(prefix, theme.bold(theme.special)))];
    for (index, character) in characters.into_iter().enumerate() {
        if character == '\n' {
            lines.push(Line::default());
            continue;
        }
        let tail_index = index.saturating_sub(gradient_start);
        let color = if index < gradient_start {
            theme.text_primary
        } else {
            animated_gradient_color(theme, tail_index, phase)
        };
        let span = Span::styled(character.to_string(), theme.style(color));
        if let Some(line) = lines.last_mut() {
            line.spans.push(span);
        }
    }
    lines
}

fn animated_gradient_color(theme: Theme, index: usize, phase: usize) -> Color {
    let wave = (index + phase) % 24;
    let amount = if wave <= 12 { wave } else { 24 - wave };
    match (theme.text_primary, theme.accent) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            blend_channel(r1, r2, amount),
            blend_channel(g1, g2, amount),
            blend_channel(b1, b2, amount),
        ),
        _ => match amount {
            0..=3 => theme.text_primary,
            4..=7 => theme.text_secondary,
            _ => theme.accent,
        },
    }
}

fn blend_channel(from: u8, to: u8, amount: usize) -> u8 {
    let from = usize::from(from);
    let to = usize::from(to);
    ((from * (12 - amount) + to * amount) / 12) as u8
}

fn buddha_art_lines(art: &str, theme: Theme) -> Vec<Line<'static>> {
    art.lines()
        .map(|line| {
            let mut spans = Vec::new();
            let mut segment = String::new();
            let mut segment_is_golden = None;
            for character in line.chars() {
                let is_golden = matches!(character, '\\' | '/' | '|' | '=' | '^');
                if segment_is_golden.is_some_and(|current| current != is_golden) {
                    let golden = segment_is_golden.unwrap_or(false);
                    spans.push(Span::styled(
                        std::mem::take(&mut segment),
                        if golden {
                            theme.bold(theme.decorative_gold)
                        } else {
                            theme.style(theme.text_primary)
                        },
                    ));
                }
                segment_is_golden = Some(is_golden);
                segment.push(character);
            }
            if !segment.is_empty() {
                spans.push(Span::styled(
                    segment,
                    if segment_is_golden.unwrap_or(false) {
                        theme.bold(theme.decorative_gold)
                    } else {
                        theme.style(theme.text_primary)
                    },
                ));
            }
            Line::from(spans)
        })
        .collect()
}

const TRAIN_BODY: [&str; 6] = [
    "                 ________        ====      ",
    "__===_____I_I__/        \\_______|  |_ D_  ",
    " |        |   |  NL2SH  |H   |  ---(_)|   ",
    " |        |   |_________|H   |  |     \\   ",
    " |        |   |          H   |  |      |  ",
    " '--(O)----(O)----(O)------------(O)---'  ",
];
const TRAIN_ENGINE_FRONT_COLUMN: usize = 39;

fn welcome_train_lines(frame: u16, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let smoke = match (frame / 3) % 4 {
        0 => "                    ( )   (@@)   ( )       ",
        1 => "                      (  )    (@)  (  )    ",
        2 => "                         (@)  (  )    (@)  ",
        _ => "                      o    ( )   (@@)      ",
    };
    let position = usize::from(frame).saturating_mul(WELCOME_TRAIN_SPEED) as isize
        - WELCOME_TRAIN_WIDTH as isize;
    let right_edge_position =
        width.saturating_sub(TRAIN_ENGINE_FRONT_COLUMN.saturating_add(1)) as isize;
    let position = if position > right_edge_position
        && position - right_edge_position < WELCOME_TRAIN_SPEED as isize
    {
        right_edge_position
    } else {
        position
    };
    let mut lines = vec![Line::styled(
        clip_moving_ascii(smoke, position, width),
        theme.style(theme.text_secondary),
    )];
    lines.extend(TRAIN_BODY.map(|line| {
        Line::styled(
            clip_moving_ascii(line, position, width),
            theme.style(theme.text_primary),
        )
    }));
    lines
}

fn clip_moving_ascii(line: &str, position: isize, width: usize) -> String {
    let indent = position.max(0) as usize;
    if indent >= width {
        return String::new();
    }
    let skip = position.saturating_neg().max(0) as usize;
    let visible: String = line
        .chars()
        .skip(skip)
        .take(width.saturating_sub(indent))
        .collect();
    if visible.is_empty() {
        String::new()
    } else {
        format!("{}{}", " ".repeat(indent), visible)
    }
}

fn append_multiline_entry<'a>(lines: &mut Vec<Line<'a>>, entry: &'a str, theme: Theme) {
    let continuation_color = conversation_color(entry, theme);
    for (index, line) in entry.lines().enumerate() {
        if index == 0 {
            lines.push(conversation_line(line, theme));
        } else {
            lines.push(Line::styled(line, theme.style(continuation_color)));
        }
    }
    if entry.is_empty() {
        lines.push(Line::default());
    }
}

fn conversation_line(value: &str, theme: Theme) -> Line<'_> {
    for prefix in [
        "> ",
        "[OUT] ",
        "[OK] ",
        "✅ ",
        "[ERR] ",
        "[ERROR] ",
        "[TIMEOUT] ",
        "❌ ",
        "⏱ ",
        "⛔ ",
        "[CMD] ",
        "💻 ",
        "[WARN] ",
        "⚠️ ",
        "[TOOL] ",
        "🔧 ",
        "[CONFIG] ",
    ] {
        if let Some(content) = value.strip_prefix(prefix) {
            return Line::from(vec![
                Span::styled(prefix, theme.style(conversation_color(value, theme))),
                Span::styled(content, theme.style(theme.text_primary)),
            ]);
        }
    }
    Line::styled(value, theme.style(conversation_color(value, theme)))
}

fn tool_result_heading(prefix: &str, label: &str, theme: Theme) -> Line<'static> {
    let color = if starts_with_any(prefix, &["[ERROR]", "[ERR]", "❌", "⛔"]) {
        theme.error
    } else {
        theme.success
    };
    Line::styled(format!("{prefix} {label}"), theme.bold(color))
}

fn tool_result_lines(details: &str, theme: Theme) -> Vec<Line<'static>> {
    let mut output_section = false;
    details
        .lines()
        .map(|line| {
            if line == "stdout:" || line == "stderr:" {
                output_section = true;
                let color = if line == "stdout:" {
                    theme.accent
                } else {
                    theme.warning
                };
                return Line::styled(line.to_owned(), theme.bold(color));
            }
            if output_section {
                return Line::styled(line.to_owned(), theme.style(theme.text_primary));
            }
            if let Some(value) = line.strip_prefix("executed_command=") {
                return Line::from(vec![
                    Span::styled("executed_command", theme.style(theme.cyan)),
                    Span::styled("=", theme.style(theme.text_muted)),
                    Span::styled(value.to_owned(), theme.style(theme.text_primary)),
                ]);
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if !fields.is_empty() && fields.iter().all(|field| field.contains('=')) {
                let mut spans = Vec::new();
                for (index, field) in fields.into_iter().enumerate() {
                    let Some((key, value)) = field.split_once('=') else {
                        continue;
                    };
                    if index > 0 {
                        spans.push(Span::styled(" ", theme.style(theme.text_muted)));
                    }
                    spans.push(Span::styled(
                        key.to_owned(),
                        theme.style(theme.text_secondary),
                    ));
                    spans.push(Span::styled("=", theme.style(theme.text_muted)));
                    spans.push(Span::styled(
                        value.to_owned(),
                        theme.style(tool_field_value_color(key, value, theme)),
                    ));
                }
                return Line::from(spans);
            }
            Line::styled(
                line.to_owned(),
                theme.style(if output_section {
                    theme.text_primary
                } else {
                    theme.text_secondary
                }),
            )
        })
        .collect()
}

fn tool_field_value_color(key: &str, value: &str, theme: Theme) -> Color {
    match (key, value) {
        ("risk", "ReadOnly" | "只读") | ("exit", "Some(0)") => theme.success,
        ("risk", "Dangerous" | "Critical" | "危险" | "严重危险") => theme.error,
        ("risk", _) => theme.warning,
        _ => theme.text_primary,
    }
}

fn conversation_color(value: &str, theme: Theme) -> Color {
    if value.starts_with("> ") {
        theme.accent
    } else if starts_with_any(value, &["[AGENT]", "🤖"]) {
        theme.text_primary
    } else if starts_with_any(value, &["[TOOL]", "🔧"]) {
        theme.cyan
    } else if starts_with_any(value, &["[CMD]", "💻", "[WARN]", "⚠️"]) {
        theme.warning
    } else if starts_with_any(value, &["[OUT]", "[OK]", "✅"]) {
        theme.success
    } else if starts_with_any(value, &["[ERR]", "[ERROR]", "[TIMEOUT]", "❌", "⏱", "⛔"]) {
        theme.error
    } else if value.starts_with("[CONFIG]") {
        theme.accent
    } else {
        theme.text_primary
    }
}

fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn bottom_left_rect(
    percent_x: u16,
    height: u16,
    anchor_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let width = area
        .width
        .saturating_mul(percent_x)
        .saturating_div(100)
        .max(3);
    let height = height.min(anchor_y.saturating_sub(area.y)).max(3);
    ratatui::layout::Rect::new(
        area.x,
        anchor_y.saturating_sub(height),
        width.min(area.width),
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn informational_popup_uses_accent_instead_of_risk_color() {
        let theme = Theme::for_mode(crate::tui::theme::ColorMode::Ansi256);
        assert_eq!(popup_color(false, true, theme), theme.accent);
        assert_eq!(popup_color(false, false, theme), theme.warning);
        assert_eq!(popup_color(true, false, theme), theme.error);
    }
    use crate::tui::{app::PopupView, input::Input};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn input_and_status_use_separate_rows() -> anyhow::Result<()> {
        let theme = Theme::detect();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut app = App {
            input: Input::from("hello"),
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: String::new(),
            cursor_visible: true,
            command_selection: 0,
            history: Vec::new(),
            conversation_scroll: 0,
            tool_results_expanded: false,
            welcome_train_frame: None,
            model: "test".into(),
            root: "Normal".into(),
            ascii: true,
            language: UiLanguage::En,
            api_type: "Responses".into(),
            mode: "Agent".into(),
            turn: 2,
            max_context: 10,
            status: "idle".into(),
            provider_balance: None,
            popup: None,
        };
        terminal.draw(|frame| draw(frame, &app))?;
        let buffer = terminal.backend().buffer();
        let row = |y| {
            (0..100)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(row(27).contains("> hello"));
        assert_eq!(buffer[(7, 27)].fg, theme.accent);
        assert!(row(28).contains("status: idle | turn 2 | context remaining 8"));
        assert_eq!(buffer[(10, 27)].bg, theme.background_alt);
        assert_eq!(buffer[(10, 28)].bg, theme.background_alt);
        assert_eq!(buffer[(10, 26)].bg, theme.background);
        assert_eq!(buffer[(10, 29)].bg, theme.background);
        assert!(row(26).contains('─'));
        assert!(row(29).contains('─'));
        assert_eq!(buffer[(0, 10)].symbol(), " ");
        assert_eq!(buffer[(99, 10)].symbol(), " ");

        app.input.set("/".into());
        terminal.draw(|frame| draw(frame, &app))?;
        let menu = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(menu.contains("/config"));
        assert!(menu.contains("/clear"));
        assert!(menu.contains("/exit"));
        assert!(menu.contains("/help"));
        assert!(menu.contains("/setting"));
        assert!(!menu.contains("/provider"));
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.bg == theme.background_alt && cell.fg == theme.cyan));

        app.history = vec![crate::tui::output::encode_tool_result(
            "[OK]",
            "executed_command=id\nrisk=ReadOnly root=false matched_rules=[]\nexit=Some(0) timed_out=false\nstdout:\nuid=0",
        )];
        let collapsed = conversation_lines(&app, 98, theme);
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].to_string().contains("collapsed"));
        assert!(!collapsed[0].to_string().contains("executed_command"));
        app.tool_results_expanded = true;
        let expanded = conversation_lines(&app, 98, theme);
        assert!(expanded.len() > 1);
        assert!(expanded
            .iter()
            .any(|line| line.to_string().contains("executed_command=id")));
        let command = expanded
            .iter()
            .find(|line| line.to_string().starts_with("executed_command="))
            .ok_or_else(|| anyhow::anyhow!("missing command field"))?;
        assert_eq!(command.spans[0].style.fg, Some(theme.cyan));
        assert_eq!(command.spans[2].style.fg, Some(theme.text_primary));
        let exit = expanded
            .iter()
            .find(|line| line.to_string().starts_with("exit="))
            .ok_or_else(|| anyhow::anyhow!("missing exit field"))?;
        assert_eq!(exit.spans[2].style.fg, Some(theme.success));
        let risk = expanded
            .iter()
            .find(|line| line.to_string().starts_with("risk="))
            .ok_or_else(|| anyhow::anyhow!("missing risk field"))?;
        assert_eq!(risk.spans[2].style.fg, Some(theme.success));
        assert_eq!(risk.spans[6].style.fg, Some(theme.text_primary));
        let raw_output = expanded
            .iter()
            .find(|line| line.to_string() == "uid=0")
            .ok_or_else(|| anyhow::anyhow!("missing stdout"))?;
        assert_eq!(raw_output.style.fg, Some(theme.text_primary));

        app.history = vec![
            "[AGENT] 查询结果汇总：\n\n## 内存占用\n| 应用 | RSS |\n|---|---|\n| example | 288 MB |"
                .into(),
        ];
        app.tool_results_expanded = false;
        let markdown = conversation_lines(&app, 98, theme);
        assert!(markdown.len() > 6);
        assert_eq!(markdown[2].to_string(), "内存占用");
        assert!(markdown
            .iter()
            .any(|line| line.to_string().starts_with('+')));
        assert!(markdown
            .iter()
            .any(|line| line.to_string().contains("example")));
        Ok(())
    }

    #[test]
    fn conversation_types_have_distinct_colors() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let user = conversation_line("> user", theme);
        let tool = conversation_line("[TOOL] call", theme);
        let agent = conversation_line("[AGENT] answer", theme);
        assert_eq!(user.spans[0].style.fg, Some(theme.accent));
        assert_eq!(user.spans[1].style.fg, Some(theme.text_primary));
        assert_eq!(tool.spans[0].style.fg, Some(theme.cyan));
        assert_eq!(agent.style.fg, Some(theme.text_primary));
        assert_eq!(tool.spans[1].style.fg, Some(theme.text_primary));
        assert_ne!(tool.spans[0].style.fg, tool.spans[1].style.fg);
    }

    #[test]
    fn streaming_agent_tail_has_animated_gradient_and_completed_text_is_plain() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let first = streaming_agent_lines("streaming response", 0, theme, true);
        let next = streaming_agent_lines("streaming response", 5, theme, true);
        assert_eq!(first[0].spans[0].content, "[AGENT] ");
        assert!(first[0]
            .spans
            .iter()
            .skip(1)
            .any(|span| span.style.fg != Some(theme.text_primary)));
        assert!(first[0]
            .spans
            .iter()
            .zip(&next[0].spans)
            .any(|(left, right)| left.style.fg != right.style.fg));

        let completed = markdown::render("[AGENT] streaming response", 80, theme, true);
        assert!(completed
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg != Some(theme.accent)));
    }

    #[test]
    fn title_status_and_confirmation_follow_semantic_roles() {
        let theme = Theme::for_mode(super::super::theme::ColorMode::TrueColor);
        let mut app = App {
            input: Input::default(),
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: String::new(),
            cursor_visible: true,
            command_selection: 0,
            history: Vec::new(),
            conversation_scroll: 0,
            tool_results_expanded: false,
            welcome_train_frame: None,
            model: "deepseek-v4-flash".into(),
            root: "Root".into(),
            ascii: false,
            language: UiLanguage::ZhCn,
            api_type: "Responses".into(),
            mode: "智能体".into(),
            turn: 3,
            max_context: 10,
            status: "空闲；上次执行 4 步".into(),
            provider_balance: None,
            popup: None,
        };
        let title = title_line(&app, theme);
        assert_eq!(title.spans[2].style.fg, Some(theme.cyan));
        assert_eq!(title.spans[4].style.fg, Some(theme.warning));
        assert_eq!(title.spans[6].style.fg, Some(theme.special));

        let status = status_line(&app, theme);
        assert_eq!(status.spans[1].style.fg, Some(theme.success));
        assert!(status
            .spans
            .iter()
            .any(|span| span.content == "4" && span.style.fg == Some(theme.text_primary)));

        let risk = popup_line("风险：严重危险 | ROOT", true, theme);
        assert_eq!(risk.spans[0].style.fg, Some(theme.text_secondary));
        assert_eq!(risk.spans[1].style.fg, Some(theme.error));
        assert_eq!(risk.spans[2].style.fg, Some(theme.warning));

        app.status = "出错后空闲".into();
        assert_eq!(status_color(&app, theme), theme.error);
    }

    #[test]
    fn approval_panel_clears_previous_content_and_fills_its_background() -> anyhow::Result<()> {
        let theme = Theme::detect();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = App {
            input: Input::default(),
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: String::new(),
            cursor_visible: true,
            command_selection: 0,
            history: vec!["background conversation text".into()],
            conversation_scroll: 0,
            tool_results_expanded: false,
            welcome_train_frame: None,
            model: "test".into(),
            root: "Normal".into(),
            ascii: true,
            language: UiLanguage::En,
            api_type: "Responses".into(),
            mode: "Agent".into(),
            turn: 0,
            max_context: 10,
            status: "waiting for confirmation".into(),
            provider_balance: None,
            popup: Some(PopupView {
                title: "Security confirmation".into(),
                lines: vec![
                    "Command: touch /tmp/file".into(),
                    "Risk: Mutating".into(),
                    "Reclassified locally".into(),
                    "> 1. Allow once [y]".into(),
                    "  2. Always allow [a]".into(),
                    "  3. Reject [n/Esc]".into(),
                    "  4. Edit [e]".into(),
                    "  5. Interactive [i]".into(),
                    "  6. UNIQUE_OPTION_RESIDUE".into(),
                ],
                dangerous: false,
                informational: false,
            }),
        };
        terminal.draw(|frame| draw(frame, &app))?;

        app.popup = Some(PopupView {
            title: "Security confirmation".into(),
            lines: vec!["High risk: type YES, then Enter:".into(), "Y".into()],
            dangerous: true,
            informational: false,
        });
        terminal.draw(|frame| draw(frame, &app))?;

        let buffer = terminal.backend().buffer();
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!rendered.contains("UNIQUE_OPTION_RESIDUE"));
        let input_top = buffer.area.height.saturating_sub(4);
        let area = bottom_left_rect(78, 11, input_top, buffer.area);
        assert_eq!(area.x, 0);
        assert_eq!(area.bottom(), input_top);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                assert_eq!(buffer[(x, y)].bg, theme.background_alt);
            }
        }
        assert!(matches!(buffer[(area.x, area.y)].symbol(), "┌" | "+"));
        assert!(matches!(
            buffer[(
                area.right().saturating_sub(1),
                area.bottom().saturating_sub(1)
            )]
                .symbol(),
            "┘" | "+"
        ));
        Ok(())
    }

    #[test]
    fn expanded_wrapped_tool_result_stays_bottom_aligned() -> anyhow::Result<()> {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend)?;
        let mut app = App {
            input: Input::default(),
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: String::new(),
            cursor_visible: true,
            command_selection: 0,
            history: vec![crate::tui::output::encode_tool_result(
                "[OK]",
                &format!("executed_command={}\nLAST_MARKER", "x".repeat(240)),
            )],
            conversation_scroll: 0,
            tool_results_expanded: true,
            welcome_train_frame: None,
            model: "test".into(),
            root: "Root".into(),
            ascii: true,
            language: UiLanguage::En,
            api_type: "Responses".into(),
            mode: "Agent".into(),
            turn: 1,
            max_context: 10,
            status: "idle".into(),
            provider_balance: None,
            popup: None,
        };
        terminal.draw(|frame| draw(frame, &app))?;
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("LAST_MARKER"));

        app.conversation_scroll = u16::MAX as usize;
        terminal.draw(|frame| draw(frame, &app))?;
        let rendered_top = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered_top.contains("Tool result"));
        Ok(())
    }

    #[test]
    fn buddha_linework_uses_bold_gold_without_styling_plain_details() {
        let theme = Theme::for_mode(crate::tui::theme::ColorMode::Ansi256);
        let lines = buddha_art_lines("\\ halo //", theme);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.iter().any(|span| {
            span.content.contains('\\')
                && span.style.fg == Some(theme.decorative_gold)
                && span
                    .style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
        }));
        assert!(lines[0].spans.iter().any(|span| {
            span.content.contains("halo") && span.style.fg == Some(theme.text_primary)
        }));
    }

    #[test]
    fn welcome_train_moves_across_the_viewport_and_contains_branding() {
        let theme = Theme::for_mode(crate::tui::theme::ColorMode::Ansi256);
        let middle = welcome_train_lines(64, 80, theme);
        let rendered = middle
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(rendered.contains("NL2SH"));
        assert!(middle.iter().all(|line| line.width() <= 80));

        let before_entry = welcome_train_lines(0, 80, theme);
        assert!(before_entry
            .iter()
            .all(|line| line.spans.iter().all(|span| span.content.is_empty())));
        let after_exit = welcome_train_lines(124, 80, theme);
        assert!(after_exit
            .iter()
            .all(|line| line.spans.iter().all(|span| span.content.is_empty())));

        let at_right_edge = welcome_train_lines(84, 80, theme);
        assert!(at_right_edge.iter().any(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .ends_with("D_")
        }));

        let at_odd_width_right_edge = welcome_train_lines(83, 79, theme);
        assert!(at_odd_width_right_edge.iter().any(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .ends_with("D_")
        }));
    }
}
