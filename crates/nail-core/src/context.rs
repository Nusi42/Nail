//! The UI context (M1b, ADR 0022): the latest report of what the UI shows
//! (`ReportUiContext`), kept in memory for MCP clients (`nail://ui/context`).
//! Several UIs may be connected; the most recent report wins.

use nail_api::UiContext;

use crate::service::{Service, lock};

impl Service {
    /// `ReportUiContext`: replaces the stored context.
    pub(crate) fn report_ui_context(&self, context: UiContext) {
        let changed = {
            let mut current = lock(&self.inner.ui_context);
            let changed = current.selected_message_ids != context.selected_message_ids
                || current.message_id != context.message_id;
            *current = context.clone();
            changed
        };
        // Plug-in jobs subscribed to `selection.changed` (M4a).
        if changed && let Some(host) = self.inner.plugins.existing_host() {
            let mut ids = context.selected_message_ids;
            if ids.is_empty() {
                ids.extend(context.message_id);
            }
            host.deliver_event(
                nail_api::plugins::jobs::PluginEventName::SelectionChanged,
                ids,
            );
        }
        // Wakes the MCP server's `nail://ui/context` subscribers.
        self.inner
            .ui_context_changed
            .send_modify(|n| *n = n.wrapping_add(1));
    }

    /// The last reported UI context (empty before the first report).
    pub fn ui_context(&self) -> UiContext {
        lock(&self.inner.ui_context).clone()
    }
}
