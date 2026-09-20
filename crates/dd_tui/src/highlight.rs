use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::theme::AppTheme;

struct Syntect {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

fn syntect() -> &'static Syntect {
    static CELL: OnceLock<Syntect> = OnceLock::new();
    CELL.get_or_init(|| Syntect {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        themes: ThemeSet::load_defaults(),
    })
}

pub fn highlight_line(line: &str, theme: &AppTheme) -> Line<'static> {
    let st = syntect();
    let syntax = st
        .syntaxes
        .find_syntax_by_extension("md")
        .unwrap_or_else(|| st.syntaxes.find_syntax_plain_text());
    let syn_theme = st
        .themes
        .themes
        .get("base16-ocean.dark")
        .or_else(|| st.themes.themes.values().next());
    let Some(syn_theme) = syn_theme else {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(theme.text_primary),
        ));
    };
    let mut h = HighlightLines::new(syntax, syn_theme);
    let with_nl = if line.ends_with('\n') {
        line.to_string()
    } else {
        format!("{line}\n")
    };
    let ranges = match h.highlight_line(&with_nl, &st.syntaxes) {
        Ok(r) => r,
        Err(_) => {
            return Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.text_primary),
            ));
        }
    };
    let mut spans = Vec::new();
    for (style, text) in ranges {
        let text = text.trim_end_matches('\n');
        if text.is_empty() {
            continue;
        }
        spans.push(Span::styled(
            text.to_string(),
            syn_style_to_ratatui(style, theme),
        ));
    }
    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

fn syn_style_to_ratatui(style: syntect::highlighting::Style, theme: &AppTheme) -> Style {
    let fg = style.foreground;
    let mut s = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
    if style.font_style.contains(FontStyle::BOLD) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    let _ = theme;
    s
}
