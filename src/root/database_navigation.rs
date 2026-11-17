//! Attach a retained database workspace to a tab. The sidebar owns its
//! lifetime; a tab only presents its editor and tools while it is active.

use super::*;

impl Zedis {
    fn attach_database_workspace(&mut self, content: Entity<ZedisContent>, cx: &mut Context<Self>) {
        if self.active_content() == content {
            return;
        }
        // Never display the same workspace entity in two tabs at once.
        if let Some(index) = self.tabs.iter().position(|tab| tab.content == content) {
            self.activate_tab(index, None, cx);
            return;
        }
        let state = content.read(cx).server_state();
        let (server, db) = (state.read(cx).server_id().to_string(), state.read(cx).db());
        self.active_content().update(cx, |old, cx| old.set_active(false, cx));
        content.update(cx, |content, cx| content.set_active(true, cx));
        self.tabs[self.active_tab] = ContentTab::new(server, db, content, cx);
        self.rebind_palettes(cx);
        self.persist_tabs(cx);
        cx.notify();
    }

    pub(super) fn activate_database_workspace(
        &mut self,
        content: Entity<ZedisContent>,
        view: ServerView,
        cx: &mut Context<Self>,
    ) {
        if self.active_content() == content
            && cx.global::<ZedisGlobalStore>().read(cx).route().server_view() == Some(view)
        {
            return;
        }
        let state = content.read(cx).server_state();
        let (server, db) = (state.read(cx).server_id().to_string(), state.read(cx).db());
        self.attach_database_workspace(content, cx);
        // Key selection in a tree is an editor action, including from a tool page.
        cx.global::<ZedisGlobalStore>().clone().update(cx, |state, cx| {
            state.connect_server(server, db, cx);
            state.go_to_view(view, cx);
        });
    }

    pub(super) fn follow_database_selection(&mut self, server: &str, db: usize, cx: &mut Context<Self>) {
        if server.is_empty() {
            self.pending_database = None;
            return;
        }
        if !self.active_content().read(cx).has_sidebar_tree() {
            return;
        }
        let cached = self.sidebar.read(cx).cached_database_workspace(server, db);
        if let Some(content) = cached {
            let changed = self.active_content() != content;
            self.pending_database = None;
            self.attach_database_workspace(content, cx);
            if changed {
                self.project_active_tab(cx);
            }
        } else {
            // A deep link can select a DB that has no workspace yet. Create
            // it on the next frame while retaining the outgoing tree.
            self.pending_database = Some((server.to_string(), db));
        }
    }

    pub(super) fn prepare_database_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((server, db)) = self.pending_database.take() else {
            self.adopt_active_database_workspace(window, cx);
            return;
        };
        let content = self.sidebar.update(cx, |sidebar, cx| {
            let content = sidebar.ensure_database_workspace(server.clone().into(), db, window, cx);
            sidebar.reveal_database_workspace(&server, db, cx);
            content
        });
        self.attach_database_workspace(content, cx);
        // Replays a pending cross-DB key handoff after the new content exists.
        self.project_active_tab(cx);
    }

    fn adopt_active_database_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.active_content();
        if content.read(cx).has_sidebar_tree()
            || cx.global::<ZedisGlobalStore>().read(cx).route().server_view().is_none()
        {
            return;
        }
        let state = content.read(cx).server_state();
        let (server, db) = (state.read(cx).server_id().to_string(), state.read(cx).db());
        // Explicit duplicate tabs keep independent editors. Ordinary startup,
        // restored tabs and connection clicks all use the sidebar layout.
        if server.is_empty()
            || self
                .tabs
                .iter()
                .enumerate()
                .any(|(index, tab)| index != self.active_tab && tab.server_id == server && tab.db == db)
        {
            return;
        }
        let cached = self.sidebar.read(cx).cached_database_workspace(&server, db);
        if let Some(cached) = cached {
            self.attach_database_workspace(cached, cx);
        } else {
            // Retain the already connected state and any selected value; do
            // not start another connection/scan just to change the layout.
            self.sidebar.update(cx, |sidebar, cx| {
                sidebar.adopt_database_workspace(content, window, cx);
            });
        }
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.reveal_database_workspace(&server, db, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::{KeyType, QueryMode, RedisBytesValue, RedisValue, RedisValueData, Route, ServerEvent};
    use crate::test_support::init_ui;
    use crate::views::sidebar_key_id;
    use gpui::{AnyWindowHandle, Role, TestAppContext, px, size};
    use gpui_kit::component::{IndexPath, Root, WindowExt};
    use gpui_kit::test::{TestQueryExt, TestWindowExt};
    use std::sync::Arc;

    fn open_root(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Zedis>) {
        init_ui(cx);
        let mut root = None;
        let window = cx.open_window(size(px(1200.), px(900.)), |window, cx| {
            let view = cx.new(|cx| Zedis::new(window, cx));
            root = Some(view.clone());
            Root::new(view, window, cx)
        });
        (window.into(), root.expect("the root was built"))
    }

    fn show_value(content: Entity<ZedisContent>, value: &'static str, cx: &mut TestAppContext) {
        let state = content.read_with(cx, |content, _| content.server_state());
        state.update(cx, |state, cx| {
            state.show_value_for_test(RedisValue {
                key_type: KeyType::String,
                data: Some(RedisValueData::Bytes(Arc::new(RedisBytesValue {
                    bytes: value.as_bytes().into(),
                    text: Some(value.into()),
                    ..Default::default()
                }))),
                ..Default::default()
            });
            cx.emit(ServerEvent::ValueLoaded);
            cx.notify();
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn an_unopened_database_routes_its_row_menu_to_its_own_workspace(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        cx.run_until_parked();
        let sidebar = root.read_with(cx, |root, _| root.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.set_databases_for_test("tree-test", 2, cx));
        for db in 0..2 {
            cx.update_window(window, |_, window, cx| {
                window.click(SharedString::from(format!("sidebar-db-toggle-tree-test-{db}")), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            let tree = sidebar.read_with(cx, |sidebar, cx| {
                sidebar
                    .cached_database_workspace("tree-test", db)
                    .expect("DB workspace")
                    .read(cx)
                    .database_tree()
                    .expect("DB tree")
            });
            tree.update(cx, |tree, cx| tree.set_rows_for_test(&["menu-key"], cx));
        }
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window
                .within(sidebar_key_id("tree-test", 0, "menu-key"))
                .click(IndexPath::new(0), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        let first = root.read_with(cx, |root, _| root.active_content());
        for item in [6usize, 7] {
            cx.update_window(window, |_, window, cx| {
                window
                    .within(sidebar_key_id("tree-test", 1, "menu-key"))
                    .right_click(IndexPath::new(0), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            assert_eq!(
                root.read_with(cx, |root, _| root.active_content()),
                first,
                "opening the menu keeps the editor"
            );
            cx.update_window(window, |_, window, cx| {
                // RowMenu's Copy Link and Rename entries, exercised through
                // the real popup and its captured focus target.
                window.within("popup-menu").click(item, cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            if item == 6 {
                assert!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .is_some_and(|text| text.contains("db=1"))
                );
                assert_eq!(root.read_with(cx, |root, _| root.active_content()), first);
            }
        }
        root.read_with(cx, |root, cx| {
            let state = root.active_content().read(cx).server_state();
            assert_eq!(state.read(cx).db(), 1);
            assert_eq!(state.read(cx).key().as_deref(), Some("menu-key"));
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.has_active_dialog(cx));
            assert!(
                window
                    .find_all_by_role(Role::TextInput)
                    .iter()
                    .any(|field| field.value() == Some("menu-key") && field.focused() == Some(true))
            );
            window.close_all_dialogs(cx);
        })
        .expect("the window is open");
    }

    #[gpui::test]
    fn an_empty_database_still_handles_the_editors_add_key_action(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        cx.run_until_parked();
        let sidebar = root.read_with(cx, |root, _| root.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.set_databases_for_test("tree-test", 1, cx));
        cx.update_window(window, |_, window, cx| {
            window.click("sidebar-db-toggle-tree-test-0", cx)
        })
        .expect("the window is open");
        cx.run_until_parked();
        let content = sidebar.read_with(cx, |sidebar, _| {
            sidebar.cached_database_workspace("tree-test", 0).expect("DB workspace")
        });
        root.update(cx, |root, cx| {
            root.activate_database_workspace(content.clone(), ServerView::Editor, cx)
        });
        cx.run_until_parked();
        let state = content.read_with(cx, |content, _| content.server_state());
        state.update(cx, |state, cx| state.emit_editor_action(EditorAction::Create, cx));
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.has_active_dialog(cx),
                "an empty virtual segment still processes editor actions"
            );
            assert!(!window.find_all_by_role(Role::TextInput).is_empty());
            window.close_all_dialogs(cx);
        })
        .expect("the window is open");
    }

    #[gpui::test]
    fn normal_connection_moves_the_existing_database_and_value_to_the_sidebar_layout(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        let original = root.read_with(cx, |root, _| root.active_content());
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>().clone().update(cx, |store, cx| {
                store.set_multi_search_pending_key("tree-test".into(), 0, "shared-name".into());
                store.connect_server("tree-test".into(), 0, cx);
            });
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .expect("the window is open");
        cx.run_until_parked();
        root.read_with(cx, |root, cx| {
            assert_eq!(
                root.active_content(),
                original,
                "startup retains the connected workspace"
            );
            assert!(original.read(cx).has_sidebar_tree());
            assert_eq!(
                original.read(cx).server_state().read(cx).key().as_deref(),
                Some("shared-name")
            );
            assert_eq!(
                root.sidebar.read(cx).cached_database_workspace("tree-test", 0),
                Some(original)
            );
        });
    }

    #[gpui::test]
    fn expanding_other_databases_keeps_the_open_key_and_value(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        cx.run_until_parked();
        let sidebar = root.read_with(cx, |root, _| root.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.set_databases_for_test("tree-test", 5, cx));
        cx.update_window(window, |_, window, cx| {
            window.click("sidebar-db-toggle-tree-test-0", cx)
        })
        .expect("the window is open");
        cx.run_until_parked();
        let content = sidebar.read_with(cx, |sidebar, _| {
            sidebar.cached_database_workspace("tree-test", 0).expect("db0 exists")
        });
        let tree = content.read_with(cx, |content, _| content.database_tree().expect("db0 tree"));
        tree.update(cx, |tree, cx| tree.set_rows_for_test(&["456"], cx));
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window
                .within(sidebar_key_id("tree-test", 0, "456"))
                .click(IndexPath::new(0), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        show_value(content.clone(), "456456", cx);
        for db in 1..5 {
            cx.update_window(window, |_, window, cx| {
                window.click(SharedString::from(format!("sidebar-db-toggle-tree-test-{db}")), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            root.read_with(cx, |root, cx| {
                assert_eq!(
                    root.active_content(),
                    content,
                    "expanding db{db} must not replace db0's editor"
                );
                assert_eq!(
                    cx.global::<ZedisGlobalStore>().read(cx).selected_server(),
                    Some(&("tree-test".to_string(), 0))
                );
            });
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                assert!(
                    window
                        .elements()
                        .iter()
                        .any(|element| element.visible() && element.value() == Some("456456"))
                );
            })
            .expect("the window is open");
        }
        let branch = sidebar.read_with(cx, |sidebar, cx| {
            sidebar
                .cached_database_workspace("tree-test", 4)
                .expect("db4")
                .read(cx)
                .database_tree()
                .expect("db4 tree")
        });
        branch.update(cx, |tree, cx| tree.set_folder_for_test(cx));
        cx.run_until_parked();
        let branch_id = branch.read_with(cx, |tree, cx| tree.sidebar_row_id("tree-test", 4, 0, cx));
        cx.update_window(window, |_, window, cx| {
            window.within(branch_id).click(IndexPath::new(0), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        assert_eq!(
            root.read_with(cx, |root, _| root.active_content()),
            content,
            "expanding a folder keeps the value"
        );
        for control in ["sidebar-db-toggle-tree-test-0", "sidebar-databases-tree-test"] {
            cx.update_window(window, |_, window, cx| window.click(control, cx))
                .expect("the window is open");
            cx.run_until_parked();
            assert_eq!(
                root.read_with(cx, |root, _| root.active_content()),
                content,
                "folding keeps the editor"
            );
        }
    }

    #[gpui::test]
    fn clicking_keys_in_two_expanded_databases_opens_the_right_editor(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        cx.run_until_parked();
        let sidebar = root.read_with(cx, |root, _| root.sidebar.clone());
        sidebar.update(cx, |sidebar, cx| sidebar.set_databases_for_test("tree-test", 2, cx));
        for db in 0..2 {
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.click(SharedString::from(format!("sidebar-db-toggle-tree-test-{db}")), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            let tree = sidebar.read_with(cx, |sidebar, cx| {
                sidebar
                    .cached_database_workspace("tree-test", db)
                    .expect("DB workspace")
                    .read(cx)
                    .database_tree()
                    .expect("DB tree")
            });
            tree.update(cx, |tree, cx| {
                tree.set_rows_for_test(&["shared-name", "second-key"], cx)
            });
            cx.run_until_parked();
        }
        for db in [0, 1, 0] {
            // A tree also has to bring the editor back from a tool page.
            cx.update(|cx| {
                cx.global::<ZedisGlobalStore>()
                    .clone()
                    .update(cx, |store, cx| store.go_to(Route::Protos, cx));
            });
            cx.run_until_parked();
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window
                    .within(sidebar_key_id("tree-test", db, "shared-name"))
                    .click(IndexPath::new(0), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
            root.read_with(cx, |root, cx| {
                let content = root.active_content();
                let expected = sidebar
                    .read(cx)
                    .cached_database_workspace("tree-test", db)
                    .expect("DB workspace");
                assert_eq!(content, expected);
                let state = content.read(cx).server_state();
                assert_eq!(state.read(cx).db(), db);
                assert_eq!(state.read(cx).key().as_deref(), Some("shared-name"));
            });
            let content = root.read_with(cx, |root, _| root.active_content());
            let payload = if db == 0 { "value from db0" } else { "value from db1" };
            show_value(content, payload, cx);
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                assert!(
                    window.try_find("empty-new-key").is_none(),
                    "selection replaces the empty editor"
                );
                let editor = window.find("zedis-editor-copy-key");
                assert!(editor.visible(), "the selected key toolbar is visible");
                assert!(editor.bounds().left() >= px(400.), "key data belongs in the right pane");
                let value = window
                    .elements()
                    .into_iter()
                    .find(|element| element.value() == Some(payload));
                let value = value.expect("the right editor renders this DB's value");
                assert!(value.visible());
                assert!(value.bounds().left() >= px(400.));
                assert!(value.bounds().right() <= window.viewport_size().width);
            })
            .expect("the window is open");
        }
    }

    #[gpui::test]
    fn changing_database_keeps_the_outgoing_tree_state_and_binds_the_incoming_editor(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        let (first, second) = cx
            .update_window(window, |_, window, cx| {
                let sidebar = root.read(cx).sidebar.clone();
                let first = sidebar.update(cx, |sidebar, cx| {
                    sidebar.ensure_database_workspace("tree-test".into(), 0, window, cx)
                });
                let second = sidebar.update(cx, |sidebar, cx| {
                    sidebar.ensure_database_workspace("tree-test".into(), 1, window, cx)
                });
                root.update(cx, |root, cx| {
                    root.activate_database_workspace(first.clone(), ServerView::Editor, cx)
                });
                (first, second)
            })
            .expect("the window is open");
        cx.run_until_parked();
        let first_state = first.read_with(cx, |content, _| content.server_state());
        first_state.update(cx, |state, cx| state.set_query_mode(QueryMode::Prefix, cx));
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>().clone().update(cx, |state, cx| {
                state.set_multi_search_pending_key("tree-test".into(), 1, "shared-name".into());
                state.set_selected_server(("tree-test".into(), 1), cx);
            });
        });
        cx.run_until_parked();
        root.read_with(cx, |root, cx| {
            assert_eq!(root.active_content(), second);
            assert_eq!(first_state.read(cx).db(), 0, "the previous scan still belongs to db0");
            assert_eq!(first_state.read(cx).query_mode(), QueryMode::Prefix);
            assert_eq!(root.active_content().read(cx).server_state().read(cx).db(), 1);
            assert_eq!(
                second.read(cx).server_state().read(cx).key().as_deref(),
                Some("shared-name")
            );
            assert!(
                first_state.read(cx).key().is_none(),
                "the key handoff belongs to db1 only"
            );
            assert_ne!(first.read(cx).database_tree(), second.read(cx).database_tree());
        });
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>()
                .clone()
                .update(cx, |state, cx| state.set_selected_server(("tree-test".into(), 0), cx));
        });
        cx.run_until_parked();
        root.read_with(cx, |root, cx| {
            assert_eq!(root.active_content(), first);
            assert_eq!(
                first_state.read(cx).query_mode(),
                QueryMode::Prefix,
                "returning does not select/reset the session"
            );
        });
    }

    #[gpui::test]
    fn a_forced_duplicate_tab_keeps_its_own_workspace(cx: &mut TestAppContext) {
        let (window, root) = open_root(cx);
        let original = cx
            .update_window(window, |_, window, cx| {
                let sidebar = root.read(cx).sidebar.clone();
                let content = sidebar.update(cx, |sidebar, cx| {
                    sidebar.ensure_database_workspace("tree-test".into(), 0, window, cx)
                });
                root.update(cx, |root, cx| {
                    root.activate_database_workspace(content.clone(), ServerView::Editor, cx)
                });
                content
            })
            .expect("the window is open");
        cx.run_until_parked();
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>()
                .clone()
                .update(cx, |state, cx| state.open_server_in_new_tab("tree-test".into(), 0, cx));
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .expect("the window is open");
        cx.run_until_parked();
        root.read_with(cx, |root, cx| {
            assert_eq!(root.tabs.len(), 2);
            assert_ne!(root.active_content(), original);
            assert!(!root.active_content().read(cx).has_sidebar_tree());
            assert!(original.read(cx).server_state().read(cx).is_background());
        });
    }
}
