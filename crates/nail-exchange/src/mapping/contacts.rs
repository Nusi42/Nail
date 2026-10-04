//! Exchange contacts ↔ Nail contacts (ADR 0072). Exchange contacts have
//! fixed slots (three email addresses, typed phone numbers, home /
//! business / other addresses); Nail's are vCard-like lists. Distribution
//! lists become contact groups (members by address; the core links them
//! to cards of the same book).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nail_api::exchange::pim::{
    ExchangeContact, ExchangeEmail, ExchangePhone, ExchangePostalAddress,
};
use nail_api::pim::contact::{
    ContactDate, ContactName, ContactOrg, ContactPhoto, MAX_CONTACT_PHOTO_BYTES,
};
use nail_api::pim::{ContactFields, LabeledValue, PostalAddress};

use super::derived_uid;

/// An Exchange contact or distribution list in Nail's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappedContact {
    /// A person.
    Contact {
        /// A stable `UID`.
        uid: String,
        /// The fields.
        fields: Box<ContactFields>,
    },
    /// A distribution list.
    Group {
        /// A stable `UID`.
        uid: String,
        /// Its name.
        name: String,
        /// Member addresses.
        member_emails: Vec<String>,
    },
}

fn phone_kinds(kind: &str) -> Vec<String> {
    match kind.to_ascii_lowercase().as_str() {
        "home" => vec!["home".into(), "voice".into()],
        "business" | "work" => vec!["work".into(), "voice".into()],
        "mobile" | "cell" => vec!["cell".into()],
        "fax" | "businessfax" => vec!["fax".into()],
        "pager" => vec!["pager".into()],
        _ => Vec::new(),
    }
}

fn phone_kind(kinds: &[String]) -> &'static str {
    let has = |k: &str| kinds.iter().any(|x| x.eq_ignore_ascii_case(k));
    if has("cell") {
        "mobile"
    } else if has("fax") {
        "fax"
    } else if has("work") {
        "business"
    } else if has("home") {
        "home"
    } else {
        "other"
    }
}

fn address_kind(kinds: &[String]) -> &'static str {
    if kinds.iter().any(|k| k.eq_ignore_ascii_case("work")) {
        "business"
    } else if kinds.iter().any(|k| k.eq_ignore_ascii_case("home")) {
        "home"
    } else {
        "other"
    }
}

fn some(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

fn birthday(text: &str) -> Option<ContactDate> {
    use chrono::Datelike;

    let date = chrono::NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()?;
    Some(ContactDate {
        // Outlook writes 1604 for "no year".
        year: (date.year() > 1604).then_some(date.year()),
        month: u8::try_from(date.month()).ok()?,
        day: u8::try_from(date.day()).ok()?,
    })
}

fn photo_of(bytes: &[u8]) -> Option<ContactPhoto> {
    if bytes.is_empty() || bytes.len() > MAX_CONTACT_PHOTO_BYTES {
        return None;
    }
    let mime = if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        return None;
    };
    Some(ContactPhoto {
        mime_type: Some(mime.to_owned()),
        sha256: None,
        url: None,
        data: Some(BASE64.encode(bytes)),
    })
}

/// An Exchange contact in Nail's shape.
pub fn contact_from_exchange(c: &ExchangeContact) -> MappedContact {
    let uid = derived_uid("contact", &c.remote_id);
    if c.is_group {
        return MappedContact::Group {
            uid,
            name: c.display_name.clone(),
            member_emails: c
                .members
                .iter()
                .map(|m| m.address.trim().to_owned())
                .filter(|a| !a.is_empty())
                .collect(),
        };
    }
    let name = ContactName {
        prefix: None,
        given: some(c.given_name.as_deref()),
        additional: some(c.middle_name.as_deref()),
        family: some(c.surname.as_deref()),
        suffix: None,
    };
    let display = [c.display_name.trim().to_owned()]
        .into_iter()
        .chain(nail_ical::vcard::display_name_from(&name))
        .chain(some(c.company.as_deref()))
        .chain(c.emails.first().map(|e| e.address.clone()))
        .find(|n| !n.trim().is_empty())
        .unwrap_or_default();
    let mut fields = ContactFields::named(display);
    fields.name = (!name.is_empty()).then_some(name);
    fields.nicknames = some(c.nickname.as_deref()).into_iter().collect();
    fields.emails = c
        .emails
        .iter()
        .filter(|e| !e.address.trim().is_empty())
        .enumerate()
        .map(|(i, e)| LabeledValue {
            value: e.address.trim().to_owned(),
            kinds: Vec::new(),
            label: None,
            preferred: i == 0,
        })
        .collect();
    fields.phones = c
        .phones
        .iter()
        .filter(|p| !p.number.trim().is_empty())
        .map(|p| LabeledValue {
            value: p.number.trim().to_owned(),
            kinds: phone_kinds(&p.kind),
            label: None,
            preferred: false,
        })
        .collect();
    fields.addresses = c
        .addresses
        .iter()
        .map(|a| PostalAddress {
            kinds: match a.kind.to_ascii_lowercase().as_str() {
                "home" => vec!["home".into()],
                "business" | "work" => vec!["work".into()],
                _ => Vec::new(),
            },
            street: some(a.street.as_deref()),
            locality: some(a.city.as_deref()),
            region: some(a.state.as_deref()),
            postal_code: some(a.postal_code.as_deref()),
            country: some(a.country.as_deref()),
            ..PostalAddress::default()
        })
        .collect();
    fields.organization = some(c.company.as_deref()).map(|name| ContactOrg {
        name,
        units: some(c.department.as_deref()).into_iter().collect(),
    });
    fields.title = some(c.job_title.as_deref());
    fields.birthday = c.birthday.as_deref().and_then(birthday);
    fields.notes = some(c.notes.as_deref());
    fields.urls = c
        .urls
        .iter()
        .filter(|u| !u.trim().is_empty())
        .map(|u| LabeledValue::new(u.trim()))
        .collect();
    fields.categories.clone_from(&c.categories);
    fields.photo = c.photo.as_deref().and_then(photo_of);
    MappedContact::Contact {
        uid,
        fields: Box::new(fields),
    }
}

/// A Nail contact for Exchange (`remote` = its server id and version).
/// Exchange keeps three email addresses: the preferred one first, the
/// rest dropped beyond three.
pub fn contact_to_exchange(
    fields: &ContactFields,
    remote: Option<(&str, Option<&str>)>,
) -> ExchangeContact {
    let name = fields.name.clone().unwrap_or_default();
    let mut emails: Vec<&LabeledValue> = fields.emails.iter().collect();
    emails.sort_by_key(|e| !e.preferred);
    ExchangeContact {
        remote_id: remote.map(|(id, _)| id.to_owned()).unwrap_or_default(),
        change_key: remote.and_then(|(_, k)| k.map(str::to_owned)),
        display_name: fields.display_name.clone(),
        given_name: name.given,
        surname: name.family,
        middle_name: name.additional,
        nickname: fields.nicknames.first().cloned(),
        company: fields.organization.as_ref().map(|o| o.name.clone()),
        department: fields
            .organization
            .as_ref()
            .and_then(|o| o.units.first().cloned()),
        job_title: fields.title.clone(),
        emails: emails
            .into_iter()
            .take(3)
            .map(|e| ExchangeEmail {
                address: e.value.clone(),
                name: None,
            })
            .collect(),
        phones: fields
            .phones
            .iter()
            .map(|p| ExchangePhone {
                kind: phone_kind(&p.kinds).to_owned(),
                number: p.value.clone(),
            })
            .collect(),
        addresses: fields
            .addresses
            .iter()
            .map(|a| ExchangePostalAddress {
                kind: address_kind(&a.kinds).to_owned(),
                street: a.street.clone(),
                city: a.locality.clone(),
                state: a.region.clone(),
                postal_code: a.postal_code.clone(),
                country: a.country.clone(),
            })
            .collect(),
        birthday: fields
            .birthday
            .map(|b| format!("{:04}-{:02}-{:02}", b.year.unwrap_or(1604), b.month, b.day)),
        notes: fields.notes.clone(),
        urls: fields.urls.iter().map(|u| u.value.clone()).collect(),
        categories: fields.categories.clone(),
        is_group: false,
        members: Vec::new(),
        photo: None,
    }
}

/// A contact group for Exchange (a distribution list).
pub fn group_to_exchange(
    name: &str,
    member_emails: &[String],
    remote: Option<(&str, Option<&str>)>,
) -> ExchangeContact {
    ExchangeContact {
        remote_id: remote.map(|(id, _)| id.to_owned()).unwrap_or_default(),
        change_key: remote.and_then(|(_, k)| k.map(str::to_owned)),
        display_name: name.to_owned(),
        given_name: None,
        surname: None,
        middle_name: None,
        nickname: None,
        company: None,
        department: None,
        job_title: None,
        emails: Vec::new(),
        phones: Vec::new(),
        addresses: Vec::new(),
        birthday: None,
        notes: None,
        urls: Vec::new(),
        categories: Vec::new(),
        is_group: true,
        members: member_emails
            .iter()
            .map(|m| ExchangeEmail {
                address: m.clone(),
                name: None,
            })
            .collect(),
        photo: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn bob() -> ExchangeContact {
        ExchangeContact {
            remote_id: "AAMkC".into(),
            change_key: Some("ck".into()),
            display_name: "Bob Builder".into(),
            given_name: Some("Bob".into()),
            surname: Some("Builder".into()),
            middle_name: None,
            nickname: Some("Bobby".into()),
            company: Some("Contoso".into()),
            department: Some("Sales".into()),
            job_title: Some("Manager".into()),
            emails: vec![
                ExchangeEmail {
                    address: "bob@contoso.example".into(),
                    name: Some("Bob".into()),
                },
                ExchangeEmail {
                    address: "bob@home.example".into(),
                    name: None,
                },
            ],
            phones: vec![
                ExchangePhone {
                    kind: "mobile".into(),
                    number: "+49 170 1234567".into(),
                },
                ExchangePhone {
                    kind: "business".into(),
                    number: "+49 30 123".into(),
                },
            ],
            addresses: vec![ExchangePostalAddress {
                kind: "business".into(),
                street: Some("Main St 1".into()),
                city: Some("Berlin".into()),
                state: None,
                postal_code: Some("10115".into()),
                country: Some("Germany".into()),
            }],
            birthday: Some("1980-05-17T00:00:00Z".into()),
            notes: Some("Met at the fair".into()),
            urls: vec!["https://contoso.example".into()],
            categories: vec!["Customers".into()],
            is_group: false,
            members: Vec::new(),
            photo: Some(vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]),
        }
    }

    #[test]
    fn contacts_map_to_nail_and_back() {
        let MappedContact::Contact { uid, fields } = contact_from_exchange(&bob()) else {
            panic!("a person");
        };
        assert!(uid.starts_with("contact-"));
        assert_eq!(fields.display_name, "Bob Builder");
        assert_eq!(fields.primary_email(), Some("bob@contoso.example"));
        assert_eq!(fields.phones[0].kinds, ["cell"]);
        assert_eq!(fields.addresses[0].kinds, ["work"]);
        assert_eq!(fields.organization.as_ref().unwrap().units, ["Sales"]);
        assert_eq!(
            fields.birthday,
            Some(ContactDate {
                year: Some(1980),
                month: 5,
                day: 17
            })
        );
        assert_eq!(
            fields.photo.as_ref().unwrap().mime_type.as_deref(),
            Some("image/jpeg")
        );
        assert!(fields.issues().is_empty(), "{:?}", fields.issues());
        let back = contact_to_exchange(&fields, Some(("AAMkC", Some("ck"))));
        let mut expected = bob();
        expected.emails[0].name = None;
        expected.birthday = Some("1980-05-17".into());
        expected.photo = None;
        assert_eq!(back, expected);
    }

    #[test]
    fn names_and_groups() {
        let mut nameless = bob();
        nameless.display_name = " ".into();
        nameless.company = None;
        let MappedContact::Contact { fields, .. } = contact_from_exchange(&nameless) else {
            panic!()
        };
        assert_eq!(fields.display_name, "Bob Builder");
        let mut only_mail = ExchangeContact {
            given_name: None,
            surname: None,
            nickname: None,
            ..nameless
        };
        only_mail.photo = Some(b"GIF89a".to_vec());
        let MappedContact::Contact { fields, .. } = contact_from_exchange(&only_mail) else {
            panic!()
        };
        assert_eq!(fields.display_name, "bob@contoso.example");
        assert_eq!(fields.photo, None, "only JPEG / PNG");

        let list = group_to_exchange("Team", &["a@x.example".into(), "b@x.example".into()], None);
        let MappedContact::Group {
            name,
            member_emails,
            ..
        } = contact_from_exchange(&list)
        else {
            panic!("a group");
        };
        assert_eq!(name, "Team");
        assert_eq!(member_emails, ["a@x.example", "b@x.example"]);
        assert_eq!(
            birthday("1604-03-01"),
            Some(ContactDate {
                year: None,
                month: 3,
                day: 1
            })
        );
    }
}
