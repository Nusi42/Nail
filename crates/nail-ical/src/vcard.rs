//! vCard 2.1 / 3.0 (RFC 2426) / 4.0 (RFC 6350) ↔ the contract's contacts
//! and groups.
//!
//! Reading ([`parse_cards`]) understands the dialects of the common
//! producers: Apple's grouped labels (`item1.EMAIL` + `item1.X-ABLabel`,
//! `_$!<Mobile>!$_`), Apple groups (`X-ADDRESSBOOKSERVER-KIND:group`),
//! vCard 4 groups (`KIND:group` + `MEMBER`), Google's `TYPE=INTERNET`,
//! Outlook's vCard 2.1 (bare types, quoted-printable, `CHARSET=`),
//! inline photos (vCard 4 `data:` URIs, vCard 3 `ENCODING=b`, 2.1
//! `BASE64`) and birthdays without a year (`--0412`,
//! `X-APPLE-OMIT-YEAR`).
//!
//! Writing ([`write_contact`], [`write_group`]) patches the stored card
//! like [`crate::write`] does for events: unchanged fields keep their
//! original lines, unknown properties stay, and the card keeps its
//! version (a vCard 2.1 card is written as 3.0 once edited).

use std::collections::{HashMap, HashSet};

use base64::Engine as _;
use chrono::{DateTime, Utc};
use nail_api::pim::contact::{
    ContactDate, ContactKind, ContactName, ContactOrg, ContactPhoto, CustomField,
    MAX_CONTACT_PHOTO_BYTES, VCardVersion,
};
use nail_api::pim::{Contact, ContactFields, LabeledValue, PostalAddress};

use crate::content::{self, Component, Param, Property, escape_text};
use crate::ical::sha256_hex;
use crate::issues::{Issue, IssueKind};
use crate::time;

/// Decoded photo data, for the blob store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoData {
    /// MIME type (`image/jpeg`), if known.
    pub mime_type: Option<String>,
    /// The image.
    pub bytes: Vec<u8>,
    /// SHA-256 (hex) of the bytes.
    pub sha256: String,
}

/// A contact read from vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedContact {
    /// `UID` (derived from the content when missing).
    pub uid: String,
    /// The fields.
    pub fields: ContactFields,
    /// The version the card is kept in (2.1 counts as 3.0).
    pub version: VCardVersion,
    /// An inline photo's data (`fields.photo.sha256` names it).
    pub photo: Option<PhotoData>,
    /// `REV` (RFC 3339 UTC).
    pub updated_at: Option<String>,
    /// The card's own text.
    pub raw: String,
    /// 1-based line of `BEGIN:VCARD`.
    pub line: u32,
}

/// How a group is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupStyle {
    /// vCard 4 `KIND:group` + `MEMBER`.
    #[default]
    Kind,
    /// Apple: `X-ADDRESSBOOKSERVER-KIND:group` +
    /// `X-ADDRESSBOOKSERVER-MEMBER` (vCard 3 books).
    Apple,
}

/// A contact group card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCard {
    /// `UID`.
    pub uid: String,
    /// Name (`FN`).
    pub name: String,
    /// Members' `UID`s as [`uid_key`]s.
    pub member_uids: Vec<String>,
    /// Members that are only an address (`MEMBER:mailto:`).
    pub member_emails: Vec<String>,
    /// Version.
    pub version: VCardVersion,
    /// Style.
    pub style: GroupStyle,
}

/// A group read from vCard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGroup {
    /// The group.
    pub group: GroupCard,
    /// The card's own text.
    pub raw: String,
    /// 1-based line.
    pub line: u32,
}

/// Cards read from vCard data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCards {
    /// Contacts.
    pub contacts: Vec<ParsedContact>,
    /// Groups.
    pub groups: Vec<ParsedGroup>,
    /// Problems.
    pub issues: Vec<Issue>,
}

/// The comparable key of a `UID` / `MEMBER` value: without `urn:uuid:`,
/// lower case (`urn:uuid:ABC` and `abc` are the same contact).
pub fn uid_key(uid: &str) -> String {
    let uid = uid.trim();
    let bare = match uid.get(..9) {
        Some(prefix) if prefix.eq_ignore_ascii_case("urn:uuid:") => &uid[9..],
        _ => uid,
    };
    bare.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Values

/// The decoded text of a property: quoted-printable and `CHARSET=`
/// (vCard 2.1 / 3.0) undone, then unescaped.
fn decoded(p: &Property) -> Property {
    if p.param("ENCODING")
        .is_some_and(|e| e.eq_ignore_ascii_case("QUOTED-PRINTABLE"))
    {
        let bytes = content::decode_quoted_printable(&p.value);
        let mut q = p.clone();
        // vCard 2.1 values are not escaped; only line breaks need it.
        q.value = content::decode_charset(&bytes, p.param("CHARSET"))
            .replace("\r\n", "\n")
            .replace('\n', "\\n");
        q.remove_param("ENCODING");
        q.remove_param("CHARSET");
        return q;
    }
    p.clone()
}

fn text_of(p: &Property) -> String {
    decoded(p).text()
}

fn non_empty(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else if trimmed.len() == text.len() {
        Some(text)
    } else {
        Some(trimmed.to_owned())
    }
}

fn card_text(c: &Component, name: &str) -> Option<String> {
    c.property(name).map(text_of).and_then(non_empty)
}

fn is_v4(c: &Component) -> bool {
    c.property("VERSION")
        .is_some_and(|v| v.value.trim().starts_with('4'))
}

fn version_of(c: &Component) -> VCardVersion {
    if is_v4(c) {
        VCardVersion::V4
    } else {
        VCardVersion::V3
    }
}

/// Apple's built-in labels (`_$!<Mobile>!$_`) → the kind they mean.
fn apple_label(label: &str) -> Option<String> {
    let inner = label.strip_prefix("_$!<")?.strip_suffix(">!$_")?;
    Some(match inner.to_ascii_lowercase().as_str() {
        "mobile" | "iphone" => "cell".to_owned(),
        "homepage" => "home".to_owned(),
        "homefax" | "workfax" | "otherfax" => "fax".to_owned(),
        other => other.to_owned(),
    })
}

/// The `X-ABLabel` of a property's group.
fn group_label<'a>(c: &'a Component, p: &Property) -> Option<&'a Property> {
    let group = p.group.as_deref()?;
    c.properties.iter().find(|l| {
        l.is("X-ABLABEL")
            && l.group
                .as_deref()
                .is_some_and(|g| g.eq_ignore_ascii_case(group))
    })
}

const IGNORED_KINDS: &[&str] = &["internet", "pref", "x400", "x-internet"];

fn labeled(c: &Component, p: &Property, value: String) -> LabeledValue {
    let mut kinds: Vec<String> = p
        .param_values("TYPE")
        .flat_map(|t| t.split(','))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty() && !IGNORED_KINDS.contains(&t.as_str()))
        .collect();
    let preferred =
        p.has_param_value("TYPE", "pref") || p.param("PREF").is_some_and(|v| v.trim() == "1");
    let mut label = None;
    if let Some(l) = group_label(c, p) {
        let text = text_of(l);
        match apple_label(&text) {
            Some(kind) => kinds.push(kind),
            None => label = non_empty(text),
        }
    }
    let mut seen = Vec::new();
    kinds.retain(|k| {
        let fresh = !seen.contains(k);
        seen.push(k.clone());
        fresh
    });
    LabeledValue {
        value,
        kinds,
        label,
        preferred,
    }
}

fn strip_scheme(value: &str, scheme: &str) -> String {
    let value = value.trim();
    match value.get(..scheme.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(scheme) => value[scheme.len()..].to_owned(),
        _ => value.to_owned(),
    }
}

fn read_email(c: &Component, p: &Property) -> LabeledValue {
    labeled(c, p, strip_scheme(&text_of(p), "mailto:"))
}

fn read_phone(c: &Component, p: &Property) -> LabeledValue {
    labeled(c, p, strip_scheme(&text_of(p), "tel:"))
}

fn read_plain(c: &Component, p: &Property) -> LabeledValue {
    labeled(c, p, text_of(p).trim().to_owned())
}

fn part(parts: &[Vec<String>], index: usize) -> Option<String> {
    parts
        .get(index)
        .map(|values| values.join(" "))
        .and_then(non_empty)
}

fn read_address(c: &Component, p: &Property) -> PostalAddress {
    let parts = decoded(p).structured();
    let l = labeled(c, p, String::new());
    PostalAddress {
        kinds: l.kinds,
        label: l.label,
        preferred: l.preferred,
        po_box: part(&parts, 0),
        extended: part(&parts, 1),
        street: parts.get(2).map(|v| v.join("\n")).and_then(non_empty),
        locality: part(&parts, 3),
        region: part(&parts, 4),
        postal_code: part(&parts, 5),
        country: part(&parts, 6),
    }
}

fn read_name(c: &Component) -> Option<ContactName> {
    let parts = decoded(c.property("N")?).structured();
    let name = ContactName {
        family: part(&parts, 0),
        given: part(&parts, 1),
        additional: part(&parts, 2),
        prefix: part(&parts, 3),
        suffix: part(&parts, 4),
    };
    (!name.is_empty()).then_some(name)
}

fn read_org(c: &Component) -> Option<ContactOrg> {
    let parts = decoded(c.property("ORG")?).structured();
    let mut values = parts
        .into_iter()
        .map(|v| v.join(" "))
        .filter(|v| !v.trim().is_empty());
    let name = values.next()?;
    Some(ContactOrg {
        name,
        units: values.collect(),
    })
}

/// Parses `19850412`, `1985-04-12`, `--0412`, `--04-12`, `1985-04-12T…`;
/// Apple's `X-APPLE-OMIT-YEAR` drops the year.
fn read_date(p: &Property) -> Option<ContactDate> {
    let value = text_of(p);
    let value = value.trim();
    let date_part = value.split('T').next().unwrap_or(value);
    let (year, rest) = if let Some(rest) = date_part.strip_prefix("--") {
        (None, rest.replace('-', ""))
    } else {
        let digits = date_part.replace('-', "");
        if digits.len() != 8 {
            return None;
        }
        (
            Some(digits[..4].parse::<i32>().ok()?),
            digits[4..].to_owned(),
        )
    };
    if rest.len() != 4 {
        return None;
    }
    let month: u8 = rest[..2].parse().ok()?;
    let day: u8 = rest[2..].parse().ok()?;
    let omit = p
        .param("X-APPLE-OMIT-YEAR")
        .and_then(|y| y.parse::<i32>().ok());
    let year = year.filter(|y| Some(*y) != omit);
    let date = ContactDate { year, month, day };
    date.is_valid().then_some(date)
}

fn write_date(date: &ContactDate, version: VCardVersion, name: &str) -> Property {
    match (date.year, version) {
        (Some(y), VCardVersion::V4) => {
            Property::new(name, format!("{y:04}{:02}{:02}", date.month, date.day))
        }
        (Some(y), VCardVersion::V3) => {
            Property::new(name, format!("{y:04}-{:02}-{:02}", date.month, date.day))
        }
        (None, VCardVersion::V4) => {
            Property::new(name, format!("--{:02}{:02}", date.month, date.day))
        }
        (None, VCardVersion::V3) => {
            Property::new(name, format!("1604-{:02}-{:02}", date.month, date.day))
                .with_param("X-APPLE-OMIT-YEAR", "1604")
        }
    }
}

fn mime_of(p: &Property, data_uri_mime: Option<&str>) -> Option<String> {
    if let Some(mime) = data_uri_mime {
        return Some(mime.to_ascii_lowercase());
    }
    if let Some(media) = p.param("MEDIATYPE") {
        return Some(media.to_ascii_lowercase());
    }
    let kind = p
        .param_values("TYPE")
        .find(|t| !t.contains('/') || t.starts_with("image/"))?
        .to_ascii_lowercase();
    Some(if kind.contains('/') {
        kind
    } else {
        format!("image/{kind}")
    })
}

/// Reads `PHOTO`: inline data (decoded) or a link.
fn read_photo(p: &Property) -> Result<(ContactPhoto, Option<PhotoData>), ()> {
    let value = p.value.trim();
    let encoded = p
        .param("ENCODING")
        .is_some_and(|e| e.eq_ignore_ascii_case("b") || e.eq_ignore_ascii_case("BASE64"));
    let (mime, b64) = if let Some(rest) = value.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',').ok_or(())?;
        if !meta.to_ascii_lowercase().ends_with(";base64") {
            return Err(());
        }
        (mime_of(p, meta.split(';').next()), data)
    } else if encoded {
        (mime_of(p, None), value)
    } else {
        return Ok((
            ContactPhoto {
                mime_type: mime_of(p, None),
                sha256: None,
                url: Some(value.to_owned()),
                data: None,
            },
            None,
        ));
    };
    let cleaned: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > MAX_CONTACT_PHOTO_BYTES {
        return Err(());
    }
    let sha256 = sha256_hex(&bytes);
    Ok((
        ContactPhoto {
            mime_type: mime.clone(),
            sha256: Some(sha256.clone()),
            url: None,
            data: None,
        },
        Some(PhotoData {
            mime_type: mime,
            bytes,
            sha256,
        }),
    ))
}

/// Properties that are Nail fields or vendor internals, not custom
/// fields.
fn is_internal(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    (upper.starts_with("X-AB") && upper != "X-ABDATE")
        || upper.starts_with("X-ADDRESSBOOKSERVER-")
        || upper.starts_with("X-NAIL-")
        || upper.starts_with("X-MOZILLA-")
        || upper.starts_with("X-EVOLUTION-")
        || upper == "X-ANNIVERSARY"
        || upper == "X-APPLE-OMIT-YEAR"
}

/// Whether a property is a custom field (an `X-` property Nail has no
/// field for).
fn is_custom(p: &Property) -> bool {
    p.name.len() > 2 && p.name[..2].eq_ignore_ascii_case("X-") && !is_internal(&p.name)
}

fn read_kind(c: &Component) -> ContactKind {
    match c
        .property("KIND")
        .map(|k| k.value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("org") => ContactKind::Organization,
        Some("location") => ContactKind::Location,
        Some(_) => ContactKind::Individual,
        None => match c.property("X-ABSHOWAS") {
            Some(p) if p.value.trim().eq_ignore_ascii_case("COMPANY") => ContactKind::Organization,
            _ => ContactKind::Individual,
        },
    }
}

fn read_photo_field(c: &Component) -> Option<ContactPhoto> {
    c.property("PHOTO")
        .and_then(|p| read_photo(p).ok())
        .map(|(photo, _)| photo)
}

fn derived_display_name(
    name: Option<&ContactName>,
    org: Option<&ContactOrg>,
    emails: &[LabeledValue],
) -> Option<String> {
    if let Some(name) = name {
        let parts: Vec<&str> = [
            &name.prefix,
            &name.given,
            &name.additional,
            &name.family,
            &name.suffix,
        ]
        .into_iter()
        .filter_map(|p| p.as_deref())
        .collect();
        if !parts.is_empty() {
            return Some(parts.join(" "));
        }
    }
    org.map(|o| o.name.clone())
        .or_else(|| emails.first().map(|e| e.value.clone()))
}

/// Reads the fields of a `VCARD`; `None` when it has no name at all.
pub(crate) fn read_fields(c: &Component) -> Option<ContactFields> {
    let name = read_name(c);
    let organization = read_org(c);
    let emails: Vec<LabeledValue> = c
        .properties_named("EMAIL")
        .map(|p| read_email(c, p))
        .filter(|e| !e.value.is_empty())
        .collect();
    let display_name = card_text(c, "FN")
        .or_else(|| derived_display_name(name.as_ref(), organization.as_ref(), &emails))?;
    let mut related: Vec<LabeledValue> = c
        .properties_named("RELATED")
        .map(|p| labeled(c, p, strip_scheme(&text_of(p), "urn:uuid:")))
        .collect();
    related.extend(
        c.properties_named("X-ABRELATEDNAMES")
            .map(|p| read_plain(c, p)),
    );
    Some(ContactFields {
        kind: read_kind(c),
        display_name,
        name,
        nicknames: c
            .properties_named("NICKNAME")
            .flat_map(|p| decoded(p).texts())
            .collect(),
        emails,
        phones: c
            .properties_named("TEL")
            .map(|p| read_phone(c, p))
            .filter(|p| !p.value.is_empty())
            .collect(),
        addresses: c
            .properties_named("ADR")
            .map(|p| read_address(c, p))
            .collect(),
        urls: c
            .properties_named("URL")
            .map(|p| read_plain(c, p))
            .filter(|u| !u.value.is_empty())
            .collect(),
        impp: c
            .properties_named("IMPP")
            .map(|p| read_plain(c, p))
            .filter(|u| !u.value.is_empty())
            .collect(),
        organization,
        title: card_text(c, "TITLE"),
        role: card_text(c, "ROLE"),
        birthday: c.property("BDAY").and_then(read_date),
        anniversary: c
            .property("ANNIVERSARY")
            .or_else(|| c.property("X-ANNIVERSARY"))
            .and_then(read_date),
        notes: card_text(c, "NOTE"),
        categories: c
            .properties_named("CATEGORIES")
            .flat_map(|p| decoded(p).texts())
            .collect(),
        photo: read_photo_field(c),
        related,
        custom: c
            .properties
            .iter()
            .filter(|p| is_custom(p))
            .map(|p| CustomField {
                name: p.name.clone(),
                value: text_of(p),
            })
            .collect(),
    })
}

fn is_group_card(c: &Component) -> Option<GroupStyle> {
    if c.property("KIND")
        .is_some_and(|k| k.value.trim().eq_ignore_ascii_case("group"))
    {
        return Some(GroupStyle::Kind);
    }
    c.property("X-ADDRESSBOOKSERVER-KIND")
        .filter(|k| k.value.trim().eq_ignore_ascii_case("group"))
        .map(|_| GroupStyle::Apple)
}

fn read_members(c: &Component) -> (Vec<String>, Vec<String>) {
    let mut uids = Vec::new();
    let mut emails = Vec::new();
    for p in c
        .properties_named("MEMBER")
        .chain(c.properties_named("X-ADDRESSBOOKSERVER-MEMBER"))
    {
        let value = text_of(p);
        let value = value.trim();
        if value.len() > 7 && value[..7].eq_ignore_ascii_case("mailto:") {
            emails.push(value[7..].to_owned());
        } else if !value.is_empty() {
            uids.push(uid_key(value));
        }
    }
    (uids, emails)
}

fn read_rev(c: &Component) -> Option<String> {
    let value = c.property("REV")?.value.trim().to_owned();
    if let Some(instant) = time::parse_instant(&value) {
        return Some(time::format_instant(instant));
    }
    match time::parse_ical_value(&value)? {
        time::IcalValue::DateTime { local, .. } => Some(time::format_instant(local.and_utc())),
        time::IcalValue::Date(d) => Some(time::format_instant(
            d.and_time(chrono::NaiveTime::MIN).and_utc(),
        )),
    }
}

/// Reads vCard text (any number of cards).
pub fn parse_cards(text: &str) -> ParsedCards {
    let doc = content::parse(text);
    let mut out = ParsedCards::default();
    let cards: Vec<&Component> = doc.components.iter().filter(|c| c.is("VCARD")).collect();
    if cards.is_empty() {
        out.issues
            .push(Issue::new(IssueKind::NotVCard, None).skipped());
        return out;
    }
    for issue in &doc.issues {
        out.issues.push(match issue.kind {
            content::ContentIssueKind::Unclosed => Issue::new(
                IssueKind::Unclosed {
                    component: "VCARD".into(),
                },
                Some(issue.line),
            ),
            _ => Issue::new(IssueKind::MalformedLine, Some(issue.line)),
        });
    }
    let single = doc.components.len() == 1;
    // Where each UID was read; a later copy replaces an earlier one
    // (indexes instead of scanning everything read so far).
    let mut group_at: HashMap<String, usize> = HashMap::new();
    let mut contact_at: HashMap<String, usize> = HashMap::new();
    let mut replaced_groups: HashSet<usize> = HashSet::new();
    let mut replaced_contacts: HashSet<usize> = HashSet::new();
    for card in cards {
        let raw = if single {
            text.to_owned()
        } else {
            card.to_text()
        };
        let name = card_text(card, "FN");
        let uid = if let Some(uid) = card_text(card, "UID") {
            uid
        } else {
            out.issues
                .push(Issue::new(IssueKind::MissingUid, Some(card.line)).item(name.clone()));
            format!("nail-{}", &sha256_hex(card.to_text().as_bytes())[..32])
        };
        if let Some(style) = is_group_card(card) {
            let (member_uids, member_emails) = read_members(card);
            if let Some(old) = group_at.insert(uid.clone(), out.groups.len()) {
                replaced_groups.insert(old);
            }
            out.groups.push(ParsedGroup {
                group: GroupCard {
                    uid,
                    name: name
                        .or_else(|| read_name(card).and_then(|n| n.family))
                        .unwrap_or_default(),
                    member_uids,
                    member_emails,
                    version: version_of(card),
                    style,
                },
                raw,
                line: card.line,
            });
            continue;
        }
        let Some(fields) = read_fields(card) else {
            out.issues.push(
                Issue::new(IssueKind::MissingName, Some(card.line))
                    .item(Some(uid))
                    .skipped(),
            );
            continue;
        };
        let photo = match card.property("PHOTO").map(read_photo) {
            Some(Ok((_, data))) => data,
            Some(Err(())) => {
                out.issues.push(
                    Issue::new(
                        IssueKind::InvalidPhoto,
                        card.property("PHOTO").map(|p| p.line),
                    )
                    .item(Some(fields.display_name.clone())),
                );
                None
            }
            None => None,
        };
        if let Some(old) = contact_at.insert(uid.clone(), out.contacts.len()) {
            out.issues.push(
                Issue::new(IssueKind::DuplicateUid, Some(card.line))
                    .item(Some(fields.display_name.clone())),
            );
            replaced_contacts.insert(old);
        }
        out.contacts.push(ParsedContact {
            uid,
            fields,
            version: version_of(card),
            photo,
            updated_at: read_rev(card),
            raw,
            line: card.line,
        });
    }
    if !replaced_groups.is_empty() {
        let mut i = 0;
        out.groups.retain(|_| {
            i += 1;
            !replaced_groups.contains(&(i - 1))
        });
    }
    if !replaced_contacts.is_empty() {
        let mut i = 0;
        out.contacts.retain(|_| {
            i += 1;
            !replaced_contacts.contains(&(i - 1))
        });
    }
    if out.contacts.is_empty() && out.groups.is_empty() {
        out.issues.push(Issue::new(IssueKind::NoContacts, None));
    }
    out
}

/// Reads vCard bytes (charset detected: UTF-8, UTF-16 BOM, else
/// Windows-1252).
pub fn parse_card_bytes(bytes: &[u8]) -> ParsedCards {
    parse_cards(&content::decode_bytes(bytes).0)
}

// ---------------------------------------------------------------------------
// Writing

/// Options for writing cards.
#[derive(Clone, Copy)]
pub struct CardWriteOptions<'a> {
    /// "Now", for `REV` and `PRODID`-less new cards.
    pub now: DateTime<Utc>,
    /// Photo bytes by SHA-256 (the blob store), for photos that are not
    /// in the original card.
    pub blobs: Option<crate::write::BlobLookup<'a>>,
}

impl std::fmt::Debug for CardWriteOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardWriteOptions")
            .field("now", &self.now)
            .field("blobs", &self.blobs.is_some())
            .finish()
    }
}

impl CardWriteOptions<'_> {
    /// Options with only a time.
    pub fn at(now: DateTime<Utc>) -> Self {
        CardWriteOptions { now, blobs: None }
    }
}

fn replace(c: &mut Component, names: &[&str], new: Vec<Property>) {
    let position = c
        .properties
        .iter()
        .position(|p| names.iter().any(|n| p.is(n)));
    c.properties.retain(|p| !names.iter().any(|n| p.is(n)));
    let at = position
        .unwrap_or(c.properties.len())
        .min(c.properties.len());
    c.properties.splice(at..at, new);
}

fn merged(original: Option<&Property>, mut new: Property, managed: &[&str]) -> Property {
    if let Some(original) = original {
        let mut params: Vec<Param> = original
            .params
            .iter()
            .filter(|p| !managed.iter().any(|m| p.is(m)))
            .filter(|p| !new.params.iter().any(|n| n.is(&p.name)))
            .cloned()
            .collect();
        params.append(&mut new.params);
        new.params = params;
        new.group.clone_from(&original.group);
        new.touch();
    }
    new
}

fn patch<T: PartialEq>(
    c: &mut Component,
    names: &[&str],
    current: &T,
    desired: &T,
    managed: &[&str],
    build: impl FnOnce(&T) -> Vec<Property>,
) {
    if current == desired {
        return;
    }
    let original = names.iter().find_map(|n| c.property(n)).cloned();
    let new = build(desired)
        .into_iter()
        .map(|p| merged(original.as_ref(), p, managed))
        .collect();
    replace(c, names, new);
}

fn structured(parts: &[Option<&str>]) -> String {
    parts
        .iter()
        .map(|p| escape_text(p.unwrap_or("")))
        .collect::<Vec<_>>()
        .join(";")
}

fn next_group(c: &Component) -> String {
    let max = c
        .properties
        .iter()
        .filter_map(|p| p.group.as_deref())
        .filter_map(|g| {
            g.to_ascii_lowercase()
                .strip_prefix("item")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    format!("item{}", max + 1)
}

fn type_params(
    kinds: &[String],
    preferred: bool,
    version: VCardVersion,
    extra: &[&str],
) -> Vec<String> {
    let mut values: Vec<String> = extra.iter().map(|s| (*s).to_owned()).collect();
    values.extend(kinds.iter().cloned());
    if preferred && version == VCardVersion::V3 {
        values.push("pref".into());
    }
    match version {
        VCardVersion::V3 => values.iter().map(|v| v.to_ascii_uppercase()).collect(),
        VCardVersion::V4 => values.iter().map(|v| v.to_ascii_lowercase()).collect(),
    }
}

fn with_types(
    p: Property,
    kinds: &[String],
    preferred: bool,
    version: VCardVersion,
    extra: &[&str],
) -> Property {
    let p = p.with_param_values("TYPE", type_params(kinds, preferred, version, extra));
    if preferred && version == VCardVersion::V4 {
        p.with_param("PREF", "1")
    } else {
        p
    }
}

/// Rewrites a list of labeled entries (emails, phones, …), keeping
/// unchanged ones and their labels as written.
#[expect(
    clippy::too_many_arguments,
    reason = "one generic patch for every labeled list; each closure is one aspect of the entry type"
)]
fn patch_list<T: PartialEq + Clone>(
    c: &mut Component,
    names: &[&str],
    desired: &[T],
    read: impl Fn(&Component, &Property) -> T,
    same_entry: impl Fn(&T, &T) -> bool,
    label_of: impl Fn(&T) -> Option<String>,
    build: impl Fn(&T) -> Property,
    managed: &[&str],
) {
    let originals: Vec<Property> = c
        .properties
        .iter()
        .filter(|p| names.iter().any(|n| p.is(n)))
        .cloned()
        .collect();
    let current: Vec<T> = originals.iter().map(|p| read(c, p)).collect();
    if current == desired {
        return;
    }
    let labels: Vec<Option<Property>> = originals
        .iter()
        .map(|p| group_label(c, p).cloned())
        .collect();
    let mut used = vec![false; originals.len()];
    let mut new = Vec::new();
    let mut fresh_groups = 0u32;
    let base_group = next_group(c);
    let base: u32 = base_group[4..].parse().unwrap_or(1);
    for (index, entry) in desired.iter().enumerate() {
        let exact = (0..originals.len()).find(|&i| !used[i] && current[i] == *entry);
        let similar = || {
            (0..originals.len())
                .find(|&i| !used[i] && same_entry(&current[i], entry))
                // An entry edited in place keeps its position's line.
                .or_else(|| (index < originals.len() && !used[index]).then_some(index))
        };
        if let Some(i) = exact {
            used[i] = true;
            new.push(originals[i].clone());
            new.extend(labels[i].clone());
        } else {
            let original = similar();
            if let Some(i) = original {
                used[i] = true;
            }
            let mut p = merged(original.map(|i| &originals[i]), build(entry), managed);
            if let Some(label) = label_of(entry) {
                let group = p.group.clone().unwrap_or_else(|| {
                    let g = format!("item{}", base + fresh_groups);
                    fresh_groups += 1;
                    g
                });
                p.group = Some(group.clone());
                p.touch();
                new.push(p);
                new.push(Property::text_value("X-ABLabel", &label).with_group(Some(group)));
            } else {
                // A label that went away goes with its group.
                p.group = None;
                p.touch();
                new.push(p);
            }
        }
    }
    // Remove the old entries and their labels, then insert the new ones.
    let old_groups: Vec<String> = originals
        .iter()
        .filter_map(|p| p.group.as_deref().map(str::to_ascii_lowercase))
        .collect();
    let position = c
        .properties
        .iter()
        .position(|p| names.iter().any(|n| p.is(n)));
    let before_position = position.map_or(0, |pos| {
        c.properties[..pos]
            .iter()
            .filter(|p| {
                !(names.iter().any(|n| p.is(n))
                    || p.is("X-ABLABEL")
                        && p.group
                            .as_deref()
                            .is_some_and(|g| old_groups.contains(&g.to_ascii_lowercase())))
            })
            .count()
    });
    c.properties.retain(|p| {
        !(names.iter().any(|n| p.is(n))
            || p.is("X-ABLABEL")
                && p.group
                    .as_deref()
                    .is_some_and(|g| old_groups.contains(&g.to_ascii_lowercase())))
    });
    let at = if position.is_some() {
        before_position
    } else {
        c.properties.len()
    };
    c.properties.splice(at..at, new);
}

fn labeled_label(v: &LabeledValue) -> Option<String> {
    v.label.clone()
}

fn same_value(a: &LabeledValue, b: &LabeledValue) -> bool {
    a.value.eq_ignore_ascii_case(&b.value)
}

const LIST_PARAMS: &[&str] = &["TYPE", "PREF", "VALUE", "ENCODING", "CHARSET"];

fn photo_prop(
    photo: &ContactPhoto,
    bytes: Option<Vec<u8>>,
    version: VCardVersion,
) -> Option<Property> {
    if let Some(bytes) = bytes {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let mime = photo
            .mime_type
            .clone()
            .unwrap_or_else(|| "image/jpeg".into());
        return Some(match version {
            VCardVersion::V4 => Property::new("PHOTO", format!("data:{mime};base64,{b64}")),
            VCardVersion::V3 => {
                let kind = mime
                    .rsplit('/')
                    .next()
                    .unwrap_or("jpeg")
                    .to_ascii_uppercase();
                Property::new("PHOTO", b64)
                    .with_param("ENCODING", "b")
                    .with_param("TYPE", kind)
            }
        });
    }
    let url = photo.url.as_ref()?;
    Some(match version {
        VCardVersion::V4 => Property::new("PHOTO", url.clone()),
        VCardVersion::V3 => Property::new("PHOTO", url.clone()).with_param("VALUE", "uri"),
    })
}

fn patch_photo(
    c: &mut Component,
    desired: Option<&ContactPhoto>,
    version: VCardVersion,
    options: &CardWriteOptions<'_>,
) {
    let current = read_photo_field(c);
    let unchanged = match (&current, desired) {
        (None, None) => true,
        (Some(a), Some(b)) => b.data.is_none() && a.sha256 == b.sha256 && a.url == b.url,
        _ => false,
    };
    if unchanged {
        return;
    }
    let new = desired.and_then(|photo| {
        let bytes = match &photo.data {
            Some(data) => base64::engine::general_purpose::STANDARD
                .decode(
                    data.chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>(),
                )
                .ok(),
            None => photo.sha256.as_ref().and_then(|sha| {
                c.property("PHOTO")
                    .and_then(|p| read_photo(p).ok())
                    .and_then(|(_, data)| data)
                    .filter(|d| d.sha256 == *sha)
                    .map(|d| d.bytes)
                    .or_else(|| options.blobs.and_then(|blobs| blobs(sha)))
            }),
        };
        photo_prop(photo, bytes, version)
    });
    replace(c, &["PHOTO"], new.into_iter().collect());
}

fn patch_fields(
    c: &mut Component,
    fields: &ContactFields,
    version: VCardVersion,
    options: &CardWriteOptions<'_>,
) {
    patch(
        c,
        &["FN"],
        &card_text(c, "FN"),
        &Some(fields.display_name.clone()),
        &[],
        |v| v.iter().map(|t| Property::text_value("FN", t)).collect(),
    );
    patch(c, &["N"], &read_name(c), &fields.name, &[], |n| {
        let n = n.clone().unwrap_or_default();
        // vCard 3 requires N; an empty one is written.
        vec![Property::new(
            "N",
            structured(&[
                n.family.as_deref(),
                n.given.as_deref(),
                n.additional.as_deref(),
                n.prefix.as_deref(),
                n.suffix.as_deref(),
            ]),
        )]
    });
    let kind = read_kind(c);
    if kind != fields.kind {
        match version {
            VCardVersion::V4 => {
                let value = match fields.kind {
                    ContactKind::Individual => None,
                    ContactKind::Organization => Some("org"),
                    ContactKind::Location => Some("location"),
                };
                replace(
                    c,
                    &["KIND"],
                    value
                        .map(|v| Property::new("KIND", v))
                        .into_iter()
                        .collect(),
                );
            }
            VCardVersion::V3 => {
                let value = (fields.kind == ContactKind::Organization)
                    .then(|| Property::new("X-ABShowAs", "COMPANY"));
                replace(c, &["X-ABSHOWAS"], value.into_iter().collect());
            }
        }
    }
    let nicknames: Vec<String> = c
        .properties_named("NICKNAME")
        .flat_map(|p| decoded(p).texts())
        .collect();
    patch(c, &["NICKNAME"], &nicknames, &fields.nicknames, &[], |n| {
        if n.is_empty() {
            Vec::new()
        } else {
            vec![Property::new(
                "NICKNAME",
                n.iter()
                    .map(|s| escape_text(s))
                    .collect::<Vec<_>>()
                    .join(","),
            )]
        }
    });
    patch_list(
        c,
        &["EMAIL"],
        &fields.emails,
        read_email,
        same_value,
        labeled_label,
        |e| {
            let extra: &[&str] = if version == VCardVersion::V3 {
                &["internet"]
            } else {
                &[]
            };
            with_types(
                Property::text_value("EMAIL", &e.value),
                &e.kinds,
                e.preferred,
                version,
                extra,
            )
        },
        LIST_PARAMS,
    );
    patch_list(
        c,
        &["TEL"],
        &fields.phones,
        read_phone,
        same_value,
        labeled_label,
        |t| {
            with_types(
                Property::text_value("TEL", &t.value),
                &t.kinds,
                t.preferred,
                version,
                &[],
            )
        },
        LIST_PARAMS,
    );
    patch_list(
        c,
        &["ADR"],
        &fields.addresses,
        read_address,
        |a, b| a.street == b.street && a.locality == b.locality,
        |a| a.label.clone(),
        |a| {
            let street = a.street.as_deref().map(|s| s.replace('\n', ", "));
            with_types(
                Property::new(
                    "ADR",
                    structured(&[
                        a.po_box.as_deref(),
                        a.extended.as_deref(),
                        a.street.as_deref().and(street.as_deref()),
                        a.locality.as_deref(),
                        a.region.as_deref(),
                        a.postal_code.as_deref(),
                        a.country.as_deref(),
                    ]),
                ),
                &a.kinds,
                a.preferred,
                version,
                &[],
            )
        },
        LIST_PARAMS,
    );
    patch_list(
        c,
        &["URL"],
        &fields.urls,
        read_plain,
        same_value,
        labeled_label,
        |u| {
            with_types(
                Property::new("URL", u.value.clone()),
                &u.kinds,
                u.preferred,
                version,
                &[],
            )
        },
        LIST_PARAMS,
    );
    patch_list(
        c,
        &["IMPP"],
        &fields.impp,
        read_plain,
        same_value,
        labeled_label,
        |u| {
            with_types(
                Property::new("IMPP", u.value.clone()),
                &u.kinds,
                u.preferred,
                version,
                &[],
            )
        },
        LIST_PARAMS,
    );
    let related_names: &[&str] = &["RELATED", "X-ABRELATEDNAMES"];
    patch_list(
        c,
        related_names,
        &fields.related,
        |card, p| {
            if p.is("RELATED") {
                labeled(card, p, strip_scheme(&text_of(p), "urn:uuid:"))
            } else {
                read_plain(card, p)
            }
        },
        same_value,
        |r| {
            r.label.clone().or_else(|| {
                (version == VCardVersion::V3).then(|| {
                    r.kinds.first().map(|k| {
                        let mut chars = k.chars();
                        let first = chars
                            .next()
                            .map(|f| f.to_ascii_uppercase())
                            .unwrap_or_default();
                        format!("_$!<{first}{}>!$_", chars.as_str())
                    })
                })?
            })
        },
        |r| match version {
            VCardVersion::V4 => with_types(
                Property::text_value("RELATED", &r.value),
                &r.kinds,
                r.preferred,
                version,
                &[],
            )
            .with_param("VALUE", "text"),
            VCardVersion::V3 => Property::text_value("X-ABRELATEDNAMES", &r.value),
        },
        LIST_PARAMS,
    );
    patch(c, &["ORG"], &read_org(c), &fields.organization, &[], |o| {
        o.iter()
            .map(|o| {
                let mut parts = vec![escape_text(&o.name)];
                parts.extend(o.units.iter().map(|u| escape_text(u)));
                Property::new("ORG", parts.join(";"))
            })
            .collect()
    });
    for (name, value) in [
        ("TITLE", &fields.title),
        ("ROLE", &fields.role),
        ("NOTE", &fields.notes),
    ] {
        patch(
            c,
            &[name],
            &card_text(c, name),
            value,
            &["ENCODING", "CHARSET"],
            |v| v.iter().map(|t| Property::text_value(name, t)).collect(),
        );
    }
    patch(
        c,
        &["BDAY"],
        &c.property("BDAY").and_then(read_date),
        &fields.birthday,
        &["VALUE", "X-APPLE-OMIT-YEAR"],
        |d| d.iter().map(|d| write_date(d, version, "BDAY")).collect(),
    );
    let anniversary_name = match version {
        VCardVersion::V4 => "ANNIVERSARY",
        VCardVersion::V3 => "X-ANNIVERSARY",
    };
    let current = c
        .property("ANNIVERSARY")
        .or_else(|| c.property("X-ANNIVERSARY"))
        .and_then(read_date);
    patch(
        c,
        &["ANNIVERSARY", "X-ANNIVERSARY"],
        &current,
        &fields.anniversary,
        &["VALUE", "X-APPLE-OMIT-YEAR"],
        |d| {
            d.iter()
                .map(|d| write_date(d, version, anniversary_name))
                .collect()
        },
    );
    let categories: Vec<String> = c
        .properties_named("CATEGORIES")
        .flat_map(|p| decoded(p).texts())
        .collect();
    patch(
        c,
        &["CATEGORIES"],
        &categories,
        &fields.categories,
        &[],
        |list| {
            if list.is_empty() {
                Vec::new()
            } else {
                vec![Property::new(
                    "CATEGORIES",
                    list.iter()
                        .map(|s| escape_text(s))
                        .collect::<Vec<_>>()
                        .join(","),
                )]
            }
        },
    );
    patch_photo(c, fields.photo.as_ref(), version, options);
    // Custom fields: X- properties Nail has no field for.
    let current: Vec<CustomField> = c
        .properties
        .iter()
        .filter(|p| is_custom(p))
        .map(|p| CustomField {
            name: p.name.clone(),
            value: text_of(p),
        })
        .collect();
    if current != fields.custom {
        let originals: Vec<Property> = c
            .properties
            .iter()
            .filter(|p| is_custom(p))
            .cloned()
            .collect();
        let mut used = vec![false; originals.len()];
        let new: Vec<Property> = fields
            .custom
            .iter()
            .filter(|f| !f.name.trim().is_empty())
            .map(|f| {
                let same = (0..originals.len()).find(|&i| !used[i] && current[i] == *f);
                if let Some(i) = same {
                    used[i] = true;
                    originals[i].clone()
                } else {
                    let name = if f.name.len() > 2 && f.name[..2].eq_ignore_ascii_case("X-") {
                        f.name.clone()
                    } else {
                        // A label the user chose becomes an X- name.
                        let cleaned: String = f
                            .name
                            .chars()
                            .map(|ch| {
                                if ch.is_ascii_alphanumeric() {
                                    ch.to_ascii_uppercase()
                                } else {
                                    '-'
                                }
                            })
                            .collect();
                        format!("X-{cleaned}")
                    };
                    Property::text_value(name, &f.value)
                }
            })
            .collect();
        let position = c.properties.iter().position(is_custom);
        let before = position.map_or(c.properties.len(), |pos| {
            c.properties[..pos].iter().filter(|p| !is_custom(p)).count()
        });
        c.properties.retain(|p| !is_custom(p));
        let at = before.min(c.properties.len());
        c.properties.splice(at..at, new);
    }
}

fn card_from(original: Option<&str>, version: VCardVersion) -> Component {
    let found = original
        .map(content::parse)
        .and_then(|doc| doc.components.into_iter().find(|c| c.is("VCARD")));
    if let Some(mut card) = found {
        let is_21 = card
            .property("VERSION")
            .is_some_and(|v| v.value.trim() == "2.1");
        if is_21 {
            // vCard 2.1 constructs (bare types, quoted-printable,
            // charsets) are rewritten as 3.0.
            for p in &mut card.properties {
                *p = decoded(p);
                p.remove_param("CHARSET");
                p.touch();
            }
        }
        if is_21 || version_of(&card) != version {
            let value = match version {
                VCardVersion::V3 => "3.0",
                VCardVersion::V4 => "4.0",
            };
            replace(
                &mut card,
                &["VERSION"],
                vec![Property::new("VERSION", value)],
            );
        }
        card
    } else {
        let mut card = Component::new("VCARD");
        card.properties.push(Property::new(
            "VERSION",
            match version {
                VCardVersion::V3 => "3.0",
                VCardVersion::V4 => "4.0",
            },
        ));
        card.properties.push(Property::new("PRODID", crate::PRODID));
        card
    }
}

fn finish_card(c: &mut Component, before: &str, uid: &str, options: &CardWriteOptions<'_>) {
    // vCard 3.0 requires N.
    if version_of(c) == VCardVersion::V3 && c.property("N").is_none() {
        let at = c
            .properties
            .iter()
            .position(|p| p.is("FN"))
            .map_or(c.properties.len(), |i| i + 1);
        c.properties.insert(at, Property::new("N", ";;;;"));
    }
    let current_uid = card_text(c, "UID");
    if current_uid.as_deref() != Some(uid) {
        replace(c, &["UID"], vec![Property::text_value("UID", uid)]);
    }
    if c.to_text() != before {
        let rev = Property::new("REV", time::ical_utc(options.now));
        if c.property("REV").is_some() {
            replace(c, &["REV"], vec![rev]);
        } else {
            c.properties.push(rev);
        }
    }
}

/// Writes a contact as vCard in its version, patching `original` (the
/// stored card) when given.
pub fn write_contact(
    contact: &Contact,
    original: Option<&str>,
    options: &CardWriteOptions<'_>,
) -> String {
    let mut card = card_from(original, contact.version);
    let before = card.to_text();
    // A 2.1 card read back unchanged is returned as it was.
    if let Some(original) = original
        && let Some(parsed) = parse_cards(original).contacts.first()
        && parsed.fields == contact.fields
        && parsed.version == contact.version
        && parsed.uid == contact.uid
    {
        return original.to_owned();
    }
    patch_fields(&mut card, &contact.fields, contact.version, options);
    finish_card(&mut card, &before, &contact.uid, options);
    card.to_text()
}

/// Writes a group card, patching `original` when given. Members are
/// written as `urn:uuid:` references (vCard 4 `MEMBER`, Apple
/// `X-ADDRESSBOOKSERVER-MEMBER`) and `mailto:` addresses.
pub fn write_group(
    group: &GroupCard,
    original: Option<&str>,
    options: &CardWriteOptions<'_>,
) -> String {
    let mut card = card_from(original, group.version);
    let before = card.to_text();
    let current_name = card_text(&card, "FN");
    patch(
        &mut card,
        &["FN"],
        &current_name,
        &Some(group.name.clone()),
        &[],
        |v| v.iter().map(|t| Property::text_value("FN", t)).collect(),
    );
    match group.style {
        GroupStyle::Kind => {
            if is_group_card(&card) != Some(GroupStyle::Kind) {
                replace(&mut card, &["KIND"], vec![Property::new("KIND", "group")]);
            }
        }
        GroupStyle::Apple => {
            if card.property("N").is_none() {
                card.properties.push(Property::text_value("N", &group.name));
            }
            if is_group_card(&card) != Some(GroupStyle::Apple) {
                replace(
                    &mut card,
                    &["X-ADDRESSBOOKSERVER-KIND"],
                    vec![Property::new("X-ADDRESSBOOKSERVER-KIND", "group")],
                );
            }
        }
    }
    let (uids, emails) = read_members(&card);
    let desired_uids: Vec<String> = group.member_uids.iter().map(|u| uid_key(u)).collect();
    if uids != desired_uids || emails != group.member_emails {
        let name = match group.style {
            GroupStyle::Kind => "MEMBER",
            GroupStyle::Apple => "X-ADDRESSBOOKSERVER-MEMBER",
        };
        let originals: Vec<Property> = card
            .properties
            .iter()
            .filter(|p| p.is("MEMBER") || p.is("X-ADDRESSBOOKSERVER-MEMBER"))
            .cloned()
            .collect();
        let mut new = Vec::new();
        for uid in &group.member_uids {
            let key = uid_key(uid);
            if let Some(p) = originals.iter().find(|p| uid_key(&text_of(p)) == key) {
                new.push(p.clone());
            } else {
                let value = if uid.contains(':') {
                    uid.clone()
                } else {
                    format!("urn:uuid:{uid}")
                };
                new.push(Property::new(name, value));
            }
        }
        for email in &group.member_emails {
            new.push(Property::new(name, format!("mailto:{email}")));
        }
        replace(&mut card, &["MEMBER", "X-ADDRESSBOOKSERVER-MEMBER"], new);
    }
    finish_card(&mut card, &before, &group.uid, options);
    card.to_text()
}

/// A display name from a structured name, for UIs that need one.
pub fn display_name_from(name: &ContactName) -> Option<String> {
    derived_display_name(Some(name), None, &[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::pim::source::PimItemSync;
    use pretty_assertions::assert_eq;

    fn now() -> DateTime<Utc> {
        time::parse_instant("2026-10-01T12:00:00Z").unwrap()
    }

    fn contact(parsed: &ParsedContact) -> Contact {
        Contact {
            id: "c".into(),
            address_book_id: "b".into(),
            uid: parsed.uid.clone(),
            fields: parsed.fields.clone(),
            version: parsed.version,
            group_ids: vec![],
            linked_ids: vec![],
            updated_at: parsed.updated_at.clone(),
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    const APPLE: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
PRODID:-//Apple Inc.//iPhone OS 17.0//EN\r\n\
N:Lovelace;Ada;Augusta;Lady;\r\n\
FN:Ada Lovelace\r\n\
NICKNAME:Ada\r\n\
ORG:Analytical Engines Ltd;Research\r\n\
TITLE:Countess\r\n\
EMAIL;type=INTERNET;type=HOME;type=pref:ada@home.example\r\n\
item1.EMAIL;type=INTERNET:ada@work.example\r\n\
item1.X-ABLabel:Lab\r\n\
TEL;type=CELL;type=VOICE;type=pref:+44 20 7946 0000\r\n\
item2.TEL:+44 20 7946 0001\r\n\
item2.X-ABLabel:_$!<Mobile>!$_\r\n\
item3.ADR;type=HOME;type=pref:;;12 St James's Square;London;;SW1Y 4JH;United Kingdom\r\n\
item3.X-ABADR:gb\r\n\
item4.URL;type=pref:https://ada.example\r\n\
item4.X-ABLabel:_$!<HomePage>!$_\r\n\
BDAY;X-APPLE-OMIT-YEAR=1604:1604-12-10\r\n\
item5.X-ABRELATEDNAMES;type=pref:William King\r\n\
item5.X-ABLabel:_$!<Spouse>!$_\r\n\
NOTE:First programmer\\, arguably.\r\n\
X-SOCIALPROFILE;type=twitter:x-apple:ada\r\n\
X-ABShowAs:PERSON\r\n\
CATEGORIES:Friends,Science\r\n\
PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQSkZJRgABAQ==\r\n\
UID:5B1C2E3D-0000-4000-8000-ADA000000001\r\n\
REV:2026-09-30T10:00:00Z\r\n\
END:VCARD\r\n";

    #[test]
    fn reads_apple_cards() {
        let parsed = parse_cards(APPLE);
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let c = &parsed.contacts[0];
        let f = &c.fields;
        assert_eq!(c.version, VCardVersion::V3);
        assert_eq!(f.display_name, "Ada Lovelace");
        let name = f.name.as_ref().unwrap();
        assert_eq!(
            (
                name.family.as_deref(),
                name.given.as_deref(),
                name.prefix.as_deref()
            ),
            (Some("Lovelace"), Some("Ada"), Some("Lady"))
        );
        assert_eq!(f.organization.as_ref().unwrap().units, ["Research"]);
        assert_eq!(f.emails[0].kinds, ["home"]);
        assert!(f.emails[0].preferred);
        assert_eq!(f.emails[1].label.as_deref(), Some("Lab"));
        assert_eq!(f.phones[1].kinds, ["cell"]);
        assert_eq!(
            f.addresses[0].street.as_deref(),
            Some("12 St James's Square")
        );
        assert_eq!(f.addresses[0].country.as_deref(), Some("United Kingdom"));
        assert_eq!(f.urls[0].kinds, ["home"]);
        assert_eq!(
            f.birthday,
            Some(ContactDate {
                year: None,
                month: 12,
                day: 10
            })
        );
        assert_eq!(f.related[0].value, "William King");
        assert_eq!(f.related[0].kinds, ["spouse"]);
        assert_eq!(f.notes.as_deref(), Some("First programmer, arguably."));
        assert_eq!(
            f.custom,
            [CustomField {
                name: "X-SOCIALPROFILE".into(),
                value: "x-apple:ada".into()
            }]
        );
        assert_eq!(f.categories, ["Friends", "Science"]);
        let photo = f.photo.as_ref().unwrap();
        assert_eq!(photo.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(photo.sha256, c.photo.as_ref().map(|p| p.sha256.clone()));
        assert_eq!(c.updated_at.as_deref(), Some("2026-09-30T10:00:00Z"));
        assert_eq!(f.issues().len(), 0);
    }

    #[test]
    fn unchanged_cards_round_trip_exactly_and_edits_are_local() {
        let parsed = parse_cards(APPLE).contacts.remove(0);
        let mut c = contact(&parsed);
        assert_eq!(
            write_contact(&c, Some(APPLE), &CardWriteOptions::at(now())),
            APPLE
        );
        c.fields.emails[1].value = "ada@lab.example".into();
        c.fields.phones.push(LabeledValue {
            value: "+44 1".into(),
            kinds: vec!["work".into()],
            label: Some("Desk".into()),
            preferred: false,
        });
        c.fields.title = None;
        let text = write_contact(&c, Some(APPLE), &CardWriteOptions::at(now()));
        assert!(
            text.contains("item1.EMAIL;type=INTERNET:ada@lab.example\r\nitem1.X-ABLabel:Lab\r\n")
                || text.contains(
                    "item1.EMAIL;TYPE=INTERNET:ada@lab.example\r\nitem1.X-ABLabel:Lab\r\n"
                ),
            "{text}"
        );
        assert!(
            text.contains("item6.TEL;TYPE=WORK:+44 1\r\nitem6.X-ABLabel:Desk\r\n"),
            "{text}"
        );
        assert!(!text.contains("TITLE"));
        assert!(text.contains("item3.X-ABADR:gb\r\n"));
        assert!(text.contains("PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQSkZJRgABAQ==\r\n"));
        assert!(text.contains("REV:20261001T120000Z\r\n"));
        let back = parse_cards(&text).contacts.remove(0);
        assert_eq!(back.fields, c.fields);
    }

    #[test]
    fn vcard4_cards_photos_and_groups() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:org\r\nFN:ACME\r\nEMAIL;PREF=1;TYPE=work:info@acme.example\r\n\
TEL;VALUE=uri;TYPE=voice:tel:+1-555-0100\r\nPHOTO:data:image/png;base64,iVBORw0KGgo=\r\nBDAY:--0229\r\nANNIVERSARY:20000101\r\n\
RELATED;TYPE=contact;VALUE=uri:urn:uuid:abc\r\nUID:urn:uuid:acme-1\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\nFN:Suppliers\r\nMEMBER:urn:uuid:ACME-1\r\nMEMBER:mailto:sales@other.example\r\nUID:urn:uuid:g1\r\nEND:VCARD\r\n";
        let parsed = parse_cards(text);
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let c = &parsed.contacts[0];
        assert_eq!(c.version, VCardVersion::V4);
        assert_eq!(c.fields.kind, ContactKind::Organization);
        assert!(c.fields.emails[0].preferred);
        assert_eq!(c.fields.phones[0].value, "+1-555-0100");
        assert_eq!(
            c.photo.as_ref().unwrap().mime_type.as_deref(),
            Some("image/png")
        );
        assert_eq!(
            c.fields.birthday,
            Some(ContactDate {
                year: None,
                month: 2,
                day: 29
            })
        );
        assert_eq!(c.fields.anniversary.unwrap().year, Some(2000));
        assert_eq!(c.fields.related[0].value, "abc");
        assert_eq!(c.raw, c.raw.clone());
        let g = &parsed.groups[0].group;
        assert_eq!(g.style, GroupStyle::Kind);
        assert_eq!(g.member_uids, [uid_key(&c.uid)]);
        assert_eq!(g.member_emails, ["sales@other.example"]);
        // Unchanged group: same text; KIND stays lower case.
        let raw = parsed.groups[0].raw.clone();
        assert_eq!(
            write_group(g, Some(&raw), &CardWriteOptions::at(now())),
            raw
        );
        let mut edited = g.clone();
        edited.member_uids.push("new-person".into());
        let out = write_group(&edited, Some(&raw), &CardWriteOptions::at(now()));
        assert!(out.contains("KIND:group\r\n"));
        assert!(out.contains("MEMBER:urn:uuid:ACME-1\r\nMEMBER:urn:uuid:new-person\r\nMEMBER:mailto:sales@other.example\r\n"), "{out}");
        // A new vCard 4 contact with a new photo.
        let mut fields = ContactFields::named("Grace Hopper");
        fields.emails.push(LabeledValue {
            preferred: true,
            ..LabeledValue::new("grace@navy.example")
        });
        fields.birthday = Some(ContactDate {
            year: None,
            month: 12,
            day: 9,
        });
        fields.photo = Some(ContactPhoto {
            mime_type: Some("image/png".into()),
            sha256: None,
            url: None,
            data: Some("iVBORw0KGgo=".into()),
        });
        let new = Contact {
            uid: "urn:uuid:grace".into(),
            fields,
            version: VCardVersion::V4,
            ..contact(c)
        };
        let out = write_contact(&new, None, &CardWriteOptions::at(now()));
        assert!(
            out.starts_with("BEGIN:VCARD\r\nVERSION:4.0\r\nPRODID:-//Nail//Nail PIM//EN\r\n"),
            "{out}"
        );
        assert!(out.contains("EMAIL;PREF=1:grace@navy.example\r\n"), "{out}");
        assert!(out.contains("BDAY:--1209\r\n"));
        assert!(out.contains("PHOTO:data:image/png;base64,iVBORw0KGgo=\r\n"));
        let back = parse_cards(&out).contacts.remove(0);
        assert_eq!(
            back.photo.as_ref().unwrap().bytes,
            c.photo.as_ref().unwrap().bytes
        );
        let mut expected = new.fields.clone();
        expected.photo = back.fields.photo.clone();
        assert_eq!(back.fields, expected);
        assert_eq!(
            expected.photo.as_ref().unwrap().sha256,
            c.fields.photo.as_ref().unwrap().sha256
        );
    }

    #[test]
    fn apple_groups() {
        let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Book club\r\nFN:Book club\r\nX-ADDRESSBOOKSERVER-KIND:group\r\nX-ADDRESSBOOKSERVER-MEMBER:urn:uuid:5B1C2E3D-0000-4000-8000-ADA000000001\r\nUID:grp-1\r\nEND:VCARD\r\n";
        let parsed = parse_cards(text);
        let g = &parsed.groups[0].group;
        assert_eq!(g.style, GroupStyle::Apple);
        assert_eq!(
            g.member_uids,
            [uid_key("5B1C2E3D-0000-4000-8000-ADA000000001")]
        );
        let mut empty = g.clone();
        empty.member_uids.clear();
        empty.name = "Readers".into();
        let out = write_group(&empty, Some(text), &CardWriteOptions::at(now()));
        assert!(!out.contains("MEMBER"));
        assert!(out.contains("FN:Readers\r\n"));
        let fresh = write_group(
            &GroupCard {
                uid: "g2".into(),
                name: "Family".into(),
                member_uids: vec!["abc".into()],
                member_emails: vec![],
                version: VCardVersion::V3,
                style: GroupStyle::Apple,
            },
            None,
            &CardWriteOptions::at(now()),
        );
        let back = parse_cards(&fresh).groups.remove(0).group;
        assert_eq!(
            (back.name.as_str(), back.style, back.member_uids.clone()),
            ("Family", GroupStyle::Apple, vec!["abc".to_owned()])
        );
    }

    #[test]
    fn outlook_vcard21_with_charsets() {
        let bytes: Vec<u8> = b"BEGIN:VCARD\r\nVERSION:2.1\r\nN;CHARSET=Windows-1252:M\xfcller;J\xfcrgen\r\nFN;ENCODING=QUOTED-PRINTABLE;CHARSET=UTF-8:J=C3=BCrgen M=C3=BCller\r\nTEL;WORK;VOICE:+49 30 1234\r\nEMAIL;PREF;INTERNET:j@example.de\r\nADR;WORK;ENCODING=QUOTED-PRINTABLE:;;Hauptstra=C3=9Fe 1=0D=0AHof;Berlin;;10115;Deutschland\r\nEND:VCARD\r\n".to_vec();
        let parsed = parse_card_bytes(&bytes);
        let c = &parsed.contacts[0];
        assert_eq!(c.fields.display_name, "Jürgen Müller");
        assert_eq!(
            c.fields.name.as_ref().unwrap().family.as_deref(),
            Some("Müller")
        );
        assert_eq!(c.fields.phones[0].kinds, ["work", "voice"]);
        assert!(c.fields.emails[0].preferred);
        assert_eq!(
            c.fields.addresses[0].street.as_deref(),
            Some("Hauptstraße 1\nHof")
        );
        assert_eq!(c.version, VCardVersion::V3);
        assert!(c.uid.starts_with("nail-"));
        assert_eq!(parsed.issues[0].kind, IssueKind::MissingUid);
        // Edited, it becomes a clean vCard 3.0.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut edited = contact(c);
        edited.fields.title = Some("Ingenieur".into());
        let out = write_contact(
            &edited,
            Some(&parse_card_bytes(&bytes).contacts[0].raw),
            &CardWriteOptions::at(now()),
        );
        assert!(
            out.contains("VERSION:3.0\r\n") && !out.contains("QUOTED-PRINTABLE"),
            "{out}"
        );
        assert!(out.contains("FN:Jürgen Müller\r\n"));
        let back = parse_cards(&out).contacts.remove(0);
        assert_eq!(back.fields.title.as_deref(), Some("Ingenieur"));
        assert_eq!(back.fields.display_name, "Jürgen Müller");
        assert!(!text.is_empty(), "{text:?}");
    }

    #[test]
    fn problems_are_reported() {
        let parsed = parse_cards(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nNOTE:nameless\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:Pic\r\nUID:p\r\nPHOTO;ENCODING=b;TYPE=JPEG:!!!notbase64\r\nEND:VCARD\r\n",
        );
        let kinds: Vec<_> = parsed.issues.iter().map(|i| (&i.kind, i.skipped)).collect();
        assert_eq!(
            kinds,
            [
                (&IssueKind::MissingUid, false),
                (&IssueKind::MissingName, true),
                (&IssueKind::InvalidPhoto, false)
            ]
        );
        assert_eq!(parsed.contacts.len(), 1);
        assert_eq!(
            parse_cards("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").issues[0].kind,
            IssueKind::NotVCard
        );
        // A card without FN takes its name from N, ORG or EMAIL.
        let derived = parse_cards(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nEMAIL:x@y.example\r\nUID:x\r\nEND:VCARD\r\n",
        );
        assert_eq!(derived.contacts[0].fields.display_name, "x@y.example");
        assert_eq!(uid_key("urn:uuid:ABC"), "abc");
    }

    /// Later cards with the same UID replace earlier ones (reported), the
    /// rest keep their order.
    #[test]
    fn duplicate_cards_are_replaced_in_order() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:a\r\nFN:A1\r\nEND:VCARD\r\n\
            BEGIN:VCARD\r\nVERSION:4.0\r\nUID:b\r\nFN:B\r\nEND:VCARD\r\n\
            BEGIN:VCARD\r\nVERSION:4.0\r\nUID:g\r\nKIND:group\r\nFN:G1\r\nEND:VCARD\r\n\
            BEGIN:VCARD\r\nVERSION:4.0\r\nUID:a\r\nFN:A2\r\nEND:VCARD\r\n\
            BEGIN:VCARD\r\nVERSION:4.0\r\nUID:g\r\nKIND:group\r\nFN:G2\r\nEND:VCARD\r\n";
        let parsed = parse_cards(text);
        let names: Vec<&str> = parsed
            .contacts
            .iter()
            .map(|c| c.fields.display_name.as_str())
            .collect();
        assert_eq!(names, ["B", "A2"]);
        let groups: Vec<&str> = parsed
            .groups
            .iter()
            .map(|g| g.group.name.as_str())
            .collect();
        assert_eq!(groups, ["G2"]);
        assert_eq!(
            parsed
                .issues
                .iter()
                .filter(|i| matches!(i.kind, IssueKind::DuplicateUid))
                .count(),
            1
        );
    }
}
