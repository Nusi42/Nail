//! The UI context (M1b, ADR 0022): the latest report of what the UI shows
//! (`ReportUiContext`), kept in memory for MCP clients (`nail://ui/context`).
//! Several UIs may be connected; the most recent report wins.

use nail_api::UiContext;

use crate::service::{Service, lock};

impl Service {
    /// `ReportUiContext`: replaces the stored context.
    pub(crate) fn report_ui_context(&self, context: UiContext) {
        *lock(&self.inner.ui_context) = context;
    }

    /// The last reported UI context (empty before the first report).
    pub fn ui_context(&self) -> UiContext {
        lock(&self.inner.ui_context).clone()
    }
}
