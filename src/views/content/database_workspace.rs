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
        self.sidebar_tree = true;
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
        self.value_editor
            .get_or_insert_with(|| cx.new(|cx| ZedisEditor::new(self.server_state.clone(), window, cx)));
        self.server_state
            .update(cx, |state, cx| state.emit_editor_action(action, cx));
    }

    pub(crate) fn has_sidebar_tree(&self) -> bool {
        self.sidebar_tree
    }
}
