//! Fixed navigation above the tree, shared by the full sidebar and its rail.

use super::*;
use crate::states::{ServerToolsAction, i18n_status_bar};
use crate::views::{ZedisContent, ZedisTitleBar};
use gpui::EventEmitter;
use gpui_kit::component::menu::DropdownMenu;

pub(crate) const SIDEBAR_MENU_ID: &str = "sidebar-menu";
pub(super) const SIDEBAR_NEW_ID: &str = "sidebar-new-connection";

pub(crate) struct SidebarToolRequested {
    pub content: Entity<ZedisContent>,
    pub action: ServerToolsAction,
}
impl EventEmitter<SidebarToolRequested> for ZedisSidebar {}

impl ZedisSidebar {
    pub(crate) fn bind_content(&mut self, content: Entity<ZedisContent>, cx: &mut Context<Self>) {
        if self.active_content.as_ref() != Some(&content) {
            if self
                .workspaces
                .focused
                .as_ref()
                .is_some_and(|(tree, _)| content.read(cx).database_tree().as_ref() != Some(tree))
            {
                self.workspaces.focused = None;
            }
            self.active_content = Some(content);
            cx.notify();
        }
    }

    fn navigation_menu(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let sidebar = cx.weak_entity();
        let active = self
            .workspaces
            .focused
            .as_ref()
            .and_then(|(tree, _)| {
                self.workspaces
                    .entries
                    .values()
                    .find(|entry| &entry.tree == tree)
                    .map(|entry| entry.content.clone())
            })
            .or_else(|| {
                self.active_content
                    .clone()
                    .filter(|_| cx.global::<ZedisGlobalStore>().read(cx).route().is_server())
            });
        let label = i18n_sidebar(cx, "menu");
        let tip = active.as_ref().map_or_else(
            || label.to_string(),
            |content| {
                let state = content.read(cx).server_state();
                let state = state.read(cx);
                let name = self
                    .state
                    .sections
                    .iter()
                    .flat_map(|section| &section.servers)
                    .find(|server| server.id.as_ref() == state.server_id())
                    .map(|server| server.name.as_ref())
                    .unwrap_or(state.server_id());
                format!("{} · {name} · db{}", i18n_status_bar(cx, "tools_tooltip"), state.db())
            },
        );
        Button::new(SIDEBAR_MENU_ID)
            .ghost()
            .small()
            .icon(IconName::Menu)
            .accessibility_label(label)
            .tooltip(tip)
            .dropdown_menu(move |menu, window, cx| {
                let Some(content) = active
                    .as_ref()
                    .filter(|content| !content.read(cx).server_state().read(cx).server_id().is_empty())
                else {
                    return ZedisTitleBar::render_settings_menu(menu, window, cx);
                };
                let menu = tools_menu::build(menu, content.clone(), sidebar.clone(), window, cx);
                menu.separator().submenu_with_icon(
                    Some(Icon::new(IconName::Settings2)),
                    i18n_sidebar(cx, "preferences"),
                    window,
                    cx,
                    ZedisTitleBar::render_settings_menu,
                )
            })
    }

    /// Fixed-height top band aligned with the editor key bar containing Home and Collapse.
    pub(super) fn home_band(
        &self,
        is_home_current: bool,
        look: &ListLook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sidebar_collapsed = look.sidebar_collapsed;
        let home_tooltip = i18n_sidebar(cx, "home");
        let collapse_label = i18n_sidebar(cx, "collapse");
        let mark = if cx.theme().is_dark() {
            self.logo_light.clone()
        } else {
            self.logo.clone()
        };
        let label_color = if is_home_current { look.selected_fg } else { look.row_fg };

        let home_button = sidebar_focus_button(
            SIDEBAR_HOME_ITEM_ID,
            home_tooltip.clone(),
            is_home_current,
            false,
            look,
            window,
            cx,
        )
        .aria_selected(is_home_current)
        .on_click(move |e: &ClickEvent, _window, cx| navigate_home(is_home_current, e.modifiers(), cx));

        let home_control = if sidebar_collapsed {
            let tip = home_tooltip.clone();
            div().flex_1().flex().items_center().justify_center().child(
                home_button
                    .size_7()
                    .justify_center()
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .child(img(mark).flex_none().size(rems(1.125))),
            )
        } else {
            div().flex_1().min_w_0().flex().items_center().child(
                home_button.w_full().h_7().px_2().justify_start().child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .child(img(mark).flex_none().size(rems(1.125)))
                        .child(
                            Label::new(home_tooltip.clone())
                                .text_sm()
                                .font_semibold()
                                .text_color(label_color)
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .flex_1()
                                .min_w_0(),
                        ),
                ),
            )
        };

        let menu = self.navigation_menu(cx);
        let new_label = format!("{}…", i18n_servers(cx, "add_server_title"));
        let new_connection = Button::new(SIDEBAR_NEW_ID)
            .ghost()
            .small()
            .icon(IconName::Plus)
            .accessibility_label(new_label.clone())
            .tooltip(new_label)
            .on_click(|_, _, cx| {
                cx.global::<ZedisGlobalStore>()
                    .clone()
                    .update(cx, |state, cx| state.new_server(cx));
            });
        if sidebar_collapsed {
            return v_flex()
                .flex_none()
                .items_center()
                .gap_1()
                .py_1()
                .id(SIDEBAR_HOME_ROW_ID)
                .child(home_control)
                .child(new_connection)
                .child(menu)
                .into_any_element();
        }
        h_flex()
            .id(SIDEBAR_HOME_ROW_ID)
            .flex_none()
            .px_3()
            .h(EDITOR_KEY_BAR_HEIGHT)
            .items_center()
            .gap_1()
            .child(home_control)
            .child(new_connection)
            .child(menu)
            .child(
                Button::new(SIDEBAR_COLLAPSE_TOGGLE_ID)
                    .ghost()
                    .small()
                    .icon(IconName::ChevronLeft)
                    .accessibility_label(collapse_label.clone())
                    .tooltip(collapse_label)
                    .on_click(|_, _, cx| toggle_sidebar_collapsed(cx)),
            )
            .border_b_1()
            .border_color(look.divider_color)
            .into_any_element()
    }
}
