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

//! The Tools menu: the server's panels in four groups by purpose, each
//! entry kept, disabled with its reason, or left out by what the server and
//! this connection can do.

use super::{ZedisSidebar, header::SidebarToolRequested};
use crate::connection::{CommandStatus, ServerCommand};
use crate::helpers::saves_files;
use crate::states::{
    ServerToolsAction, ServerView, ZedisGlobalStore, i18n_acl, i18n_benchmark, i18n_common, i18n_config_editor,
    i18n_features, i18n_functions, i18n_hotkeys, i18n_key_tree, i18n_keyspace_notifications, i18n_lua_scripts,
    i18n_monitor, i18n_persistence, i18n_search, i18n_server_info, i18n_server_load, i18n_status_bar, i18n_timeseries,
    i18n_topology, i18n_trash, i18n_troubleshoot, i18n_value_search, i18n_workbench,
};
use crate::{assets::CustomIconName, views::ZedisContent};
use gpui::{Context, Entity, SharedString, WeakEntity, Window};
use gpui_kit::component::{
    Icon, IconName,
    menu::{PopupMenu, PopupMenuItem},
};
use rust_i18n::t;

/// One Tools-menu entry, built when the menu opens and moved into the
/// submenu that draws it.
struct ToolEntry {
    label: SharedString,
    icon: Icon,
    action: ServerToolsAction,
    disabled: bool,
}

/// Server tools for the workspace targeted by the sidebar. Read capability
/// and access gates when opening the menu, including for independent duplicate tabs.
pub(super) fn build(
    this: PopupMenu,
    content: Entity<ZedisContent>,
    sidebar: WeakEntity<ZedisSidebar>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let state = content.read(cx).server_state();
    let state = state.read(cx);
    let supports_search = state.supports_search();
    let supports_acl = state.supports_acl();
    let supports_functions = state.supports_functions();
    let supports_topology = state.supports_topology();
    let readonly = state.readonly();
    let features = state.features();
    let locale = cx.global::<ZedisGlobalStore>().read(cx).locale().to_string();
    let tool = |label: SharedString,
                icon: Icon,
                action: ServerToolsAction,
                required: &[ServerCommand],
                writes: bool|
     -> Option<ToolEntry> {
        let statuses: Vec<(ServerCommand, CommandStatus)> = required
            .iter()
            .map(|command| (*command, features.status(*command)))
            .filter(|(_, status)| !status.is_usable())
            .collect();
        if statuses.iter().any(|(_, status)| *status == CommandStatus::Missing) {
            return None;
        }
        // Read-only wins the wording: it is the reason the user can undo.
        let reason = if writes && readonly {
            Some(i18n_common(cx, "disable_in_readonly"))
        } else {
            statuses.first().map(|(command, _)| {
                t!("features.menu_suffix", command = command.label(), locale = &locale)
                    .to_string()
                    .into()
            })
        };
        Some(ToolEntry {
            label: match &reason {
                Some(reason) => format!("{label}  ·  {reason}").into(),
                None => label,
            },
            icon,
            action,
            disabled: reason.is_some(),
        })
    };

    // A panel this build does not have is not listed
    // (`ServerView::in_this_build`): the browser has no `MONITOR`, no
    // keyspace events, no Pub/Sub and no Topology, and an entry that
    // opened a page saying so was an entry to nothing.
    let observability: Vec<ToolEntry> = [
        ServerView::Monitor
            .in_this_build()
            .then(|| {
                tool(
                    i18n_monitor(cx, "title"),
                    Icon::new(CustomIconName::Radar),
                    ServerToolsAction::Monitor,
                    ServerView::Monitor.required_commands(),
                    false,
                )
            })
            .flatten(),
        tool(
            i18n_server_load(cx, "title"),
            Icon::new(CustomIconName::Zap),
            ServerToolsAction::ServerLoad,
            ServerView::ServerLoad.required_commands(),
            false,
        ),
        // HOTKEYS tracking — Redis 8.6; absent from the menu elsewhere.
        tool(
            i18n_hotkeys(cx, "title"),
            Icon::new(CustomIconName::Flame),
            ServerToolsAction::Hotkeys,
            ServerView::Hotkeys.required_commands(),
            false,
        ),
        // `notify-keyspace-events` may be empty: the panel offers a
        // one-click Enable. Its hard dependency is SUBSCRIBE.
        ServerView::KeyspaceNotifications
            .in_this_build()
            .then(|| {
                tool(
                    i18n_keyspace_notifications(cx, "title"),
                    Icon::new(CustomIconName::AudioWaveform),
                    ServerToolsAction::KeyspaceNotifications,
                    ServerView::KeyspaceNotifications.required_commands(),
                    false,
                )
            })
            .flatten(),
        // RedisTimeSeries only.
        tool(
            i18n_timeseries(cx, "explorer_title"),
            Icon::new(CustomIconName::Activity),
            ServerToolsAction::TimeSeriesExplorer,
            ServerView::TimeSeriesExplorer.required_commands(),
            false,
        ),
        // Pub/Sub is the editor suite's channel mode, mirrored here next
        // to its observability siblings.
        tool(
            i18n_key_tree(cx, "pubsub_mode"),
            Icon::new(CustomIconName::Rss),
            ServerToolsAction::PubsubMode,
            &[],
            false,
        ),
        tool(
            i18n_server_info(cx, "title"),
            Icon::new(IconName::Info),
            ServerToolsAction::ServerInfo,
            ServerView::ServerInfo.required_commands(),
            false,
        ),
        tool(
            i18n_benchmark(cx, "title"),
            Icon::new(CustomIconName::Zap),
            ServerToolsAction::Benchmark,
            ServerView::Benchmark.required_commands(),
            false,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    let scripting: Vec<ToolEntry> = [
        tool(
            i18n_value_search(cx, "title"),
            Icon::new(IconName::Search),
            ServerToolsAction::ValueSearch,
            ServerView::ValueSearch.required_commands(),
            false,
        ),
        // RediSearch / valkey-search, when the module is loaded.
        supports_search
            .then(|| {
                tool(
                    i18n_search(cx, "title"),
                    Icon::new(IconName::Search),
                    ServerToolsAction::Search,
                    ServerView::Search.required_commands(),
                    false,
                )
            })
            .flatten(),
        // `FUNCTION`, Redis 7.0+.
        supports_functions
            .then(|| {
                tool(
                    i18n_functions(cx, "title"),
                    Icon::new(IconName::Asterisk),
                    ServerToolsAction::Functions,
                    ServerView::Functions.required_commands(),
                    false,
                )
            })
            .flatten(),
        // The saved scripts are local, so the library opens everywhere;
        // running one needs EVAL and degrades inside.
        tool(
            i18n_lua_scripts(cx, "title"),
            Icon::new(IconName::SquareTerminal),
            ServerToolsAction::LuaScripts,
            &[],
            false,
        ),
        tool(
            i18n_workbench(cx, "title"),
            Icon::new(IconName::SquareTerminal),
            ServerToolsAction::Workbench,
            &[],
            false,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mut data: Vec<ToolEntry> = Vec::new();
    // The recycle bin — the local database's, or the bridge's.
    data.extend(tool(
        i18n_trash(cx, "menu"),
        Icon::new(CustomIconName::FileXCorner),
        ServerToolsAction::Trash,
        &[],
        false,
    ));
    // Import: the migration window on the desktop, a JSON/CSV dialog in
    // a page. Disabled, not hidden, on a read-only connection so the user
    // knows where it lives.
    data.extend(tool(
        i18n_status_bar(cx, "import_keys_menu"),
        Icon::new(CustomIconName::Upload),
        ServerToolsAction::ImportKeys,
        &[],
        true,
    ));
    // Export: the migration window on the desktop, a download in a page.
    if saves_files() {
        data.extend(tool(
            i18n_status_bar(cx, "export_keys_menu"),
            Icon::new(CustomIconName::Download),
            ServerToolsAction::ExportKeys,
            &[],
            false,
        ));
    }
    // A window on the desktop, a dialog in the browser.
    data.extend(tool(
        i18n_status_bar(cx, "compare_keys_menu"),
        Icon::new(CustomIconName::GitCompareArrows),
        ServerToolsAction::CompareKeys,
        &[],
        false,
    ));

    let mut admin: Vec<ToolEntry> = [
        // CONFIG GET is the one managed clouds most often take away.
        tool(
            i18n_config_editor(cx, "title"),
            Icon::new(IconName::Settings),
            ServerToolsAction::Config,
            ServerView::Config.required_commands(),
            false,
        ),
        // ACL, Redis 6.0+.
        supports_acl
            .then(|| {
                tool(
                    i18n_acl(cx, "title"),
                    Icon::new(IconName::CircleUser),
                    ServerToolsAction::Acl,
                    ServerView::Acl.required_commands(),
                    false,
                )
            })
            .flatten(),
        tool(
            i18n_persistence(cx, "title"),
            Icon::new(CustomIconName::HardDrive),
            ServerToolsAction::Persistence,
            ServerView::Persistence.required_commands(),
            false,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    // FLUSHDB / FLUSHALL go through the same destructive-command confirm
    // a typed one hits in the terminal: the menu adds an entry point, not
    // a second policy. Kept apart from the panels below a divider.
    let flush: Vec<ToolEntry> = [
        tool(
            i18n_status_bar(cx, "flush_db_menu"),
            Icon::new(CustomIconName::Eraser),
            ServerToolsAction::FlushDb,
            &[ServerCommand::FlushDb],
            true,
        ),
        tool(
            i18n_status_bar(cx, "flush_all_menu"),
            Icon::new(CustomIconName::DatabaseZap),
            ServerToolsAction::FlushAll,
            &[ServerCommand::FlushAll],
            true,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    let flush_after = (!admin.is_empty() && !flush.is_empty()).then_some(admin.len());
    admin.extend(flush);

    let groups = [
        (
            Icon::new(IconName::ChartPie),
            i18n_status_bar(cx, "group_observability"),
            observability,
            None,
        ),
        (
            Icon::new(CustomIconName::Braces),
            i18n_status_bar(cx, "group_scripting"),
            scripting,
            None,
        ),
        (
            Icon::new(CustomIconName::Database),
            i18n_status_bar(cx, "group_data"),
            data,
            None,
        ),
        (
            Icon::new(IconName::Settings),
            i18n_status_bar(cx, "group_admin"),
            admin,
            flush_after,
        ),
    ];
    let topology_label = i18n_topology(cx, "title");
    // The probed command matrix: a count suffix here rather than a badge
    // in the bar.
    let unusable = features.unusable().len();
    let capabilities_label: SharedString = if unusable == 0 {
        format!("{}…", i18n_features(cx, "dialog_title")).into()
    } else {
        format!(
            "{}…  ·  {}",
            i18n_features(cx, "dialog_title"),
            t!("status_bar.capabilities_limited", count = unusable, locale = &locale)
        )
        .into()
    };

    let item = move |label: SharedString, icon: Icon, action: ServerToolsAction, disabled: bool| {
        let sidebar = sidebar.clone();
        let content = content.clone();
        PopupMenuItem::new(label)
            .icon(icon)
            .action(Box::new(action))
            .disabled(disabled)
            .on_click(move |_, _, cx| {
                let _ = sidebar.update(cx, |_, cx| {
                    cx.emit(SidebarToolRequested {
                        content: content.clone(),
                        action,
                    })
                });
            })
    };
    let mut menu = this;
    for (icon, label, entries, separator_at) in groups {
        if entries.is_empty() {
            continue;
        }
        let item = item.clone();
        menu = menu.submenu_with_icon(Some(icon), label, window, cx, move |mut submenu, _, _| {
            for (index, entry) in entries.iter().enumerate() {
                if separator_at == Some(index) {
                    submenu = submenu.separator();
                }
                submenu = submenu.item(item(
                    entry.label.clone(),
                    entry.icon.clone(),
                    entry.action,
                    entry.disabled,
                ));
            }
            submenu
        });
    }
    // The page adapts to Cluster / Sentinel / Standalone; absent only
    // while the server type is unknown.
    if supports_topology && ServerView::Topology.in_this_build() {
        menu = menu.item(item(
            topology_label,
            Icon::new(CustomIconName::Network),
            ServerToolsAction::Topology,
            false,
        ));
    }
    menu.item(item(
        capabilities_label,
        Icon::new(CustomIconName::ListCheck),
        ServerToolsAction::Capabilities,
        false,
    ))
    // Everything the server says about itself, read once, to paste into
    // a ticket — beside the matrix, the other thing asked of a server
    // that misbehaves.
    .item(item(
        i18n_troubleshoot(cx, "menu"),
        // Two `IconName`s are in scope here: the component set's has no
        // FileText.
        Icon::new(gpui_kit::assets::IconName::FileText),
        ServerToolsAction::TroubleshootingReport,
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::ZedisServerState;
    use crate::test_support::init_ui;
    use crate::views::sidebar::header::SIDEBAR_MENU_ID;
    use crate::views::{ZedisContent, ZedisSidebar};
    use gpui::{AnyWindowHandle, ElementId, TestAppContext, prelude::*, px, size};
    use gpui_kit::component::Root;
    use gpui_kit::test::{TestQueryExt, TestWindowExt};

    fn open_sidebar(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ZedisServerState>) {
        let mut state = None;
        let handle = cx.open_window(size(px(1200.), px(720.)), |window, cx| {
            let content = cx.new(|cx| ZedisContent::new(window, cx));
            let sidebar = cx.new(|cx| ZedisSidebar::new(window, cx));
            state = Some(content.read(cx).server_state());
            sidebar.update(cx, |sidebar, cx| sidebar.bind_content(content, cx));
            Root::new(sidebar, window, cx)
        });
        let state = state.expect("workspace state");
        cx.update(|cx| {
            cx.global::<ZedisGlobalStore>()
                .clone()
                .update(cx, |s, cx| s.connect_server("scratch".into(), 0, cx))
        });
        state.update(cx, |s, cx| s.select("scratch".into(), 0, cx));
        cx.run_until_parked();
        (handle.into(), state)
    }
    /// What the open Tools menu lists, top to bottom: its own entries, and
    /// those of the group whose submenu is open.
    fn menu_labels(window: &Window) -> (Vec<String>, Vec<String>) {
        let menu = ElementId::Name("popup-menu".into());
        let submenu = ElementId::Name("submenu".into());
        let mut entries: Vec<(bool, f32, String)> = window
            .elements()
            .into_iter()
            .filter(|element| element.path().contains(&menu))
            .filter_map(|element| {
                let label = element.label()?.to_string();
                Some((
                    element.path().contains(&submenu),
                    f32::from(element.bounds().origin.y),
                    label,
                ))
            })
            .collect();
        entries.sort_by(|a, b| a.1.total_cmp(&b.1));
        let pick = |inner: bool| entries.iter().filter(|e| e.0 == inner).map(|e| e.2.clone()).collect();
        (pick(false), pick(true))
    }

    /// Opens the Tools menu, walks its groups with the arrow key and closes
    /// it again: the menu's own entries, and each group's in turn.
    fn walk_tools_menu(cx: &mut TestAppContext, handle: AnyWindowHandle) -> (Vec<String>, Vec<Vec<String>>) {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(SIDEBAR_MENU_ID, cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.press("down", cx))
            .expect("open menu");
        cx.run_until_parked();
        let mut top = Vec::new();
        let mut groups = Vec::new();
        for _ in 0..4 {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let (entries, group) = menu_labels(window);
                top = entries;
                groups.push(group);
                window.press("down", cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
        }
        cx.update_window(handle, |_, window, cx| window.press("escape", cx))
            .expect("the window is open");
        cx.run_until_parked();
        (top, groups)
    }

    /// The Tools menu: four groups by purpose and the two reports beside
    /// them, each group's panels in its order — and on a read-only
    /// connection the writes stay listed, with the reason they are off.
    #[gpui::test]
    fn the_tools_menu_groups_the_panels_and_says_why_a_write_is_off(cx: &mut TestAppContext) {
        init_ui(cx);
        let (handle, server_state) = open_sidebar(cx);
        let (top, groups) = walk_tools_menu(cx, handle);
        assert_eq!(
            top,
            [
                "Observability",
                "Query & scripting",
                "Keys & data",
                "Administration",
                "Server capabilities…",
                "Troubleshooting report…",
                "Preferences"
            ]
        );
        assert_eq!(
            groups,
            [
                vec![
                    "Live monitor",
                    "Command stats",
                    "Hot keys",
                    "Keyspace notifications",
                    "Time series explorer",
                    "Pub/Sub mode",
                    "Server info (INFO)",
                    "Benchmark"
                ],
                vec!["Value search", "Lua scripts", "Query workbench"],
                vec![
                    "Recycle bin…",
                    "Import keys…",
                    "Export loaded keys…",
                    "Compare with another server…"
                ],
                vec![
                    "Config",
                    "Persistence",
                    "Flush current database…",
                    "Flush all databases…"
                ],
            ]
        );

        server_state.update(cx, |state, cx| state.toggle_readonly(cx));
        cx.run_until_parked();
        let (_, groups) = walk_tools_menu(cx, handle);
        assert_eq!(
            groups[2],
            [
                "Recycle bin…",
                "Import keys…  ·  Disabled in read-only mode",
                "Export loaded keys…",
                "Compare with another server…"
            ]
        );
        assert_eq!(
            groups[3],
            [
                "Config",
                "Persistence",
                "Flush current database…  ·  Disabled in read-only mode",
                "Flush all databases…  ·  Disabled in read-only mode"
            ]
        );
    }
}
