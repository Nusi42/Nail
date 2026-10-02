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

/// A row while parsing: when it was first seen (file order) and its cells.
type Row = (u64, HashMap<String, String>);

/// One change inside a transaction group, undone when the group aborts.
enum Undo {
    Column(String, Option<String>),
    Value(String, Option<String>),
    /// A row was cut (`[-id]`): its old content.
    RowCut(String, Row),
    /// A row was created.
    RowCreated(String),
    /// A cell was set: its old value.
    Cell(String, String, Option<String>),
}

#[derive(Default)]
struct State {
    columns: HashMap<String, String>,
    values: HashMap<String, String>,
    rows: HashMap<String, Row>,
    next_seq: u64,
    /// Inside a transaction group: the changes since it started (undone
    /// on abort). A log instead of a snapshot per group, so a file with
    /// many groups (a long edit history) parses in linear time.
    undo: Option<Vec<Undo>>,
}

fn restore(map: &mut HashMap<String, String>, key: String, old: Option<String>) {
    match old {
        Some(old) => map.insert(key, old),
        None => map.remove(&key),
    };
}

impl State {
    fn log(&mut self, change: impl FnOnce() -> Undo) {
        if let Some(undo) = self.undo.as_mut() {
            undo.push(change());
        }
    }

    fn set_column(&mut self, id: String, value: String) {
        let old = self.columns.insert(id.clone(), value);
        self.log(|| Undo::Column(id, old));
    }

    fn set_value(&mut self, id: String, value: String) {
        let old = self.values.insert(id.clone(), value);
        self.log(|| Undo::Value(id, old));
    }

    fn cut_row(&mut self, key: &str) {
        if let Some(row) = self.rows.remove(key) {
            self.log(|| Undo::RowCut(key.to_owned(), row));
        }
    }

    fn set_cell(&mut self, key: &str, column: String, value: String) {
        if !self.rows.contains_key(key) {
            self.next_seq += 1;
            self.rows
                .insert(key.to_owned(), (self.next_seq, HashMap::new()));
            self.log(|| Undo::RowCreated(key.to_owned()));
        }
        let Some((_, cells)) = self.rows.get_mut(key) else {
            return;
        };
        let old = cells.insert(column.clone(), value);
        self.log(|| Undo::Cell(key.to_owned(), column, old));
    }

    fn begin_group(&mut self) {
        // A group that never ended stays (as before: only the latest
        // group can be aborted).
        self.undo = Some(Vec::new());
    }

    fn end_group(&mut self, abort: bool) {
        let Some(undo) = self.undo.take() else {
            return;
        };
        if !abort {
            return;
        }
        for change in undo.into_iter().rev() {
            match change {
                Undo::Column(id, old) => restore(&mut self.columns, id, old),
                Undo::Value(id, old) => restore(&mut self.values, id, old),
                Undo::RowCut(key, row) => {
                    self.rows.insert(key, row);
                }
                Undo::RowCreated(key) => {
                    self.rows.remove(&key);
                }
                Undo::Cell(key, column, old) => {
                    if let Some((_, cells)) = self.rows.get_mut(&key) {
                        restore(cells, column, old);
                    }
                }
            }
        }
    }

    fn into_rows(self) -> MorkRows {
        let mut keyed: Vec<(u64, String)> = self
            .rows
            .iter()
            .map(|(k, (seq, _))| (*seq, k.clone()))
            .collect();
        keyed.sort_unstable();
        MorkRows {
            order: keyed.into_iter().map(|(_, k)| k).collect(),
            rows: self
                .rows
                .into_iter()
                .map(|(k, (_, cells))| (k, cells))
                .collect(),
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    state: State,
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
                        self.state.set_column(id.to_ascii_uppercase(), value);
                    } else {
                        self.state.set_value(id.to_ascii_uppercase(), value);
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
            self.state.cut_row(&key);
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
                    self.state.set_cell(&key, column, value);
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
            self.state.begin_group();
            self.skip_to(b"{@");
            true
        } else if self.starts_with(b"@$$}") {
            let start = self.pos;
            self.skip_to(b"}@");
            let marker = &self.bytes[start..self.pos];
            let abort = marker.windows(7).any(|w| w == b"~abort~");
            self.state.end_group(abort);
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
        state: State::default(),
    };
    parser.run();
    parser.state.into_rows()
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
    fn an_aborted_group_undoes_cuts_cells_and_dictionaries() {
        let text = format!(
            "{MAB}@$${{7{{@\n<(93=changed@example.org)>\n[-2:^80]\n[1:^80(^85=Renamed)]\n[9:^80(^85=New)]\n@$$}}~abort~7}}@\n"
        );
        let before = parse(MAB.as_bytes());
        let after = parse(text.as_bytes());
        assert_eq!(after, before);
        assert_eq!(after.rows["1:80"]["DisplayName"], "Ada Lovelace");
        assert_eq!(after.rows["1:80"]["PrimaryEmail"], "ada@example.org");
        assert_eq!(after.order, before.order);
    }

    #[test]
    fn a_row_cut_and_written_again_moves_to_the_end() {
        let rows = parse(b"[1(Name=a)][2(Name=b)][-1][1(Name=c)]");
        assert_eq!(rows.order, vec!["2:", "1:"]);
        assert_eq!(rows.rows["1:"].len(), 1);
        assert_eq!(rows.rows["1:"]["Name"], "c");
    }

    /// Many transaction groups (a long edit history) parse in linear time:
    /// 20,000 rows in 20,000 groups took minutes with a snapshot per group.
    #[test]
    fn many_groups_parse_quickly() {
        let mut text = String::from(MAB);
        for n in 0..20_000u32 {
            let row = n + 0x100;
            text.push_str(&format!(
                "@$${{{n:X}{{@\n[{row:X}:^80(^85=Person {n})(^86=p{n}@example.org)]\n@$$}}{n:X}}}@\n"
            ));
            if n % 3 == 0 {
                text.push_str(&format!("[-{row:X}:^80]\n"));
            }
        }
        let started = std::time::Instant::now();
        let rows = parse(text.as_bytes());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(rows.order.len(), 3 + 20_000 - 6_667);
        assert_eq!(rows.rows.len(), rows.order.len());
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
