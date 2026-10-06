//! Address books, contacts and groups (vCard 3.0 RFC 2426 / 4.0 RFC 6350,
//! CardDAV RFC 6352).
//!
//! A contact keeps the vCard version it came in and is written back in
//! it; unknown properties survive edits (`nail-ical` keeps the original
//! card and patches only what changed). Groups are vCard 4 `KIND:group`
//! cards with `MEMBER`s, or Apple-style `X-ADDRESSBOOKSERVER-KIND:group`
//! cards in vCard 3 address books.
//!
//! **Merging** ([`crate::Request::MergeContacts`]): contacts in the same
//! address book as the primary one are folded into it (their cards are
//! deleted); contacts in other address books stay where they are and are
//! **linked** to it, so lists show the person once while every source
//! keeps its card ([`Contact::linked_ids`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::source::PimItemSync;
use super::{MAX_LIST_ENTRIES, PimIssue, has_url_scheme, is_plausible_email, push_issue};
use crate::ids::{AccountId, AddressBookId, ContactGroupId, ContactId, PimSourceId};

/// Well-known `TYPE` values of contact entries (lowercase). Others are
/// kept as written; [`LabeledValue::label`] holds custom labels.
pub const CONTACT_ENTRY_KINDS: &[&str] = &[
    "home",
    "work",
    "cell",
    "voice",
    "fax",
    "text",
    "video",
    "pager",
    "textphone",
    "main",
    "other",
];

/// A special role of an address book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AddressBookRole {
    /// The local "Collected" book of auto-collected recipients (kept apart
    /// so they never pollute the real books; used for autocomplete).
    Collected,
}

/// The user-editable settings of an address book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AddressBookSettings {
    /// Display name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: Option<String>,
    /// Shown in contact lists (hidden books still sync and autocomplete).
    #[serde(default = "yes")]
    pub visible: bool,
    /// Used for address autocomplete in the composer.
    #[serde(default = "yes")]
    pub autocomplete: bool,
    /// Position in lists (ascending).
    #[serde(default)]
    pub order: i32,
}

fn yes() -> bool {
    true
}

impl AddressBookSettings {
    /// Settings with a name and defaults for the rest.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            visible: true,
            autocomplete: true,
            order: 0,
        }
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<PimIssue> {
        if self.name.trim().is_empty() {
            vec![PimIssue::MissingName]
        } else {
            Vec::new()
        }
    }
}

/// An address book of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AddressBook {
    /// The address book.
    pub id: AddressBookId,
    /// Its source.
    pub source_id: PimSourceId,
    /// The mail account the source belongs to, if any.
    pub account_id: Option<AccountId>,
    /// The user's settings.
    pub settings: AddressBookSettings,
    /// Read-only (shared books without write access).
    pub read_only: bool,
    /// A special role, if any.
    pub role: Option<AddressBookRole>,
    /// Exchange data (M3c: the shared / delegate mailbox, a special
    /// role); absent for other sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exchange: Option<crate::exchange::ExchangeCollectionInfo>,
}

/// vCard version of a contact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
pub enum VCardVersion {
    /// vCard 3.0.
    #[serde(rename = "3.0")]
    V3,
    /// vCard 4.0 (new local contacts).
    #[default]
    #[serde(rename = "4.0")]
    V4,
}

/// `KIND` of a contact (groups are [`ContactGroup`]s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ContactKind {
    /// A person (the default).
    #[default]
    Individual,
    /// An organization (`KIND:org`).
    Organization,
    /// A place (`KIND:location`).
    Location,
}

/// The structured name (`N`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ContactName {
    /// Honorific prefixes (`Dr.`).
    pub prefix: Option<String>,
    /// Given names.
    pub given: Option<String>,
    /// Additional (middle) names.
    pub additional: Option<String>,
    /// Family names.
    pub family: Option<String>,
    /// Honorific suffixes (`Jr.`).
    pub suffix: Option<String>,
}

impl ContactName {
    /// Whether every part is empty.
    pub fn is_empty(&self) -> bool {
        [
            &self.prefix,
            &self.given,
            &self.additional,
            &self.family,
            &self.suffix,
        ]
        .iter()
        .all(|p| p.as_deref().is_none_or(|s| s.trim().is_empty()))
    }
}

/// A typed entry: email, phone, URL, instant-messaging handle, related
/// person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LabeledValue {
    /// The value (an address, number, URL, …).
    pub value: String,
    /// `TYPE` values, lowercase (see [`CONTACT_ENTRY_KINDS`]).
    #[serde(default)]
    pub kinds: Vec<String>,
    /// A custom label (`X-ABLabel`), if any.
    #[serde(default)]
    pub label: Option<String>,
    /// Preferred (`PREF=1` / `TYPE=pref`).
    #[serde(default)]
    pub preferred: bool,
}

impl LabeledValue {
    /// An entry without kinds or label.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            kinds: Vec::new(),
            label: None,
            preferred: false,
        }
    }
}

/// A postal address (`ADR`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct PostalAddress {
    /// `TYPE` values (`home`, `work`, …).
    pub kinds: Vec<String>,
    /// Custom label.
    pub label: Option<String>,
    /// Preferred.
    pub preferred: bool,
    /// Post office box.
    pub po_box: Option<String>,
    /// Extended address (apartment, suite).
    pub extended: Option<String>,
    /// Street.
    pub street: Option<String>,
    /// City.
    pub locality: Option<String>,
    /// Region / state.
    pub region: Option<String>,
    /// Postal code.
    pub postal_code: Option<String>,
    /// Country.
    pub country: Option<String>,
}

/// Organization (`ORG`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactOrg {
    /// Name.
    pub name: String,
    /// Units, outermost first.
    #[serde(default)]
    pub units: Vec<String>,
}

/// A date that may lack its year (vCard 4 `--MMDD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct ContactDate {
    /// Year, if known.
    #[serde(default)]
    pub year: Option<i32>,
    /// Month (1–12).
    pub month: u8,
    /// Day (1–31).
    pub day: u8,
}

impl ContactDate {
    /// Whether month and day exist (Feb 29 is allowed without a year).
    pub fn is_valid(&self) -> bool {
        let max = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => match self.year {
                Some(y) if !((y % 4 == 0 && y % 100 != 0) || y % 400 == 0) => 28,
                _ => 29,
            },
            _ => 0,
        };
        self.day >= 1 && self.day <= max
    }
}

/// A contact's photo (`PHOTO`). Reading: `sha256` (the image in the blob
/// store, fetched with [`crate::Request::GetContactPhoto`]) or `url`.
/// Writing: `data` (base64) sets a new image, an unchanged `sha256` keeps
/// it, `null` for the whole photo removes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactPhoto {
    /// MIME type (`image/jpeg`, `image/png`).
    #[serde(default)]
    pub mime_type: Option<String>,
    /// SHA-256 (hex) of the stored image.
    #[serde(default)]
    pub sha256: Option<String>,
    /// A linked image (not fetched by Nail without the user's consent).
    #[serde(default)]
    pub url: Option<String>,
    /// New image data (base64), in write requests only.
    #[serde(default)]
    pub data: Option<String>,
}

/// Largest contact photo Nail stores, bytes (decoded).
pub const MAX_CONTACT_PHOTO_BYTES: usize = 1024 * 1024;

/// A custom field: an `X-` property or a labelled value Nail has no field
/// for. Kept and shown; editable as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CustomField {
    /// Property name as in the card (`X-SPOUSE`), or a label the user chose.
    pub name: String,
    /// Value.
    pub value: String,
}

/// The editable fields of a contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactFields {
    /// `KIND`.
    #[serde(default)]
    pub kind: ContactKind,
    /// `FN`: the name shown. Required (derived from the structured name,
    /// organization or first email when a card lacks it).
    pub display_name: String,
    /// `N`.
    #[serde(default)]
    pub name: Option<ContactName>,
    /// `NICKNAME`.
    #[serde(default)]
    pub nicknames: Vec<String>,
    /// `EMAIL`.
    #[serde(default)]
    pub emails: Vec<LabeledValue>,
    /// `TEL`.
    #[serde(default)]
    pub phones: Vec<LabeledValue>,
    /// `ADR`.
    #[serde(default)]
    pub addresses: Vec<PostalAddress>,
    /// `URL`.
    #[serde(default)]
    pub urls: Vec<LabeledValue>,
    /// `IMPP`.
    #[serde(default)]
    pub impp: Vec<LabeledValue>,
    /// `ORG`.
    #[serde(default)]
    pub organization: Option<ContactOrg>,
    /// `TITLE`.
    #[serde(default)]
    pub title: Option<String>,
    /// `ROLE`.
    #[serde(default)]
    pub role: Option<String>,
    /// `BDAY`.
    #[serde(default)]
    pub birthday: Option<ContactDate>,
    /// `ANNIVERSARY` (vCard 4; `X-ANNIVERSARY` in 3.0).
    #[serde(default)]
    pub anniversary: Option<ContactDate>,
    /// `NOTE`.
    #[serde(default)]
    pub notes: Option<String>,
    /// `CATEGORIES`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// `PHOTO`.
    #[serde(default)]
    pub photo: Option<ContactPhoto>,
    /// `RELATED` (vCard 4) / `X-ABRELATEDNAMES`.
    #[serde(default)]
    pub related: Vec<LabeledValue>,
    /// Custom fields.
    #[serde(default)]
    pub custom: Vec<CustomField>,
}

impl ContactFields {
    /// A person with a display name.
    pub fn named(display_name: impl Into<String>) -> Self {
        Self {
            kind: ContactKind::Individual,
            display_name: display_name.into(),
            name: None,
            nicknames: Vec::new(),
            emails: Vec::new(),
            phones: Vec::new(),
            addresses: Vec::new(),
            urls: Vec::new(),
            impp: Vec::new(),
            organization: None,
            title: None,
            role: None,
            birthday: None,
            anniversary: None,
            notes: None,
            categories: Vec::new(),
            photo: None,
            related: Vec::new(),
            custom: Vec::new(),
        }
    }

    /// The preferred email address (else the first).
    pub fn primary_email(&self) -> Option<&str> {
        self.emails
            .iter()
            .find(|e| e.preferred)
            .or_else(|| self.emails.first())
            .map(|e| e.value.as_str())
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<PimIssue> {
        let mut out = Vec::new();
        if self.display_name.trim().is_empty() {
            push_issue(&mut out, PimIssue::MissingName);
        }
        if self.emails.iter().any(|e| !is_plausible_email(&e.value)) {
            push_issue(&mut out, PimIssue::InvalidEmail);
        }
        if self
            .birthday
            .iter()
            .chain(&self.anniversary)
            .any(|d| !d.is_valid())
        {
            push_issue(&mut out, PimIssue::InvalidDate);
        }
        if self
            .urls
            .iter()
            .any(|u| !has_url_scheme(&u.value, &["http", "https"]))
        {
            push_issue(&mut out, PimIssue::InvalidUrl);
        }
        let lists = [
            self.nicknames.len(),
            self.emails.len(),
            self.phones.len(),
            self.addresses.len(),
            self.urls.len(),
            self.impp.len(),
            self.categories.len(),
            self.related.len(),
            self.custom.len(),
        ];
        if lists.into_iter().any(|n| n > MAX_LIST_ENTRIES) {
            push_issue(&mut out, PimIssue::TooMany);
        }
        out
    }
}

/// A contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Contact {
    /// The contact.
    pub id: ContactId,
    /// Its address book.
    pub address_book_id: AddressBookId,
    /// vCard `UID`.
    pub uid: String,
    /// The fields.
    pub fields: ContactFields,
    /// The card's version (kept when writing).
    pub version: VCardVersion,
    /// Groups it belongs to.
    pub group_ids: Vec<ContactGroupId>,
    /// Contacts in other address books linked to it by a merge (the same
    /// person); lists show the person once.
    pub linked_ids: Vec<ContactId>,
    /// `REV` (RFC 3339 UTC), if known.
    pub updated_at: Option<String>,
    /// Read-only (its address book is).
    pub read_only: bool,
    /// Sync state.
    pub sync: PimItemSync,
}

/// A contact in a list ([`crate::Request::ListContacts`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactSummary {
    /// The contact.
    pub id: ContactId,
    /// Its address book.
    pub address_book_id: AddressBookId,
    /// `FN`.
    pub display_name: String,
    /// Sort key (family name first where known), lowercase.
    pub sort_name: String,
    /// Kind.
    pub kind: ContactKind,
    /// The primary email address.
    pub email: Option<String>,
    /// Every email address.
    pub emails: Vec<String>,
    /// The first phone number.
    pub phone: Option<String>,
    /// Organization name.
    pub organization: Option<String>,
    /// Whether it has a stored photo.
    pub has_photo: bool,
    /// Groups it belongs to.
    pub group_ids: Vec<ContactGroupId>,
    /// Linked contacts in other books.
    pub linked_ids: Vec<ContactId>,
    /// Read-only.
    pub read_only: bool,
}

/// A contact group / distribution list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactGroup {
    /// The group.
    pub id: ContactGroupId,
    /// Its address book (members come from the same book).
    pub address_book_id: AddressBookId,
    /// vCard `UID` of the group card.
    pub uid: String,
    /// Name.
    pub name: String,
    /// Member contacts.
    pub member_ids: Vec<ContactId>,
    /// Members that are only an address (`MEMBER:mailto:`), not a card.
    pub member_emails: Vec<String>,
    /// Read-only.
    pub read_only: bool,
    /// Sync state.
    pub sync: PimItemSync,
}

/// Which field a contact search matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ContactMatchField {
    /// A name (display, structured, nickname).
    Name,
    /// An email address.
    Email,
    /// A phone number.
    Phone,
    /// The organization.
    Organization,
    /// Something else (notes, address, custom field).
    Other,
}

/// A hit of [`crate::Request::SearchContacts`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactMatch {
    /// The contact.
    pub contact: ContactSummary,
    /// What matched.
    pub field: ContactMatchField,
    /// The matching value (e.g. the email address), if any.
    pub value: Option<String>,
}

/// Why contacts look like duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DuplicateReason {
    /// A shared email address.
    Email,
    /// A shared phone number (normalized).
    Phone,
    /// The same name.
    Name,
}

/// Contacts that look like the same person
/// ([`crate::Request::FindDuplicateContacts`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DuplicateGroup {
    /// The contacts (two or more; already linked ones are left out).
    pub contact_ids: Vec<ContactId>,
    /// Why, strongest first.
    pub reasons: Vec<DuplicateReason>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn contact_fields_defaults_and_issues() {
        let fields: ContactFields =
            serde_json::from_value(json!({"display_name": "Ada Lovelace"})).unwrap();
        assert_eq!(fields, ContactFields::named("Ada Lovelace"));
        assert_eq!(fields.issues().len(), 0);
        let mut f = fields;
        f.emails = vec![
            LabeledValue::new("ada@work.example"),
            LabeledValue {
                preferred: true,
                ..LabeledValue::new("ada@home.example")
            },
        ];
        assert_eq!(f.primary_email(), Some("ada@home.example"));
        f.emails.push(LabeledValue::new("nope"));
        f.birthday = Some(ContactDate {
            year: Some(2025),
            month: 2,
            day: 29,
        });
        f.display_name = " ".into();
        assert_eq!(
            f.issues(),
            [
                PimIssue::MissingName,
                PimIssue::InvalidEmail,
                PimIssue::InvalidDate
            ]
        );
    }

    #[test]
    fn contact_dates() {
        let no_year = ContactDate {
            year: None,
            month: 2,
            day: 29,
        };
        assert!(no_year.is_valid());
        assert!(
            !ContactDate {
                month: 4,
                day: 31,
                ..no_year
            }
            .is_valid()
        );
        assert!(
            !ContactDate {
                month: 13,
                day: 1,
                ..no_year
            }
            .is_valid()
        );
        assert!(
            ContactDate {
                year: Some(2000),
                ..no_year
            }
            .is_valid()
        );
    }

    #[test]
    fn wire_shapes() {
        assert_eq!(
            serde_json::to_value(VCardVersion::V3).unwrap(),
            json!("3.0")
        );
        assert_eq!(
            serde_json::to_value(ContactKind::Organization).unwrap(),
            json!("organization")
        );
        assert_eq!(
            serde_json::to_value(AddressBookRole::Collected).unwrap(),
            json!("collected")
        );
        let settings: AddressBookSettings =
            serde_json::from_value(json!({"name": "Friends"})).unwrap();
        assert_eq!(settings, AddressBookSettings::named("Friends"));
        assert_eq!(
            AddressBookSettings::named("").issues(),
            [PimIssue::MissingName]
        );
        let address: PostalAddress =
            serde_json::from_value(json!({"kinds": ["home"], "locality": "London"})).unwrap();
        assert_eq!(address.locality.as_deref(), Some("London"));
        assert!(ContactName::default().is_empty());
        assert!(
            !ContactName {
                family: Some("Lovelace".into()),
                ..ContactName::default()
            }
            .is_empty()
        );
        let group = DuplicateGroup {
            contact_ids: vec!["c1".into(), "c2".into()],
            reasons: vec![DuplicateReason::Email],
        };
        assert_eq!(
            serde_json::to_value(&group).unwrap(),
            json!({"contact_ids": ["c1", "c2"], "reasons": ["email"]})
        );
    }
}
