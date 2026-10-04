//! Contacts (`t:Contact`) and distribution lists (`t:DistributionList`) ↔
//! [`ExchangeContact`].
//!
//! EWS keeps addresses in keyed dictionaries: three e-mail slots
//! (`EmailAddress1`–`3`), phone keys (`HomePhone`, `MobilePhone`, …) and
//! three postal addresses (`Home`, `Business`, `Other`). Nail's phone
//! kinds map onto those keys; numbers beyond the free keys of a kind are
//! dropped. One web page is kept (`BusinessHomePage`).

use std::fmt::Write as _;

use nail_api::exchange::pim::{
    ExchangeContact, ExchangeEmail, ExchangePhone, ExchangePostalAddress,
};

use crate::mail::categories_xml;
use crate::types::{Field, ItemRef, Update, parse_date};
use crate::xml::{Element, el, escape};

/// The contact properties Nail reads with `GetItem` (`AllProperties`
/// covers the dictionaries; these are the extras).
pub const CONTACT_SHAPE: &str = concat!(
    "<m:ItemShape><t:BaseShape>AllProperties</t:BaseShape><t:AdditionalProperties>",
    r#"<t:FieldURI FieldURI="item:Body"/>"#,
    r#"<t:FieldURI FieldURI="contacts:Birthday"/>"#,
    r#"<t:FieldURI FieldURI="distributionlist:Members"/>"#,
    "</t:AdditionalProperties></m:ItemShape>"
);

const EMAIL_KEYS: [&str; 3] = ["EmailAddress1", "EmailAddress2", "EmailAddress3"];

/// Phone kinds and the EWS keys they use, in order.
const PHONE_KEYS: [(&str, &[&str]); 5] = [
    ("home", &["HomePhone", "HomePhone2"]),
    (
        "business",
        &["BusinessPhone", "BusinessPhone2", "CompanyMainPhone"],
    ),
    ("mobile", &["MobilePhone"]),
    ("fax", &["BusinessFax", "HomeFax", "OtherFax"]),
    (
        "other",
        &["OtherTelephone", "PrimaryPhone", "CarPhone", "Pager"],
    ),
];

const ADDRESS_KEYS: [(&str, &str); 3] = [
    ("home", "Home"),
    ("business", "Business"),
    ("other", "Other"),
];

const ADDRESS_PARTS: [&str; 5] = ["Street", "City", "State", "CountryOrRegion", "PostalCode"];

fn phone_kind(key: &str) -> &'static str {
    PHONE_KEYS
        .iter()
        .find(|(_, keys)| keys.contains(&key))
        .map_or("other", |(kind, _)| kind)
}

fn entries<'a>(item: &'a Element, list: &str) -> impl Iterator<Item = &'a Element> + 'a {
    item.child(list)
        .into_iter()
        .flat_map(|l| l.children_named("Entry"))
}

/// A contact or distribution list element as an [`ExchangeContact`].
pub fn parse_contact(item: &Element) -> Option<ExchangeContact> {
    let id = item.child("ItemId").and_then(ItemRef::from_element)?;
    let is_group = item.name == "DistributionList";
    let mut emails = Vec::new();
    for key in EMAIL_KEYS {
        if let Some(entry) = entries(item, "EmailAddresses").find(|e| e.attr("Key") == Some(key)) {
            let address = entry.text.trim();
            if !address.is_empty() {
                emails.push(ExchangeEmail {
                    address: address.strip_prefix("SMTP:").unwrap_or(address).to_owned(),
                    name: entry.attr("Name").map(str::to_owned),
                });
            }
        }
    }
    let phones = entries(item, "PhoneNumbers")
        .filter(|e| !e.text.trim().is_empty())
        .map(|e| ExchangePhone {
            kind: phone_kind(e.attr("Key").unwrap_or("")).to_owned(),
            number: e.text.trim().to_owned(),
        })
        .collect();
    let addresses = entries(item, "PhysicalAddresses")
        .filter_map(|e| {
            let kind = ADDRESS_KEYS
                .iter()
                .find(|(_, k)| e.attr("Key") == Some(k))
                .map(|(kind, _)| *kind)?;
            let address = ExchangePostalAddress {
                kind: kind.to_owned(),
                street: e.child_value("Street"),
                city: e.child_value("City"),
                state: e.child_value("State"),
                postal_code: e.child_value("PostalCode"),
                country: e.child_value("CountryOrRegion"),
            };
            (address
                != ExchangePostalAddress {
                    kind: kind.to_owned(),
                    ..ExchangePostalAddress::default()
                })
            .then_some(address)
        })
        .collect();
    let members = item
        .child("Members")
        .map(|list| {
            list.children_named("Member")
                .filter_map(|m| {
                    let mailbox = m.child("Mailbox")?;
                    Some(ExchangeEmail {
                        address: mailbox.child_value("EmailAddress")?,
                        name: mailbox.child_value("Name"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ExchangeContact {
        remote_id: id.id,
        change_key: id.change_key,
        display_name: item
            .child_value("DisplayName")
            .or_else(|| item.child_value("FileAs"))
            .unwrap_or_default(),
        given_name: item.child_value("GivenName"),
        surname: item.child_value("Surname"),
        middle_name: item.child_value("MiddleName"),
        nickname: item.child_value("Nickname"),
        company: item.child_value("CompanyName"),
        department: item.child_value("Department"),
        job_title: item.child_value("JobTitle"),
        emails,
        phones,
        addresses,
        birthday: item
            .child_text("Birthday")
            .and_then(parse_date)
            .map(|d| d.format("%Y-%m-%d").to_string()),
        notes: item
            .child("Body")
            .map(|b| b.text.trim().to_owned())
            .filter(|t| !t.is_empty()),
        urls: item.child_value("BusinessHomePage").into_iter().collect(),
        categories: item.strings("Categories", "String"),
        is_group,
        members,
        photo: None,
    })
}

fn opt(tag: &str, value: Option<&String>) -> Option<String> {
    value.filter(|v| !v.is_empty()).map(|v| el(tag, v))
}

/// The phone entries of a contact by EWS key.
fn phone_entries(contact: &ExchangeContact) -> Vec<(&'static str, String)> {
    let mut used: Vec<&'static str> = Vec::new();
    let mut out = Vec::new();
    for phone in &contact.phones {
        let keys = PHONE_KEYS
            .iter()
            .find(|(kind, _)| kind.eq_ignore_ascii_case(&phone.kind))
            .map_or(PHONE_KEYS[4].1, |(_, keys)| *keys);
        if let Some(key) = keys.iter().find(|k| !used.contains(k)) {
            used.push(key);
            out.push((*key, phone.number.clone()));
        }
    }
    out
}

fn address_entry(address: &ExchangePostalAddress, key: &str) -> String {
    let parts: String = ADDRESS_PARTS
        .iter()
        .filter_map(|part| {
            let value = match *part {
                "Street" => &address.street,
                "City" => &address.city,
                "State" => &address.state,
                "CountryOrRegion" => &address.country,
                _ => &address.postal_code,
            };
            opt(&format!("t:{part}"), value.as_ref())
        })
        .collect();
    format!(r#"<t:Entry Key="{key}">{parts}</t:Entry>"#)
}

fn address_key(kind: &str) -> Option<&'static str> {
    ADDRESS_KEYS
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(kind))
        .map(|(_, key)| *key)
}

/// The `<t:Contact>` / `<t:DistributionList>` of a new contact.
pub fn contact_create_xml(contact: &ExchangeContact) -> String {
    let notes = contact
        .notes
        .as_ref()
        .map(|n| format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, escape(n)))
        .unwrap_or_default();
    let categories = if contact.categories.is_empty() {
        String::new()
    } else {
        categories_xml(&contact.categories)
    };
    if contact.is_group {
        let members: String = contact.members.iter().fold(String::new(), |mut out, m| {
            let name = m
                .name
                .as_deref()
                .map(|n| el("t:Name", n))
                .unwrap_or_default();
            let _ = write!(
                out,
                "<t:Member><t:Mailbox>{name}{}</t:Mailbox></t:Member>",
                el("t:EmailAddress", &m.address)
            );
            out
        });
        return format!(
            "<t:DistributionList>{notes}{categories}{}{}<t:Members>{members}</t:Members></t:DistributionList>",
            el("t:FileAs", &contact.display_name),
            el("t:DisplayName", &contact.display_name)
        );
    }
    let emails: String =
        contact
            .emails
            .iter()
            .zip(EMAIL_KEYS)
            .fold(String::new(), |mut out, (e, key)| {
                let _ = write!(
                    out,
                    r#"<t:Entry Key="{key}">{}</t:Entry>"#,
                    escape(&e.address)
                );
                out
            });
    let addresses: String = contact
        .addresses
        .iter()
        .filter_map(|a| address_key(&a.kind).map(|key| address_entry(a, key)))
        .collect();
    let phones: String =
        phone_entries(contact)
            .into_iter()
            .fold(String::new(), |mut out, (key, number)| {
                let _ = write!(out, r#"<t:Entry Key="{key}">{}</t:Entry>"#, escape(&number));
                out
            });
    let wrap = |tag: &str, inner: String| {
        if inner.is_empty() {
            String::new()
        } else {
            format!("<t:{tag}>{inner}</t:{tag}>")
        }
    };
    let fields = [
        Some(notes),
        Some(categories),
        Some(el("t:FileAs", &contact.display_name)),
        Some(el("t:DisplayName", &contact.display_name)),
        opt("t:GivenName", contact.given_name.as_ref()),
        opt("t:MiddleName", contact.middle_name.as_ref()),
        opt("t:Nickname", contact.nickname.as_ref()),
        opt("t:CompanyName", contact.company.as_ref()),
        Some(wrap("EmailAddresses", emails)),
        Some(wrap("PhysicalAddresses", addresses)),
        Some(wrap("PhoneNumbers", phones)),
        contact
            .birthday
            .as_ref()
            .map(|b| el("t:Birthday", &format!("{b}T00:00:00Z"))),
        opt("t:BusinessHomePage", contact.urls.first()),
        opt("t:Department", contact.department.as_ref()),
        opt("t:JobTitle", contact.job_title.as_ref()),
        opt("t:Surname", contact.surname.as_ref()),
    ];
    format!(
        "<t:Contact>{}</t:Contact>",
        fields.into_iter().flatten().collect::<String>()
    )
}

/// The item element name of a contact (`Contact` / `DistributionList`).
pub fn contact_tag(contact: &ExchangeContact) -> &'static str {
    if contact.is_group {
        "DistributionList"
    } else {
        "Contact"
    }
}

/// The updates that replace a contact's fields (dictionary entries Nail
/// manages are set or removed one by one).
pub fn contact_updates(contact: &ExchangeContact) -> Vec<Update> {
    let set_or_delete = |field: Field, value: Option<String>| match value {
        Some(v) => Update::Set(field, v),
        None => Update::Delete(field),
    };
    let mut out = vec![
        set_or_delete(
            Field::Uri("item:Body"),
            contact
                .notes
                .as_ref()
                .map(|n| format!(r#"<t:Body BodyType="Text">{}</t:Body>"#, escape(n))),
        ),
        set_or_delete(
            Field::Uri("item:Categories"),
            (!contact.categories.is_empty()).then(|| categories_xml(&contact.categories)),
        ),
        Update::Set(
            Field::Uri("contacts:FileAs"),
            el("t:FileAs", &contact.display_name),
        ),
        Update::Set(
            Field::Uri("contacts:DisplayName"),
            el("t:DisplayName", &contact.display_name),
        ),
    ];
    if contact.is_group {
        let members: String = contact.members.iter().fold(String::new(), |mut out, m| {
            let _ = write!(
                out,
                "<t:Member><t:Mailbox>{}</t:Mailbox></t:Member>",
                el("t:EmailAddress", &m.address)
            );
            out
        });
        out.push(Update::Set(
            Field::Uri("distributionlist:Members"),
            format!("<t:Members>{members}</t:Members>"),
        ));
        return out;
    }
    for (uri, tag, value) in [
        ("contacts:GivenName", "t:GivenName", &contact.given_name),
        ("contacts:MiddleName", "t:MiddleName", &contact.middle_name),
        ("contacts:Nickname", "t:Nickname", &contact.nickname),
        ("contacts:CompanyName", "t:CompanyName", &contact.company),
        ("contacts:Department", "t:Department", &contact.department),
        ("contacts:JobTitle", "t:JobTitle", &contact.job_title),
        ("contacts:Surname", "t:Surname", &contact.surname),
    ] {
        out.push(set_or_delete(Field::Uri(uri), opt(tag, value.as_ref())));
    }
    out.push(set_or_delete(
        Field::Uri("contacts:BusinessHomePage"),
        opt("t:BusinessHomePage", contact.urls.first()),
    ));
    out.push(set_or_delete(
        Field::Uri("contacts:Birthday"),
        contact
            .birthday
            .as_ref()
            .map(|b| el("t:Birthday", &format!("{b}T00:00:00Z"))),
    ));
    for (i, key) in EMAIL_KEYS.iter().enumerate() {
        let field = Field::Indexed("contacts:EmailAddress", (*key).to_owned());
        out.push(set_or_delete(
            field,
            contact.emails.get(i).map(|e| {
                format!(
                    r#"<t:EmailAddresses><t:Entry Key="{key}">{}</t:Entry></t:EmailAddresses>"#,
                    escape(&e.address)
                )
            }),
        ));
    }
    let phones = phone_entries(contact);
    for key in PHONE_KEYS.iter().flat_map(|(_, keys)| keys.iter()) {
        let field = Field::Indexed("contacts:PhoneNumber", (*key).to_owned());
        out.push(set_or_delete(
            field,
            phones.iter().find(|(k, _)| k == key).map(|(_, number)| {
                format!(
                    r#"<t:PhoneNumbers><t:Entry Key="{key}">{}</t:Entry></t:PhoneNumbers>"#,
                    escape(number)
                )
            }),
        ));
    }
    for (kind, key) in ADDRESS_KEYS {
        let address = contact
            .addresses
            .iter()
            .find(|a| a.kind.eq_ignore_ascii_case(kind));
        for part in ADDRESS_PARTS {
            let value = address.and_then(|a| match part {
                "Street" => a.street.clone(),
                "City" => a.city.clone(),
                "State" => a.state.clone(),
                "CountryOrRegion" => a.country.clone(),
                _ => a.postal_code.clone(),
            });
            let field = Field::Indexed(physical_uri(part), key.to_owned());
            out.push(set_or_delete(
                field,
                value.filter(|v| !v.is_empty()).map(|v| {
                    format!(
                        r#"<t:PhysicalAddresses><t:Entry Key="{key}">{}</t:Entry></t:PhysicalAddresses>"#,
                        el(&format!("t:{part}"), &v)
                    )
                }),
            ));
        }
    }
    out
}

fn physical_uri(part: &str) -> &'static str {
    match part {
        "Street" => "contacts:PhysicalAddress:Street",
        "City" => "contacts:PhysicalAddress:City",
        "State" => "contacts:PhysicalAddress:State",
        "CountryOrRegion" => "contacts:PhysicalAddress:CountryOrRegion",
        _ => "contacts:PhysicalAddress:PostalCode",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    const CONTACT: &str = concat!(
        r#"<t:Contact xmlns:t="t"><t:ItemId Id="C1" ChangeKey="EQAA"/><t:Body BodyType="Text">Met at FOSDEM</t:Body>"#,
        r#"<t:Categories><t:String>Friends</t:String></t:Categories><t:FileAs>Lovelace, Ada</t:FileAs>"#,
        r#"<t:DisplayName>Ada Lovelace</t:DisplayName><t:GivenName>Ada</t:GivenName><t:CompanyName>Analytical</t:CompanyName>"#,
        r#"<t:EmailAddresses><t:Entry Key="EmailAddress1" Name="Ada">ada@corp.example</t:Entry>"#,
        r#"<t:Entry Key="EmailAddress2">SMTP:ada@home.example</t:Entry></t:EmailAddresses>"#,
        r#"<t:PhysicalAddresses><t:Entry Key="Business"><t:Street>1 Main St</t:Street><t:City>London</t:City>"#,
        r#"<t:CountryOrRegion>UK</t:CountryOrRegion></t:Entry><t:Entry Key="Home"/></t:PhysicalAddresses>"#,
        r#"<t:PhoneNumbers><t:Entry Key="MobilePhone">+44 1</t:Entry><t:Entry Key="BusinessFax">+44 2</t:Entry>"#,
        r#"<t:Entry Key="HomePhone"/></t:PhoneNumbers><t:Birthday>1815-12-10T00:00:00Z</t:Birthday>"#,
        r#"<t:BusinessHomePage>https://ada.example</t:BusinessHomePage><t:JobTitle>Analyst</t:JobTitle>"#,
        r#"<t:Surname>Lovelace</t:Surname></t:Contact>"#
    );

    #[test]
    fn contacts_round_trip() {
        let contact = parse_contact(&parse(CONTACT).unwrap()).unwrap();
        assert_eq!(contact.display_name, "Ada Lovelace");
        assert_eq!(contact.emails.len(), 2);
        assert_eq!(contact.emails[0].name.as_deref(), Some("Ada"));
        assert_eq!(contact.emails[1].address, "ada@home.example");
        assert_eq!(contact.phones.len(), 2);
        assert_eq!(contact.phones[0].kind, "mobile");
        assert_eq!(contact.phones[1].kind, "fax");
        assert_eq!(contact.addresses.len(), 1);
        assert_eq!(contact.addresses[0].kind, "business");
        assert_eq!(contact.addresses[0].country.as_deref(), Some("UK"));
        assert_eq!(contact.birthday.as_deref(), Some("1815-12-10"));
        assert_eq!(contact.notes.as_deref(), Some("Met at FOSDEM"));
        assert_eq!(contact.urls, ["https://ada.example"]);
        assert!(!contact.is_group);
        let xml = contact_create_xml(&contact);
        let wrapped = parse(&format!(r#"<x xmlns:t="t">{xml}</x>"#)).unwrap();
        let mut back = wrapped.child("Contact").unwrap().clone();
        back.children
            .insert(0, parse(r#"<ItemId Id="C1" ChangeKey="EQAA"/>"#).unwrap());
        let mut reparsed = parse_contact(&back).unwrap();
        reparsed.emails[0].name = Some("Ada".into());
        assert_eq!(reparsed, contact);
        let updates = contact_updates(&contact);
        let xml: String = updates.iter().map(|u| u.xml("Contact")).collect();
        assert!(xml.contains(r#"FieldIndex="EmailAddress3""#));
        assert!(xml.contains(r#"<t:Entry Key="MobilePhone">+44 1</t:Entry>"#));
        assert!(
            xml.contains(r#"FieldURI="contacts:PhysicalAddress:Street" FieldIndex="Business""#)
        );
        assert!(updates.contains(&Update::Delete(Field::Uri("contacts:Nickname"))));
    }

    #[test]
    fn distribution_lists() {
        let item = parse(concat!(
            r#"<DistributionList><ItemId Id="D1"/><DisplayName>Team</DisplayName><Members>"#,
            r#"<Member><Mailbox><Name>Bob</Name><EmailAddress>bob@corp.example</EmailAddress></Mailbox></Member>"#,
            r#"<Member><Mailbox><Name>Nobody</Name></Mailbox></Member></Members></DistributionList>"#
        ))
        .unwrap();
        let list = parse_contact(&item).unwrap();
        assert!(list.is_group);
        assert_eq!(list.members.len(), 1);
        assert_eq!(contact_tag(&list), "DistributionList");
        let xml = contact_create_xml(&list);
        assert!(xml.starts_with("<t:DistributionList>"));
        assert!(xml.contains("<t:EmailAddress>bob@corp.example</t:EmailAddress>"));
        let updates = contact_updates(&list);
        assert!(
            updates
                .iter()
                .any(|u| matches!(u, Update::Set(Field::Uri("distributionlist:Members"), _)))
        );
    }
}
