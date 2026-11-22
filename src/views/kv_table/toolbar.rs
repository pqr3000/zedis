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

//! Footer toolbar for [`ZedisKvTable`]: add / bulk paste, keyword filter,
//! CSV export, sorted-set score filter, type-native key ops, batch selection
//! actions, and pagination status.

use super::*;

use gpui::rems;

impl<T: ZedisKvFetcher> ZedisKvTable<T> {
    pub(super) fn render_row_count(&self, cx: &App) -> impl IntoElement {
        let color = cx.theme().muted_foreground;
        h_flex()
            .flex_none()
            .ml_auto()
            .items_center()
            .gap_2()
            .child(
                Icon::new(if self.done {
                    CustomIconName::CircleCheckBig
                } else {
                    CustomIconName::CircleDotDashed
                })
                .text_color(color),
            )
            .child(
                Label::new(format!("{} / {}", self.items_count, self.total_count))
                    .text_sm()
                    .text_color(color),
            )
    }

    fn entry_filter(&self, sidebar: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let search_btn = Button::new(COLLECTION_FILTER_ID)
            .small()
            .ghost()
            .icon(IconName::Search)
            .accessibility_label(i18n_kv_table(cx, "search_tooltip"))
            .tooltip(i18n_kv_table(cx, "search_tooltip"))
            .loading(self.loading)
            .disabled(self.loading)
            .on_click(cx.listener(|this, _, _, cx| {
                this.handle_filter(cx);
            }));

        div()
            .flex_none()
            .max_w_full()
            .map(|slot| if sidebar { slot.w_full() } else { slot.w(rems(12.5)) })
            .child(
                Input::new(&self.keyword_state)
                    .small()
                    .suffix(search_btn)
                    .cleanable(true),
            )
    }

    pub(super) fn render_footer(&self, sidebar: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = cx.theme().muted_foreground;

        // Determine if operations are allowed based on mode
        let selected_count = self.table_state.read(cx).delegate().selected_count();
        let key_ops = self.key_ops();
        let score_range = self.server_state.read(cx).zset_score_range();
        let can_add = self.mode.contains(KvTableMode::ADD);
        let can_filter = self.mode.contains(KvTableMode::FILTER);

        let table_focus = self.focus_handle.clone();

        h_flex()
            .flex_none()
            .w_full()
            .px_3()
            .py_2()
            // In the sidebar the filter has its own line above the row actions.
            .child(
                h_flex()
                    .flex_wrap()
                    .when(sidebar, |bar| bar.flex_1())
                    .min_w_0()
                    .gap_2()
                    .when(sidebar && can_filter, |bar| bar.child(self.entry_filter(true, cx)))
                    .when(can_add, |this| {
                        this.child(
                            Button::new(COLLECTION_ADD_ID)
                                .small()
                                .outline()
                                .icon(CustomIconName::FilePlusCorner)
                                .accessibility_label(i18n_kv_table(cx, "add_value_tooltip"))
                                .tooltip(i18n_kv_table(cx, "add_value_tooltip"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.handle_add_row(window, cx);
                                })),
                        )
                    })
                    .when(!sidebar && can_filter, |bar| bar.child(self.entry_filter(false, cx)))
                    // List and Stream have no server-side scan, so
                    // their filter only sees the loaded rows. Say so
                    // next to the box: a result that looks like a
                    // whole-key search but is not would be worse than
                    // no filter at all.
                    .when(can_filter && self.fetcher.filters_client_side(), |this| {
                        this.child(
                            Label::new(t!(
                                "kv_table.filter_loaded_only",
                                count = self.items_count,
                                locale = cx.global::<ZedisGlobalStore>().read(cx).locale()
                            ))
                            .text_xs()
                            .text_color(text_color),
                        )
                    })
                    .when_some(self.action_button_factory.as_ref(), |this, factory| {
                        this.children(factory(window, cx).into_iter().map(|button| button.small()))
                    })
                    // Export loaded rows to CSV — one button on the
                    // shared table covers every collection type.
                    .when(
                        saves_files() && self.items_count > 0 && Capability::ExportCsv.allowed(self.readonly),
                        |this| {
                            this.child(
                                Button::new(COLLECTION_EXPORT_ID)
                                    .small()
                                    .ghost()
                                    .icon(CustomIconName::Download)
                                    .accessibility_label(i18n_common(cx, "export_csv"))
                                    .tooltip(i18n_common(cx, "export_csv"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.export_csv(cx);
                                    })),
                            )
                        },
                    )
                    // A sorted set's own query: members are ordered
                    // by score, so a score window is what the type is
                    // for — the keyword box can only match names.
                    .when(can_filter && self.fetcher.key_type() == KeyType::Zset, |this| {
                        let active = score_range.is_some();
                        this.child(
                            Button::new("kv-table-score-filter")
                                .small()
                                .outline()
                                .selected(active)
                                .icon(CustomIconName::Equal)
                                .accessibility_label(i18n_zset_editor(cx, "score_filter"))
                                .tooltip(i18n_zset_editor(cx, "score_filter"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    open_score_filter_dialog(this.server_state.clone(), window, cx);
                                })),
                        )
                    })
                    .when_some(score_range, |this, (min, max)| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Label::new(format!("{min} … {max}"))
                                        .text_xs()
                                        .font_family(get_mono_font_family())
                                        .text_color(text_color),
                                )
                                .child(
                                    Button::new("kv-table-score-filter-clear")
                                        .small()
                                        .ghost()
                                        .icon(CustomIconName::X)
                                        .accessibility_label(i18n_zset_editor(cx, "score_filter_clear"))
                                        .tooltip(i18n_zset_editor(cx, "score_filter_clear"))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.server_state
                                                .update(cx, |state, cx| state.clear_zset_score_filter(cx));
                                        })),
                                ),
                        )
                    })
                    // Type-native operations (LTRIM / ZINCRBY / …)
                    // behind one menu, so the footer keeps its shape
                    // whether a type has three of them or none.
                    .when(!key_ops.is_empty(), |this| {
                        this.child(
                            Button::new(COLLECTION_OPERATIONS_ID)
                                .small()
                                .ghost()
                                .icon(CustomIconName::Zap)
                                .accessibility_label(i18n_key_ops(cx, "menu_tooltip"))
                                .tooltip(i18n_key_ops(cx, "menu_tooltip"))
                                .dropdown_menu(move |menu, _window, _cx| {
                                    let mut menu = menu.action_context(table_focus.clone());
                                    for action in key_ops {
                                        let label_key = key_op_title_key(*action);
                                        menu = menu.menu_element(Box::new(*action), move |_, cx| {
                                            Label::new(i18n_key_ops(cx, label_key))
                                        });
                                    }
                                    menu
                                }),
                        )
                    })
                    // The selection chip only exists while something
                    // is ticked, so the toolbar keeps its usual shape
                    // until there is a batch to act on.
                    .when(selected_count > 0, |this| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Label::new(t!(
                                        "kv_table.selected_count",
                                        count = selected_count,
                                        locale = cx.global::<ZedisGlobalStore>().read(cx).locale()
                                    ))
                                    .text_xs()
                                    .text_color(text_color),
                                )
                                .child(
                                    Button::new("kv-table-remove-selected")
                                        .small()
                                        .danger()
                                        .icon(CustomIconName::FileXCorner)
                                        .accessibility_label(i18n_kv_table(cx, "remove_selected_tooltip"))
                                        .tooltip(i18n_kv_table(cx, "remove_selected_tooltip"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.handle_remove_selected(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("kv-table-clear-selection")
                                        .small()
                                        .ghost()
                                        .icon(CustomIconName::X)
                                        .accessibility_label(i18n_kv_table(cx, "clear_selection_tooltip"))
                                        .tooltip(i18n_kv_table(cx, "clear_selection_tooltip"))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.table_state
                                                .update(cx, |state, _| state.delegate_mut().clear_selection());
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    // Status icon and count, at the right end of
                    // whichever line they land on: part of the
                    // wrapping group, or a narrow footer drew them
                    // over the end of the filter box.
                    .when(!sidebar, |bar| bar.child(self.render_row_count(cx)))
                    .flex_1(),
            )
    }
}
