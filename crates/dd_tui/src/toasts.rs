use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

pub const TOAST_TTL: Duration = Duration::from_secs(5);
const TOAST_CAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Success/Info are used by later PRs and tests
pub enum ToastLevel {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub level: ToastLevel,
    pub message: String,
    pub shown_at: Instant,
}

pub fn push_toast(toasts: &mut Vec<Toast>, level: ToastLevel, message: impl Into<String>) {
    toasts.push(Toast {
        level,
        message: message.into(),
        shown_at: Instant::now(),
    });
    if toasts.len() > TOAST_CAP {
        toasts.remove(0);
    }
}

pub fn prune_toasts(toasts: &mut Vec<Toast>) {
    let now = Instant::now();
    toasts.retain(|t| now.duration_since(t.shown_at) < TOAST_TTL);
}

pub fn render_toasts(app: &App, frame: &mut ratatui::Frame, area: Rect, lift: u16) {
    if app.toasts.is_empty() {
        return;
    }
    let toast_w: u16 = 60;
    let gap: u16 = 1;
    let max_width = area.width.saturating_sub(2);
    let width = toast_w.min(max_width);
    if width < 10 {
        return;
    }
    let right_x = area.x + area.width.saturating_sub(width + 1);
    let toast_h: u16 = 3;
    // Sit on the body, above the footer and the AI card.
    let mut y = area.y + area.height.saturating_sub(toast_h + 1 + lift);
    for toast in app.toasts.iter().rev() {
        if y + toast_h > area.y + area.height {
            break;
        }
        let rect = Rect {
            x: right_x,
            y,
            width,
            height: toast_h,
        };
        let (glyph, accent) = match toast.level {
            ToastLevel::Success => ("✓", app.theme.success),
            ToastLevel::Info => ("ℹ", app.theme.info),
            ToastLevel::Warning => ("⚠", app.theme.warning),
            ToastLevel::Error => ("✕", app.theme.error),
        };
        frame.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(app.theme.modal_background))
            .border_style(Style::default().fg(accent));
        frame.render_widget(block, rect);
        let body = Paragraph::new(format!("{glyph} {}", toast.message))
            .style(Style::default().fg(accent).bg(app.theme.modal_background));
        frame.render_widget(
            body,
            Rect {
                x: rect.x + 2,
                y: rect.y + 1,
                width: rect.width.saturating_sub(4),
                height: 1,
            },
        );
        if y < area.y + toast_h + gap {
            break;
        }
        y = y.saturating_sub(toast_h + gap);
    }
}
