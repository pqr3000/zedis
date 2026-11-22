//! Reuse a complete workspace so its sidebar tree and editor always operate
//! on the same server state. Only the layout and database identity are pinned.

use super::*;

impl ZedisContent {
    pub(crate) fn new_database_workspace(
        server: SharedString,
        db: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut content = Self::new(window, cx);
        content.set_active(false, cx);
        content
            .server_state
            .update(cx, |state, cx| state.select(server, db, cx));
        content.use_sidebar_tree(window, cx);
        content
    }

    pub(crate) fn use_sidebar_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tools = self
            .sidebar_tools
            .get_or_insert_with(|| cx.new(|_| CollectionTools::default()));
        if let Some(editor) = &self.value_editor {
            editor.update(cx, |editor, cx| {
                editor.sidebar_tools = Some(tools.clone());
                cx.notify();
            });
        }
        self.key_tree
            .get_or_insert_with(|| cx.new(|cx| ZedisKeyTree::new(self.server_state.clone(), window, cx)));
        cx.notify();
    }

    pub(crate) fn database_tree(&self) -> Option<Entity<ZedisKeyTree>> {
        self.key_tree.clone()
    }

    pub(crate) fn sidebar_editor_action(&mut self, action: EditorAction, window: &mut Window, cx: &mut Context<Self>) {
        // An unopened DB has no editor yet. Install its event subscriber
        // before forwarding a row-menu action such as Rename.
        self.value_editor_for_workspace(window, cx);
        self.server_state
            .update(cx, |state, cx| state.emit_editor_action(action, cx));
    }

    pub(crate) fn has_sidebar_tree(&self) -> bool {
        self.sidebar_tools.is_some()
    }

    pub(in crate::views) fn collection_tools(&self) -> Option<Entity<CollectionTools>> {
        self.sidebar_tools.clone()
    }

    pub(super) fn value_editor_for_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ZedisEditor> {
        self.value_editor
            .get_or_insert_with(|| {
                cx.new(|cx| {
                    let mut editor = ZedisEditor::new(self.server_state.clone(), window, cx);
                    editor.sidebar_tools = self.sidebar_tools.clone();
                    editor
                })
            })
            .clone()
    }
}
