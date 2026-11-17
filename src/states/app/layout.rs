use super::ZedisAppState;
use crate::constants::{SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_WIDTH};
use gpui::Pixels;

impl ZedisAppState {
    pub fn sidebar_px(&self, viewport: Pixels) -> Pixels {
        if self.sidebar_collapsed() {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            Self::fit_sidebar(self.sidebar_width.unwrap_or(SIDEBAR_WIDTH), viewport)
        }
    }

    /// Leave at least half of a narrow window for the active workspace.
    pub(crate) fn fit_sidebar(width: Pixels, viewport: Pixels) -> Pixels {
        width.max(SIDEBAR_MIN_WIDTH).min(viewport * 0.5)
    }

    pub(crate) fn set_sidebar_width(&mut self, width: Pixels) {
        self.sidebar_width = Some(width);
    }

    pub fn content_width(&self) -> Option<Pixels> {
        let width = self.bounds?.size.width;
        Some(width - self.sidebar_px(width))
    }
}
