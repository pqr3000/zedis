//! The sidebar presents the active collection table's own controls. The table
//! still owns its input, selection, file export and Redis operations.

use super::*;
use gpui::{AnyElement, App, EntityId};

type RenderControls = Box<dyn Fn(&mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Default)]
pub(crate) struct CollectionTools {
    source: Option<EntityId>,
    render_controls: Option<RenderControls>,
    watch: Option<Subscription>,
}

pub(crate) trait CollectionEditor: Render {
    fn bind_tools(&mut self, host: &Entity<CollectionTools>, cx: &mut App);
}

pub(crate) fn collection_view<E: CollectionEditor>(
    editor: &Entity<E>,
    host: Option<&Entity<CollectionTools>>,
    cx: &mut App,
) -> AnyElement {
    if let Some(host) = host {
        editor.update(cx, |editor, cx| editor.bind_tools(host, cx));
    }
    editor.clone().into_any_element()
}

impl CollectionTools {
    pub(crate) fn clear(host: &Entity<Self>, cx: &mut App) {
        if host.read(cx).source.is_some() {
            host.update(cx, |tools, cx| {
                *tools = Self::default();
                cx.notify();
            });
        }
    }

    pub(crate) fn bind<T: ZedisKvFetcher>(host: &Entity<Self>, table: &Entity<ZedisKvTable<T>>, cx: &mut App) {
        if host.read(cx).source != Some(table.entity_id()) {
            table.update(cx, |table, cx| {
                table.sidebar_controls = true;
                cx.notify();
            });
            host.update(cx, |tools, cx| {
                tools.source = Some(table.entity_id());
                tools.watch = Some(cx.observe(table, |_, _, cx| cx.notify()));
                let weak = table.downgrade();
                tools.render_controls = Some(Box::new(move |window, cx| {
                    weak.update(cx, |table, cx| {
                        let state = table.server_state.read(cx);
                        let value = state.value()?;
                        (value.key_type() == table.fetcher.key_type()
                            && value.data.is_some()
                            && value.failure_message().is_none())
                        .then(|| table.render_footer(true, window, cx).into_any_element())
                    })
                    .ok()
                    .flatten()
                }));
                cx.notify();
            });
        }
    }
}

impl Render for CollectionTools {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let controls = self.render_controls.as_ref().and_then(|render| render(window, cx));
        v_flex()
            .id("sidebar-collection-tools")
            .flex_none()
            .when_some(controls, |bar, controls| {
                bar.border_t_1().border_color(cx.theme().sidebar_border).child(controls)
            })
    }
}
