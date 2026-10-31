use std::time::{Duration, Instant};

use ratatui::layout::Rect;

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

pub fn prune_toasts(toasts: &mut Vec<Toast>) -> bool {
    let before = toasts.len();
    let now = Instant::now();
    toasts.retain(|t| now.duration_since(t.shown_at) < TOAST_TTL);
    toasts.len() != before
}

/// Time until the next toast expires, if any are showing.
pub fn next_expiry(toasts: &[Toast]) -> Option<Duration> {
    let now = Instant::now();
    toasts
        .iter()
        .map(|t| TOAST_TTL.saturating_sub(now.duration_since(t.shown_at)))
        .min()
}

pub fn render_toasts(app: &App, frame: &mut ratatui::Frame, area: Rect, lift: u16) {
    let lines: Vec<crate::toast_chrome::ToastLine> = app
        .toasts
        .iter()
        .map(|toast| crate::toast_chrome::ToastLine {
            kind: match toast.level {
                ToastLevel::Success => crate::toast_chrome::ToastKind::Success,
                ToastLevel::Info => crate::toast_chrome::ToastKind::Info,
                ToastLevel::Warning => crate::toast_chrome::ToastKind::Warning,
                ToastLevel::Error => crate::toast_chrome::ToastKind::Error,
            },
            message: toast.message.as_str(),
        })
        .collect();
    let _ = crate::toast_chrome::render_toast_stack(
        frame,
        area,
        &lines,
        crate::toast_chrome::ToastPaint {
            modal_background: app.theme.modal_background,
            text: app.theme.text_primary,
            success: app.theme.success,
            info: app.theme.info,
            warning: app.theme.warning,
            error: app.theme.error,
        },
        lift,
    );
}
