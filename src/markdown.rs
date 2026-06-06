use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, opts);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut code_block_text = String::new();
    let mut in_code_block = false;
    let mut list_item_count: usize = 0;

    let flush_paragraph = |spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>| {
        if !spans.is_empty() {
            let trimmed_spans: Vec<Span<'static>> = std::mem::take(spans);
            lines.push(Line::from(trimmed_spans));
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {
                // start of paragraph
            }
            Event::End(TagEnd::Paragraph) => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_paragraph(&mut spans, &mut lines);
                let heading_style = Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(match level {
                        HeadingLevel::H1 => Color::Cyan,
                        HeadingLevel::H2 => Color::LightCyan,
                        HeadingLevel::H3 => Color::White,
                        _ => Color::White,
                    });
                spans.push(Span::styled(
                    match level {
                        HeadingLevel::H1 => "# ",
                        HeadingLevel::H2 => "## ",
                        HeadingLevel::H3 => "### ",
                        HeadingLevel::H4 => "#### ",
                        HeadingLevel::H5 => "##### ",
                        HeadingLevel::H6 => "###### ",
                    }
                    .to_string(),
                    heading_style,
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_paragraph(&mut spans, &mut lines);
                in_code_block = true;
                code_block_text.clear();
                if let CodeBlockKind::Fenced(lang) = kind {
                    if !lang.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("```{}", lang),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        )));
                    }
                } else {
                    lines.push(Line::from(Span::styled(
                        "```",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                let code_style = Style::default().fg(Color::Green);
                for line in code_block_text.lines() {
                    lines.push(Line::from(Span::styled(format!("  {}", line), code_style)));
                }
                lines.push(Line::from(Span::styled(
                    "```",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
                code_block_text.clear();
            }
            Event::Start(Tag::List(_)) => {
                flush_paragraph(&mut spans, &mut lines);
                list_item_count = 0;
            }
            Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::Item) => {
                flush_paragraph(&mut spans, &mut lines);
                list_item_count += 1;
                spans.push(Span::styled(
                    format!("  {} ", list_item_count),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Event::End(TagEnd::Item) => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::End(TagEnd::BlockQuote) => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::Start(Tag::Emphasis) => {
                // will handle via Start/End
            }
            Event::End(TagEnd::Emphasis) => {}
            Event::Start(Tag::Strong) => {}
            Event::End(TagEnd::Strong) => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                spans.push(Span::styled(
                    "[",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    dest_url.to_string(),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    "]",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            Event::End(TagEnd::Link) => {}
            Event::SoftBreak | Event::HardBreak => {
                flush_paragraph(&mut spans, &mut lines);
            }
            Event::Text(cow_text) => {
                let text_str = cow_text.to_string();
                if in_code_block {
                    code_block_text.push_str(&text_str);
                } else {
                    spans.push(Span::raw(text_str));
                }
            }
            Event::Code(cow_text) => {
                spans.push(Span::styled(
                    cow_text.to_string(),
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Event::Html(cow_text) => {
                spans.push(Span::styled(
                    cow_text.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
            }
            _ => {}
        }
    }

    flush_paragraph(&mut spans, &mut lines);

    // Trim trailing blank lines
    while lines.last().map(|l| l.width() == 0).unwrap_or(false) {
        lines.pop();
    }

    lines
}
