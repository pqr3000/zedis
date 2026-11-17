//! Adapter for drawing existing key rows in the sidebar's one virtual list.
//! Scans, expansion, selection, context menus and values remain owned here.

use super::bar_menus::{FilterMenu, TreeMoreMenu};
use super::*;
use crate::states::i18n_sidebar;
use gpui::EventEmitter;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

pub(crate) struct SidebarWorkspaceRequested {
    pub view: ServerView,
    pub editor_action: Option<EditorAction>,
}
impl EventEmitter<SidebarWorkspaceRequested> for ZedisKeyTree {}

pub(crate) fn sidebar_key_id(server: &str, db: usize, key: &str) -> SharedString {
    format!("sidebar-key-{server}-{db}-{key}").into()
}

struct SidebarSearch {
    input: Entity<InputState>,
}

impl Render for SidebarSearch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Input::new(&self.input).id(KEYWORD_INPUT_ID)
    }
}

impl ZedisKeyTree {
    /// The DB row and the standalone tree share the same filter and action menus.
    pub(crate) fn database_menu(
        tree: &Entity<Self>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let state = tree.read(cx);
        let filters = FilterMenu::for_tree(state, cx);
        let more = TreeMoreMenu::for_tree(state, cx);
        let can_create = state.server_state.read(cx).can(Capability::CreateKey);
        let target = tree.clone();
        let menu = menu
            .when_some(state.sidebar_focus.clone(), |menu, focus| menu.action_context(focus))
            .item(
                PopupMenuItem::new(i18n_sidebar(cx, "server_open")).on_click(move |_, _, cx| {
                    target.update(cx, |_, cx| {
                        cx.emit(SidebarWorkspaceRequested {
                            view: ServerView::Editor,
                            editor_action: None,
                        })
                    });
                }),
            )
            .menu(i18n_common(cx, "search"), Box::new(EditorAction::Search))
            .menu_with_enable(
                i18n_key_tree(cx, "add_key_title"),
                Box::new(EditorAction::Create),
                can_create,
            )
            .separator();
        filters.build(menu, window, cx).separator().submenu(
            i18n_key_tree(cx, "more_tooltip"),
            window,
            cx,
            move |menu, window, cx| more.build(menu, window, cx),
        )
    }

    pub(crate) fn sidebar_row_id(&self, server: &str, db: usize, index: usize, cx: &App) -> SharedString {
        let list = self.key_tree_list_state.read(cx);
        let item = &list.delegate().items[index];
        if item.is_folder || item.load_more_prefix.is_some() {
            format!("sidebar-folder-{server}-{db}-{}", item.id).into()
        } else {
            sidebar_key_id(server, db, &item.id)
        }
    }
    pub(crate) fn set_sidebar_focus(&mut self, focus: FocusHandle) {
        self.sidebar_focus = Some(focus);
    }

    pub(super) fn open_sidebar_search(&self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.keyword_state.clone();
        let view = cx.new(|_| SidebarSearch { input: input.clone() });
        let tree = cx.entity().downgrade();
        ZedisDialog::new(i18n_common(cx, "keyword_placeholder"))
            .w(px(420.))
            .ok_text(i18n_common(cx, "submit"))
            .cancel_text(i18n_common(cx, "cancel"))
            .child(move || view.clone())
            .on_ok(move |_, _, cx| {
                let _ = tree.update(cx, |tree, cx| tree.handle_filter(true, cx));
                true
            })
            .open(window, cx);
        input.focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn sidebar_row_selected(&self, index: usize, cx: &App) -> bool {
        let state = self.server_state.read(cx);
        !state.is_background()
            && self
                .key_tree_list_state
                .read(cx)
                .delegate()
                .items
                .get(index)
                .is_some_and(|item| !item.is_folder && state.key().as_ref() == Some(&item.id))
    }
    pub(crate) fn sidebar_row_count(&self, cx: &App) -> usize {
        self.key_tree_list_state.read(cx).delegate().items.len()
    }

    pub(crate) fn render_sidebar_row(&self, index: usize, window: &mut Window, cx: &mut App) -> Option<ListItem> {
        self.key_tree_list_state.update(cx, |list, cx| {
            list.delegate_mut().sidebar = true;
            list.delegate_mut().render_item(IndexPath::new(index), window, cx)
        })
    }

    pub(crate) fn select_sidebar_row(
        &mut self,
        index: usize,
        confirm: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = IndexPath::new(index);
        self.key_tree_list_state.update(cx, |list, cx| {
            list.set_selected_index(Some(ix), window, cx);
            cx.notify();
        });
        self.select_item_by_index(&ix, confirm, cx);
    }

    pub(crate) fn sidebar_action(&mut self, action: &KeyTreeAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            KeyTreeAction::RenameKey(key) => {
                self.server_state
                    .update(cx, |state, cx| state.select_key(key.clone(), cx));
                cx.emit(SidebarWorkspaceRequested {
                    view: ServerView::Editor,
                    editor_action: Some(EditorAction::Rename),
                });
            }
            KeyTreeAction::WatchFolderPrefix(id) => {
                self.server_state.update(cx, |state, _| {
                    state.set_pending_keyspace_prefix(state.folder_prefix(id).into());
                });
                cx.emit(SidebarWorkspaceRequested {
                    view: ServerView::KeyspaceNotifications,
                    editor_action: None,
                });
            }
            _ => actions::on_tree_action(self, action, window, cx),
        }
    }

    pub(crate) fn sidebar_editor_action(&mut self, action: &EditorAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            EditorAction::Create => self.handle_add_key(window, cx),
            EditorAction::ReloadKeyTree => self.handle_filter(false, cx),
            _ => actions::on_editor_action(self, action, window, cx),
        }
    }

    pub(crate) fn prepare_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<usize> {
        if std::mem::take(&mut self.state.clear_selection) {
            self.key_tree_list_state
                .update(cx, |state, cx| state.set_selected_index(None, window, cx));
        }
        if let Some(true) = self.should_enter_add_key_mode.take() {
            self.handle_add_key(window, cx);
        }
        self.state.scroll_to_index.take().map(|index| index.row)
    }
}
