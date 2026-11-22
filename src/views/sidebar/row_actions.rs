//! Quick actions for the selected tree node. Overflow menus also expose them
//! when the pane is too narrow for a row of buttons.

use super::{ZedisSidebar, workspaces::DatabaseWorkspace};
use crate::assets::CustomIconName;
use crate::connection::Capability;
use crate::helpers::EditorAction;
use crate::states::{ServerView, ZedisGlobalStore, i18n_common, i18n_key_tree};
use crate::views::{
    ZedisKeyTree,
    key_tree::{KeyTreeAction, SidebarNode},
    kv_table::CollectionTools,
};
use gpui::{AnyElement, App, Entity, SharedString, Window, prelude::*, rems};
use gpui_kit::component::{
    Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::DropdownMenu,
};

fn action_button(id: &str, command: &str, icon: Icon, label: SharedString) -> Button {
    Button::new(SharedString::from(format!("{id}-{command}")))
        .ghost()
        .xsmall()
        .icon(icon)
        .accessibility_label(label.clone())
        .tooltip(label)
}

impl ZedisSidebar {
    pub(super) fn room_for_row_actions(window: &Window, cx: &App) -> bool {
        let width = cx
            .global::<ZedisGlobalStore>()
            .read(cx)
            .sidebar_px(window.viewport_size().width);
        width >= rems(20.).to_pixels(window.rem_size())
    }

    pub(super) fn database_actions(&self, server: &str, db: usize, entry: &DatabaseWorkspace, cx: &App) -> AnyElement {
        let tree = &entry.tree;
        let id = format!("sidebar-db-{server}-{db}");
        let can_add = entry
            .content
            .read(cx)
            .server_state()
            .read(cx)
            .can(Capability::CreateKey);
        let commands = [
            (
                "search",
                Icon::new(IconName::Search),
                i18n_common(cx, "search"),
                EditorAction::Search,
            ),
            (
                "refresh",
                Icon::new(CustomIconName::RotateCw),
                i18n_key_tree(cx, "refresh_keys"),
                EditorAction::ReloadKeyTree,
            ),
            (
                "add",
                Icon::new(CustomIconName::FilePlusCorner),
                i18n_key_tree(cx, "add_key_title"),
                EditorAction::Create,
            ),
        ];
        h_flex()
            .flex_none()
            .children(
                commands
                    .into_iter()
                    .filter(|(_, _, _, action)| *action != EditorAction::Create || can_add)
                    .map(|(name, icon, label, action)| {
                        let tree = tree.clone();
                        action_button(&id, name, icon, label).on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            tree.update(cx, |tree, cx| tree.sidebar_editor_action(&action, window, cx));
                        })
                    }),
            )
            .into_any_element()
    }

    pub(super) fn item_actions(
        &self,
        tree: &Entity<ZedisKeyTree>,
        node: &SidebarNode,
        id: &str,
        offset: usize,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let commands = match node {
            SidebarNode::Folder(prefix) => [
                (
                    "refresh",
                    Icon::new(CustomIconName::RotateCw),
                    "refresh_folder_tooltip",
                    KeyTreeAction::RefreshFolder(prefix.clone()),
                ),
                (
                    "copy",
                    Icon::new(IconName::Copy),
                    "copy_prefix_tooltip",
                    KeyTreeAction::CopyFolderPrefix(prefix.clone()),
                ),
            ],
            SidebarNode::Key(key) => [
                (
                    "copy",
                    Icon::new(IconName::Copy),
                    "copy_key_tooltip",
                    KeyTreeAction::CopyKeyName(key.clone()),
                ),
                (
                    "rename",
                    Icon::new(CustomIconName::FilePenLine),
                    "rename_key_tooltip",
                    KeyTreeAction::RenameKey(key.clone()),
                ),
            ],
            SidebarNode::Database => return h_flex().into_any_element(),
        };
        let more = i18n_key_tree(cx, "more_tooltip");
        let can_rename = tree.read(cx).can_rename(cx);
        h_flex()
            .flex_none()
            .when(Self::room_for_row_actions(window, cx), |bar| {
                bar.children(
                    commands
                        .into_iter()
                        .filter(|(name, ..)| *name != "rename" || can_rename)
                        .map(|(name, icon, tip, action)| {
                            let tree = tree.clone();
                            action_button(id, name, icon, i18n_key_tree(cx, tip)).on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                tree.update(cx, |tree, cx| tree.sidebar_action(&action, window, cx));
                            })
                        }),
                )
            })
            .child(
                Button::new(SharedString::from(format!("{id}-more")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .accessibility_label(more.clone())
                    .tooltip(more)
                    .dropdown_menu(tree.read(cx).sidebar_row_menu(offset, cx)),
            )
            .into_any_element()
    }

    pub(super) fn selected_collection_tools(&self, cx: &App) -> Option<Entity<CollectionTools>> {
        if cx.global::<ZedisGlobalStore>().read(cx).route().server_view() != Some(ServerView::Editor) {
            return None;
        }
        let content = self.active_content.as_ref()?.read(cx);
        let state = content.server_state();
        let state = state.read(cx);
        let key = state.key()?;
        if state.is_terminal()
            || self.workspaces.focused.as_ref().is_some_and(|(tree, node)| {
                Some(tree) != content.database_tree().as_ref()
                    || !matches!(node, SidebarNode::Key(selected) if selected == &key)
            })
        {
            return None;
        }
        content.collection_tools()
    }
}
