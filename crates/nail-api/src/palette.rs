//! The command palette (M4c, ADR 0094).
//!
//! The palette lists every action the UI can run: built-in and feature
//! actions (`ui/src/actions`, `ui/src/features/*`), the layout's aliases
//! and plug-in commands, with their titles, keywords, the key bindings the
//! layout gives them and whether they are available in the current
//! context. That list is assembled in the UI (`ui/src/actions/palette.ts`);
//! the core only keeps the **recently used** actions, so they survive a
//! restart and follow the user between the desktop and the local web UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How many recent actions the core keeps.
pub const MAX_RECENT_ACTIONS: usize = 20;

/// Longest action id the core records.
pub const MAX_ACTION_ID_CHARS: usize = 160;

/// The `app_settings` key of the recent actions.
pub const RECENT_ACTIONS_KEY: &str = "palette.recent";

labeled_enum! {
    /// Where an action can run (`action-availability-*`). The names match
    /// the layout's key-binding contexts where those exist.
    pub enum ActionAvailability : "action-availability" {
        /// Anywhere.
        Global = "global",
        /// With a message list focused / a message selected.
        List = "list",
        /// With a message open in the reader.
        Reader = "reader",
        /// In the composer.
        Composer = "composer",
        /// In the calendar.
        Calendar = "calendar",
        /// In the contacts view.
        Contacts = "contacts",
        /// In the task list.
        Tasks = "tasks",
        /// Only in the desktop app.
        Desktop = "desktop",
    }
}

/// One recently used action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecentAction {
    /// The action id (`mail.reply`, a layout alias, a plug-in command).
    pub action_id: String,
    /// When it was last run from the palette (RFC 3339).
    pub last_used_at: String,
    /// How often it was run from the palette.
    pub count: u32,
}

/// The recent actions, most recent first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RecentActions {
    /// Most recent first, at most [`MAX_RECENT_ACTIONS`].
    pub actions: Vec<RecentAction>,
}

/// Whether `id` looks like an action id: 1..=[`MAX_ACTION_ID_CHARS`]
/// characters, no whitespace or control characters.
pub fn is_action_id(id: &str) -> bool {
    let chars = id.chars().count();
    (1..=MAX_ACTION_ID_CHARS).contains(&chars)
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl RecentActions {
    /// Records a use: moves the action to the front (counting it) and
    /// drops the oldest beyond [`MAX_RECENT_ACTIONS`]. Returns `false`
    /// (and changes nothing) for an invalid id.
    pub fn record(&mut self, action_id: &str, now: &str) -> bool {
        if !is_action_id(action_id) {
            return false;
        }
        let count = match self.actions.iter().position(|a| a.action_id == action_id) {
            Some(index) => self.actions.remove(index).count.saturating_add(1),
            None => 1,
        };
        self.actions.insert(
            0,
            RecentAction {
                action_id: action_id.to_owned(),
                last_used_at: now.to_owned(),
                count,
            },
        );
        self.actions.truncate(MAX_RECENT_ACTIONS);
        true
    }

    /// Forgets one action (e.g. a removed plug-in's command).
    pub fn forget(&mut self, action_id: &str) {
        self.actions.retain(|a| a.action_id != action_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn labels_match_the_wire() {
        assert_wire_labels!(ActionAvailability);
    }

    #[test]
    fn records_most_recent_first_and_caps() {
        let mut recent = RecentActions::default();
        assert!(recent.record("mail.reply", "t1"));
        assert!(recent.record("mail.archive", "t2"));
        assert!(recent.record("mail.reply", "t3"));
        assert_eq!(recent.actions[0].action_id, "mail.reply");
        assert_eq!(recent.actions[0].count, 2);
        assert_eq!(recent.actions[0].last_used_at, "t3");
        assert_eq!(recent.actions.len(), 2);
        for n in 0..30 {
            recent.record(&format!("a.{n}"), "t");
        }
        assert_eq!(recent.actions.len(), MAX_RECENT_ACTIONS);
        assert_eq!(recent.actions[0].action_id, "a.29");
        recent.forget("a.29");
        assert_eq!(recent.actions[0].action_id, "a.28");
    }

    #[test]
    fn refuses_bad_ids() {
        let mut recent = RecentActions::default();
        assert!(!recent.record("", "t"));
        assert!(!recent.record("mail reply", "t"));
        assert!(!recent.record(&"x".repeat(MAX_ACTION_ID_CHARS + 1), "t"));
        assert!(recent.actions.is_empty(), "{:?}", recent.actions);
        assert!(is_action_id("plugin.follow-up.remind"));
    }
}
