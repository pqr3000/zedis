//! A single virtual viewport for connection, DB and existing key-tree rows.
//! Segments describe whole DB ranges without copying every loaded key.

use super::databases::{database_id, disclosure_id, open_database};
use super::*;
use crate::states::i18n_common;
use crate::views::{
    ZedisKeyTree,
    key_tree::{on_query_mode, on_type_filter},
};
use gpui::{Action, ClickEvent, Entity, Focusable, MouseButton, MouseDownEvent, ScrollStrategy, WeakEntity};
use gpui_kit::component::{
    IndexPath, Selectable,
    list::{List, ListDelegate, ListEvent, ListState},
};
use std::ops::Range;

#[derive(Clone)]
enum NavigationRow {
    Group(usize),
    Connection(usize, usize),
    Database(SharedString, usize),
    Discovery(SharedString),
    Keys {
        server: SharedString,
        db: usize,
        tree: Entity<ZedisKeyTree>,
    },
}

struct Segment {
    range: Range<usize>,
    row: NavigationRow,
}

pub(super) struct NavigationDelegate {
    sidebar: WeakEntity<ZedisSidebar>,
    segments: Vec<Segment>,
}

impl NavigationDelegate {
    fn row(&self, index: usize) -> Option<(NavigationRow, usize)> {
        let segment = self.segments.iter().find(|segment| segment.range.contains(&index))?;
        Some((segment.row.clone(), index - segment.range.start))
    }
}

#[derive(IntoElement)]
pub(super) struct NavigationItem(AnyElement);

impl Selectable for NavigationItem {
    fn is_selected(&self) -> bool {
        false
    }
    fn selected(self, _: bool) -> Self {
        self
    }
}

impl RenderOnce for NavigationItem {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .key_context("SidebarTreeRow")
            .h_8()
            .w_full()
            .overflow_hidden()
            .child(self.0)
    }
}

impl ListDelegate for NavigationDelegate {
    type Item = NavigationItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.segments.last().map_or(0, |segment| segment.range.end)
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let (row, offset) = self.row(index.row)?;
        self.sidebar
            .update(cx, |sidebar, cx| {
                NavigationItem(sidebar.render_navigation_row(row, offset, window, cx))
            })
            .ok()
    }

    fn set_selected_index(&mut self, _: Option<IndexPath>, _: &mut Window, _: &mut Context<ListState<Self>>) {}
}

impl ZedisSidebar {
    pub(super) fn new_navigation(window: &mut Window, cx: &mut Context<Self>) -> Entity<ListState<NavigationDelegate>> {
        let sidebar = cx.entity().downgrade();
        cx.new(|cx| {
            ListState::new(
                NavigationDelegate {
                    sidebar,
                    segments: Vec::new(),
                },
                window,
                cx,
            )
        })
    }

    pub(super) fn on_navigation_event(
        &mut self,
        list: &Entity<ListState<NavigationDelegate>>,
        event: &ListEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (index, confirm) = match event {
            ListEvent::Select(index) => (*index, false),
            ListEvent::Confirm(index) => (*index, true),
            _ => return,
        };
        if let Some((NavigationRow::Keys { tree, .. }, offset)) = list.read(cx).delegate().row(index.row) {
            if let Some(node) = tree.read(cx).sidebar_node(offset, cx) {
                self.workspaces.focused = Some((tree.clone(), node));
                cx.notify();
            }
            tree.update(cx, |tree, cx| tree.select_sidebar_row(offset, confirm, window, cx));
        }
    }

    fn navigation_segments(&self, cx: &App) -> Vec<Segment> {
        let store = cx.global::<ZedisGlobalStore>().read(cx);
        let mut segments = Vec::new();
        let mut start = 0;
        let mut push = |row, len| {
            // Empty DBs still process deferred actions, such as Add Key.
            segments.push(Segment {
                range: start..start + len,
                row,
            });
            start += len;
        };
        for (section_ix, section) in self.state.sections.iter().enumerate() {
            let show_group = section.group.is_some() || self.state.sections.len() > 1;
            if show_group {
                push(NavigationRow::Group(section_ix), 1);
                if store.is_server_group_collapsed(&section.key) {
                    continue;
                }
            }
            for (server_ix, server) in section.servers.iter().enumerate() {
                push(NavigationRow::Connection(section_ix, server_ix), 1);
                if !self.databases.expanded.contains(server.id.as_ref()) {
                    continue;
                }
                let Some(summary) = self.databases.servers.get(server.id.as_ref()) else {
                    push(NavigationRow::Discovery(server.id.clone()), 1);
                    continue;
                };
                for db in 0..summary.keys.len() {
                    push(NavigationRow::Database(server.id.clone(), db), 1);
                    if let Some(entry) = self
                        .workspaces
                        .entries
                        .get(&(server.id.to_string(), db))
                        .filter(|entry| entry.expanded)
                    {
                        push(
                            NavigationRow::Keys {
                                server: server.id.clone(),
                                db,
                                tree: entry.tree.clone(),
                            },
                            entry.tree.read(cx).sidebar_row_count(cx),
                        );
                    }
                }
            }
        }
        segments
    }

    pub(super) fn render_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let segments = self.navigation_segments(cx);
        let mut scroll_to = None;
        for segment in &segments {
            if let NavigationRow::Keys { tree, .. } = &segment.row {
                let index = tree.update(cx, |tree, cx| tree.prepare_sidebar(window, cx));
                if let Some(index) = index.filter(|index| *index < segment.range.len()) {
                    scroll_to = Some(segment.range.start + index);
                }
            }
        }
        self.navigation.update(cx, |list, cx| {
            list.delegate_mut().segments = segments;
            if let Some(index) = scroll_to {
                list.set_selected_index(Some(IndexPath::new(index)), window, cx);
                list.scroll_to_item(IndexPath::new(index), ScrollStrategy::Top, window, cx);
            }
        });
        v_flex()
            .size_full()
            .min_h_0()
            .child(
                v_flex()
                    .id(SIDEBAR_LIST_ID)
                    .flex_1()
                    .min_h_0()
                    .child(List::new(&self.navigation)),
            )
            .on_action(cx.listener(Self::focused_tree_action(ZedisKeyTree::sidebar_action)))
            .on_action(cx.listener(Self::focused_tree_action(ZedisKeyTree::sidebar_editor_action)))
            .on_action(cx.listener(Self::focused_tree_action(on_query_mode)))
            .on_action(cx.listener(Self::focused_tree_action(on_type_filter)))
            .into_any_element()
    }

    fn focused_tree_action<A: Action>(
        handler: fn(&mut ZedisKeyTree, &A, &mut Window, &mut Context<ZedisKeyTree>),
    ) -> impl Fn(&mut Self, &A, &mut Window, &mut Context<Self>) {
        move |sidebar, action, window, cx| {
            if let Some((tree, _)) = sidebar.workspaces.focused.clone() {
                tree.update(cx, |tree, cx| handler(tree, action, window, cx));
            } else {
                cx.propagate();
            }
        }
    }

    fn render_navigation_row(
        &mut self,
        row: NavigationRow,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            NavigationRow::Keys { server, db, tree } => {
                let node = tree.read(cx).sidebar_node(offset, cx);
                let selected = self.workspaces.focused.as_ref().map_or_else(
                    || tree.read(cx).sidebar_row_selected(offset, cx),
                    |(focused, selection)| focused == &tree && Some(selection) == node.as_ref(),
                );
                let id = tree.read(cx).sidebar_row_id(&server, db, offset, cx);
                let actions = node
                    .as_ref()
                    .filter(|_| selected)
                    .map(|node| self.item_actions(&tree, node, &id, offset, window, cx));
                let item = tree.update(cx, |tree, cx| tree.render_sidebar_row(offset, selected, window, cx));
                h_flex()
                    .id(id)
                    .pl_10()
                    .h_8()
                    .w_full()
                    .when(selected, |row| row.bg(cx.theme().list_active))
                    .capture_any_mouse_down(cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        if matches!(event.button, MouseButton::Left | MouseButton::Right) {
                            if let Some(node) = &node {
                                this.workspaces.focused = Some((tree.clone(), node.clone()));
                                cx.notify();
                            }
                            this.navigation.read(cx).focus_handle(cx).focus(window, cx);
                        }
                    }))
                    .child(div().flex_1().min_w_0().children(item))
                    .children(actions)
                    .into_any_element()
            }
            NavigationRow::Database(server, db) => self.navigation_database(server, db, window, cx),
            NavigationRow::Discovery(server) => {
                let loading = self.databases.loading.contains_key(server.as_ref());
                Button::new(SharedString::from(format!("sidebar-db-open-{server}")))
                    .ghost()
                    .small()
                    .ml_10()
                    .icon(IconName::RefreshCw)
                    .loading(loading)
                    .label(i18n_common(cx, if loading { "loading" } else { "reload" }))
                    .when_some(self.databases.errors.get(server.as_ref()), |button, error| {
                        button.tooltip(error.clone())
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.load_databases(server.clone(), cx)))
                    .into_any_element()
            }
            NavigationRow::Group(index) => {
                let section = &self.state.sections[index];
                let collapsed = cx
                    .global::<ZedisGlobalStore>()
                    .read(cx)
                    .is_server_group_collapsed(&section.key);
                group_header(
                    section,
                    collapsed,
                    &i18n_servers(cx, "ungrouped_label"),
                    &list_look(cx),
                    window,
                    cx,
                )
            }
            NavigationRow::Connection(section, index) => {
                let entry = self.state.sections[section].servers[index].clone();
                self.navigation_connection(entry, self.state.sections[section].group.clone(), window, cx)
            }
        }
    }

    pub(super) fn toggle_navigation_connection(&mut self, server: SharedString, cx: &mut Context<Self>) {
        if !self.databases.expanded.remove(server.as_ref()) {
            self.databases.expanded.insert(server.to_string());
            if !self.databases.servers.contains_key(server.as_ref()) {
                self.load_databases(server, cx);
            }
        }
        cx.notify();
    }

    fn navigation_connection(
        &self,
        entry: SidebarServerEntry,
        group: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let server = entry.id.clone();
        let expanded = self.databases.expanded.contains(server.as_ref());
        let health = self.state.health.get(server.as_ref()).copied().unwrap_or_default();
        let menu = ServerMenu {
            sidebar: cx.entity().downgrade(),
            id: entry.id.clone(),
            group,
            groups: self
                .state
                .sections
                .iter()
                .filter_map(|section| section.group.clone())
                .collect(),
            ungrouped: i18n_servers(cx, "ungrouped_label"),
            link: match health {
                ConnectionHealth::Connected | ConnectionHealth::Reconnecting => Some(true),
                ConnectionHealth::Offline => Some(false),
                ConnectionHealth::Unknown => None,
            },
        };
        h_flex()
            .id(SharedString::from(format!("sidebar-connection-{}", entry.id)))
            .h_8()
            .items_center()
            .pl_2()
            .child(
                Button::new(disclosure_id(&server))
                    .ghost()
                    .xsmall()
                    .icon(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .accessibility_label(i18n_servers(cx, "databases"))
                    .toggled(expanded)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_navigation_connection(server.clone(), cx))),
            )
            .child(div().flex_1().min_w_0().child(server_row(
                ServerRowInput {
                    is_current: self.state.server_id == entry.id,
                    is_menu_open: self.state.menu_open.as_ref() == Some(&entry.id),
                    entry,
                    health,
                    monogram: SharedString::default(),
                    menu,
                    look: &list_look(cx),
                },
                window,
                cx,
            )))
            .into_any_element()
    }

    fn navigation_database(
        &self,
        server: SharedString,
        db: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = self
            .databases
            .servers
            .get(server.as_ref())
            .and_then(|summary| summary.keys.get(db))
            .copied()
            .flatten();
        let label = count.map_or_else(|| format!("db{db}"), |count| format!("db{db}  ({count})"));
        let target = server.clone();
        let active = cx
            .global::<ZedisGlobalStore>()
            .read(cx)
            .selected_server()
            .is_some_and(|(id, current)| id == server.as_ref() && *current == db);
        let row = Button::new(database_id(&server, db))
            .ghost()
            .small()
            .h_8()
            .w_full()
            .px_1()
            .accessibility_label(label.clone())
            .selected(active)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(CustomIconName::DatabaseTree).text_color(cx.theme().danger))
                    .child(Label::new(label)),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.navigation.read(cx).focus_handle(cx).focus(window, cx);
                let modifiers = event.modifiers();
                let secondary = modifiers.secondary(cx);
                let new_tab = (secondary && modifiers.shift)
                    || (cx.global::<ZedisGlobalStore>().read(cx).sidebar_click_new_tab() != secondary);
                if new_tab {
                    open_database(target.to_string(), db, secondary, modifiers.shift, cx);
                } else {
                    this.open_database_workspace(target.clone(), db, window, cx);
                }
            }))
            .into_any_element();
        self.database_tree_row(server, db, row, window, cx)
    }
}
