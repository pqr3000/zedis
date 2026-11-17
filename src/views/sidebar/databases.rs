// Copyright 2026 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Connection → database navigation. This module owns disclosure and cached
//! metadata only; the existing workspace owns selection, scanning and editing.
//! Keeping database identity explicit here also leaves room for independent
//! database key trees without changing connection or command execution.

use super::ZedisSidebar;
use crate::connection::{ServerDb, server_summary};
use crate::states::{ZedisGlobalStore, ZedisServerState};
use gpui::{App, AppContext, Context, Entity, SharedString, Task};

use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct DatabaseNavigation {
    pub(super) expanded: HashSet<String>,
    selected_server: String,
    /// Last successful observation per server, retained across DB switches.
    /// Once observed, disclosure requires no requests or additional scans.
    pub(super) servers: HashMap<String, DatabaseSummary>,
    pub(super) loading: HashMap<String, Task<()>>,
    pub(super) errors: HashMap<String, SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DatabaseSummary {
    pub(super) keys: Vec<Option<u64>>,
}

impl DatabaseSummary {
    fn read(state: &ZedisServerState) -> Option<Self> {
        // `reset` retains the old database count until connect completes.
        // The cleared version prevents attributing that count to a new server.
        if state.server_id().is_empty() || state.version().is_empty() || state.databases() == 0 {
            return None;
        }
        let mut keys = vec![None; state.databases()];
        if let Some(info) = state.redis_info() {
            for (db, count) in keys.iter_mut().enumerate() {
                // An absent keyspace section may be unavailable under ACLs;
                // do not turn unknown totals into a claim that every DB is empty.
                if !info.keyspace.is_empty() {
                    *count = Some(info.keyspace.get(&format!("db{db}")).map_or(0, |stats| stats.keys));
                }
            }
        }
        if let Some(count) = keys.get_mut(state.db()) {
            *count = state.dbsize().or(*count);
        }
        Some(Self { keys })
    }
}

impl DatabaseNavigation {
    pub(super) fn reveal(&mut self, server: &str) {
        if self.selected_server != server {
            if !server.is_empty() {
                self.expanded.insert(server.to_string());
            }
            self.selected_server = server.to_string();
        }
    }

    pub(super) fn retain<'a>(&mut self, servers: impl Iterator<Item = &'a str>) {
        let ids: HashSet<_> = servers.collect();
        self.expanded.retain(|id| ids.contains(id.as_str()));
        self.servers.retain(|id, _| ids.contains(id.as_str()));
        self.loading.retain(|id, _| ids.contains(id.as_str()));
        self.errors.retain(|id, _| ids.contains(id.as_str()));
    }
}

pub(super) fn disclosure_id(server: &str) -> SharedString {
    format!("sidebar-databases-{server}").into()
}

pub(super) fn database_id(server: &str, db: usize) -> SharedString {
    format!("sidebar-db-{server}-{db}").into()
}

/// Match the connection rows' tab gestures, including the user's preference.
pub(super) fn open_database(server: String, db: usize, secondary: bool, shift: bool, cx: &mut App) {
    let store = cx.global::<ZedisGlobalStore>().clone();
    let force_new = secondary && shift;
    let new_tab = force_new || (store.read(cx).sidebar_click_new_tab() != secondary);
    store.update(cx, |state, cx| {
        if force_new {
            state.open_server_in_new_tab(server, db, cx);
        } else if new_tab {
            state.reveal_or_open_server_tab(server, db, cx);
        } else {
            state.set_selected_server((server, db), cx);
        }
    });
}

impl ZedisSidebar {
    /// Discover an unopened connection without selecting it or scanning keys.
    /// The pooled summary operation already applies server/cluster DB limits.
    pub(super) fn load_databases(&mut self, server: SharedString, cx: &mut Context<Self>) {
        if self.databases.loading.contains_key(server.as_ref()) {
            return;
        }
        self.databases.errors.remove(server.as_ref());
        let loading_id = server.to_string();
        let db = cx.global::<ZedisGlobalStore>().read(cx).open_db_for(&server);
        let request_server = server.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { server_summary(&ServerDb::new(request_server.as_ref(), db)).await })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(summary) => {
                        let mut keys = vec![None; summary.databases];
                        if let Some(count) = keys.get_mut(db) {
                            *count = Some(summary.dbsize);
                        }
                        // A workspace heartbeat may have supplied fresher, more
                        // complete metadata while discovery was in flight.
                        this.databases
                            .servers
                            .entry(server.to_string())
                            .or_insert(DatabaseSummary { keys });
                    }
                    Err(error) => {
                        this.databases
                            .errors
                            .insert(server.to_string(), error.to_string().into());
                    }
                }
                this.databases.loading.remove(server.as_ref());
                cx.notify();
            });
        });
        self.databases.loading.insert(loading_id, task);
        cx.notify();
    }

    /// Project existing workspace metadata into navigation, without owning it.
    pub(crate) fn update_databases(&mut self, state: &Entity<ZedisServerState>, cx: &mut Context<Self>) {
        let state = state.read(cx);
        let Some(summary) = DatabaseSummary::read(state) else {
            return;
        };
        let id = state.server_id().to_string();
        if self.databases.servers.get(&id) != Some(&summary) {
            self.databases.servers.insert(id, summary);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::workspaces::database_menu_id;
    use super::super::{DatabaseWorkspaceSelected, SidebarSection, SidebarServerEntry};
    use super::*;
    use crate::states::{GlobalEvent, KeyType, Route};
    use crate::test_support::init_ui;
    use crate::views::key_tree::{KEYWORD_INPUT_ID, sidebar_key_id};
    use gpui::{AnyWindowHandle, Modifiers, ScrollDelta, TestAppContext, point, px, size};
    use gpui_kit::component::{IndexPath, Root, WindowExt};
    use gpui_kit::test::TestWindowExt;
    use std::cell::RefCell;
    use std::rc::Rc;

    impl ZedisSidebar {
        pub(crate) fn set_databases_for_test(&mut self, server: &str, count: usize, cx: &mut Context<Self>) {
            self.state.sections = vec![SidebarSection {
                key: "__none__".into(),
                group: None,
                servers: vec![SidebarServerEntry {
                    id: server.to_string().into(),
                    name: server.to_string().into(),
                    ..Default::default()
                }],
            }];
            self.databases.expanded.insert(server.into());
            self.databases.servers.insert(
                server.into(),
                DatabaseSummary {
                    keys: vec![Some(2); count],
                },
            );
            cx.notify();
        }
    }

    fn open_tree(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ZedisSidebar>) {
        open_tree_at_height(cx, 640.)
    }

    #[gpui::test]
    fn database_menu_targets_its_row_even_when_another_database_had_focus(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree(cx);
        let opened = Rc::new(RefCell::new(Vec::new()));
        let recorded = opened.clone();
        let _selected = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &DatabaseWorkspaceSelected, cx| {
                recorded
                    .borrow_mut()
                    .push(event.content.read(cx).server_state().read(cx).db());
            })
        });
        // Both DBs start empty. Leave the tree focused on db1.
        for db in 0..2 {
            cx.update_window(handle, |_, window, cx| window.click(database_id("a", db), cx))
                .expect("open sidebar");
            cx.run_until_parked();
        }
        cx.update_window(handle, |_, window, cx| window.click(database_menu_id("a", 0), cx))
            .expect("open sidebar");
        cx.run_until_parked();
        // Use the actual Type submenu while db1 still owns keyboard focus.
        cx.update_window(handle, |_, window, cx| window.within("popup-menu").hover(8usize, cx))
            .expect("open menu");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.within("submenu").click(1usize, cx))
            .expect("type filter menu");
        cx.run_until_parked();
        sidebar.read_with(cx, |sidebar, cx| {
            let filter = |db| {
                sidebar
                    .cached_database_workspace("a", db)
                    .expect("expanded database")
                    .read(cx)
                    .server_state()
                    .read(cx)
                    .type_filter()
            };
            assert_eq!(filter(0), Some(KeyType::String));
            assert_eq!(filter(1), None);
        });
        assert!(opened.borrow().is_empty(), "filtering leaves the current editor alone");
        cx.update_window(handle, |_, window, cx| window.click(database_menu_id("a", 0), cx))
            .expect("open sidebar");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.within("popup-menu").click(0usize, cx))
            .expect("Open action");
        cx.run_until_parked();
        assert_eq!(*opened.borrow(), [0], "an empty DB can open without selecting a key");
    }

    fn open_tree_at_height(cx: &mut TestAppContext, height: f32) -> (AnyWindowHandle, Entity<ZedisSidebar>) {
        init_ui(cx);
        let mut sidebar = None;
        let handle = cx.open_window(size(px(440.), px(height)), |window, cx| {
            let view = cx.new(|cx| ZedisSidebar::new(window, cx));
            sidebar = Some(view.clone());
            Root::new(view, window, cx)
        });
        let sidebar = sidebar.expect("the sidebar was created");
        sidebar.update(cx, |sidebar, cx| {
            sidebar.state.sections = vec![SidebarSection {
                key: "__none__".into(),
                group: None,
                servers: ["a", "b"]
                    .map(|id| SidebarServerEntry {
                        id: id.into(),
                        name: id.into(),
                        ..Default::default()
                    })
                    .to_vec(),
            }];
            for id in ["a", "b"] {
                sidebar.databases.expanded.insert(id.into());
                sidebar.databases.servers.insert(
                    id.into(),
                    DatabaseSummary {
                        keys: vec![Some(10), Some(0)],
                    },
                );
            }
            cx.notify();
        });
        (handle.into(), sidebar)
    }

    #[gpui::test]
    fn collapsing_one_connection_keeps_the_other_database_branch(cx: &mut TestAppContext) {
        let (handle, _) = open_tree(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(database_id("a", 1)).is_some());
            assert!(window.try_find(database_id("b", 1)).is_some());
            assert!(
                window.try_find(database_id("a", 2)).is_none(),
                "only discovered databases"
            );
            window.click(disclosure_id("a"), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(database_id("a", 0)).is_none());
            assert!(window.try_find(database_id("b", 1)).is_some());
            // Native buttons preserve the editor's focus on pointer clicks.
            // This sidebar-only window has no editor to establish initial focus.
            window.focus_next(cx);
            window.render_frame(cx);
            // Reach the disclosure through Tab before activating with Space.
            for _ in 0..8 {
                if window.find(disclosure_id("a")).focused() == Some(true) {
                    break;
                }
                window.press("tab", cx);
            }
            assert_eq!(window.find(disclosure_id("a")).focused(), Some(true));
            window.press("space", cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(database_id("a", 1)).is_some());
        })
        .expect("the window is open");
    }

    #[gpui::test]
    fn database_rows_reuse_the_same_workspace_and_reveal_its_tree(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree(cx);
        let events = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &DatabaseWorkspaceSelected, _| {
                recorded.borrow_mut().push(event.content.clone());
            })
        });
        for _ in 0..2 {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.click(database_id("a", 1), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
        }
        let workspace = sidebar.read_with(cx, |sidebar, _| {
            sidebar.cached_database_workspace("a", 1).expect("db1 is retained")
        });
        let tree = workspace.read_with(cx, |content, _| content.database_tree().expect("db1 tree"));
        tree.update(cx, |tree, cx| tree.set_rows_for_test(&["retained-key"], cx));
        cx.run_until_parked();
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>()
                .clone()
                .update(cx, |state, cx| state.go_to(Route::Protos, cx));
        });
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("sidebar-db-toggle-a-1", cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.find_all(KEYWORD_INPUT_ID).is_empty(),
                "the collapsed tree is absent"
            );
            window.click(database_id("a", 1), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window
                    .within(sidebar_key_id("a", 1, "retained-key"))
                    .find(IndexPath::new(0))
                    .visible()
            );
            assert!(
                window.find_all(KEYWORD_INPUT_ID).is_empty(),
                "the tree has no search bars"
            );
        })
        .expect("the window is open");
        let workspace = sidebar.read_with(cx, |sidebar, _| {
            sidebar.cached_database_workspace("a", 1).expect("db1 is retained")
        });
        workspace.read_with(cx, |content, cx| {
            assert!(content.has_sidebar_tree());
            assert_eq!(content.server_state().read(cx).server_id(), "a");
            assert_eq!(content.server_state().read(cx).db(), 1);
        });
        assert!(events.borrow().is_empty(), "DB navigation does not replace the editor");
    }

    #[gpui::test]
    fn database_tab_gestures_keep_server_and_database_identity(cx: &mut TestAppContext) {
        let (handle, _) = open_tree(cx);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        let _subscription = cx.update(|cx| {
            let state = cx.global::<ZedisGlobalStore>().state();
            cx.subscribe(&state, move |_, event, _| {
                if let GlobalEvent::ServerOpenInNewTab(id, db, duplicate) = event {
                    recorded.borrow_mut().push((id.to_string(), *db, *duplicate));
                }
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let secondary = Modifiers::secondary_key(cx);
            window.click_with_modifiers(database_id("a", 1), secondary, cx);
            window.click_with_modifiers(
                database_id("b", 0),
                Modifiers {
                    shift: true,
                    ..secondary
                },
                cx,
            );
        })
        .expect("the window is open");
        cx.run_until_parked();
        assert_eq!(*requests.borrow(), vec![("a".into(), 1, false), ("b".into(), 0, true)]);
    }

    #[gpui::test]
    fn discovery_failure_keeps_the_workspace_and_offers_retry(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree(cx);
        let unknown = "sidebar-never-configured";
        sidebar.update(cx, |sidebar, cx| {
            sidebar.state.sections[0].servers.push(SidebarServerEntry {
                id: unknown.into(),
                name: unknown.into(),
                ..Default::default()
            });
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(disclosure_id(unknown), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find(database_id(unknown, 0)).is_none(),
                "no guessed DBs on failure"
            );
            assert!(
                window
                    .try_find(SharedString::from(format!("sidebar-db-open-{unknown}")))
                    .is_some()
            );
            assert!(cx.global::<ZedisGlobalStore>().read(cx).selected_server().is_none());
        })
        .expect("the window is open");
        sidebar.read_with(cx, |sidebar, _| {
            assert!(sidebar.databases.errors.contains_key(unknown));
            assert!(!sidebar.databases.loading.contains_key(unknown));
        });
    }

    #[gpui::test]
    fn expanding_a_database_does_not_activate_its_editor(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree(cx);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let recorded = selected.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&sidebar, move |_, event: &DatabaseWorkspaceSelected, _| {
                recorded.borrow_mut().push(event.content.clone());
            })
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("sidebar-db-toggle-a-0", cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        sidebar.read_with(cx, |sidebar, _| {
            sidebar.cached_database_workspace("a", 0).expect("db0 was expanded")
        });
        assert!(
            selected.borrow().is_empty(),
            "expanding is independent of the right editor"
        );
    }

    #[gpui::test]
    fn small_database_trees_leave_room_for_following_databases(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree_at_height(cx, 600.);
        sidebar.update(cx, |sidebar, cx| {
            sidebar.databases.servers.get_mut("a").expect("server a").keys = vec![Some(10); 16];
            cx.notify();
        });
        for db in 0..2 {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.click(SharedString::from(format!("sidebar-db-toggle-a-{db}")), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            sidebar
                .read_with(cx, |sidebar, cx| {
                    sidebar
                        .cached_database_workspace("a", db)
                        .expect("expanded DB")
                        .read(cx)
                        .database_tree()
                        .expect("DB tree")
                })
                .update(cx, |tree, cx| tree.set_rows_for_test(&["first", "second"], cx));
            cx.run_until_parked();
        }
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let first = window
                .within(sidebar_key_id("a", 0, "first"))
                .find(IndexPath::new(0))
                .bounds();
            let second_db = window.find(database_id("a", 1)).bounds();
            assert!(
                second_db.top() - first.top() <= px(64.),
                "two keys must not reserve a full pane: input={first:?}, next DB={second_db:?}"
            );
            let last = window.within(sidebar_key_id("a", 0, "second")).find(IndexPath::new(1));
            assert!(last.visible());
            assert!(
                last.bounds().bottom() <= second_db.top(),
                "keys must not overlap the next DB"
            );
            assert!(
                window.find(database_id("a", 2)).visible(),
                "later databases stay reachable"
            );
            assert!(window.find_all(KEYWORD_INPUT_ID).is_empty());
            assert!(window.find_all("key-tree-more-dropdown").is_empty());
        })
        .expect("the window is open");
    }

    #[gpui::test]
    fn two_database_trees_keep_their_search_and_identity_when_folded(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree_at_height(cx, 1200.);
        let mut original = Vec::new();
        for (db, query) in [(0, "users:"), (1, "orders:")] {
            cx.update_window(handle, |_, window, cx| {
                window.click(SharedString::from(format!("sidebar-db-toggle-a-{db}")), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            let workspace = sidebar.read_with(cx, |sidebar, _| sidebar.cached_database_workspace("a", db).expect("DB"));
            let tree = workspace.read_with(cx, |content, _| content.database_tree().expect("tree"));
            cx.update_window(handle, |_, window, cx| {
                tree.update(cx, |tree, cx| tree.focus_search(window, cx));
            })
            .expect("the window is open");
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.input(query, cx);
                assert_eq!(window.find(KEYWORD_INPUT_ID).value(), Some(query));
                window.close_dialog(cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            original.push(workspace);
        }
        for (db, query) in [(0, "users:"), (1, "orders:")] {
            for _ in 0..2 {
                cx.update_window(handle, |_, window, cx| {
                    window.click(SharedString::from(format!("sidebar-db-toggle-a-{db}")), cx)
                })
                .expect("the window is open");
                cx.run_until_parked();
            }
            let workspace = sidebar.read_with(cx, |sidebar, _| sidebar.cached_database_workspace("a", db).expect("DB"));
            assert_eq!(workspace, original[db]);
            cx.update_window(handle, |_, window, cx| {
                assert!(window.find_all(KEYWORD_INPUT_ID).is_empty());
                let tree = workspace.read(cx).database_tree().expect("tree");
                tree.update(cx, |tree, cx| tree.focus_search(window, cx));
            })
            .expect("the window is open");
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(window.find(KEYWORD_INPUT_ID).value(), Some(query));
                window.close_dialog(cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn one_virtual_scroll_reaches_the_database_after_ten_thousand_keys(cx: &mut TestAppContext) {
        let (handle, sidebar) = open_tree_at_height(cx, 600.);
        cx.update_window(handle, |_, window, cx| window.click("sidebar-db-toggle-a-0", cx))
            .expect("the window is open");
        cx.run_until_parked();
        let tree = sidebar.read_with(cx, |sidebar, cx| {
            sidebar
                .cached_database_workspace("a", 0)
                .expect("DB")
                .read(cx)
                .database_tree()
                .expect("tree")
        });
        let names: Vec<_> = (0..10_000).map(|index| format!("key-{index}")).collect();
        let keys: Vec<_> = names.iter().map(String::as_str).collect();
        tree.update(cx, |tree, cx| tree.set_rows_for_test(&keys, cx));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find_all(KEYWORD_INPUT_ID).is_empty());
            window.within(sidebar_key_id("a", 0, "key-0")).scroll(
                IndexPath::new(0),
                ScrollDelta::Pixels(point(px(0.), px(-1_000_000.))),
                cx,
            );
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let last = window
                .within(sidebar_key_id("a", 0, "key-9999"))
                .find(IndexPath::new(9_999));
            let next = window.find(database_id("a", 1));
            assert!(last.visible());
            assert!(next.visible(), "one scrollbar reaches the following DB");
            assert!(last.bounds().bottom() <= next.bounds().top());
            assert!(next.bounds().bottom() <= window.viewport_size().height);
        })
        .expect("the window is open");
    }
}
