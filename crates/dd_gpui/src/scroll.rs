//! Scrollbar thumb geometry and the wheel/model sync for the three panes.

use gpui::{ScrollHandle, point, px};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thumb {
    pub y: f32,
    pub h: f32,
}

/// One pane's GPUI scroll handle plus the last positive offset we wrote.
///
/// The handle's y is 0 at the top and negative once scrolled. `written` is that
/// distance as a positive pixel count, so a later wheel can be told apart from
/// our own push.
pub struct ScrollPort {
    pub handle: ScrollHandle,
    written: f32,
}

impl ScrollPort {
    pub fn new() -> Self {
        Self {
            handle: ScrollHandle::new(),
            written: 0.0,
        }
    }

    /// Positive pixels scrolled down, when the handle moved since the last push.
    pub fn pull_px(&mut self) -> Option<f32> {
        let y = (-self.handle.offset().y / px(1.)).max(0.0);
        if (y - self.written).abs() > 0.5 {
            self.written = y;
            Some(y)
        } else {
            None
        }
    }

    /// Write a positive scroll offset back onto the handle.
    pub fn push_px(&mut self, y: f32) {
        let y = y.max(0.0);
        self.written = y;
        self.handle.set_offset(point(px(0.), px(-y)));
    }

    /// Last laid-out viewport height. Empty until the scroller has been painted.
    pub fn view_px(&self) -> Option<f32> {
        let height = self.handle.bounds().size.height / px(1.);
        if height > 1.0 { Some(height) } else { None }
    }
}

/// Viewport height for a pane's rows.
///
/// A scrollport that has been laid out wins. Otherwise a rows canvas that is
/// shorter than its pane is the rows viewport. A canvas at least as tall as
/// the pane grew with the document, so the pane minus chrome is used instead.
pub fn rows_viewport_px(
    port_h: Option<f32>,
    rows_h: Option<f32>,
    pane_h: Option<f32>,
    chrome: f32,
    fallback: f32,
) -> f32 {
    if let Some(height) = port_h
        && height > 1.0
    {
        return height;
    }
    if let Some(rows_h) = rows_h
        && let Some(pane_h) = pane_h
    {
        if rows_h + 1.0 < pane_h {
            return rows_h.max(1.0);
        }
        return (pane_h - chrome).max(1.0);
    }
    if let Some(rows_h) = rows_h {
        return rows_h.max(1.0);
    }
    if let Some(pane_h) = pane_h {
        return (pane_h - chrome).max(1.0);
    }
    fallback.max(1.0)
}

/// Thumb inside a track. `offset` and `visible` use the same unit as `total`.
/// Returns nothing when the content fits.
pub fn thumb_geom(offset: f32, visible: f32, total: f32, track: f32) -> Option<Thumb> {
    if visible <= 0.0 || track <= 1.0 || total <= visible + 0.5 {
        return None;
    }
    let h = (track * (visible / total)).clamp(16.0, track);
    let travel = (track - h).max(0.0);
    let max_off = (total - visible).max(1.0);
    let y = ((offset.max(0.0) / max_off) * travel).clamp(0.0, travel);
    Some(Thumb { y, h })
}

/// Content offset for a thumb whose top sits at `thumb_y` in the track.
pub fn offset_for_thumb(thumb_y: f32, visible: f32, total: f32, track: f32) -> f32 {
    if visible <= 0.0 || track <= 1.0 || total <= visible {
        return 0.0;
    }
    let h = (track * (visible / total)).clamp(16.0, track);
    let travel = (track - h).max(0.0);
    let max_off = (total - visible).max(0.0);
    if travel <= 0.0 {
        return 0.0;
    }
    ((thumb_y.max(0.0) / travel) * max_off).clamp(0.0, max_off)
}

/// Block index whose vertical span contains `y`.
pub fn index_at(tops: &[f32], heights: &[f32], y: f32) -> usize {
    let mut found = 0usize;
    for (i, top) in tops.iter().enumerate() {
        let h = heights.get(i).copied().unwrap_or(0.0);
        found = i;
        if y < top + h {
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    #[test]
    fn hidden_when_the_content_fits() {
        assert!(thumb_geom(0.0, 20.0, 20.0, 200.0).is_none());
        assert!(thumb_geom(0.0, 20.0, 10.0, 200.0).is_none());
    }

    #[test]
    fn thumb_tracks_the_offset() {
        let track = 200.0;
        let visible = 50.0;
        let total = 200.0;
        let top = thumb_geom(0.0, visible, total, track).expect("thumb");
        assert_eq!(top.y, 0.0);
        let max_off = total - visible;
        let bottom = thumb_geom(max_off, visible, total, track).expect("thumb");
        let travel = track - bottom.h;
        assert!((bottom.y - travel).abs() < 0.01, "{}", bottom.y);
        assert!((offset_for_thumb(0.0, visible, total, track)).abs() < 0.01);
        let back = offset_for_thumb(bottom.y, visible, total, track);
        assert!((back - max_off).abs() < 0.5, "{back}");
    }

    #[test]
    fn port_pull_ignores_our_own_push() {
        let mut port = ScrollPort::new();
        assert!(port.pull_px().is_none());
        port.push_px(40.0);
        assert!(port.pull_px().is_none());
        assert_eq!(port.handle.offset().y, px(-40.));
        port.handle.set_offset(point(px(0.), px(-80.)));
        assert_eq!(port.pull_px(), Some(80.0));
        assert!(port.pull_px().is_none());
    }

    #[test]
    fn port_pull_clamps_a_positive_offset_to_the_top() {
        let mut port = ScrollPort::new();
        port.push_px(40.0);
        port.handle.set_offset(point(px(0.), px(12.)));
        assert_eq!(port.pull_px(), Some(0.0));
    }

    #[test]
    fn grown_canvas_is_not_the_viewport() {
        let grown = rows_viewport_px(None, Some(800.0), Some(800.0), 28.0, 400.0);
        assert_eq!(grown, 772.0);
        let rows = rows_viewport_px(None, Some(400.0), Some(800.0), 28.0, 100.0);
        assert_eq!(rows, 400.0);
        let laid_out = rows_viewport_px(Some(360.0), Some(800.0), Some(800.0), 28.0, 100.0);
        assert_eq!(laid_out, 360.0);
        let fallback = rows_viewport_px(None, None, None, 28.0, 222.0);
        assert_eq!(fallback, 222.0);
        let tiny_port = rows_viewport_px(Some(1.0), Some(180.0), Some(500.0), 28.0, 100.0);
        assert_eq!(tiny_port, 180.0);
    }

    #[test]
    fn index_at_lands_inside_the_block() {
        let tops = [0.0, 40.0, 70.0];
        let heights = [40.0, 30.0, 20.0];
        assert_eq!(index_at(&tops, &heights, 0.0), 0);
        assert_eq!(index_at(&tops, &heights, 39.0), 0);
        assert_eq!(index_at(&tops, &heights, 40.0), 1);
        assert_eq!(index_at(&tops, &heights, 100.0), 2);
    }
}
