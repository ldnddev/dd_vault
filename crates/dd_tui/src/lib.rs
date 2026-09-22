//! Family chrome (header / body / footer / F1 / F2 / toasts) for `dd_vault`.

mod ai;
mod app;
mod busy;
mod draw;
mod events;
mod git;
mod help;
mod highlight;
mod images;
mod palette;
mod theme;
mod toasts;
mod tree;
mod watch;

pub use app::run;
pub use theme::{AppTheme, ThemeLoad};

#[cfg(test)]
mod tests;
