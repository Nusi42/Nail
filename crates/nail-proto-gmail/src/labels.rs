//! `users.labels`: list / get / create / patch / delete, and the mapping
//! between Gmail's label resources and Nail's label settings
//! ([`GmailLabelSettings`]). System labels shown as folders map to
//! [`GmailFolder`]s; user labels are Nail tags named like the label.

use std::collections::HashMap;

use nail_api::gmail::{
    GmailFolder, GmailLabelColor, GmailLabelKind, GmailLabelListVisibility, GmailLabelSettings,
    GmailMessageListVisibility, system,
};
use reqwest::Method;

use crate::client::GmailClient;
use crate::error::GmailError;
use crate::wire::{Label, LabelColor, ListLabelsResponse};

pub use nail_api::gmail::LABEL_PALETTE;

/// Whether Gmail accepts `color` (`#rrggbb`, any case) for labels.
pub fn is_palette_color(color: &str) -> bool {
    let lower = color.to_ascii_lowercase();
    LABEL_PALETTE.contains(&lower.as_str())
}

/// A label as the engine reports it to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmailLabelInfo {
    /// Gmail's id.
    pub id: String,
    /// System or user label.
    pub kind: GmailLabelKind,
    /// Name, colour, visibility.
    pub settings: GmailLabelSettings,
    /// The folder of a system label shown as a folder.
    pub folder: Option<GmailFolder>,
    /// Messages with the label (`labels.get` only).
    pub messages_total: Option<u64>,
    /// Unread messages with the label (`labels.get` only).
    pub messages_unread: Option<u64>,
}

impl GmailLabelInfo {
    /// The Nail tag of a user label (its name).
    pub fn tag(&self) -> Option<&str> {
        (self.kind == GmailLabelKind::User).then_some(self.settings.name.as_str())
    }
}

/// `labelListVisibility` on the wire.
pub fn list_visibility_wire(v: GmailLabelListVisibility) -> &'static str {
    match v {
        GmailLabelListVisibility::Show => "labelShow",
        GmailLabelListVisibility::ShowIfUnread => "labelShowIfUnread",
        GmailLabelListVisibility::Hide => "labelHide",
    }
}

/// `messageListVisibility` on the wire.
pub fn message_visibility_wire(v: GmailMessageListVisibility) -> &'static str {
    match v {
        GmailMessageListVisibility::Show => "show",
        GmailMessageListVisibility::Hide => "hide",
    }
}

fn parse_list_visibility(text: Option<&str>) -> GmailLabelListVisibility {
    match text {
        Some("labelShowIfUnread") => GmailLabelListVisibility::ShowIfUnread,
        Some("labelHide") => GmailLabelListVisibility::Hide,
        _ => GmailLabelListVisibility::Show,
    }
}

fn parse_message_visibility(text: Option<&str>) -> GmailMessageListVisibility {
    match text {
        Some("hide") => GmailMessageListVisibility::Hide,
        _ => GmailMessageListVisibility::Show,
    }
}

/// Whether a label id is one of Gmail's system labels (by the server's
/// `type`, else by the id: user label ids are `Label_<n>`).
pub fn is_system_label(label: &Label) -> bool {
    match label.kind.as_deref() {
        Some("system") => true,
        Some("user") => false,
        _ => is_system_id(&label.id),
    }
}

/// Whether a bare label id looks like a system label.
pub fn is_system_id(id: &str) -> bool {
    // Gmail's system ids are upper case (`INBOX`, `CATEGORY_SOCIAL`,
    // `YELLOW_STAR`, `BLUE_INFO`); user labels are `Label_<n>`.
    system::ALL.contains(&id)
        || (!id.is_empty()
            && !id.starts_with("Label_")
            && id.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
}

/// A label resource as Nail sees it.
pub fn label_info(label: &Label) -> GmailLabelInfo {
    let kind = if is_system_label(label) {
        GmailLabelKind::System
    } else {
        GmailLabelKind::User
    };
    let folder = match kind {
        GmailLabelKind::System => GmailFolder::ALL
            .into_iter()
            .find(|f| f.label() == Some(label.id.as_str())),
        GmailLabelKind::User => None,
    };
    GmailLabelInfo {
        id: label.id.clone(),
        kind,
        settings: GmailLabelSettings {
            name: label.name.clone(),
            color: label.color.as_ref().map(|c| GmailLabelColor {
                background: c.background_color.clone(),
                text: c.text_color.clone(),
            }),
            list_visibility: parse_list_visibility(label.label_list_visibility.as_deref()),
            message_visibility: parse_message_visibility(label.message_list_visibility.as_deref()),
        },
        folder,
        messages_total: label.messages_total,
        messages_unread: label.messages_unread,
    }
}

/// The request body of `labels.create` / `labels.patch` for `settings`.
pub fn label_body(settings: &GmailLabelSettings) -> Label {
    Label {
        name: settings.name.trim().to_owned(),
        label_list_visibility: Some(list_visibility_wire(settings.list_visibility).to_owned()),
        message_list_visibility: Some(
            message_visibility_wire(settings.message_visibility).to_owned(),
        ),
        color: settings.color.as_ref().map(|c| LabelColor {
            text_color: c.text.to_ascii_lowercase(),
            background_color: c.background.to_ascii_lowercase(),
        }),
        ..Label::default()
    }
}

/// The account's labels by id, with lookups by name for tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabelTable {
    by_id: HashMap<String, GmailLabelInfo>,
}

impl LabelTable {
    /// A table of `labels`.
    pub fn new(labels: impl IntoIterator<Item = GmailLabelInfo>) -> Self {
        Self {
            by_id: labels.into_iter().map(|l| (l.id.clone(), l)).collect(),
        }
    }

    /// The label with this id.
    pub fn get(&self, id: &str) -> Option<&GmailLabelInfo> {
        self.by_id.get(id)
    }

    /// Whether the id is known.
    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    /// Adds or replaces a label.
    pub fn insert(&mut self, label: GmailLabelInfo) {
        self.by_id.insert(label.id.clone(), label);
    }

    /// Removes a label.
    pub fn remove(&mut self, id: &str) {
        self.by_id.remove(id);
    }

    /// Every label.
    pub fn labels(&self) -> impl Iterator<Item = &GmailLabelInfo> {
        self.by_id.values()
    }

    /// The id of the user label named `name` (exact match first, then
    /// case-insensitive, as Gmail treats names).
    pub fn user_label_for_tag(&self, name: &str) -> Option<&str> {
        let users = || {
            self.by_id
                .values()
                .filter(|l| l.kind == GmailLabelKind::User)
        };
        users()
            .find(|l| l.settings.name == name)
            .or_else(|| users().find(|l| l.settings.name.to_lowercase() == name.to_lowercase()))
            .map(|l| l.id.as_str())
    }

    /// The Nail tags of `label_ids` (names of user labels, sorted; system
    /// and unknown ids are left out).
    pub fn tags_for(&self, label_ids: &[String]) -> Vec<String> {
        let mut tags: Vec<String> = label_ids
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .filter_map(|l| l.tag().map(str::to_owned))
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Whether some of `label_ids` are neither known nor system labels (the
    /// table needs a refresh).
    pub fn has_unknown(&self, label_ids: &[String]) -> bool {
        label_ids
            .iter()
            .any(|id| !self.by_id.contains_key(id) && !is_system_id(id))
    }
}

impl GmailClient {
    /// `labels.list`: every label (without counts).
    pub async fn list_labels(&self) -> Result<Vec<Label>, GmailError> {
        let page: ListLabelsResponse = self
            .json(
                Method::GET,
                &["labels"],
                &[],
                None::<&()>,
                true,
                "users.labels.list",
            )
            .await?;
        Ok(page.labels)
    }

    /// `labels.get`: one label with its counts.
    pub async fn get_label(&self, id: &str) -> Result<Label, GmailError> {
        self.json(
            Method::GET,
            &["labels", id],
            &[],
            None::<&()>,
            true,
            "users.labels.get",
        )
        .await
    }

    /// `labels.create`.
    pub async fn create_label(&self, settings: &GmailLabelSettings) -> Result<Label, GmailError> {
        self.json(
            Method::POST,
            &["labels"],
            &[],
            Some(&label_body(settings)),
            false,
            "users.labels.create",
        )
        .await
    }

    /// `labels.patch`: name, colour and visibility of a user label.
    pub async fn patch_label(
        &self,
        id: &str,
        settings: &GmailLabelSettings,
    ) -> Result<Label, GmailError> {
        let mut body = serde_json::to_value(label_body(settings))
            .map_err(|e| GmailError::Protocol(e.to_string()))?;
        if settings.color.is_none() {
            // Patch semantics: an explicit null clears the colour.
            body["color"] = serde_json::Value::Null;
        }
        self.json(
            Method::PATCH,
            &["labels", id],
            &[],
            Some(&body),
            true,
            "users.labels.patch",
        )
        .await
    }

    /// `labels.delete`: removes a user label (and it from every message).
    pub async fn delete_label(&self, id: &str) -> Result<(), GmailError> {
        self.call(
            Method::DELETE,
            &["labels", id],
            &[],
            None::<&()>,
            true,
            "users.labels.delete",
        )
        .await
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded() -> Vec<Label> {
        let page: ListLabelsResponse =
            serde_json::from_str(include_str!("../fixtures/responses/labels-list.json")).unwrap();
        page.labels
    }

    #[test]
    fn recorded_labels_map_to_nail() {
        let labels: Vec<GmailLabelInfo> = recorded().iter().map(label_info).collect();
        let inbox = labels.iter().find(|l| l.id == "INBOX").unwrap();
        assert_eq!(inbox.kind, GmailLabelKind::System);
        assert_eq!(inbox.folder, Some(GmailFolder::Inbox));
        assert_eq!(inbox.tag(), None);
        let unread = labels.iter().find(|l| l.id == "UNREAD").unwrap();
        assert_eq!(unread.folder, None);
        let work = labels.iter().find(|l| l.id == "Label_7").unwrap();
        assert_eq!(work.kind, GmailLabelKind::User);
        assert_eq!(work.tag(), Some("Work/Projects"));
        assert_eq!(
            work.settings.color,
            Some(GmailLabelColor {
                background: "#fb4c2f".into(),
                text: "#ffffff".into()
            })
        );
        let receipts = labels.iter().find(|l| l.id == "Label_12").unwrap();
        assert_eq!(
            receipts.settings.list_visibility,
            GmailLabelListVisibility::ShowIfUnread
        );
        assert_eq!(
            receipts.settings.message_visibility,
            GmailMessageListVisibility::Hide
        );

        let table = LabelTable::new(labels);
        assert_eq!(table.user_label_for_tag("Work/Projects"), Some("Label_7"));
        assert_eq!(table.user_label_for_tag("receipts"), Some("Label_12"));
        assert_eq!(table.user_label_for_tag("INBOX"), None);
        assert_eq!(
            table.tags_for(&[
                "INBOX".into(),
                "Label_12".into(),
                "Label_7".into(),
                "Label_99".into()
            ]),
            ["Receipts", "Work/Projects"]
        );
        assert!(table.has_unknown(&["Label_99".into()]));
        assert!(!table.has_unknown(&["INBOX".into(), "YELLOW_STAR".into(), "Label_7".into()]));
    }

    #[test]
    fn settings_round_trip_through_the_wire() {
        let settings = GmailLabelSettings {
            name: " Travel ".into(),
            color: Some(GmailLabelColor {
                background: "#16A766".into(),
                text: "#FFFFFF".into(),
            }),
            list_visibility: GmailLabelListVisibility::Hide,
            message_visibility: GmailMessageListVisibility::Hide,
        };
        let mut body = label_body(&settings);
        assert_eq!(body.name, "Travel");
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["labelListVisibility"], "labelHide");
        assert_eq!(json["color"]["backgroundColor"], "#16a766");
        assert!(json.get("id").is_none());
        body.id = "Label_3".into();
        body.kind = Some("user".into());
        let info = label_info(&body);
        assert_eq!(info.settings.name, "Travel");
        assert_eq!(
            info.settings.list_visibility,
            GmailLabelListVisibility::Hide
        );
        assert_eq!(info.settings.color.unwrap().background, "#16a766");
    }

    #[test]
    fn palette() {
        let mut seen = std::collections::HashSet::new();
        for color in LABEL_PALETTE {
            assert!(seen.insert(color), "{color} twice");
            assert_eq!(color.len(), 7);
            assert_eq!(color, color.to_ascii_lowercase());
        }
        assert!(is_palette_color("#FB4C2F"));
        assert!(!is_palette_color("#123456"));
    }

    #[test]
    fn system_ids() {
        for id in [
            "INBOX",
            "CATEGORY_SOCIAL",
            "YELLOW_STAR",
            "CHAT",
            "BLUE_INFO",
        ] {
            assert!(is_system_id(id), "{id}");
        }
        for id in ["Label_1", "Work", "label_x"] {
            assert!(!is_system_id(id), "{id}");
        }
    }
}
