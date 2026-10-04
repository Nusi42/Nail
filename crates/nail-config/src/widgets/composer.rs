//! Widgets of M1b WP21 (composer). Only that package edits this table (ADR 0022).

use super::WidgetSpec;
#[expect(
    unused_imports,
    reason = "the table's helpers stay imported for the package that adds widgets (ADR 0022)"
)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature, e.g.
/// `widget("my-widget", &[("option", OptionType::Bool)], &[], false)`.
pub(crate) const WIDGETS: &[WidgetSpec] = &[];
