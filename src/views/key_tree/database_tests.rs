//! Regression coverage for retaining the existing tree's state in DB workspaces.

use super::*;
use crate::test_support::init_ui;
use crate::views::ZedisContent;
use gpui::{ScrollDelta, TestAppContext, point, size};
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;

impl ZedisKeyTree {
    pub(crate) fn set_folder_for_test(&mut self, cx: &mut Context<Self>) {
        self.key_tree_list_state.update(cx, |list, cx| {
            list.delegate_mut().items = vec![KeyTreeItem {
                id: "branch".into(),
                label: "branch".into(),
                is_folder: true,
                children_count: 2,
                ..Default::default()
            }];
            cx.notify();
        });
        cx.notify();
    }
    pub(crate) fn set_rows_for_test(&mut self, keys: &[&str], cx: &mut Context<Self>) {
        self.key_tree_list_state.update(cx, |list, cx| {
            list.delegate_mut().items = keys
                .iter()
                .map(|key| KeyTreeItem {
                    id: key.to_string().into(),
                    label: key.to_string().into(),
                    key_type: KeyType::String,
                    ..Default::default()
                })
                .collect();
            cx.notify();
        });
        cx.notify();
    }
}

#[gpui::test]
fn database_tree_list_stays_inside_its_short_viewport(cx: &mut TestAppContext) {
    init_ui(cx);
    let mut tree = None;
    let handle = cx.open_window(size(px(400.), px(360.)), |window, cx| {
        let state = cx.new(|_| ZedisServerState::default());
        let view = cx.new(|cx| ZedisKeyTree::new(state, window, cx));
        tree = Some(view.clone());
        Root::new(view, window, cx)
    });
    let tree = tree.expect("the tree was built");
    cx.run_until_parked();
    tree.update(cx, |tree, cx| {
        tree.set_external_visibility(true, cx);
        tree.key_tree_list_state.update(cx, |list, cx| {
            list.delegate_mut().items = (0..80)
                .map(|ix| KeyTreeItem {
                    id: format!("key-{ix}").into(),
                    label: format!("key-{ix}").into(),
                    key_type: KeyType::String,
                    ..Default::default()
                })
                .collect();
            cx.notify();
        });
        cx.notify();
    });
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let list = tree.read(cx).key_tree_list_state.read(cx);
        let viewport = list.scroll_handle().base_handle().bounds();
        let search = window.find(KEYWORD_INPUT_ID).bounds();
        assert!(
            viewport.top() >= search.bottom(),
            "rows must start below the search bar"
        );
        assert!(
            viewport.size.height > px(200.),
            "rows need a usable scroll viewport: {viewport:?}"
        );
        assert!(
            viewport.bottom() <= window.viewport_size().height,
            "the list must not scroll behind the following DB: {viewport:?}"
        );
        window.scroll(IndexPath::new(0), ScrollDelta::Pixels(point(px(0.), px(-10_000.))), cx);
        window.render_frame(cx);
        let last = window.find(IndexPath::new(79));
        assert!(last.visible(), "scrolling reaches the last key");
        assert!(
            last.bounds().bottom() <= viewport.bottom(),
            "the last key must be fully visible"
        );
    })
    .expect("the window is open");
}

#[gpui::test]
fn database_navigation_and_visibility_preserve_expanded_folders_and_scan_identity(cx: &mut TestAppContext) {
    init_ui(cx);
    let mut workspace = None;
    let _window = cx.open_window(size(px(500.), px(700.)), |window, cx| {
        let content = cx.new(|cx| ZedisContent::new_database_workspace("retained-tree".into(), 0, window, cx));
        let tree = content.read(cx).database_tree().expect("the workspace owns its tree");
        workspace = Some((content, tree.clone()));
        Root::new(tree, window, cx)
    });
    cx.run_until_parked();
    let (content, tree) = workspace.expect("the workspace was created");
    tree.update(cx, |tree, cx| {
        tree.state.expanded_items.insert("users".into());
        tree.state.suppressed_auto_expand.insert("orders".into());
        tree.state.last_scan = Some(("users:*".into(), QueryMode::Prefix));
        tree.set_external_visibility(false, cx);
    });
    content.update(cx, |content, cx| content.set_active(true, cx));
    cx.update(|cx| {
        cx.global::<ZedisGlobalStore>().clone().update(cx, |state, cx| {
            state.set_selected_server(("retained-tree".into(), 1), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|cx| {
        cx.global::<ZedisGlobalStore>().clone().update(cx, |state, cx| {
            state.set_selected_server(("retained-tree".into(), 0), cx);
        });
    });
    cx.run_until_parked();
    tree.update(cx, |tree, cx| {
        tree.set_external_visibility(true, cx);
        assert!(tree.state.expanded_items.contains("users"));
        assert!(tree.state.suppressed_auto_expand.contains("orders"));
        assert_eq!(tree.state.last_scan, Some(("users:*".into(), QueryMode::Prefix)));
        assert_eq!(tree.server_state.read(cx).db(), 0);
    });
    assert_eq!(content.read_with(cx, |content, _| content.database_tree()), Some(tree));
}
