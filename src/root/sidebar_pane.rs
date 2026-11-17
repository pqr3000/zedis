use super::Zedis;
use crate::states::{ZedisAppState, ZedisGlobalStore, update_app_state_and_save_quiet_debounced};
use gpui::{App, Axis, DragMoveEvent, EmptyView, Window, div, prelude::*};
use gpui_kit::component::resizable::{HandleEdge, resize_handle};

const SIDEBAR_RESIZE_ID: &str = "sidebar-resize";

struct ResizeSidebar;

impl Zedis {
    pub(super) fn sidebar_pane(&self, window: &Window, cx: &App) -> impl IntoElement {
        let state = cx.global::<ZedisGlobalStore>().read(cx);
        div()
            .relative()
            .w(state.sidebar_px(window.viewport_size().width))
            .flex_none()
            .h_full()
            .child(self.sidebar.clone())
            .when(!state.sidebar_collapsed(), |pane| {
                pane.child(
                    resize_handle(SIDEBAR_RESIZE_ID, Axis::Horizontal)
                        .inside(HandleEdge::Trailing)
                        .on_drag(ResizeSidebar, |_, _, _, cx| cx.new(|_| EmptyView)),
                )
                .on_drag_move(|event: &DragMoveEvent<ResizeSidebar>, window, cx| {
                    let viewport = window.viewport_size().width;
                    let width = ZedisAppState::fit_sidebar(event.event.position.x - event.bounds.left(), viewport);
                    if cx.global::<ZedisGlobalStore>().read(cx).sidebar_px(viewport) != width {
                        update_app_state_and_save_quiet_debounced(cx, "resize_sidebar", move |state, _| {
                            state.set_sidebar_width(width);
                        });
                        window.refresh();
                    }
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_WIDTH};
    use crate::test_support::init_ui;
    use gpui::{TestAppContext, point, px, size};
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    #[gpui::test]
    fn dragging_the_tree_resizes_the_workspace_and_survives_collapsing(cx: &mut TestAppContext) {
        init_ui(cx);
        let handle = cx
            .open_window(size(px(1200.), px(800.)), |window, cx| {
                let root = cx.new(|cx| Zedis::new(window, cx));
                Root::new(root, window, cx)
            })
            .into();
        cx.run_until_parked();

        for (target, expected) in [
            (px(510.), px(510.)),
            (px(60.), SIDEBAR_MIN_WIDTH),
            (px(1100.), px(600.)),
        ] {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let edge = window.find(SIDEBAR_RESIZE_ID).bounds().center();
                window.drag(edge, point(target, edge.y), cx);
            })
            .expect("open workspace");
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(window.find("sidebar-container").bounds().size.width, expected);
                assert_eq!(window.find("main-container").bounds().left(), expected);
            })
            .expect("open workspace");
        }

        cx.update_window(handle, |_, window, cx| window.click("sidebar-collapse-toggle", cx))
            .expect("open workspace");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(SIDEBAR_RESIZE_ID).is_none());
            assert_eq!(
                window.find("sidebar-container").bounds().size.width,
                SIDEBAR_COLLAPSED_WIDTH
            );
            window.click("sidebar-rail-toggle", cx);
        })
        .expect("open workspace");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let width = window.find("sidebar-container").bounds().size.width;
            assert_eq!(width, px(600.));
            let saved = toml::to_string(cx.global::<ZedisGlobalStore>().read(cx)).expect("serialize layout");
            let restored: ZedisAppState = toml::from_str(&saved).expect("read saved layout");
            assert_eq!(restored.sidebar_px(px(1200.)), width);
            assert_eq!(ZedisAppState::default().sidebar_px(px(1200.)), SIDEBAR_WIDTH);
        })
        .expect("open workspace");
    }
}
