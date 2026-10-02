//! Thunderbird address books in SQLite (`abook.sqlite`, `history.sqlite`,
//! `abook-N.sqlite`; Thunderbird 78+): `properties(card, name, value)`,
//! `lists(uid, name, nickName, description)`, `list_cards(list, card)`.
//! Cards with a `_vCard` property (Thunderbird 102+) are read from it;
//! older cards are built from their named properties.

use std::collections::BTreeMap;
use std::path::Path;

use nail_ical::content::escape_text;
use nail_ical::vcard::{ParsedCards, parse_cards};
use rusqlite::{Connection, OpenFlags};

use crate::error::{PortabilityError, Result};

/// Card properties by card id, in id order.
pub type CardProperties = BTreeMap<String, BTreeMap<String, String>>;

/// A mailing list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MailList {
    /// Its id.
    pub uid: String,
    /// Its name.
    pub name: String,
    /// Its nick name.
    pub nick_name: Option<String>,
    /// Its description.
    pub description: Option<String>,
    /// Member card ids.
    pub members: Vec<String>,
}

/// Reads an address book file read-only into cards and groups.
pub fn read_sqlite(path: &Path) -> Result<ParsedCards> {
    let (cards, lists) = read_tables(path)?;
    Ok(cards_from(&cards, &lists))
}

/// Reads the tables.
pub fn read_tables(path: &Path) -> Result<(CardProperties, Vec<MailList>)> {
    let sql =
        |e: rusqlite::Error| PortabilityError::InvalidSource(format!("{}: {e}", path.display()));
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sql)?;
    let mut cards = CardProperties::new();
    {
        let mut statement = connection
            .prepare("SELECT card, name, value FROM properties ORDER BY card")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (card, name, value) = row.map_err(sql)?;
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                cards.entry(card).or_default().insert(name, value);
            }
        }
    }
    let mut lists = Vec::new();
    let has_lists: bool = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'lists'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sql)?
        > 0;
    if has_lists {
        let mut statement = connection
            .prepare("SELECT uid, name, nickName, description FROM lists ORDER BY name")
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(MailList {
                    uid: row.get(0)?,
                    name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    nick_name: row.get::<_, Option<String>>(2)?.filter(|s| !s.is_empty()),
                    description: row.get::<_, Option<String>>(3)?.filter(|s| !s.is_empty()),
                    members: Vec::new(),
                })
            })
            .map_err(sql)?;
        for row in rows {
            lists.push(row.map_err(sql)?);
        }
        let mut members = connection
            .prepare("SELECT card FROM list_cards WHERE list = ?1 ORDER BY card")
            .map_err(sql)?;
        for list in &mut lists {
            let rows = members
                .query_map([&list.uid], |row| row.get::<_, String>(0))
                .map_err(sql)?;
            for row in rows {
                list.members.push(row.map_err(sql)?);
            }
        }
    }
    Ok((cards, lists))
}

/// Builds cards and groups from Thunderbird card properties and lists.
pub fn cards_from(cards: &CardProperties, lists: &[MailList]) -> ParsedCards {
    let mut text = String::new();
    for (uid, properties) in cards {
        text.push_str(&card_text(uid, properties));
    }
    for list in lists {
        text.push_str(&list_text(list));
    }
    if text.is_empty() {
        return ParsedCards::default();
    }
    parse_cards(&text)
}

fn line(out: &mut String, name: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    out.push_str(name);
    out.push(':');
    out.push_str(&escape_text(value));
    out.push_str("\r\n");
}

/// The vCard of one card: its `_vCard` (with the card's `UID` added when
/// missing), else vCard 4 built from Thunderbird's property names.
pub fn card_text(uid: &str, p: &BTreeMap<String, String>) -> String {
    let get = |name: &str| p.get(name).map(String::as_str).unwrap_or("");
    if let Some(vcard) = p.get("_vCard").filter(|v| v.contains("BEGIN:VCARD")) {
        let mut text = vcard.replace("\r\n", "\n").replace('\n', "\r\n");
        if !text.to_ascii_uppercase().contains("\r\nUID:") {
            text = text.replacen("\r\nEND:VCARD", &format!("\r\nUID:{uid}\r\nEND:VCARD"), 1);
        }
        if !text.ends_with("\r\n") {
            text.push_str("\r\n");
        }
        return text;
    }
    let mut out = String::from("BEGIN:VCARD\r\nVERSION:4.0\r\n");
    line(&mut out, "UID", uid);
    let display = [get("DisplayName"), get("PrimaryEmail")]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!("{} {}", get("FirstName"), get("LastName"))
                .trim()
                .to_owned()
        });
    line(&mut out, "FN", &display);
    if !get("FirstName").is_empty() || !get("LastName").is_empty() {
        out.push_str(&format!(
            "N:{};{};;;\r\n",
            escape_text(get("LastName")),
            escape_text(get("FirstName"))
        ));
    }
    line(&mut out, "NICKNAME", get("NickName"));
    line(&mut out, "EMAIL;PREF=1", get("PrimaryEmail"));
    line(&mut out, "EMAIL", get("SecondEmail"));
    line(&mut out, "TEL;TYPE=work", get("WorkPhone"));
    line(&mut out, "TEL;TYPE=home", get("HomePhone"));
    line(&mut out, "TEL;TYPE=cell", get("CellularNumber"));
    line(&mut out, "TEL;TYPE=fax", get("FaxNumber"));
    line(&mut out, "TEL;TYPE=pager", get("PagerNumber"));
    if !get("Company").is_empty() || !get("Department").is_empty() {
        out.push_str(&format!(
            "ORG:{};{}\r\n",
            escape_text(get("Company")),
            escape_text(get("Department"))
        ));
    }
    line(&mut out, "TITLE", get("JobTitle"));
    line(&mut out, "NOTE", get("Notes"));
    line(&mut out, "URL;TYPE=work", get("WebPage1"));
    line(&mut out, "URL;TYPE=home", get("WebPage2"));
    for (kind, prefix) in [("home", "Home"), ("work", "Work")] {
        let parts = ["Address2", "Address", "City", "State", "ZipCode", "Country"]
            .map(|f| get(&format!("{prefix}{f}")).to_owned());
        if parts.iter().any(|s| !s.trim().is_empty()) {
            out.push_str(&format!(
                "ADR;TYPE={kind}:;{};{};{};{};{};{}\r\n",
                escape_text(&parts[0]),
                escape_text(&parts[1]),
                escape_text(&parts[2]),
                escape_text(&parts[3]),
                escape_text(&parts[4]),
                escape_text(&parts[5]),
            ));
        }
    }
    let (y, m, d) = (get("BirthYear"), get("BirthMonth"), get("BirthDay"));
    if let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>()) {
        match y.parse::<u32>() {
            Ok(y) => out.push_str(&format!("BDAY:{y:04}{m:02}{d:02}\r\n")),
            Err(_) => out.push_str(&format!("BDAY:--{m:02}{d:02}\r\n")),
        }
    }
    out.push_str("END:VCARD\r\n");
    out
}

/// The group card of a mailing list.
pub fn list_text(list: &MailList) -> String {
    let mut out = String::from("BEGIN:VCARD\r\nVERSION:4.0\r\nKIND:group\r\n");
    line(&mut out, "UID", &list.uid);
    line(&mut out, "FN", &list.name);
    if let Some(nick) = &list.nick_name {
        line(&mut out, "NICKNAME", nick);
    }
    if let Some(description) = &list.description {
        line(&mut out, "NOTE", description);
    }
    for member in &list.members {
        out.push_str(&format!("MEMBER:urn:uuid:{member}\r\n"));
    }
    out.push_str("END:VCARD\r\n");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Creates a Thunderbird-style address book (also used by the profile
    /// fixture generator).
    pub(crate) fn create_abook(
        path: &Path,
        cards: &[(&str, &[(&str, &str)])],
        lists: &[(&str, &str, &[&str])],
    ) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE properties (card TEXT, name TEXT, value TEXT);
                 CREATE TABLE lists (uid TEXT PRIMARY KEY, name TEXT, nickName TEXT, description TEXT);
                 CREATE TABLE list_cards (list TEXT, card TEXT, PRIMARY KEY(list, card));",
            )
            .unwrap();
        for (card, properties) in cards {
            for (name, value) in *properties {
                connection
                    .execute(
                        "INSERT INTO properties (card, name, value) VALUES (?1, ?2, ?3)",
                        [card, name, value],
                    )
                    .unwrap();
            }
        }
        for (uid, name, members) in lists {
            connection
                .execute(
                    "INSERT INTO lists (uid, name, nickName, description) VALUES (?1, ?2, '', '')",
                    [uid, name],
                )
                .unwrap();
            for member in *members {
                connection
                    .execute(
                        "INSERT INTO list_cards (list, card) VALUES (?1, ?2)",
                        [uid, member],
                    )
                    .unwrap();
            }
        }
    }

    #[test]
    fn reads_cards_lists_and_vcard_properties() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abook.sqlite");
        create_abook(
            &path,
            &[
                (
                    "card-1",
                    &[
                        ("DisplayName", "Ada Lovelace"),
                        ("FirstName", "Ada"),
                        ("LastName", "Lovelace"),
                        ("PrimaryEmail", "ada@example.org"),
                        ("WorkPhone", "+44 1"),
                        ("Company", "Analytical; Engines"),
                        ("BirthMonth", "12"),
                        ("BirthDay", "10"),
                        ("BirthYear", "1815"),
                    ],
                ),
                (
                    "card-2",
                    &[(
                        "_vCard",
                        "BEGIN:VCARD\nVERSION:4.0\nFN:Charles Babbage\nEMAIL:charles@example.org\nEND:VCARD",
                    )],
                ),
                ("card-3", &[("PrimaryEmail", "only@example.org")]),
            ],
            &[("list-1", "Engineers", &["card-1", "card-2"])],
        );
        let cards = read_sqlite(&path).unwrap();
        assert!(cards.issues.is_empty(), "{:?}", cards.issues);
        assert_eq!(cards.contacts.len(), 3);
        let ada = &cards.contacts[0];
        assert_eq!(ada.uid, "card-1");
        assert_eq!(ada.fields.display_name, "Ada Lovelace");
        assert_eq!(ada.fields.emails[0].value, "ada@example.org");
        assert_eq!(cards.contacts[1].uid, "card-2");
        assert_eq!(cards.contacts[1].fields.display_name, "Charles Babbage");
        assert_eq!(cards.contacts[2].fields.display_name, "only@example.org");
        assert_eq!(cards.groups.len(), 1);
        assert_eq!(cards.groups[0].group.name, "Engineers");
        assert_eq!(cards.groups[0].group.member_uids, vec!["card-1", "card-2"]);
    }

    #[test]
    fn not_a_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abook.sqlite");
        std::fs::write(&path, "not sqlite at all, just text that is long enough").unwrap();
        assert!(matches!(
            read_sqlite(&path),
            Err(PortabilityError::InvalidSource(_))
        ));
    }
}
