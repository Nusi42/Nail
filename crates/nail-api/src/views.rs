//! The shell of M5b (ADR 0109): top-level views behind a navigation rail,
//! the folder pane's sections, the message list's filter chips, toolbar
//! overflow and the settings page.
//!
//! The layout model itself lives in [`crate::config::layout`]
//! ([`ResolvedLayout::views`], [`ResolvedLayout::rail`],
//! [`ResolvedLayout::settings`], [`ToolbarItem::More`],
//! [`ResolvedLayout::toolbar_settings`]); this module holds the ids, widget
//! kinds and action names the UI and `nail-config` share, and the small
//! pure rules both sides implement (the UI mirrors them in
//! `ui/src/layout/{views,overflow}.ts` and `ui/src/widgets/folderPane.ts`,
//! tested against the same cases).
//!
//! * **Views:** `mail`, `calendar`, `contacts`, `tasks`, `settings`
//!   ([`BUILTIN_VIEWS`]). [`VIEW_SHOW_ACTION`] `"<view>"` switches (the
//!   layout binds `ctrl+1`… through aliases such as `view.calendar`);
//!   without a `view-host` in the active window tree it opens the view's
//!   window instead, so the actions work with any layout.
//! * **Badges:** [`crate::config::layout::BadgeSource`] — unread mail in
//!   the inboxes, confirmations waiting.
//! * **Folder pane:** sections Favorites (the unified folders,
//!   [`FavoriteFolder`]), one per account (with a colour slot,
//!   [`account_color_slot`]) and smart folders (only when there is one):
//!   [`folder_pane_sections`]. Collapsed sections are kept in the UI state
//!   ([`crate::ui_state`], key [`UI_STATE_COLLAPSED_SECTIONS`]).
//! * **List filters:** [`ListFilter`] chips map to search terms.
//! * **Toolbar overflow:** [`overflow_order`].
//! * **Settings page:** groups [`SETTINGS_GROUPS`]; the UI builds the
//!   search index from the page's sections and the setting labels each
//!   feature registers per widget kind (`Feature.settings`).

use crate::config::layout::{ResolvedLayout, ToolbarItem};
use crate::ids::AccountId;

/// The Mail view.
pub const VIEW_MAIL: &str = "mail";
/// The Calendar view.
pub const VIEW_CALENDAR: &str = "calendar";
/// The Contacts view.
pub const VIEW_CONTACTS: &str = "contacts";
/// The Tasks view.
pub const VIEW_TASKS: &str = "tasks";
/// The Settings view (the settings page).
pub const VIEW_SETTINGS: &str = "settings";
/// The views of the default layout, in rail order.
pub const BUILTIN_VIEWS: &[&str] = &[
    VIEW_MAIL,
    VIEW_CALENDAR,
    VIEW_CONTACTS,
    VIEW_TASKS,
    VIEW_SETTINGS,
];

/// `view.show "<view>"`: switch to a view (or open its window when the
/// layout has no `view-host`).
pub const VIEW_SHOW_ACTION: &str = "view.show";
/// `view.next`: the next view on the rail (wraps around).
pub const VIEW_NEXT_ACTION: &str = "view.next";
/// `view.previous`: the previous view on the rail (wraps around).
pub const VIEW_PREVIOUS_ACTION: &str = "view.previous";

/// The navigation rail widget (renders [`ResolvedLayout::rail`]).
pub const NAV_RAIL_WIDGET: &str = "nav-rail";
/// The view host widget (renders the active view's window tree).
pub const VIEW_HOST_WIDGET: &str = "view-host";
/// The settings page widget (renders [`ResolvedLayout::settings`]).
pub const SETTINGS_PAGE_WIDGET: &str = "settings-page";
/// The message list header widget (search, title with counts, filter
/// chips).
pub const LIST_HEADER_WIDGET: &str = "list-header";

/// The settings groups of the default layout, in navigation order (Fluent
/// ids `settings-group-<id>`).
pub const SETTINGS_GROUPS: &[&str] = &["general", "look-and-feel", "security", "extensions"];
/// `settings.section "<id>"`: show the settings page at a section.
pub const SETTINGS_SECTION_ACTION: &str = "settings.section";
/// `settings.search`: focus the settings search.
pub const SETTINGS_SEARCH_ACTION: &str = "settings.search";

/// `list.filter "<filter>"`: set the message list's filter chip.
pub const LIST_FILTER_ACTION: &str = "list.filter";
/// `folders.toggle-section "<section id>"`: collapse / expand a folder-pane
/// section.
pub const FOLDER_SECTION_TOGGLE_ACTION: &str = "folders.toggle-section";

/// UI-state key: the ids of the collapsed folder-pane sections (a JSON
/// array of strings).
pub const UI_STATE_COLLAPSED_SECTIONS: &str = "folder-pane.collapsed";
/// UI-state key: the last active view (a JSON string).
pub const UI_STATE_VIEW: &str = "shell.view";
/// UI-state key: the last settings section (a JSON string).
pub const UI_STATE_SETTINGS_SECTION: &str = "settings.section";

labeled_enum! {
    /// A filter chip of the message list header.
    pub enum ListFilter : "list-filter" {
        /// Every message.
        All = "all",
        /// Unread messages.
        Unread = "unread",
        /// Flagged messages.
        Flagged = "flagged",
        /// Messages with attachments.
        Attachments = "attachments",
    }
}

impl ListFilter {
    /// The search-language term the filter adds (`None` for
    /// [`ListFilter::All`]).
    pub fn search_term(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Unread => Some("is:unread"),
            Self::Flagged => Some("is:flagged"),
            Self::Attachments => Some("has:attachment"),
        }
    }
}

labeled_enum! {
    /// A unified folder the Favorites section can show (`folder-tree
    /// favorites="all-inboxes flagged snoozed drafts"`).
    pub enum FavoriteFolder : "favorite-folder" {
        /// The inboxes of every account.
        AllInboxes = "all-inboxes",
        /// Unread mail of every account.
        Unread = "unread",
        /// Flagged mail of every account.
        Flagged = "flagged",
        /// Snoozed mail.
        Snoozed = "snoozed",
        /// Drafts of every account.
        Drafts = "drafts",
        /// Sent mail of every account.
        Sent = "sent",
        /// The outbox.
        Outbox = "outbox",
    }
}

/// The Favorites of the default layout.
pub const DEFAULT_FAVORITES: &[FavoriteFolder] = &[
    FavoriteFolder::AllInboxes,
    FavoriteFolder::Flagged,
    FavoriteFolder::Snoozed,
    FavoriteFolder::Drafts,
];

labeled_enum! {
    /// A kind of folder-pane section (`folder-tree sections="favorites
    /// accounts smart-folders"`).
    pub enum FolderPaneSectionKind : "folder-section" {
        /// The unified folders.
        Favorites = "favorites",
        /// One section per account.
        Accounts = "accounts",
        /// The saved searches (only when there is one).
        SmartFolders = "smart-folders",
    }
}

/// How many account colours a theme defines (`color.avatar-1` …
/// `color.avatar-8`; the folder pane reuses the avatar palette).
pub const ACCOUNT_COLOR_SLOTS: u8 = 8;

/// The colour slot (1-based) of the account at `index` in the account list
/// (display order): the first eight accounts get distinct colours.
pub fn account_color_slot(index: usize) -> u8 {
    // The remainder is below ACCOUNT_COLOR_SLOTS, so it fits a u8.
    u8::try_from(index % usize::from(ACCOUNT_COLOR_SLOTS)).map_or(1, |slot| slot + 1)
}

/// The section id of the Favorites section.
pub const FAVORITES_SECTION_ID: &str = "favorites";
/// The section id of the smart-folders section.
pub const SMART_FOLDERS_SECTION_ID: &str = "smart-folders";

/// The section id of an account's section: `account:<id>`.
pub fn account_section_id(account: &AccountId) -> String {
    format!("account:{account}")
}

/// One section of the folder pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderPaneSection {
    /// Stable id: [`FAVORITES_SECTION_ID`], [`account_section_id`] or
    /// [`SMART_FOLDERS_SECTION_ID`] (the key in
    /// [`UI_STATE_COLLAPSED_SECTIONS`]).
    pub id: String,
    /// The kind.
    pub kind: FolderPaneSectionKind,
    /// The account of an account section.
    pub account_id: Option<AccountId>,
    /// The colour slot of an account section ([`account_color_slot`]).
    pub color_slot: Option<u8>,
    /// Whether the section is collapsed.
    pub collapsed: bool,
}

/// The folder pane's sections, in the order of `kinds` (the widget's
/// `sections` option): Favorites (only when it lists a folder), one section
/// per account of `accounts` (display order), smart folders only when
/// `smart_folders > 0`. `collapsed` holds the collapsed section ids.
pub fn folder_pane_sections(
    kinds: &[FolderPaneSectionKind],
    favorites: &[FavoriteFolder],
    accounts: &[AccountId],
    smart_folders: usize,
    collapsed: &[String],
) -> Vec<FolderPaneSection> {
    let is_collapsed = |id: &str| collapsed.iter().any(|c| c == id);
    let mut out = Vec::new();
    for kind in kinds {
        match kind {
            FolderPaneSectionKind::Favorites if !favorites.is_empty() => {
                out.push(FolderPaneSection {
                    id: FAVORITES_SECTION_ID.to_owned(),
                    kind: *kind,
                    account_id: None,
                    color_slot: None,
                    collapsed: is_collapsed(FAVORITES_SECTION_ID),
                });
            }
            FolderPaneSectionKind::Accounts => {
                for (index, account) in accounts.iter().enumerate() {
                    let id = account_section_id(account);
                    out.push(FolderPaneSection {
                        collapsed: is_collapsed(&id),
                        id,
                        kind: *kind,
                        account_id: Some(account.clone()),
                        color_slot: Some(account_color_slot(index)),
                    });
                }
            }
            FolderPaneSectionKind::SmartFolders if smart_folders > 0 => {
                out.push(FolderPaneSection {
                    id: SMART_FOLDERS_SECTION_ID.to_owned(),
                    kind: *kind,
                    account_id: None,
                    color_slot: None,
                    collapsed: is_collapsed(SMART_FOLDERS_SECTION_ID),
                });
            }
            _ => {}
        }
    }
    out
}

/// The order in which a toolbar's items move into its "More" menu when the
/// bar is too narrow: indices of [`ToolbarItem::Action`] items only.
/// Unranked items go first, from the end of the bar; then ranked items,
/// highest rank first (ties: later item first). `rank=1` stays longest.
/// The renderer moves items in this order until the rest fits; spacers and
/// separators next to moved items collapse.
pub fn overflow_order(items: &[ToolbarItem]) -> Vec<usize> {
    let mut actions: Vec<(usize, Option<u32>)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            ToolbarItem::Action { rank, .. } => Some((i, *rank)),
            _ => None,
        })
        .collect();
    // Unranked (None) sorts as "infinitely high" rank: moved first.
    actions.sort_by(|(ia, ra), (ib, rb)| {
        let ka = ra.map_or(u64::MAX, u64::from);
        let kb = rb.map_or(u64::MAX, u64::from);
        kb.cmp(&ka).then(ib.cmp(ia))
    });
    actions.into_iter().map(|(i, _)| i).collect()
}

/// The view whose window is `window`, if any.
pub fn view_of_window<'a>(layout: &'a ResolvedLayout, window: &str) -> Option<&'a str> {
    layout
        .views
        .iter()
        .find(|(_, def)| def.window == window)
        .map(|(id, _)| id.as_str())
}

/// The view after (`step = 1`) or before (`step = -1`) `current` in rail
/// order (the rail's view entries, else the definition order), wrapping
/// around; the start view when `current` is unknown.
pub fn step_view<'a>(layout: &'a ResolvedLayout, current: &str, step: i32) -> Option<&'a str> {
    let mut order: Vec<&str> = layout
        .rail
        .iter()
        .filter_map(|item| match item {
            crate::config::layout::RailItem::View { view } if layout.views.contains_key(view) => {
                Some(view.as_str())
            }
            _ => None,
        })
        .collect();
    if order.is_empty() {
        order = layout.views.keys().map(String::as_str).collect();
    }
    let len = i64::try_from(order.len()).ok().filter(|n| *n > 0)?;
    let Some(pos) = order.iter().position(|v| *v == current) else {
        return layout.start_view();
    };
    let next = (i64::try_from(pos).ok()? + i64::from(step)).rem_euclid(len);
    order.get(usize::try_from(next).ok()?).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::layout::{RailItem, ViewDef};
    use crate::labels::assert_labels;

    #[test]
    fn labels() {
        assert_labels(ListFilter::ALL, ListFilter::as_str, ListFilter::message_id);
        assert_labels(
            FavoriteFolder::ALL,
            FavoriteFolder::as_str,
            FavoriteFolder::message_id,
        );
        assert_labels(
            FolderPaneSectionKind::ALL,
            FolderPaneSectionKind::as_str,
            FolderPaneSectionKind::message_id,
        );
    }

    #[test]
    fn filters_map_to_search_terms() {
        assert_eq!(ListFilter::All.search_term(), None);
        assert_eq!(ListFilter::Unread.search_term(), Some("is:unread"));
        assert_eq!(ListFilter::Flagged.search_term(), Some("is:flagged"));
        assert_eq!(
            ListFilter::Attachments.search_term(),
            Some("has:attachment")
        );
    }

    #[test]
    fn account_colours_cycle() {
        let slots: Vec<u8> = (0..10).map(account_color_slot).collect();
        assert_eq!(slots, [1, 2, 3, 4, 5, 6, 7, 8, 1, 2]);
    }

    #[test]
    fn folder_pane_sections_follow_the_rules() {
        let accounts = vec![AccountId::from("work"), AccountId::from("home")];
        let all = FolderPaneSectionKind::ALL;
        let ids = |sections: &[FolderPaneSection]| -> Vec<String> {
            sections.iter().map(|s| s.id.clone()).collect()
        };
        // Smart folders only when there is one.
        let sections = folder_pane_sections(all, DEFAULT_FAVORITES, &accounts, 0, &[]);
        assert_eq!(
            ids(&sections),
            ["favorites", "account:work", "account:home"]
        );
        assert_eq!(sections[1].color_slot, Some(1));
        assert_eq!(sections[2].color_slot, Some(2));
        assert_eq!(sections[2].account_id, Some(AccountId::from("home")));
        let sections = folder_pane_sections(
            all,
            DEFAULT_FAVORITES,
            &accounts,
            2,
            &["account:home".into(), "smart-folders".into()],
        );
        assert_eq!(
            ids(&sections),
            ["favorites", "account:work", "account:home", "smart-folders"]
        );
        let collapsed: Vec<bool> = sections.iter().map(|s| s.collapsed).collect();
        assert_eq!(collapsed, [false, false, true, true]);
        // No favorites listed → no Favorites section; the order follows
        // the option.
        let sections = folder_pane_sections(
            &[
                FolderPaneSectionKind::SmartFolders,
                FolderPaneSectionKind::Favorites,
                FolderPaneSectionKind::Accounts,
            ],
            &[],
            &accounts[..1],
            1,
            &[],
        );
        assert_eq!(ids(&sections), ["smart-folders", "account:work"]);
    }

    fn act(action: &str, rank: Option<u32>) -> ToolbarItem {
        ToolbarItem::Action {
            action: action.into(),
            args: vec![],
            label: None,
            icon: None,
            rank,
            display: None,
        }
    }

    #[test]
    fn overflow_moves_unranked_from_the_end_then_highest_rank() {
        let items = vec![
            act("mail.reply", Some(1)),
            act("mail.reply-all", Some(2)),
            ToolbarItem::Separator,
            act("mail.archive", Some(4)),
            act("mail.delete", Some(4)),
            act("junk.mark", None),
            ToolbarItem::Spacer,
            act("print.message", None),
            ToolbarItem::More {
                label: None,
                icon: None,
                items: vec![],
            },
        ];
        assert_eq!(overflow_order(&items), [7, 5, 4, 3, 1, 0]);
        assert_eq!(overflow_order(&[ToolbarItem::Spacer]).len(), 0);
    }

    fn layout() -> ResolvedLayout {
        let view = |window: &str| ViewDef {
            label: None,
            icon: None,
            window: window.into(),
            badge: None,
        };
        let mut layout: ResolvedLayout = serde_json::from_value(serde_json::json!({
            "name": "x",
            "windows": {}
        }))
        .unwrap();
        layout.views.insert("mail".into(), view("mail"));
        layout.views.insert("calendar".into(), view("calendar"));
        layout.views.insert("settings".into(), view("settings"));
        layout
    }

    #[test]
    fn views_step_in_rail_order_and_wrap() {
        let mut layout = layout();
        assert_eq!(view_of_window(&layout, "calendar"), Some("calendar"));
        assert_eq!(view_of_window(&layout, "composer"), None);
        // No rail: definition order.
        assert_eq!(step_view(&layout, "mail", 1), Some("calendar"));
        assert_eq!(step_view(&layout, "mail", -1), Some("settings"));
        assert_eq!(step_view(&layout, "nope", 1), Some("mail"));
        // The rail's order wins; unknown rail entries are skipped.
        layout.rail = vec![
            RailItem::View {
                view: "settings".into(),
            },
            RailItem::Spacer,
            RailItem::View {
                view: "ghost".into(),
            },
            RailItem::View {
                view: "mail".into(),
            },
        ];
        assert_eq!(step_view(&layout, "settings", 1), Some("mail"));
        assert_eq!(step_view(&layout, "mail", 1), Some("settings"));
        layout.views.clear();
        layout.rail.clear();
        assert_eq!(step_view(&layout, "mail", 1), None);
    }
}
