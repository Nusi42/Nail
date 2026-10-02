//! A small reader for Mork (`.mab` address books of Thunderbird before
//! version 78): dictionaries `< (id=value) >` (the column dictionary
//! marked `<(a=c)>`), tables `{ … }`, rows `[id(^col^val)(^col=literal)]`,
//! transaction groups `@$${n{@ … @$$}n}@` (aborted groups are dropped),
//! `\` / `$XX` escapes and `//` comments. Enough for address books; other
//! Mork files are not needed.

use std::collections::HashMap;
use std::path::Path;

use nail_ical::vcard::ParsedCards;

use super::abook::{CardProperties, MailList, cards_from};
use crate::error::{PortabilityError, Result};

/// The rows of a Mork file: row id → column name → value, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MorkRows {
    /// Row ids in first-seen order.
    pub order: Vec<String>,
    /// The cells of each row.
    pub rows: HashMap<String, HashMap<String, String>>,
}

#[derive(Clone)]
struct State {
    columns: HashMap<String, String>,
    values: HashMap<String, String>,
    rows: MorkRows,
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    state: State,
    /// The state before the current group (restored on abort).
    group_start: Option<State>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(s)
    }

    fn skip_space_and_comments(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else if self.starts_with(b"//") {
                while let Some(b) = self.peek() {
                    self.pos += 1;
                    if b == b'\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Reads an id up to one of `stops` (or whitespace).
    fn id(&mut self, stops: &[u8]) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if stops.contains(&b) || b.is_ascii_whitespace() {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned()
    }

    /// Reads a literal value up to the closing `)`, undoing escapes.
    fn literal(&mut self) -> String {
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            match b {
                b')' => break,
                b'\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'\r') => {
                            self.pos += 1;
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        Some(b'\n') => self.pos += 1,
                        Some(c) => {
                            out.push(c);
                            self.pos += 1;
                        }
                        None => break,
                    }
                }
                b'$' => {
                    let hex = self.bytes.get(self.pos + 1..self.pos + 3);
                    match hex
                        .and_then(|h| std::str::from_utf8(h).ok())
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                    {
                        Some(byte) => {
                            out.push(byte);
                            self.pos += 3;
                        }
                        None => {
                            out.push(b'$');
                            self.pos += 1;
                        }
                    }
                }
                _ => {
                    out.push(b);
                    self.pos += 1;
                }
            }
        }
        match String::from_utf8(out) {
            Ok(text) => text,
            // Old books are Latin-1.
            Err(e) => e.into_bytes().iter().map(|&b| char::from(b)).collect(),
        }
    }

    fn dict(&mut self) {
        // After `<`.
        let mut columns = false;
        loop {
            self.skip_space_and_comments();
            match self.peek() {
                None => return,
                Some(b'>') => {
                    self.pos += 1;
                    return;
                }
                Some(b'<') => {
                    // Meta dictionary: `<(a=c)>`.
                    self.pos += 1;
                    let start = self.pos;
                    while let Some(b) = self.peek() {
                        self.pos += 1;
                        if b == b'>' {
                            break;
                        }
                    }
                    let meta =
                        String::from_utf8_lossy(&self.bytes[start..self.pos]).replace(' ', "");
                    if meta.contains("(a=c)") {
                        columns = true;
                    }
                }
                Some(b'(') => {
                    self.pos += 1;
                    let id = self.id(b"=)");
                    let value = if self.peek() == Some(b'=') {
                        self.pos += 1;
                        self.literal()
                    } else {
                        String::new()
                    };
                    if self.peek() == Some(b')') {
                        self.pos += 1;
                    }
                    if columns {
                        self.state.columns.insert(id.to_ascii_uppercase(), value);
                    } else {
                        self.state.values.insert(id.to_ascii_uppercase(), value);
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn row(&mut self, table_scope: Option<&str>) {
        // After `[`.
        self.skip_space_and_comments();
        let cut = self.peek() == Some(b'-');
        if cut {
            self.pos += 1;
        }
        let raw_id = self.id(b"([]");
        let key = row_key(&raw_id, table_scope);
        if cut {
            self.state.rows.rows.remove(&key);
            self.state.rows.order.retain(|k| k != &key);
        }
        loop {
            self.skip_space_and_comments();
            match self.peek() {
                None => return,
                Some(b']') => {
                    self.pos += 1;
                    return;
                }
                Some(b'[') => {
                    // Row meta: skip.
                    self.skip_balanced(b'[', b']');
                }
                Some(b'(') => {
                    self.pos += 1;
                    let column = if self.peek() == Some(b'^') {
                        self.pos += 1;
                        let id = self.id(b"^=)").to_ascii_uppercase();
                        self.state.columns.get(&id).cloned().unwrap_or(id)
                    } else {
                        self.id(b"^=)")
                    };
                    let value = match self.peek() {
                        Some(b'^') => {
                            self.pos += 1;
                            let id = self.id(b")").to_ascii_uppercase();
                            self.state.values.get(&id).cloned().unwrap_or_default()
                        }
                        Some(b'=') => {
                            self.pos += 1;
                            self.literal()
                        }
                        _ => String::new(),
                    };
                    if self.peek() == Some(b')') {
                        self.pos += 1;
                    }
                    if !self.state.rows.rows.contains_key(&key) {
                        self.state.rows.order.push(key.clone());
                    }
                    self.state
                        .rows
                        .rows
                        .entry(key.clone())
                        .or_default()
                        .insert(column, value);
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn skip_balanced(&mut self, open: u8, close: u8) {
        let mut depth = 0usize;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == open {
                depth += 1;
            } else if b == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return;
                }
            }
        }
    }

    fn table(&mut self) {
        // After `{`.
        self.skip_space_and_comments();
        let id = self.id(b"{[}");
        let scope = id
            .split_once(':')
            .map(|(_, s)| s.trim_start_matches('^').to_ascii_uppercase());
        loop {
            self.skip_space_and_comments();
            match self.peek() {
                None => return,
                Some(b'}') => {
                    self.pos += 1;
                    return;
                }
                Some(b'{') => self.skip_balanced(b'{', b'}'),
                Some(b'[') => {
                    self.pos += 1;
                    self.row(scope.as_deref());
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn group(&mut self) -> bool {
        // `@$${id{@` starts, `@$$}id}@` ends, `@$$}~abort~id}@` aborts.
        if self.starts_with(b"@$${") {
            self.group_start = Some(self.state.clone());
            self.skip_to(b"{@");
            true
        } else if self.starts_with(b"@$$}") {
            let start = self.pos;
            self.skip_to(b"}@");
            let marker = &self.bytes[start..self.pos];
            if marker.windows(7).any(|w| w == b"~abort~")
                && let Some(state) = self.group_start.take()
            {
                self.state = state;
            }
            self.group_start = None;
            true
        } else {
            false
        }
    }

    fn skip_to(&mut self, end: &[u8]) {
        while self.pos < self.bytes.len() {
            if self.starts_with(end) {
                self.pos += end.len();
                return;
            }
            self.pos += 1;
        }
    }

    fn run(&mut self) {
        loop {
            self.skip_space_and_comments();
            match self.peek() {
                None => return,
                Some(b'<') => {
                    self.pos += 1;
                    self.dict();
                }
                Some(b'{') => {
                    self.pos += 1;
                    self.table();
                }
                Some(b'[') => {
                    self.pos += 1;
                    self.row(None);
                }
                Some(b'@') => {
                    if !self.group() {
                        self.pos += 1;
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }
}

/// `1:^80` → `1:80`; rows without a scope take the table's.
fn row_key(raw: &str, table_scope: Option<&str>) -> String {
    match raw.split_once(':') {
        Some((id, scope)) => format!(
            "{}:{}",
            id.to_ascii_uppercase(),
            scope.trim_start_matches('^').to_ascii_uppercase()
        ),
        None => format!("{}:{}", raw.to_ascii_uppercase(), table_scope.unwrap_or("")),
    }
}

/// Parses Mork text into rows.
pub fn parse(bytes: &[u8]) -> MorkRows {
    let mut parser = Parser {
        bytes,
        pos: 0,
        state: State {
            columns: HashMap::new(),
            values: HashMap::new(),
            rows: MorkRows::default(),
        },
        group_start: None,
    };
    parser.run();
    parser.state.rows
}

/// Reads a `.mab` address book: rows with `ListName` are lists (members in
/// `Address1`, `Address2`, … naming card row ids), rows with a name or
/// address are cards.
pub fn read_address_book(path: &Path) -> Result<ParsedCards> {
    let bytes =
        std::fs::read(path).map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
    if !bytes.starts_with(b"//")
        || !String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).contains("mork")
    {
        return Err(PortabilityError::InvalidSource(format!(
            "{} is not a Mork file",
            path.display()
        )));
    }
    let rows = parse(&bytes);
    let mut cards = CardProperties::new();
    let mut lists = Vec::new();
    let mut card_ids: HashMap<String, String> = HashMap::new();
    for key in &rows.order {
        let cells = &rows.rows[key];
        let id = key.split(':').next().unwrap_or(key).to_owned();
        if let Some(name) = cells.get("ListName").filter(|n| !n.is_empty()) {
            let mut members: Vec<(u32, String)> = cells
                .iter()
                .filter_map(|(column, value)| {
                    let n: u32 = column.strip_prefix("Address")?.parse().ok()?;
                    Some((n, value.clone()))
                })
                .collect();
            members.sort();
            lists.push(MailList {
                uid: format!("mab-list-{id}"),
                name: name.clone(),
                nick_name: cells.get("ListNickName").filter(|s| !s.is_empty()).cloned(),
                description: cells
                    .get("ListDescription")
                    .filter(|s| !s.is_empty())
                    .cloned(),
                members: members.into_iter().map(|(_, v)| v).collect(),
            });
            continue;
        }
        let is_card = ["DisplayName", "PrimaryEmail", "FirstName", "LastName"]
            .iter()
            .any(|c| cells.get(*c).is_some_and(|v| !v.is_empty()));
        if is_card {
            let uid = format!("mab-card-{id}");
            card_ids.insert(id.clone(), uid.clone());
            cards.insert(
                uid,
                cells
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            );
        }
    }
    // Members name card rows by id (hex, as written) or decimal.
    for list in &mut lists {
        list.members = list
            .members
            .iter()
            .filter_map(|m| {
                card_ids
                    .get(&m.to_ascii_uppercase())
                    .or_else(|| {
                        m.parse::<u64>()
                            .ok()
                            .and_then(|n| card_ids.get(&format!("{n:X}")))
                    })
                    .cloned()
            })
            .collect();
    }
    Ok(cards_from(&cards, &lists))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const MAB: &str = r"// <!-- <mdb:mork:z v=&quot;1.4&quot;/> -->
< <(a=c)> // (f=iso-8859-1)
  (B8=LastModifiedDate)(B9=RecordKey)(BA=AddrCharSet)
  (80=ns:addrbk:db:row:scope:card:all)(81=ns:addrbk:db:row:scope:list:all)
  (82=ns:addrbk:db:row:scope:data:all)(83=FirstName)(84=LastName)
  (85=DisplayName)(86=PrimaryEmail)(87=Notes)(88=ListName)(89=Address1)
  (8A=Address2)(8B=ListNickName)>

<(90=Ada)(91=Lovelace)(92=Ada Lovelace)(93=ada@example.org)(94=Babbage)
  (95=charles@example.org)(96=Engineers)>

{1:^80 {(k^C0:c)(s=9)}
  [1(^83^90)(^84^91)(^85^92)(^86^93)(^87=Line one\
 continued \) with $C3$BC)]
  [2(^84^94)(^86^95)]
  [3:^81(^88^96)(^89=1)(^8A=2)]}

@$${4{@
[5:^80(^85=Removed later)(^86=gone@example.org)]
@$$}4}@
@$${5{@
[6:^80(^85=Aborted)(^86=aborted@example.org)]
@$$}~abort~5}@
@$${6{@
[-5:^80]
@$$}6}@
";

    #[test]
    fn parses_rows_with_dictionaries_escapes_and_groups() {
        let rows = parse(MAB.as_bytes());
        let ada = &rows.rows["1:80"];
        assert_eq!(ada["FirstName"], "Ada");
        assert_eq!(ada["DisplayName"], "Ada Lovelace");
        assert_eq!(ada["Notes"], "Line one continued ) with ü");
        assert_eq!(rows.rows["2:80"]["PrimaryEmail"], "charles@example.org");
        assert_eq!(rows.rows["3:81"]["ListName"], "Engineers");
        assert!(!rows.rows.contains_key("5:80"), "cut row");
        assert!(!rows.rows.contains_key("6:80"), "aborted group");
    }

    #[test]
    fn reads_an_address_book() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abook.mab");
        std::fs::write(&path, MAB).unwrap();
        let cards = read_address_book(&path).unwrap();
        assert_eq!(cards.contacts.len(), 2);
        assert_eq!(cards.contacts[0].fields.display_name, "Ada Lovelace");
        assert_eq!(
            cards.contacts[1].fields.emails[0].value,
            "charles@example.org"
        );
        assert_eq!(cards.groups.len(), 1);
        assert_eq!(
            cards.groups[0].group.member_uids,
            vec!["mab-card-1", "mab-card-2"]
        );
    }

    #[test]
    fn other_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.mab");
        std::fs::write(&path, "hello").unwrap();
        assert!(matches!(
            read_address_book(&path),
            Err(PortabilityError::InvalidSource(_))
        ));
    }
}
