//! The content-line layer shared by iCalendar (RFC 5545 section 3.1) and
//! vCard (RFC 6350 section 3.3, RFC 2426, vCard 2.1): unfolding, parameter
//! parsing (quoted values, RFC 6868 caret escapes, vCard 2.1 bare types),
//! components, and **lossless** writing.
//!
//! Every property parsed from text remembers its original physical lines.
//! Writing an untouched property emits those lines verbatim, so unknown
//! properties, unusual parameter spellings, lower-case values and the
//! original folding survive a round trip byte for byte (line endings are
//! normalized to CRLF). Properties that were built or changed are
//! serialized canonically and folded at 75 octets on character
//! boundaries.

use std::fmt::Write as _;

/// Longest physical line Nail writes, in octets (RFC 5545 3.1).
pub const FOLD_OCTETS: usize = 75;

/// A property parameter (`TZID=Europe/Berlin`, `TYPE=work,voice`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Name as written (compare with [`Param::is`]).
    pub name: String,
    /// Values, unquoted and with RFC 6868 escapes decoded.
    pub values: Vec<String>,
}

impl Param {
    /// A parameter with one value.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: vec![value.into()],
        }
    }

    /// Whether the parameter has this name (ASCII case-insensitive).
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// One property (content line).
#[derive(Debug, Clone)]
pub struct Property {
    /// vCard group (`item1` in `item1.TEL`), if any.
    pub group: Option<String>,
    /// Name as written (compare with [`Property::is`]).
    pub name: String,
    /// Parameters in their original order.
    pub params: Vec<Param>,
    /// The value as written (still escaped), unfolded.
    pub value: String,
    /// 1-based line of the property in the source (0 when built).
    pub line: u32,
    /// The original physical lines, `None` once built or changed.
    raw: Option<String>,
}

impl PartialEq for Property {
    /// Semantic equality: group, name (case-insensitive), parameters and
    /// value; the original text is not compared.
    fn eq(&self, other: &Self) -> bool {
        self.name.eq_ignore_ascii_case(&other.name)
            && match (&self.group, &other.group) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            }
            && self.params.len() == other.params.len()
            && self
                .params
                .iter()
                .zip(&other.params)
                .all(|(a, b)| a.is(&b.name) && a.values == b.values)
            && self.value == other.value
    }
}

impl Property {
    /// A new property with a raw (already escaped) value.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            group: None,
            name: name.into(),
            params: Vec::new(),
            value: value.into(),
            line: 0,
            raw: None,
        }
    }

    /// A new property with a text value (escaped here).
    pub fn text_value(name: impl Into<String>, text: &str) -> Self {
        Self::new(name, escape_text(text))
    }

    /// Adds a parameter (builder).
    #[must_use]
    pub fn with_param(mut self, name: &str, value: impl Into<String>) -> Self {
        self.params.push(Param::new(name, value));
        self.raw = None;
        self
    }

    /// Adds a parameter with several values (builder).
    #[must_use]
    pub fn with_param_values(mut self, name: &str, values: Vec<String>) -> Self {
        if !values.is_empty() {
            self.params.push(Param {
                name: name.to_owned(),
                values,
            });
            self.raw = None;
        }
        self
    }

    /// Sets the vCard group (builder).
    #[must_use]
    pub fn with_group(mut self, group: Option<String>) -> Self {
        self.group = group;
        self.raw = None;
        self
    }

    /// Whether the property has this name (ASCII case-insensitive).
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// Whether this property still carries its original text.
    pub fn is_original(&self) -> bool {
        self.raw.is_some()
    }

    /// The original text (physical lines joined by CRLF), if unchanged.
    pub fn original_text(&self) -> Option<&str> {
        self.raw.as_deref()
    }

    /// Drops the original text so the property is written canonically.
    pub fn touch(&mut self) {
        self.raw = None;
    }

    /// First value of a parameter.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .filter(|p| p.is(name))
            .flat_map(|p| p.values.iter())
            .map(String::as_str)
            .next()
    }

    /// Every value of a parameter (repeated parameters and comma lists).
    pub fn param_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.params
            .iter()
            .filter(move |p| p.is(name))
            .flat_map(|p| p.values.iter())
            .map(String::as_str)
    }

    /// Whether a parameter has a value (ASCII case-insensitive).
    pub fn has_param_value(&self, name: &str, value: &str) -> bool {
        self.param_values(name)
            .any(|v| v.eq_ignore_ascii_case(value))
    }

    /// Whether a parameter is present.
    pub fn has_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.is(name))
    }

    /// Replaces a parameter (all occurrences) with these values; removes
    /// it when `values` is empty. Keeps the position of the first one.
    pub fn set_param(&mut self, name: &str, values: Vec<String>) {
        let position = self.params.iter().position(|p| p.is(name));
        self.params.retain(|p| !p.is(name));
        if !values.is_empty() {
            let param = Param {
                name: name.to_owned(),
                values,
            };
            match position {
                Some(i) => self.params.insert(i.min(self.params.len()), param),
                None => self.params.push(param),
            }
        }
        self.raw = None;
    }

    /// Removes a parameter.
    pub fn remove_param(&mut self, name: &str) {
        if self.has_param(name) {
            self.params.retain(|p| !p.is(name));
            self.raw = None;
        }
    }

    /// Replaces the raw value.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.raw = None;
    }

    /// The value as one text (RFC 5545 TEXT / vCard text unescaped).
    pub fn text(&self) -> String {
        unescape_text(&self.value)
    }

    /// The value as a comma-separated text list (`CATEGORIES`).
    pub fn texts(&self) -> Vec<String> {
        split_unescaped(&self.value, ',')
            .into_iter()
            .map(unescape_text)
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// The value as a structured text (`N`, `ADR`, `ORG`): components
    /// split at `;`, each a list split at `,`.
    pub fn structured(&self) -> Vec<Vec<String>> {
        split_unescaped(&self.value, ';')
            .into_iter()
            .map(|part| {
                split_unescaped(part, ',')
                    .into_iter()
                    .map(unescape_text)
                    .collect()
            })
            .collect()
    }

    /// Writes the property: the original text when unchanged, else the
    /// canonical, folded form. Ends with CRLF.
    pub fn write(&self, out: &mut String) {
        if let Some(raw) = &self.raw {
            out.push_str(raw);
            out.push_str("\r\n");
            return;
        }
        let mut line = String::new();
        if let Some(group) = &self.group {
            line.push_str(group);
            line.push('.');
        }
        line.push_str(&self.name);
        for param in &self.params {
            line.push(';');
            line.push_str(&param.name);
            line.push('=');
            for (i, value) in param.values.iter().enumerate() {
                if i > 0 {
                    line.push(',');
                }
                write_param_value(&mut line, value);
            }
        }
        line.push(':');
        line.push_str(&self.value);
        fold_into(out, &line);
    }
}

fn write_param_value(out: &mut String, value: &str) {
    let encoded = encode_param_value(value);
    if encoded.contains([':', ';', ',']) || encoded.is_empty() && !value.is_empty() {
        let _ = write!(out, "\"{encoded}\"");
    } else {
        out.push_str(&encoded);
    }
}

/// RFC 6868: `^` → `^^`, newline → `^n`, `"` → `^'`.
fn encode_param_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => out.push_str("^^"),
            '"' => out.push_str("^'"),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => out.push_str("^n"),
            c => out.push(c),
        }
    }
    out
}

/// Decodes RFC 6868 caret escapes (`^n`, `^^`, `^'`); other carets stay.
fn decode_param_value(value: &str) -> String {
    if !value.contains('^') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' {
            match chars.peek() {
                Some('n' | 'N') => {
                    out.push('\n');
                    chars.next();
                }
                Some('^') => {
                    out.push('^');
                    chars.next();
                }
                Some('\'') => {
                    out.push('"');
                    chars.next();
                }
                _ => out.push('^'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Folds a logical line into physical lines of at most [`FOLD_OCTETS`]
/// octets, never splitting a UTF-8 sequence; appends CRLF.
pub fn fold_into(out: &mut String, line: &str) {
    let mut start = 0;
    let mut limit = FOLD_OCTETS;
    while line.len() - start > limit {
        let mut end = start + limit;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&line[start..end]);
        out.push_str("\r\n ");
        start = end;
        // Continuation lines start with a space, which counts.
        limit = FOLD_OCTETS - 1;
    }
    out.push_str(&line[start..]);
    out.push_str("\r\n");
}

/// Escapes text for a TEXT value (RFC 5545 3.3.11 / RFC 6350 3.4):
/// backslash, `;`, `,` and newlines (CRLF and CR become one `\n`).
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Undoes [`escape_text`]; unknown escapes keep the escaped character
/// (lenient, as many producers escape `:`).
pub fn unescape_text(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits at unescaped `sep` (a backslash escapes the next character).
pub fn split_unescaped(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == sep {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

/// A component (`VCALENDAR`, `VEVENT`, `VALARM`, `VCARD`, …).
#[derive(Debug, Clone)]
pub struct Component {
    /// Name as written (compare with [`Component::is`]).
    pub name: String,
    /// Properties in order.
    pub properties: Vec<Property>,
    /// Sub-components in order.
    pub components: Vec<Component>,
    /// 1-based line of `BEGIN` (0 when built).
    pub line: u32,
    begin_raw: Option<String>,
    end_raw: Option<String>,
}

impl Component {
    /// An empty component.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: Vec::new(),
            components: Vec::new(),
            line: 0,
            begin_raw: None,
            end_raw: None,
        }
    }

    /// Whether the component has this name (ASCII case-insensitive).
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// The first property with this name.
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.is(name))
    }

    /// Every property with this name.
    pub fn properties_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> {
        self.properties.iter().filter(move |p| p.is(name))
    }

    /// Text of the first property with this name.
    pub fn text(&self, name: &str) -> Option<String> {
        self.property(name).map(Property::text)
    }

    /// Sub-components with this name.
    pub fn components_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Component> + 'a {
        self.components.iter().filter(move |c| c.is(name))
    }

    /// Whether everything in it still carries its original text.
    pub fn is_original(&self) -> bool {
        self.begin_raw.is_some()
            && self.properties.iter().all(Property::is_original)
            && self.components.iter().all(Component::is_original)
    }

    /// Writes the component (CRLF line ends).
    pub fn write(&self, out: &mut String) {
        match &self.begin_raw {
            Some(raw) => {
                out.push_str(raw);
                out.push_str("\r\n");
            }
            None => fold_into(out, &format!("BEGIN:{}", self.name)),
        }
        for property in &self.properties {
            property.write(out);
        }
        for component in &self.components {
            component.write(out);
        }
        match &self.end_raw {
            Some(raw) => {
                out.push_str(raw);
                out.push_str("\r\n");
            }
            None => fold_into(out, &format!("END:{}", self.name)),
        }
    }

    /// A copy with the given properties only and no sub-components
    /// (keeps the original `BEGIN` / `END` lines).
    #[must_use]
    pub fn shell(&self, keep: impl Fn(&Property) -> bool) -> Component {
        Component {
            name: self.name.clone(),
            properties: self
                .properties
                .iter()
                .filter(|p| keep(p))
                .cloned()
                .collect(),
            components: Vec::new(),
            line: self.line,
            begin_raw: self.begin_raw.clone(),
            end_raw: self.end_raw.clone(),
        }
    }

    /// The component as text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
}

/// What is wrong with a line of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentIssueKind {
    /// A line that is neither a property nor `BEGIN` / `END`.
    MalformedLine,
    /// An `END` without a matching `BEGIN`.
    UnexpectedEnd,
    /// A component that never ends (closed at the end of the input).
    Unclosed,
    /// A property outside any component.
    OutsideComponent,
    /// A component nested deeper than [`MAX_DEPTH`]; it and everything
    /// in it was skipped.
    TooDeep,
}

/// Deepest component nesting [`parse`] keeps (real data nests at most
/// four levels: `VCALENDAR` > `VTIMEZONE` > `STANDARD`, `VEVENT` >
/// `VALARM`). Deeper components are skipped so that hostile input cannot
/// build a tree whose recursive drop / write overflows the stack.
pub const MAX_DEPTH: usize = 32;

/// A problem found while reading content lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIssue {
    /// 1-based line.
    pub line: u32,
    /// What.
    pub kind: ContentIssueKind,
}

/// Parsed content: top-level components plus problems.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Top-level components (`VCALENDAR`s, `VCARD`s).
    pub components: Vec<Component>,
    /// Problems; affected lines were skipped.
    pub issues: Vec<ContentIssue>,
}

impl Document {
    /// Writes every component.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for component in &self.components {
            component.write(&mut out);
        }
        out
    }
}

/// A logical (unfolded) line with its original physical lines.
struct Logical {
    text: String,
    raw: String,
    line: u32,
}

/// Whether the name / parameter part of a line asks for quoted-printable
/// (vCard 2.1 / 3.0 style; soft line breaks continue the value).
fn is_quoted_printable(text: &str) -> bool {
    let head = match text.find(':') {
        Some(i) => &text[..i],
        None => text,
    };
    head.to_ascii_uppercase().contains("QUOTED-PRINTABLE")
}

fn is_base64_head(text: &str) -> bool {
    let head = match text.find(':') {
        Some(i) => &text[..i],
        None => text,
    };
    let upper = head.to_ascii_uppercase();
    upper.contains("ENCODING=BASE64") || upper.contains(";BASE64")
}

fn unfold(text: &str) -> Vec<Logical> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<Logical> = Vec::new();
    let mut physical = text.split('\n').enumerate().peekable();
    let mut base64_open = false;
    while let Some((index, line)) = physical.next() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if line.starts_with([' ', '\t'])
            && let Some(last) = out.last_mut()
        {
            last.text.push_str(&line[1..]);
            last.raw.push_str("\r\n");
            last.raw.push_str(line);
            continue;
        }
        if line.trim().is_empty() {
            base64_open = false;
            continue;
        }
        // vCard 2.1 base64 continues on lines without a colon until a
        // blank line.
        if base64_open
            && !line.contains(':')
            && let Some(last) = out.last_mut()
        {
            last.text.push_str(line.trim());
            last.raw.push_str("\r\n");
            last.raw.push_str(line);
            continue;
        }
        let mut logical = Logical {
            text: line.to_owned(),
            raw: line.to_owned(),
            line: number,
        };
        // Quoted-printable soft line breaks: `=` at the end continues
        // the value on the next physical line.
        if is_quoted_printable(&logical.text) {
            while logical.text.ends_with('=') {
                let Some((_, next)) = physical.peek() else {
                    break;
                };
                let next = next.strip_suffix('\r').unwrap_or(next);
                logical.text.pop();
                logical.text.push_str(next);
                logical.raw.push_str("\r\n");
                logical.raw.push_str(next);
                physical.next();
            }
        }
        base64_open = is_base64_head(&logical.text);
        out.push(logical);
    }
    out
}

/// Parses one logical line into a property; `None` when malformed.
pub(crate) fn parse_line(text: &str) -> Option<Property> {
    // Name: up to the first `;` or `:`.
    let name_end = text.find([';', ':'])?;
    let full_name = text[..name_end].trim();
    if full_name.is_empty()
        || !full_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
    {
        return None;
    }
    let (group, name) = match full_name.rsplit_once('.') {
        Some((group, name)) if !group.is_empty() && !name.is_empty() => {
            (Some(group.to_owned()), name.to_owned())
        }
        _ => (None, full_name.to_owned()),
    };
    let mut params = Vec::new();
    let bytes = text.as_bytes();
    let mut i = name_end;
    while i < bytes.len() && bytes[i] == b';' {
        i += 1;
        // Parameter name.
        let start = i;
        while i < bytes.len() && !matches!(bytes[i], b'=' | b';' | b':') {
            i += 1;
        }
        let pname = text[start..i].trim().to_owned();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            let mut values = Vec::new();
            loop {
                let value;
                if i < bytes.len() && bytes[i] == b'"' {
                    let close = text[i + 1..].find('"').map(|p| p + i + 1)?;
                    value = decode_param_value(&text[i + 1..close]);
                    i = close + 1;
                    // Tolerate junk up to the next separator.
                    while i < bytes.len() && !matches!(bytes[i], b',' | b';' | b':') {
                        i += 1;
                    }
                } else {
                    let start = i;
                    while i < bytes.len() && !matches!(bytes[i], b',' | b';' | b':') {
                        i += 1;
                    }
                    value = decode_param_value(&text[start..i]);
                }
                values.push(value);
                if i < bytes.len() && bytes[i] == b',' {
                    i += 1;
                    continue;
                }
                break;
            }
            if !pname.is_empty() {
                params.push(Param {
                    name: pname,
                    values,
                });
            }
        } else if !pname.is_empty() {
            // vCard 2.1: `TEL;WORK;VOICE:` — a bare type; encodings and
            // charsets are recognized by value.
            let upper = pname.to_ascii_uppercase();
            let param = if matches!(
                upper.as_str(),
                "QUOTED-PRINTABLE" | "BASE64" | "B" | "8BIT" | "7BIT"
            ) {
                Param::new("ENCODING", pname)
            } else {
                Param::new("TYPE", pname)
            };
            params.push(param);
        }
    }
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    Some(Property {
        group,
        name,
        params,
        value: text[i + 1..].to_owned(),
        line: 0,
        raw: None,
    })
}

/// Parses iCalendar or vCard text. Lenient: malformed lines are reported
/// and skipped, unclosed components are closed at the end.
pub fn parse(text: &str) -> Document {
    let mut doc = Document::default();
    let mut stack: Vec<Component> = Vec::new();
    // Open components inside a skipped (too deep) one.
    let mut skipping: usize = 0;
    for logical in unfold(text) {
        let upper_head = logical
            .text
            .get(..logical.text.len().min(6))
            .map(str::to_ascii_uppercase)
            .unwrap_or_default();
        if skipping > 0 {
            if upper_head == "BEGIN:" {
                skipping += 1;
            } else if upper_head.starts_with("END:") {
                skipping -= 1;
            }
            continue;
        }
        if upper_head == "BEGIN:" && stack.len() >= MAX_DEPTH {
            doc.issues.push(ContentIssue {
                line: logical.line,
                kind: ContentIssueKind::TooDeep,
            });
            skipping = 1;
            continue;
        }
        if upper_head == "BEGIN:" {
            let mut component = Component::new(logical.text[6..].trim());
            component.line = logical.line;
            component.begin_raw = Some(logical.raw);
            stack.push(component);
            continue;
        }
        if upper_head.starts_with("END:") {
            let name = logical.text[4..].trim();
            match stack.iter().rposition(|c| c.is(name)) {
                Some(position) => {
                    // Close anything left open inside it.
                    // `open[0]` is the component this END closes; anything
                    // after it was left open inside it.
                    let mut open = stack.split_off(position);
                    while let Some(mut inner) = open.pop() {
                        if let Some(parent) = open.last_mut() {
                            doc.issues.push(ContentIssue {
                                line: inner.line,
                                kind: ContentIssueKind::Unclosed,
                            });
                            inner.end_raw = None;
                            parent.components.push(inner);
                        } else {
                            inner.end_raw = Some(logical.raw);
                            match stack.last_mut() {
                                Some(parent) => parent.components.push(inner),
                                None => doc.components.push(inner),
                            }
                            break;
                        }
                    }
                }
                None => doc.issues.push(ContentIssue {
                    line: logical.line,
                    kind: ContentIssueKind::UnexpectedEnd,
                }),
            }
            continue;
        }
        match parse_line(&logical.text) {
            Some(mut property) => {
                property.line = logical.line;
                property.raw = Some(logical.raw);
                match stack.last_mut() {
                    Some(component) => component.properties.push(property),
                    None => doc.issues.push(ContentIssue {
                        line: logical.line,
                        kind: ContentIssueKind::OutsideComponent,
                    }),
                }
            }
            None => doc.issues.push(ContentIssue {
                line: logical.line,
                kind: ContentIssueKind::MalformedLine,
            }),
        }
    }
    while let Some(mut open) = stack.pop() {
        doc.issues.push(ContentIssue {
            line: open.line,
            kind: ContentIssueKind::Unclosed,
        });
        open.end_raw = None;
        match stack.last_mut() {
            Some(parent) => parent.components.push(open),
            None => doc.components.push(open),
        }
    }
    doc
}

/// Decodes file bytes to text: UTF-8 (with or without BOM), UTF-16 with a
/// BOM, else Windows-1252 (old Outlook / vCard 2.1 exports). Returns the
/// text and the encoding used.
pub fn decode_bytes(bytes: &[u8]) -> (String, &'static str) {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let (text, _) = encoding.decode_without_bom_handling(&bytes[bom..]);
        return (text.into_owned(), encoding.name());
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        (text.to_owned(), "UTF-8")
    } else {
        let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
        (text.into_owned(), "windows-1252")
    }
}

/// Decodes quoted-printable (`=XX`, soft breaks already removed) to bytes.
pub fn decode_quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'='
            && bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            let hex = &text[i + 1..i + 3];
            out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Decodes bytes in a named charset (`CHARSET=` parameter); unknown
/// labels fall back to UTF-8 (lossy).
pub fn decode_charset(bytes: &[u8], charset: Option<&str>) -> String {
    let named = charset.and_then(|label| encoding_rs::Encoding::for_label(label.trim().as_bytes()));
    let encoding = match named {
        Some(encoding) => encoding,
        // No charset: UTF-8 when the bytes are, else the Windows code
        // page old Outlook exports use.
        None if std::str::from_utf8(bytes).is_ok() => encoding_rs::UTF_8,
        None => encoding_rs::WINDOWS_1252,
    };
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_and_keeps_original_lines() {
        let text = "BEGIN:VCALENDAR\r\nX-LONG;x-param=\"a:b\":first\r\n  second\r\n\tthird\r\nsummary:lower case\r\nEND:VCALENDAR\r\n";
        let doc = parse(text);
        assert!(doc.issues.is_empty(), "{:?}", doc.issues);
        let cal = &doc.components[0];
        let long = cal.property("x-long").unwrap();
        assert_eq!(long.value, "first secondthird");
        assert_eq!(long.param("X-PARAM"), Some("a:b"));
        assert_eq!(long.line, 2);
        assert_eq!(doc.to_text(), text);
        assert!(cal.is_original());
    }

    #[test]
    fn lf_only_input_is_written_with_crlf() {
        let doc = parse("BEGIN:VCARD\nFN:Ada\nEND:VCARD\n");
        assert_eq!(doc.to_text(), "BEGIN:VCARD\r\nFN:Ada\r\nEND:VCARD\r\n");
    }

    #[test]
    fn changed_properties_are_written_canonically_and_folded() {
        let mut doc = parse("BEGIN:VEVENT\r\nSUMMARY:x\r\nEND:VEVENT\r\n");
        let summary = &mut doc.components[0].properties[0];
        summary.set_value(escape_text(&"äöü, long; ".repeat(10)));
        summary.set_param("LANGUAGE", vec!["de".into()]);
        let text = doc.to_text();
        for line in text.split("\r\n") {
            assert!(line.len() <= FOLD_OCTETS, "{line:?}");
        }
        let again = parse(&text);
        assert_eq!(
            again.components[0].properties[0].text(),
            "äöü, long; ".repeat(10)
        );
        assert_eq!(
            again.components[0].properties[0].param("language"),
            Some("de")
        );
    }

    #[test]
    fn parameters_quotes_carets_and_bare_types() {
        let p = parse_line("ATTENDEE;CN=\"Doe, Jane\";X-NOTE=a^nb^'c^^:mailto:j@x").unwrap();
        assert_eq!(p.param("cn"), Some("Doe, Jane"));
        assert_eq!(p.param("x-note"), Some("a\nb\"c^"));
        assert_eq!(p.value, "mailto:j@x");
        let mut out = String::new();
        let mut q = p.clone();
        q.touch();
        q.write(&mut out);
        assert_eq!(parse_line(out.trim_end()).unwrap(), p);

        let v21 = parse_line("TEL;WORK;VOICE;ENCODING=QUOTED-PRINTABLE:123").unwrap();
        assert!(v21.has_param_value("TYPE", "work"));
        assert!(v21.has_param_value("type", "VOICE"));
        assert_eq!(v21.param("encoding"), Some("QUOTED-PRINTABLE"));

        let typed = parse_line("item1.EMAIL;type=INTERNET,pref:a@b").unwrap();
        assert_eq!(typed.group.as_deref(), Some("item1"));
        assert!(typed.has_param_value("TYPE", "pref"));
        assert!(parse_line("no colon here").is_none());
        assert!(parse_line(":value").is_none());
    }

    #[test]
    fn text_escaping_round_trips() {
        let text = "a\\b;c,d\ne\r\nf";
        let escaped = escape_text(text);
        assert_eq!(escaped, "a\\\\b\\;c\\,d\\ne\\nf");
        assert_eq!(unescape_text(&escaped), "a\\b;c,d\ne\nf");
        assert_eq!(unescape_text("a\\:b\\N"), "a:b\n");
        assert_eq!(split_unescaped("a\\,b,c", ','), ["a\\,b", "c"]);
        let p = Property::new("CATEGORIES", "Work,Fun\\, games,");
        assert_eq!(p.texts(), ["Work", "Fun, games"]);
        let n = Property::new("N", "Doe;Jane,J.;;Dr.;");
        assert_eq!(n.structured()[1], ["Jane", "J."]);
    }

    #[test]
    fn malformed_and_unclosed_input_is_reported() {
        let doc = parse(
            "garbage\r\nBEGIN:VCALENDAR\r\nnot a property\r\nBEGIN:VEVENT\r\nUID:1\r\nEND:VCALENDAR\r\nEND:VTODO\r\n",
        );
        let kinds: Vec<_> = doc.issues.iter().map(|i| (i.line, i.kind)).collect();
        assert_eq!(
            kinds,
            [
                (1, ContentIssueKind::MalformedLine),
                (3, ContentIssueKind::MalformedLine),
                (4, ContentIssueKind::Unclosed),
                (7, ContentIssueKind::UnexpectedEnd)
            ]
        );
        assert_eq!(doc.components[0].components[0].text("UID").unwrap(), "1");
    }

    #[test]
    fn quoted_printable_soft_breaks_and_charsets() {
        let doc = parse(
            "BEGIN:VCARD\r\nVERSION:2.1\r\nNOTE;ENCODING=QUOTED-PRINTABLE;CHARSET=ISO-8859-1:Gr=FC=DFe=\r\n aus K=F6ln\r\nEND:VCARD\r\n",
        );
        let note = doc.components[0].property("NOTE").unwrap();
        let bytes = decode_quoted_printable(&note.value);
        assert_eq!(
            decode_charset(&bytes, note.param("charset")),
            "Grüße aus Köln"
        );
        assert!(doc.to_text().contains("Gr=FC=DFe=\r\n aus K=F6ln"));
        assert_eq!(decode_quoted_printable("a=3Db=XYc="), b"a=b=XYc=");
        assert_eq!(decode_charset(b"Stra\xDFe", None), "Straße");
        assert_eq!(decode_charset("Straße".as_bytes(), None), "Straße");
    }

    #[test]
    fn decodes_bytes_by_bom_or_fallback() {
        assert_eq!(decode_bytes(b"\xef\xbb\xbfFN:\xc3\xa4").0, "FN:ä");
        assert_eq!(decode_bytes(b"FN:\xe4").0, "FN:ä");
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("FN:ä".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_bytes(&utf16), ("FN:ä".to_owned(), "UTF-16LE"));
    }

    #[test]
    fn vcard21_base64_continuation_lines() {
        let doc = parse(
            "BEGIN:VCARD\r\nVERSION:2.1\r\nPHOTO;JPEG;ENCODING=BASE64:\r\n  /9j/4AAQ\r\nSkZJRg==\r\n\r\nFN:A\r\nEND:VCARD\r\n",
        );
        let photo = doc.components[0].property("PHOTO").unwrap();
        assert_eq!(photo.value, " /9j/4AAQSkZJRg==");
        assert_eq!(doc.components[0].text("FN").unwrap(), "A");
    }

    /// 100k nested `BEGIN`s (hostile feed / import) on a small stack:
    /// parsing, writing and dropping must not recurse that deep.
    #[test]
    fn deep_nesting_is_cut_off_without_overflowing_the_stack() {
        let worker = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let depth = 100_000;
                let mut text = String::from("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\n");
                for _ in 0..depth {
                    text.push_str("BEGIN:X\r\n");
                }
                text.push_str("SUMMARY:inside\r\n");
                for _ in 0..depth {
                    text.push_str("END:X\r\n");
                }
                text.push_str("SUMMARY:kept\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
                let doc = parse(&text);
                let too_deep: Vec<_> = doc
                    .issues
                    .iter()
                    .filter(|i| i.kind == ContentIssueKind::TooDeep)
                    .collect();
                assert_eq!(
                    too_deep.len(),
                    1,
                    "{:?}",
                    &doc.issues[..doc.issues.len().min(5)]
                );
                assert_eq!(
                    too_deep[0].line,
                    3 + u32::try_from(MAX_DEPTH).unwrap() - 2 + 1
                );
                let event = &doc.components[0].components[0];
                // The properties after the skipped block still belong to the event.
                assert_eq!(event.text("SUMMARY").as_deref(), Some("kept"));
                let mut depth_seen = 0;
                let mut c = event;
                while let Some(child) = c.components.first() {
                    depth_seen += 1;
                    c = child;
                }
                assert_eq!(depth_seen + 2, MAX_DEPTH);
                let written = doc.to_text();
                assert!(written.len() < 2_000);
                drop(doc);
                // Unclosed deep nesting: closed at the end, still cut off.
                let unclosed = "BEGIN:VCARD\r\n".repeat(depth);
                let doc = parse(&unclosed);
                assert!(
                    doc.issues
                        .iter()
                        .any(|i| i.kind == ContentIssueKind::TooDeep)
                );
                drop(doc);
                // The format parsers on top survive too.
                drop(crate::ical::parse_calendar(
                    &text,
                    crate::ical::ParseOptions::default(),
                ));
                drop(crate::vcard::parse_cards(&unclosed));
                drop(crate::feed::parse_feed_bytes(text.as_bytes()));
            })
            .unwrap();
        worker.join().expect("no stack overflow");
    }
}
