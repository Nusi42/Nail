//! Widgets of M1b WP22 (rules). Only that package edits this table (ADR 0022).

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature.
///
/// `rule-manager`: the rule list with the visual / text editor, test
/// preview and run history (`ui/src/features/rules/RuleManager.tsx`).
pub(crate) const WIDGETS: &[WidgetSpec] = &[widget("rule-manager", &[], &[], false)];
