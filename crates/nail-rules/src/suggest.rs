//! "Create rule from message" (`RuleFromMessage`; M2c WP53): a rule
//! pre-filled from the message's full header list.
//!
//! The best handle on a message is its mailing list, then its sender:
//!
//! 1. `List-Id` (RFC 2919): `list-id contains="<id>"`, named after the
//!    list's description (`Rust Weekly <rust-weekly.example>` → "Rust
//!    Weekly"), else the id.
//! 2. Lists without `List-Id`: the posting address of `List-Post`
//!    (RFC 2369, `<mailto:list@host>`), else of `X-Mailing-List` /
//!    `Mailing-List` (`list rust@host; contact …`), as a `header … contains`
//!    condition, named after that address.
//! 3. The sender: `from equals="<address>"`, named after the address.
//! 4. Neither: the subject.
//!
//! The rule runs on arrival, everywhere, and has no actions yet: the user
//! picks them in the editor.

use nail_api::RuleId;
use nail_api::rules::{AddressField, Condition, Rule, RuleTrigger, TextMatch, TextOp};

use crate::message::MessageView;

/// A rule pre-filled from a message (see the module docs).
pub fn rule_from_message(message: &dyn MessageView) -> Rule {
    let (condition, name) = list_condition(message)
        .or_else(|| sender_condition(message))
        .unwrap_or_else(|| {
            let subject = message.subject();
            (
                Condition::Subject {
                    matches: contains_or_equals(TextOp::Equals, &subject),
                },
                subject,
            )
        });
    Rule {
        id: RuleId::new(""),
        name,
        enabled: true,
        triggers: vec![RuleTrigger::Arrival],
        scope: Vec::new(),
        conditions: Condition::All {
            conditions: vec![condition],
        },
        actions: Vec::new(),
        source: None,
    }
}

fn contains_or_equals(op: TextOp, value: &str) -> TextMatch {
    TextMatch {
        op,
        value: value.trim().to_owned(),
        case_sensitive: false,
    }
}

/// The mailing list the message came from, with a rule name.
fn list_condition(message: &dyn MessageView) -> Option<(Condition, String)> {
    if let Some((id, description)) = message
        .header_values("List-Id")
        .iter()
        .find_map(|v| parse_list_id(v))
    {
        let name = description.unwrap_or_else(|| id.clone());
        return Some((
            Condition::ListId {
                matches: Some(contains_or_equals(TextOp::Contains, &id)),
            },
            name,
        ));
    }
    let header_address = |header: &str, parse: fn(&str) -> Option<String>| {
        message
            .header_values(header)
            .iter()
            .find_map(|v| parse(v))
            .map(|address| {
                (
                    Condition::Header {
                        name: header.to_owned(),
                        matches: Some(contains_or_equals(TextOp::Contains, &address)),
                    },
                    address,
                )
            })
    };
    header_address("List-Post", parse_list_post)
        .or_else(|| header_address("X-Mailing-List", parse_mailing_list))
        .or_else(|| header_address("Mailing-List", parse_mailing_list))
}

fn sender_condition(message: &dyn MessageView) -> Option<(Condition, String)> {
    let from = message
        .addresses(AddressField::From)
        .into_iter()
        .map(|a| a.email.trim().to_owned())
        .find(|e| !e.is_empty())?;
    Some((
        Condition::Address {
            field: AddressField::From,
            matches: contains_or_equals(TextOp::Equals, &from),
        },
        from,
    ))
}

/// `List-Id: [phrase] <id>` → the id and the phrase (unquoted; `None` when
/// empty or still an encoded word). A value without angle brackets is taken
/// as the id itself (seen in the wild).
pub(crate) fn parse_list_id(value: &str) -> Option<(String, Option<String>)> {
    let value = value.trim();
    let (id, phrase) = match (value.rfind('<'), value.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            (&value[open + 1..close], value[..open].trim())
        }
        _ => (value, ""),
    };
    let id = id.trim();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    let phrase = phrase.trim_matches('"').trim();
    let phrase = (!phrase.is_empty() && !phrase.contains("=?")).then(|| phrase.to_owned());
    Some((id.to_owned(), phrase))
}

/// `List-Post: <mailto:list@host?subject=x>, <https://…>` → `list@host`
/// (`NO` for announcement-only lists → `None`).
pub(crate) fn parse_list_post(value: &str) -> Option<String> {
    value.split(',').find_map(|part| {
        let part = part.trim().trim_start_matches('<').trim_end_matches('>');
        let address = part
            .get(..7)
            .filter(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
            .map(|_| &part[7..])?;
        let address = address.split('?').next().unwrap_or_default().trim();
        is_address(address).then(|| address.to_owned())
    })
}

/// `X-Mailing-List: <list@host>` or `Mailing-List: list list@host;
/// contact owner@host` → `list@host`.
pub(crate) fn parse_mailing_list(value: &str) -> Option<String> {
    let first = value.split(';').next().unwrap_or_default();
    first
        .split(|c: char| c.is_whitespace() || c == '<' || c == '>')
        .map(|w| w.trim_start_matches("mailto:"))
        .find(|w| is_address(w))
        .map(str::to_owned)
}

fn is_address(text: &str) -> bool {
    text.split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && !text.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MessageData;
    use nail_api::Address;

    fn message(headers: &[(&str, &str)], from: Option<&str>, subject: &str) -> MessageData {
        let mut m = MessageData::new("m", "a", "f");
        m.headers = headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect();
        m.from = from
            .map(|email| Address {
                name: None,
                email: email.to_owned(),
            })
            .into_iter()
            .collect();
        m.subject = subject.to_owned();
        m
    }

    fn only_condition(rule: &Rule) -> &Condition {
        match &rule.conditions {
            Condition::All { conditions } if conditions.len() == 1 => &conditions[0],
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn list_ids_are_parsed() {
        assert_eq!(
            parse_list_id("Rust Weekly <rust-weekly.example.org>"),
            Some(("rust-weekly.example.org".into(), Some("Rust Weekly".into())))
        );
        assert_eq!(
            parse_list_id("\"Rust, users\" <rust.lists.example>"),
            Some(("rust.lists.example".into(), Some("Rust, users".into())))
        );
        assert_eq!(
            parse_list_id("<bare.lists.example>"),
            Some(("bare.lists.example".into(), None))
        );
        assert_eq!(
            parse_list_id("=?utf-8?q?Gr=C3=BC=C3=9Fe?= <gruss.example>"),
            Some(("gruss.example".into(), None))
        );
        assert_eq!(
            parse_list_id("no-brackets.example"),
            Some(("no-brackets.example".into(), None))
        );
        assert_eq!(parse_list_id("  "), None);
        assert_eq!(parse_list_id("two words"), None);
    }

    #[test]
    fn list_post_and_mailing_list_headers_are_parsed() {
        assert_eq!(
            parse_list_post("<mailto:rust@lists.example?subject=hi>"),
            Some("rust@lists.example".into())
        );
        assert_eq!(
            parse_list_post("<https://lists.example/post>, <MAILTO:dev@lists.example>"),
            Some("dev@lists.example".into())
        );
        assert_eq!(parse_list_post("NO (posting not allowed)"), None);
        assert_eq!(
            parse_mailing_list("list rust@lists.example; contact rust-owner@lists.example"),
            Some("rust@lists.example".into())
        );
        assert_eq!(
            parse_mailing_list("<ietf@ietf.example>"),
            Some("ietf@ietf.example".into())
        );
        assert_eq!(parse_mailing_list("nothing here"), None);
    }

    #[test]
    fn a_list_wins_over_the_sender_and_names_the_rule() {
        let m = message(
            &[("List-Id", "Rust Weekly <rust-weekly.example.org>")],
            Some("ada@example.org"),
            "Issue 42",
        );
        let rule = rule_from_message(&m);
        assert_eq!(rule.name, "Rust Weekly");
        assert_eq!(rule.triggers, [RuleTrigger::Arrival]);
        assert!(rule.actions.is_empty() && rule.scope.is_empty());
        assert_eq!(
            only_condition(&rule),
            &Condition::ListId {
                matches: Some(TextMatch {
                    op: TextOp::Contains,
                    value: "rust-weekly.example.org".into(),
                    case_sensitive: false,
                })
            }
        );
        // Without a description the id names it.
        let m = message(&[("list-id", "<x.example>")], None, "");
        assert_eq!(rule_from_message(&m).name, "x.example");
    }

    #[test]
    fn lists_without_list_id_use_their_posting_address() {
        let m = message(
            &[
                ("List-Post", "<mailto:dev@lists.example>"),
                ("X-Mailing-List", "<other@lists.example>"),
            ],
            Some("ada@example.org"),
            "",
        );
        let rule = rule_from_message(&m);
        assert_eq!(rule.name, "dev@lists.example");
        assert_eq!(
            only_condition(&rule),
            &Condition::Header {
                name: "List-Post".into(),
                matches: Some(TextMatch {
                    op: TextOp::Contains,
                    value: "dev@lists.example".into(),
                    case_sensitive: false,
                })
            }
        );
        let m = message(
            &[(
                "Mailing-List",
                "list old@lists.example; contact x@y.example",
            )],
            Some("ada@example.org"),
            "",
        );
        let rule = rule_from_message(&m);
        assert!(
            matches!(only_condition(&rule), Condition::Header { name, .. } if name == "Mailing-List")
        );
        assert_eq!(rule.name, "old@lists.example");
    }

    #[test]
    fn otherwise_the_sender_then_the_subject() {
        let m = message(
            &[("List-Post", "NO")],
            Some(" boss@work.example "),
            "Budget",
        );
        let rule = rule_from_message(&m);
        assert_eq!(rule.name, "boss@work.example");
        assert!(matches!(
            only_condition(&rule),
            Condition::Address { field: AddressField::From, matches } if matches.value == "boss@work.example"
        ));
        let rule = rule_from_message(&message(&[], None, " Budget "));
        assert_eq!(rule.name, " Budget ");
        assert!(matches!(
            only_condition(&rule),
            Condition::Subject { matches } if matches.value == "Budget" && matches.op == TextOp::Equals
        ));
    }
}
