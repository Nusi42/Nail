//! Shared EWS request pieces: item and folder ids (with the change key,
//! distinguished folders of other mailboxes), field paths, and times.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use nail_api::exchange::ExchangeWellKnownFolder;

use crate::xml::{Element, escape};

/// An item id with its version (`t:ItemId Id ChangeKey`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ItemRef {
    /// The id.
    pub id: String,
    /// The change key, if known.
    pub change_key: Option<String>,
}

impl ItemRef {
    /// An id without change key.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            change_key: None,
        }
    }

    /// An id with a change key.
    pub fn with_key(id: impl Into<String>, change_key: Option<String>) -> Self {
        Self {
            id: id.into(),
            change_key,
        }
    }

    /// Reads `<t:ItemId Id=… ChangeKey=…/>` (or any element with those
    /// attributes).
    pub fn from_element(element: &Element) -> Option<Self> {
        Some(Self {
            id: element.attr("Id")?.to_owned(),
            change_key: element.attr("ChangeKey").map(str::to_owned),
        })
    }

    /// `<t:{tag} Id=… ChangeKey=…/>`.
    pub fn xml_as(&self, tag: &str) -> String {
        match &self.change_key {
            Some(key) => format!(
                r#"<t:{tag} Id="{}" ChangeKey="{}"/>"#,
                escape(&self.id),
                escape(key)
            ),
            None => format!(r#"<t:{tag} Id="{}"/>"#, escape(&self.id)),
        }
    }

    /// `<t:ItemId …/>`.
    pub fn xml(&self) -> String {
        self.xml_as("ItemId")
    }
}

/// A folder as a request names it: by id, or a distinguished folder of
/// the own or another mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRef<'a> {
    /// A folder id.
    Id(&'a str),
    /// A distinguished folder (`inbox`, `msgfolderroot`, …), of `mailbox`
    /// (`None` = own).
    Distinguished {
        /// The EWS name.
        name: &'a str,
        /// Another mailbox.
        mailbox: Option<&'a str>,
    },
}

impl<'a> FolderRef<'a> {
    /// The folder a caller names: a distinguished folder name
    /// (`inbox`, `calendar`, `msgfolderroot`) of `mailbox`, else a folder
    /// id.
    pub fn named(mailbox: Option<&'a str>, folder: &'a str) -> Self {
        let distinguished = ExchangeWellKnownFolder::parse(folder)
            .and_then(ExchangeWellKnownFolder::ews_name)
            .as_ref()
            .is_some_and(|n| n.eq_ignore_ascii_case(folder))
            || matches!(
                folder,
                "root" | "publicfoldersroot" | "archivemsgfolderroot" | "searchfolders"
            );
        if distinguished {
            Self::Distinguished {
                name: folder,
                mailbox,
            }
        } else {
            Self::Id(folder)
        }
    }

    /// A distinguished folder.
    pub fn distinguished(name: &'a str, mailbox: Option<&'a str>) -> Self {
        Self::Distinguished { name, mailbox }
    }

    /// `<t:FolderId Id=…/>` or `<t:DistinguishedFolderId Id=…>` with the
    /// mailbox.
    pub fn xml(&self) -> String {
        match self {
            Self::Id(id) => format!(r#"<t:FolderId Id="{}"/>"#, escape(id)),
            Self::Distinguished {
                name,
                mailbox: None,
            } => format!(r#"<t:DistinguishedFolderId Id="{}"/>"#, escape(name)),
            Self::Distinguished {
                name,
                mailbox: Some(mailbox),
            } => format!(
                r#"<t:DistinguishedFolderId Id="{}"><t:Mailbox><t:EmailAddress>{}</t:EmailAddress></t:Mailbox></t:DistinguishedFolderId>"#,
                escape(name),
                escape(mailbox)
            ),
        }
    }
}

/// A property path (`t:FieldURI`, `t:IndexedFieldURI`,
/// `t:ExtendedFieldURI`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// `<t:FieldURI FieldURI="item:Subject"/>`.
    Uri(&'static str),
    /// `<t:IndexedFieldURI FieldURI="contacts:EmailAddress" FieldIndex="EmailAddress1"/>`.
    Indexed(&'static str, String),
    /// `<t:ExtendedFieldURI PropertyTag="0x1081" PropertyType="Integer"/>`.
    Extended {
        /// The MAPI property tag.
        tag: &'static str,
        /// Its type.
        kind: &'static str,
    },
}

impl Field {
    /// The path element.
    pub fn xml(&self) -> String {
        match self {
            Self::Uri(uri) => format!(r#"<t:FieldURI FieldURI="{uri}"/>"#),
            Self::Indexed(uri, index) => format!(
                r#"<t:IndexedFieldURI FieldURI="{uri}" FieldIndex="{}"/>"#,
                escape(index)
            ),
            Self::Extended { tag, kind } => {
                format!(r#"<t:ExtendedFieldURI PropertyTag="{tag}" PropertyType="{kind}"/>"#)
            }
        }
    }
}

/// `PR_LAST_VERB_EXECUTED`: 102 reply, 103 reply all, 104 forward.
pub const LAST_VERB: Field = Field::Extended {
    tag: "0x1081",
    kind: "Integer",
};

/// `PR_MESSAGE_FLAGS`: bit 1 = read; without bit 8 (unsent) an appended
/// message is not a draft.
pub const MESSAGE_FLAGS: Field = Field::Extended {
    tag: "0x0E07",
    kind: "Integer",
};

/// One change of an `UpdateItem` / `UpdateFolder`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Set a field: the path and the item element's content
    /// (`<t:IsRead>true</t:IsRead>`).
    Set(Field, String),
    /// Remove a field.
    Delete(Field),
}

impl Update {
    /// The update element inside an item of type `item_tag` (`Message`,
    /// `CalendarItem`, …).
    pub fn xml(&self, item_tag: &str) -> String {
        match self {
            Self::Set(field, content) => format!(
                "<t:SetItemField>{}<t:{item_tag}>{content}</t:{item_tag}></t:SetItemField>",
                field.xml()
            ),
            Self::Delete(field) => {
                format!("<t:DeleteItemField>{}</t:DeleteItemField>", field.xml())
            }
        }
    }
}

/// An extended property value element.
pub fn extended_value(field: &Field, value: &str) -> String {
    format!(
        "<t:ExtendedProperty>{}<t:Value>{}</t:Value></t:ExtendedProperty>",
        field.xml(),
        escape(value)
    )
}

/// The value of an extended property of an item, by property tag
/// (`0x1081`; compared case-insensitively, decimal tags too).
pub fn extended_property<'a>(item: &'a Element, tag: &str) -> Option<&'a str> {
    let want = parse_tag(tag)?;
    item.children_named("ExtendedProperty").find_map(|p| {
        let uri = p.child("ExtendedFieldURI")?;
        (parse_tag(uri.attr("PropertyTag")?)? == want).then(|| p.child_text("Value"))?
    })
}

fn parse_tag(tag: &str) -> Option<u32> {
    match tag.strip_prefix("0x").or_else(|| tag.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => tag.parse().ok(),
    }
}

/// Parses an EWS `xs:dateTime` (`Z`, an offset, or none = UTC).
pub fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(text) {
        return Some(t.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(text, format) {
            return Some(t.and_utc());
        }
    }
    None
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
pub fn format_utc(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// An RFC 3339 time as `YYYY-MM-DDTHH:MM:SSZ` (UTC).
pub fn normalize_utc(text: &str) -> Option<String> {
    parse_time(text).map(format_utc)
}

/// The date part of an EWS `xs:date` (`2026-10-05`, `2026-10-05Z`,
/// `2026-10-05+02:00`).
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim().get(..10)?, "%Y-%m-%d").ok()
}

/// Unix seconds of a time.
pub fn unix(text: &str) -> Option<i64> {
    parse_time(text).map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;

    #[test]
    fn ids_and_folders() {
        let item = ItemRef::with_key("AA=", Some("CQ".into()));
        assert_eq!(item.xml(), r#"<t:ItemId Id="AA=" ChangeKey="CQ"/>"#);
        assert_eq!(ItemRef::new("A&B").xml(), r#"<t:ItemId Id="A&amp;B"/>"#);
        let parsed = ItemRef::from_element(&parse(r#"<ItemId Id="x" ChangeKey="y"/>"#).unwrap());
        assert_eq!(parsed, Some(ItemRef::with_key("x", Some("y".into()))));
        assert_eq!(
            FolderRef::named(None, "inbox"),
            FolderRef::distinguished("inbox", None)
        );
        assert_eq!(FolderRef::named(None, "AAMkAD="), FolderRef::Id("AAMkAD="));
        assert_eq!(
            FolderRef::named(Some("shared@corp.example"), "msgfolderroot").xml(),
            r#"<t:DistinguishedFolderId Id="msgfolderroot"><t:Mailbox><t:EmailAddress>shared@corp.example</t:EmailAddress></t:Mailbox></t:DistinguishedFolderId>"#
        );
        assert_eq!(FolderRef::Id("F1").xml(), r#"<t:FolderId Id="F1"/>"#);
    }

    #[test]
    fn fields_and_updates() {
        assert_eq!(
            Field::Uri("message:IsRead").xml(),
            r#"<t:FieldURI FieldURI="message:IsRead"/>"#
        );
        assert_eq!(
            Field::Indexed("contacts:EmailAddress", "EmailAddress1".into()).xml(),
            r#"<t:IndexedFieldURI FieldURI="contacts:EmailAddress" FieldIndex="EmailAddress1"/>"#
        );
        let set = Update::Set(
            Field::Uri("message:IsRead"),
            "<t:IsRead>true</t:IsRead>".into(),
        );
        assert_eq!(
            set.xml("Message"),
            r#"<t:SetItemField><t:FieldURI FieldURI="message:IsRead"/><t:Message><t:IsRead>true</t:IsRead></t:Message></t:SetItemField>"#
        );
        assert!(Update::Delete(LAST_VERB).xml("Message").contains("0x1081"));
        let item = parse(concat!(
            r#"<Message><ExtendedProperty><ExtendedFieldURI PropertyTag="0x1081" PropertyType="Integer"/>"#,
            r#"<Value>102</Value></ExtendedProperty></Message>"#
        ))
        .unwrap();
        assert_eq!(extended_property(&item, "0x1081"), Some("102"));
        assert_eq!(extended_property(&item, "4225"), Some("102"));
        assert_eq!(extended_property(&item, "0x0E07"), None);
        assert!(extended_value(&MESSAGE_FLAGS, "1").contains("<t:Value>1</t:Value>"));
    }

    #[test]
    fn times() {
        assert_eq!(
            normalize_utc("2026-10-05T09:00:00+02:00").as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(
            normalize_utc("2026-10-05T07:00:00.123Z").as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(
            normalize_utc("2026-10-05T07:00:00").as_deref(),
            Some("2026-10-05T07:00:00Z")
        );
        assert_eq!(unix("1970-01-01T00:01:00Z"), Some(60));
        assert_eq!(
            parse_date("2026-10-05+02:00"),
            NaiveDate::from_ymd_opt(2026, 10, 5)
        );
        assert_eq!(parse_time("soon"), None);
    }
}
