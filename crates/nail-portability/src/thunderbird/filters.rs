//! `msgFilterRules.dat` → Nail rule proposals. What has no Nail
//! equivalent is dropped with a `filter-not-expressible` issue naming it;
//! such rules start disabled.

use nail_api::domain::FolderRole;
use nail_api::ids::{AccountId, RuleId};
use nail_api::portability::{
    IssueSeverity, ItemIssue, ItemRef, LOCAL_FOLDERS_ACCOUNT_ID, PortabilityIssueCode, RuleProposal,
};
use nail_api::rules::{
    AddressField, Comparison, Condition, DateComparison, FolderRef, MessageFlag, Rule, RuleAction,
    RuleScope, RuleTrigger, TextMatch, TextOp,
};

/// One filter as written in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawFilter {
    /// `name`.
    pub name: String,
    /// `enabled="yes"`.
    pub enabled: bool,
    /// `type` bits.
    pub filter_type: u32,
    /// `(action, actionValue)` pairs in order.
    pub actions: Vec<(String, Option<String>)>,
    /// `condition`.
    pub condition: String,
}

/// Parses the file's `key="value"` lines into filters.
pub fn parse_file(text: &str) -> Vec<RawFilter> {
    let mut filters: Vec<RawFilter> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "name" => filters.push(RawFilter {
                name: value,
                filter_type: 17,
                ..RawFilter::default()
            }),
            "enabled" => {
                if let Some(f) = filters.last_mut() {
                    f.enabled = value == "yes";
                }
            }
            "type" => {
                if let Some(f) = filters.last_mut() {
                    f.filter_type = value.parse().unwrap_or(17);
                }
            }
            "action" => {
                if let Some(f) = filters.last_mut() {
                    f.actions.push((value, None));
                }
            }
            "actionValue" => {
                if let Some(f) = filters.last_mut()
                    && let Some(last) = f.actions.last_mut()
                {
                    last.1 = Some(value);
                }
            }
            "condition" => {
                if let Some(f) = filters.last_mut() {
                    f.condition = value;
                }
            }
            _ => {}
        }
    }
    filters
}

/// `"text"` with `\"` and `\\` escapes.
fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// One `(attribute,operator,value)` term with its connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// `AND` (true) or `OR`.
    pub and: bool,
    /// The attribute (`subject`, `"X-Header"` → `X-Header` with
    /// `custom: true`).
    pub attribute: String,
    /// A custom header.
    pub custom: bool,
    /// The operator.
    pub op: String,
    /// The value.
    pub value: String,
}

/// Parses a condition string: `ALL`, or `AND (a,op,v) OR (a,op,v)…`.
pub fn parse_condition(text: &str) -> Option<Vec<Term>> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("ALL") {
        return Some(Vec::new());
    }
    let mut terms = Vec::new();
    let mut rest = text;
    while !rest.trim().is_empty() {
        let trimmed = rest.trim_start();
        let (and, after) = if let Some(r) = trimmed.strip_prefix("AND") {
            (true, r)
        } else {
            (false, trimmed.strip_prefix("OR")?)
        };
        let after = after.trim_start().strip_prefix('(')?;
        let (term, remaining) = parse_term(after)?;
        terms.push(Term { and, ..term });
        rest = remaining;
    }
    Some(terms)
}

/// Parses `attr,op,value)`; returns the term and the text after `)`.
fn parse_term(text: &str) -> Option<(Term, &str)> {
    let (attribute, custom, rest) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"')?;
        (
            quoted[..end].to_owned(),
            true,
            quoted[end + 1..].strip_prefix(',')?,
        )
    } else {
        let comma = text.find(',')?;
        (text[..comma].trim().to_owned(), false, &text[comma + 1..])
    };
    let comma = rest.find(',')?;
    let op = rest[..comma].trim().to_owned();
    let rest = &rest[comma + 1..];
    let (value, remaining) = if let Some(quoted) = rest.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = quoted.char_indices();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, next)) = chars.next() {
                        value.push(next);
                    }
                }
                '"' => {
                    end = Some(i + 1);
                    break;
                }
                _ => value.push(c),
            }
        }
        let after = quoted[end?..].trim_start().strip_prefix(')')?;
        (value, after)
    } else {
        // The value ends at the `)` followed by the end or a connector.
        let mut found = None;
        for (i, c) in rest.char_indices() {
            if c == ')' {
                let after = rest[i + 1..].trim_start();
                if after.is_empty() || after.starts_with("AND (") || after.starts_with("OR (") {
                    found = Some(i);
                    break;
                }
            }
        }
        let i = found?;
        (rest[..i].to_owned(), &rest[i + 1..])
    };
    Some((
        Term {
            and: true,
            attribute,
            custom,
            op,
            value,
        },
        remaining,
    ))
}

/// A folder URI (`mailbox://nobody@Local%20Folders/Archives`,
/// `imap://user@host/INBOX/Sub`) → server user / host and path.
pub fn parse_folder_uri(uri: &str) -> Option<(String, String)> {
    let (_, rest) = uri.split_once("://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority.rsplit('@').next().unwrap_or(authority);
    Some((percent_decode(host), percent_decode(path)))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the converter knows about the filter's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterContext {
    /// The account's key (`account1`), `None` for Local Folders.
    pub account_key: Option<String>,
    /// Whether the filters belong to Local Folders.
    pub local_folders: bool,
    /// The server host of the account (to tell its folders from others).
    pub host: Option<String>,
    /// The `Local Folders` server name as used in folder URIs.
    pub local_folders_name: String,
    /// Minutes between periodic runs (`periodicFilterRateMinutes`).
    pub periodic_minutes: u32,
}

struct Converter<'a> {
    filter: &'a RawFilter,
    issues: Vec<ItemIssue>,
}

impl Converter<'_> {
    fn drop(&mut self, what: impl Into<String>) {
        self.issues.push(ItemIssue::new(
            PortabilityIssueCode::FilterNotExpressible,
            IssueSeverity::Warning,
            ItemRef {
                label: Some(self.filter.name.clone()),
                ..ItemRef::default()
            },
            what,
        ));
    }

    fn text_match(&mut self, term: &Term) -> Option<(TextMatch, bool)> {
        let (op, negate) = match term.op.as_str() {
            "contains" => (TextOp::Contains, false),
            "doesn't contain" => (TextOp::Contains, true),
            "is" => (TextOp::Equals, false),
            "isn't" => (TextOp::Equals, true),
            "begins with" => (TextOp::StartsWith, false),
            "ends with" => (TextOp::EndsWith, false),
            "matches" | "regex" => (TextOp::Regex, false),
            "doesn't match" => (TextOp::Regex, true),
            other => {
                self.drop(format!("condition ({},{other},…)", term.attribute));
                return None;
            }
        };
        Some((
            TextMatch {
                op,
                value: term.value.clone(),
                case_sensitive: false,
            },
            negate,
        ))
    }

    fn term(&mut self, term: &Term) -> Option<Condition> {
        let negate_if = |c: Condition, negate: bool| {
            if negate {
                Condition::Not {
                    condition: Box::new(c),
                }
            } else {
                c
            }
        };
        if term.custom {
            return match term.op.as_str() {
                "is empty" => Some(Condition::Not {
                    condition: Box::new(Condition::Header {
                        name: term.attribute.clone(),
                        matches: None,
                    }),
                }),
                "isn't empty" => Some(Condition::Header {
                    name: term.attribute.clone(),
                    matches: None,
                }),
                _ => {
                    let (matches, negate) = self.text_match(term)?;
                    Some(negate_if(
                        Condition::Header {
                            name: term.attribute.clone(),
                            matches: Some(matches),
                        },
                        negate,
                    ))
                }
            };
        }
        let address = |field| move |m| Condition::Address { field, matches: m };
        let build: Box<dyn Fn(TextMatch) -> Condition> = match term.attribute.as_str() {
            "subject" => Box::new(|m| Condition::Subject { matches: m }),
            "body" => Box::new(|m| Condition::Body { matches: m }),
            "from" => Box::new(address(AddressField::From)),
            "to" => Box::new(address(AddressField::To)),
            "cc" => Box::new(address(AddressField::Cc)),
            "to or cc" => Box::new(address(AddressField::Recipients)),
            "all addresses" => Box::new(|m: TextMatch| Condition::Any {
                conditions: vec![
                    Condition::Address {
                        field: AddressField::From,
                        matches: m.clone(),
                    },
                    Condition::Address {
                        field: AddressField::Recipients,
                        matches: m,
                    },
                ],
            }),
            "date" => return self.date(term),
            "age in days" => {
                return self.number(term, |cmp, n| Condition::Age {
                    cmp,
                    days: u32::try_from(n).unwrap_or(u32::MAX),
                });
            }
            "size" => {
                return self.number(term, |cmp, n| Condition::Size {
                    cmp,
                    bytes: n * 1024,
                });
            }
            "status" => return self.status(term),
            "tag" => {
                let negate = match term.op.as_str() {
                    "contains" | "is" => false,
                    "doesn't contain" | "isn't" => true,
                    other => {
                        self.drop(format!("condition (tag,{other},…)"));
                        return None;
                    }
                };
                return Some(negate_if(
                    Condition::Tag {
                        tag: term.value.clone(),
                    },
                    negate,
                ));
            }
            "has attachment status" => {
                let wanted = !term.value.eq_ignore_ascii_case("false")
                    && !term.value.to_ascii_lowercase().contains("no");
                let positive = (term.op == "is") == wanted;
                return Some(negate_if(
                    Condition::Attachment {
                        name: None,
                        mime: None,
                    },
                    !positive,
                ));
            }
            other => {
                self.drop(format!("condition ({other},{},…)", term.op));
                return None;
            }
        };
        if term.op == "is in ab" || term.op == "isn't in ab" {
            self.drop(format!("condition ({},{},…)", term.attribute, term.op));
            return None;
        }
        let (matches, negate) = self.text_match(term)?;
        Some(negate_if(build(matches), negate))
    }

    fn date(&mut self, term: &Term) -> Option<Condition> {
        let Some(date) = parse_tb_date(&term.value) else {
            self.drop(format!("condition (date,{},{})", term.op, term.value));
            return None;
        };
        let condition = |cmp| Condition::Date {
            cmp,
            date: date.clone(),
        };
        match term.op.as_str() {
            "is before" => Some(condition(DateComparison::Before)),
            "is after" => Some(condition(DateComparison::After)),
            "is" => Some(Condition::All {
                conditions: vec![
                    Condition::Not {
                        condition: Box::new(condition(DateComparison::Before)),
                    },
                    Condition::Not {
                        condition: Box::new(condition(DateComparison::After)),
                    },
                ],
            }),
            "isn't" => Some(Condition::Any {
                conditions: vec![
                    condition(DateComparison::Before),
                    condition(DateComparison::After),
                ],
            }),
            other => {
                self.drop(format!("condition (date,{other},…)"));
                None
            }
        }
    }

    fn number(
        &mut self,
        term: &Term,
        build: impl Fn(Comparison, u64) -> Condition,
    ) -> Option<Condition> {
        let value: Option<u64> = term.value.trim().parse().ok();
        match (term.op.as_str(), value) {
            ("is greater than", Some(n)) => Some(build(Comparison::Greater, n)),
            ("is less than", Some(n)) => Some(build(Comparison::Less, n)),
            (op, _) => {
                self.drop(format!(
                    "condition ({},{op},{})",
                    term.attribute, term.value
                ));
                None
            }
        }
    }

    fn status(&mut self, term: &Term) -> Option<Condition> {
        let flag = match term.value.to_ascii_lowercase().as_str() {
            "read" => MessageFlag::Seen,
            "replied" => MessageFlag::Answered,
            "flagged" | "starred" | "marked" => MessageFlag::Flagged,
            other => {
                self.drop(format!("condition (status,{},{other})", term.op));
                return None;
            }
        };
        match term.op.as_str() {
            "is" => Some(Condition::Flag { flag, set: true }),
            "isn't" => Some(Condition::Flag { flag, set: false }),
            other => {
                self.drop(format!("condition (status,{other},…)"));
                None
            }
        }
    }

    fn folder(&mut self, uri: &str, ctx: &FilterContext) -> Option<FolderRef> {
        let Some((server, path)) = parse_folder_uri(uri) else {
            self.drop(format!("folder {uri}"));
            return None;
        };
        let path = path.trim_matches('/').to_owned();
        if uri.starts_with("mailbox://") && server.eq_ignore_ascii_case(&ctx.local_folders_name) {
            // Imported Local Folders live below the "Local Folders"
            // folder of the Local Folders account.
            return Some(FolderRef::Path {
                path: format!("{}/{path}", ctx.local_folders_name),
                account_id: Some(AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID)),
            });
        }
        let own = ctx
            .host
            .as_deref()
            .is_some_and(|host| host.eq_ignore_ascii_case(&server));
        if !own {
            self.drop(format!("folder {uri} of another account"));
            return None;
        }
        if path.eq_ignore_ascii_case("INBOX") {
            return Some(FolderRef::Role {
                role: FolderRole::Inbox,
                account_id: None,
            });
        }
        Some(FolderRef::Path {
            path,
            account_id: None,
        })
    }

    fn action(
        &mut self,
        name: &str,
        value: Option<&str>,
        ctx: &FilterContext,
    ) -> Option<RuleAction> {
        let value_or_drop = |this: &mut Self| -> Option<String> {
            let v = value
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned);
            if v.is_none() {
                this.drop(format!("action {name} without a value"));
            }
            v
        };
        match name {
            "Move to folder" => {
                let uri = value_or_drop(self)?;
                Some(RuleAction::Move {
                    folder: self.folder(&uri, ctx)?,
                })
            }
            "Copy to folder" => {
                let uri = value_or_drop(self)?;
                Some(RuleAction::Copy {
                    folder: self.folder(&uri, ctx)?,
                })
            }
            "Delete" => Some(RuleAction::Delete { permanent: false }),
            "Mark read" => Some(RuleAction::MarkRead { read: true }),
            "Mark unread" => Some(RuleAction::MarkRead { read: false }),
            "Mark flagged" => Some(RuleAction::Flag { flagged: true }),
            "AddTag" => Some(RuleAction::Tag {
                tag: value_or_drop(self)?,
            }),
            "Label" => {
                let n = value_or_drop(self)?;
                Some(RuleAction::Tag {
                    tag: format!("$label{n}"),
                })
            }
            "Forward" => Some(RuleAction::Forward {
                to: vec![value_or_drop(self)?],
                identity_id: None,
            }),
            "Stop execution" => Some(RuleAction::Stop),
            other => {
                self.drop(format!("action {other}"));
                None
            }
        }
    }
}

/// `01-Oct-2026` → `2026-10-01`.
fn parse_tb_date(value: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%d-%b-%Y")
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

/// Thunderbird's filter type bits (`16` = manual: every Nail rule can be
/// run by hand).
mod bits {
    pub(super) const INBOX: u32 = 1;
    pub(super) const POST_PLUGIN: u32 = 32;
    pub(super) const POST_OUTGOING: u32 = 64;
    pub(super) const ARCHIVE: u32 = 128;
    pub(super) const PERIODIC: u32 = 256;
}

/// Converts one filter.
pub fn convert(filter: &RawFilter, index: usize, ctx: &FilterContext) -> RuleProposal {
    let mut c = Converter {
        filter,
        issues: Vec::new(),
    };
    let mut triggers = Vec::new();
    if filter.filter_type & (bits::INBOX | bits::POST_PLUGIN) != 0 {
        triggers.push(RuleTrigger::Arrival);
    }
    if filter.filter_type & bits::POST_OUTGOING != 0 {
        triggers.push(RuleTrigger::OnSend);
    }
    if filter.filter_type & bits::PERIODIC != 0 {
        triggers.push(RuleTrigger::Schedule {
            interval_minutes: ctx.periodic_minutes.max(1),
        });
    }
    if filter.filter_type & bits::ARCHIVE != 0 {
        c.drop("trigger: when archiving");
    }
    if triggers.is_empty() {
        triggers.push(RuleTrigger::Manual);
    }
    let conditions = if let Some(terms) = parse_condition(&filter.condition) {
        let mut result: Option<Condition> = None;
        for term in &terms {
            let Some(next) = c.term(term) else {
                continue;
            };
            result = Some(match result {
                None => next,
                Some(previous) => {
                    // Thunderbird evaluates left to right.
                    let merge = |previous: Condition, and: bool| match (previous, and) {
                        (Condition::All { mut conditions }, true) => {
                            conditions.push(next.clone());
                            Condition::All { conditions }
                        }
                        (Condition::Any { mut conditions }, false) => {
                            conditions.push(next.clone());
                            Condition::Any { conditions }
                        }
                        (previous, true) => Condition::All {
                            conditions: vec![previous, next.clone()],
                        },
                        (previous, false) => Condition::Any {
                            conditions: vec![previous, next.clone()],
                        },
                    };
                    merge(previous, term.and)
                }
            });
        }
        match result {
            Some(condition) => condition,
            None if terms.is_empty() => Condition::always(),
            // Every condition was dropped: never match by itself.
            None => Condition::Any {
                conditions: Vec::new(),
            },
        }
    } else {
        c.drop(format!("condition {}", filter.condition));
        Condition::Any {
            conditions: Vec::new(),
        }
    };
    let actions: Vec<RuleAction> = filter
        .actions
        .iter()
        .filter_map(|(name, value)| c.action(name, value.as_deref(), ctx))
        .collect();
    let complete = c.issues.is_empty();
    let scope = if ctx.local_folders {
        vec![RuleScope::Account {
            account_id: AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID),
        }]
    } else {
        Vec::new()
    };
    RuleProposal {
        key: format!(
            "{}/{index}",
            ctx.account_key
                .as_deref()
                .unwrap_or(LOCAL_FOLDERS_ACCOUNT_ID)
        ),
        account_key: ctx.account_key.clone(),
        rule: Rule {
            id: RuleId::new(""),
            name: filter.name.clone(),
            enabled: filter.enabled && complete,
            triggers,
            scope,
            conditions,
            actions,
            source: None,
        },
        complete,
        issues: c.issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const FILE: &str = r#"version="9"
logging="no"
name="Lists"
enabled="yes"
type="17"
action="Move to folder"
actionValue="imap://ada%40example.org@imap.example.org/Lists/Rust"
action="AddTag"
actionValue="$label1"
condition="AND (subject,contains,[rust]) AND (from,ends with,@rust-lang.org)"
name="Big or old"
enabled="yes"
type="1"
action="Mark read"
action="Change priority"
actionValue="Lowest"
condition="OR (size,is greater than,2048) OR (age in days,is greater than,30) OR (priority,is,Lowest)"
name="Archive local"
enabled="no"
type="16"
action="Copy to folder"
actionValue="mailbox://nobody@Local%20Folders/Archives/2024"
action="Stop execution"
condition="AND (\"X-Spam-Flag\",is,YES) AND (status,isn't,Read) AND (date,is before,01-Oct-2026)"
name="Quoted value"
enabled="yes"
type="1"
action="Delete"
condition="AND (subject,contains,\"a) AND (b\")"
name="Everything"
enabled="yes"
type="64"
action="Mark flagged"
condition="ALL"
"#;

    fn ctx() -> FilterContext {
        FilterContext {
            account_key: Some("account1".into()),
            local_folders: false,
            host: Some("imap.example.org".into()),
            local_folders_name: "Local Folders".into(),
            periodic_minutes: 10,
        }
    }

    #[test]
    fn parses_the_file() {
        let filters = parse_file(FILE);
        assert_eq!(filters.len(), 5);
        assert_eq!(filters[0].name, "Lists");
        assert!(filters[0].enabled);
        assert_eq!(filters[0].actions.len(), 2);
        assert_eq!(filters[1].actions[0], ("Mark read".to_owned(), None));
        assert_eq!(
            filters[2].condition,
            r#"AND ("X-Spam-Flag",is,YES) AND (status,isn't,Read) AND (date,is before,01-Oct-2026)"#
        );
    }

    #[test]
    fn conditions() {
        let terms =
            parse_condition("AND (subject,contains,a,b) OR (\"X-H\",is,\"q\\\"x\")").unwrap();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].value, "a,b");
        assert!(terms[0].and && !terms[1].and);
        assert!(terms[1].custom);
        assert_eq!(terms[1].value, "q\"x");
        assert_eq!(parse_condition("ALL"), Some(vec![]));
        assert_eq!(parse_condition("garbage"), None);
        let terms = parse_condition("AND (subject,contains,x)) OR (y)").unwrap_or_default();
        assert!(terms.is_empty() || terms[0].value.contains(')'));
    }

    #[test]
    fn folder_uris() {
        assert_eq!(
            parse_folder_uri("mailbox://nobody@Local%20Folders/Archives/2024"),
            Some(("Local Folders".into(), "Archives/2024".into()))
        );
        assert_eq!(
            parse_folder_uri("imap://ada%40example.org@imap.example.org/INBOX"),
            Some(("imap.example.org".into(), "INBOX".into()))
        );
    }

    #[test]
    fn complete_rules() {
        let filters = parse_file(FILE);
        let lists = convert(&filters[0], 0, &ctx());
        assert!(lists.complete, "{:?}", lists.issues);
        assert_eq!(lists.key, "account1/0");
        assert!(lists.rule.enabled);
        assert_eq!(lists.rule.triggers, vec![RuleTrigger::Arrival]);
        assert_eq!(
            lists.rule.conditions,
            Condition::All {
                conditions: vec![
                    Condition::Subject {
                        matches: TextMatch {
                            op: TextOp::Contains,
                            value: "[rust]".into(),
                            case_sensitive: false
                        }
                    },
                    Condition::Address {
                        field: AddressField::From,
                        matches: TextMatch {
                            op: TextOp::EndsWith,
                            value: "@rust-lang.org".into(),
                            case_sensitive: false
                        }
                    }
                ]
            }
        );
        assert_eq!(
            lists.rule.actions,
            vec![
                RuleAction::Move {
                    folder: FolderRef::Path {
                        path: "Lists/Rust".into(),
                        account_id: None
                    }
                },
                RuleAction::Tag {
                    tag: "$label1".into()
                }
            ]
        );
    }

    #[test]
    fn incomplete_rules_start_disabled_with_reasons() {
        let filters = parse_file(FILE);
        let big = convert(&filters[1], 1, &ctx());
        assert!(!big.complete);
        assert!(!big.rule.enabled);
        assert_eq!(big.issues.len(), 2, "{:?}", big.issues);
        assert!(
            big.issues
                .iter()
                .all(|i| i.code == PortabilityIssueCode::FilterNotExpressible)
        );
        assert!(big.issues.iter().any(|i| i.detail.contains("priority")));
        assert!(
            big.issues
                .iter()
                .any(|i| i.detail.contains("Change priority"))
        );
        assert_eq!(
            big.rule.conditions,
            Condition::Any {
                conditions: vec![
                    Condition::Size {
                        cmp: Comparison::Greater,
                        bytes: 2048 * 1024
                    },
                    Condition::Age {
                        cmp: Comparison::Greater,
                        days: 30
                    }
                ]
            }
        );
        assert_eq!(big.rule.actions, vec![RuleAction::MarkRead { read: true }]);
    }

    #[test]
    fn local_folders_targets_and_headers() {
        let filters = parse_file(FILE);
        let local = FilterContext {
            account_key: None,
            local_folders: true,
            host: None,
            ..ctx()
        };
        let archive = convert(&filters[2], 2, &local);
        assert!(archive.complete, "{:?}", archive.issues);
        assert!(!archive.rule.enabled, "disabled in Thunderbird");
        assert_eq!(archive.key, "local-folders/2");
        assert_eq!(archive.rule.triggers, vec![RuleTrigger::Manual]);
        assert_eq!(
            archive.rule.scope,
            vec![RuleScope::Account {
                account_id: AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID)
            }]
        );
        assert_eq!(
            archive.rule.actions[0],
            RuleAction::Copy {
                folder: FolderRef::Path {
                    path: "Local Folders/Archives/2024".into(),
                    account_id: Some(AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID))
                }
            }
        );
        let Condition::All { conditions } = &archive.rule.conditions else {
            panic!("not all");
        };
        assert!(
            matches!(&conditions[0], Condition::Header { name, matches: Some(m) } if name == "X-Spam-Flag" && m.op == TextOp::Equals)
        );
        assert_eq!(
            conditions[1],
            Condition::Flag {
                flag: MessageFlag::Seen,
                set: false
            }
        );
        assert_eq!(
            conditions[2],
            Condition::Date {
                cmp: DateComparison::Before,
                date: "2026-10-01".into()
            }
        );
    }

    #[test]
    fn quoted_values_send_and_all() {
        let filters = parse_file(FILE);
        let quoted = convert(&filters[3], 3, &ctx());
        assert!(
            matches!(&quoted.rule.conditions, Condition::Subject { matches } if matches.value == "a) AND (b")
        );
        let all = convert(&filters[4], 4, &ctx());
        assert_eq!(all.rule.conditions, Condition::always());
        assert_eq!(all.rule.triggers, vec![RuleTrigger::OnSend]);
        // Another account's folder cannot be named.
        let other = RawFilter {
            name: "x".into(),
            enabled: true,
            filter_type: 1,
            actions: vec![(
                "Move to folder".into(),
                Some("imap://u@imap.other.org/A".into()),
            )],
            condition: "ALL".into(),
        };
        let p = convert(&other, 0, &ctx());
        assert!(!p.complete);
        assert!(p.rule.actions.is_empty(), "{:?}", p.rule.actions);
    }
}
