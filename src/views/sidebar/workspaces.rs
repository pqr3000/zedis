//! Retain existing workspace/tree entities per database rather than copying
//! search, scan cursors or expanded-folder state into another implementation.

use super::*;
use crate::states::{ServerEvent, ServerView, i18n_key_tree};
use crate::views::key_tree::SidebarWorkspaceRequested;
use crate::views::{ZedisContent, ZedisKeyTree};
use gpui::{Anchor, Entity, EventEmitter, Focusable};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use std::collections::HashSet;

pub(crate) struct DatabaseWorkspaceSelected {
    pub content: Entity<ZedisContent>,
    pub view: ServerView,
}

pub(super) fn database_menu_id(server: &str, db: usize) -> SharedString {
    format!("sidebar-db-menu-{server}-{db}").into()
}

impl EventEmitter<DatabaseWorkspaceSelected> for ZedisSidebar {}

pub(super) struct DatabaseWorkspace {
    pub(super) content: Entity<ZedisContent>,
    pub(super) tree: Entity<ZedisKeyTree>,
    pub(super) expanded: bool,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct DatabaseWorkspaces {
    pub(super) entries: HashMap<(String, usize), DatabaseWorkspace>,
    pub(super) focused: Option<Entity<ZedisKeyTree>>,
}

impl DatabaseWorkspaces {
    pub(super) fn retain<'a>(&mut self, servers: impl Iterator<Item = &'a str>) {
        let ids: HashSet<_> = servers.collect();
        self.entries.retain(|(id, _), _| ids.contains(id.as_str()));
        if self
            .focused
            .as_ref()
            .is_some_and(|focused| !self.entries.values().any(|entry| &entry.tree == focused))
        {
            self.focused = None;
        }
    }
}

impl ZedisSidebar {
    pub(super) fn open_database_workspace(
        &mut self,
        server: SharedString,
        db: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.ensure_database_workspace(server.clone(), db, window, cx);
        self.workspaces.focused = content.read(cx).database_tree();
        self.reveal_database_workspace(&server, db, cx);
    }

    pub(crate) fn cached_database_workspace(&self, server: &str, db: usize) -> Option<Entity<ZedisContent>> {
        self.workspaces
            .entries
            .get(&(server.to_string(), db))
            .map(|entry| entry.content.clone())
    }

    pub(crate) fn reveal_database_workspace(&mut self, server: &str, db: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.workspaces.entries.get_mut(&(server.to_string(), db)) {
            entry.expanded = true;
            self.databases.expanded.insert(server.to_string());
            cx.notify();
        }
    }

    pub(crate) fn ensure_database_workspace(
        &mut self,
        server: SharedString,
        db: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ZedisContent> {
        if let Some(content) = self.cached_database_workspace(&server, db) {
            return content;
        }
        let focus = window
            .focused(cx)
            .unwrap_or_else(|| self.navigation.read(cx).focus_handle(cx));
        let content = cx.new(|cx| ZedisContent::new_database_workspace(server.clone(), db, window, cx));
        focus.focus(window, cx);
        self.retain_database_workspace(content.clone(), window, cx);
        content
    }

    pub(crate) fn adopt_database_workspace(
        &mut self,
        content: Entity<ZedisContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = window
            .focused(cx)
            .unwrap_or_else(|| self.navigation.read(cx).focus_handle(cx));
        content.update(cx, |content, cx| content.use_sidebar_tree(window, cx));
        focus.focus(window, cx);
        self.retain_database_workspace(content, window, cx);
    }

    fn retain_database_workspace(
        &mut self,
        content: Entity<ZedisContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = content
            .read(cx)
            .database_tree()
            .expect("a database workspace owns its tree");
        let focus = self.navigation.read(cx).focus_handle(cx);
        tree.update(cx, |tree, cx| {
            tree.set_sidebar_focus(focus);
            tree.set_external_visibility(false, cx);
        });
        let state = content.read(cx).server_state();
        let (server, db) = (state.read(cx).server_id().to_string(), state.read(cx).db());
        let updates = cx.subscribe(&state, move |this, state, event, cx| {
            if matches!(
                event,
                ServerEvent::ServerInfoUpdated | ServerEvent::ServerRedisInfoUpdated
            ) {
                this.update_databases(&state, cx);
            }
        });
        let selected_content = content.clone();
        let opened = cx.subscribe_in(
            &tree,
            window,
            move |this, tree, event: &SidebarWorkspaceRequested, window, cx| {
                this.workspaces.focused = Some(tree.clone());
                if let Some(action) = event.editor_action {
                    selected_content.update(cx, |content, cx| content.sidebar_editor_action(action, window, cx));
                }
                cx.emit(DatabaseWorkspaceSelected {
                    content: selected_content.clone(),
                    view: event.view,
                });
            },
        );
        let rows = cx.observe(&tree, |_, _, cx| cx.notify());
        self.workspaces.entries.insert(
            (server, db),
            DatabaseWorkspace {
                content: content.clone(),
                tree,
                expanded: false,
                _subscriptions: vec![updates, opened, rows],
            },
        );
    }

    pub(super) fn update_database_visibility(&self, cx: &mut Context<Self>) {
        let store = cx.global::<ZedisGlobalStore>().read(cx);
        let rail = store.sidebar_collapsed();
        let visible_servers: HashSet<_> = self
            .state
            .sections
            .iter()
            .filter(|section| {
                (section.group.is_none() && self.state.sections.len() == 1)
                    || !store.is_server_group_collapsed(&section.key)
            })
            .flat_map(|section| section.servers.iter().map(|server| server.id.as_ref()))
            .collect();
        for ((server, _), entry) in &self.workspaces.entries {
            let visible = !rail
                && entry.expanded
                && visible_servers.contains(server.as_str())
                && self.databases.expanded.contains(server);
            entry
                .tree
                .update(cx, |tree, cx| tree.set_external_visibility(visible, cx));
        }
    }

    pub(super) fn database_tree_row(
        &self,
        server: SharedString,
        db: usize,
        row: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entry = self.workspaces.entries.get(&(server.to_string(), db));
        let expanded = entry.is_some_and(|entry| entry.expanded);
        let toggle_server = server.clone();
        let toggle = Button::new(SharedString::from(format!("sidebar-db-toggle-{server}-{db}")))
            .ghost()
            .xsmall()
            .icon(if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .accessibility_label(format!("db{db}"))
            .toggled(expanded)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.navigation.read(cx).focus_handle(cx).focus(window, cx);
                let entry = this.workspaces.entries.get_mut(&(toggle_server.to_string(), db));
                if let Some(entry) = entry.filter(|entry| entry.expanded) {
                    entry.expanded = false;
                    entry
                        .tree
                        .update(cx, |tree, cx| tree.set_external_visibility(false, cx));
                    cx.notify();
                } else {
                    this.open_database_workspace(toggle_server.clone(), db, window, cx);
                }
            }));
        let row = h_flex()
            .ml_6()
            .h_8()
            .items_center()
            .child(toggle)
            .child(div().flex_1().min_w_0().child(row));
        let Some(entry) = entry.filter(|entry| entry.expanded) else {
            return row.into_any_element();
        };
        let tree = entry.tree.clone();
        let sidebar = cx.entity().downgrade();
        let menu = move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
            let _ = sidebar.update(cx, |sidebar, _| sidebar.workspaces.focused = Some(tree.clone()));
            ZedisKeyTree::database_menu(&tree, menu, window, cx)
        };
        let more = i18n_key_tree(cx, "more_tooltip");
        row.child(
            Button::new(database_menu_id(&server, db))
                .ghost()
                .xsmall()
                .icon(IconName::Ellipsis)
                .accessibility_label(format!("db{db} · {more}"))
                .tooltip(more)
                .dropdown_menu_with_anchor(Anchor::TopRight, menu.clone()),
        )
        .context_menu(menu)
        .into_any_element()
    }
}
