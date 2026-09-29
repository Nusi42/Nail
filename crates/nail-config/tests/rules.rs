//! `rules.kdl`: reading every condition, action, trigger and scope, error
//! reporting (all errors at once, with positions), the text form of a
//! single rule and the canonical writer.

use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef,
    MessageFlag, Rule, RuleAction, RuleScope, RuleTrigger, TextMatch, TextOp,
};
use nail_api::{FolderRole, RuleId};
use nail_config::rules::{
    RULE_TEXT_SOURCE, RULES_FILE, normalize_rule, parse_rule_text, parse_rules, render_rules,
    rule_to_text,
};
use pretty_assertions::assert_eq;

fn parse(text: &str) -> Vec<Rule> {
    parse_rules(text, RULES_FILE).unwrap_or_else(|errors| {
        panic!(
            "{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

fn errors(text: &str) -> Vec<String> {
    parse_rules(text, RULES_FILE)
        .expect_err("expected errors")
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn one_error(text: &str) -> String {
    let errors = errors(text);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    errors.into_iter().next().unwrap()
}

fn tm(op: TextOp, value: &str) -> TextMatch {
    TextMatch {
        op,
        value: value.into(),
        case_sensitive: false,
    }
}

fn path(p: &str) -> FolderRef {
    FolderRef::Path {
        path: p.into(),
        account_id: None,
    }
}

/// The example from design.md, "Filters & rules".
const DESIGN_EXAMPLE: &str = r#"rule "Newsletters" trigger="arrival" scope="virtual:Everything" {
    when any {
        header "List-Id" exists=#true
        query "from:*@substack.com"
    }
    then {
        tag "newsletter"
        move "Reading"
        notify #false
    }
}
"#;

#[test]
fn the_design_example_parses() {
    let rules = parse(DESIGN_EXAMPLE);
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(rule.id.as_str(), "newsletters");
    assert_eq!(rule.name, "Newsletters");
    assert!(rule.enabled);
    assert_eq!(rule.triggers, [RuleTrigger::Arrival]);
    assert_eq!(
        rule.scope,
        [RuleScope::Account {
            account_id: "Everything".into()
        }]
    );
    assert_eq!(
        rule.conditions,
        Condition::Any {
            conditions: vec![
                Condition::Header {
                    name: "List-Id".into(),
                    matches: None
                },
                Condition::Query {
                    query: "from:*@substack.com".into()
                },
            ]
        }
    );
    assert_eq!(
        rule.actions,
        [
            RuleAction::Tag {
                tag: "newsletter".into()
            },
            RuleAction::Move {
                folder: path("Reading")
            },
            RuleAction::Notify { notify: false },
        ]
    );
    let source = rule.source.as_ref().unwrap();
    assert_eq!(
        (source.file.as_str(), source.line, source.column),
        ("rules.kdl", 1, 1)
    );
}

/// Every condition kind.
const ALL_CONDITIONS: &str = r#"
rule "Everything" {
    when {
        all {
            header "X-Spam" exists=#false
            header "Subject" regex="^\\[list\\]" case-sensitive=#true
        }
        any {
            from contains="@example.com"
            to equals="me@example.com"
            cc starts-with="team"
            recipients ends-with=".org"
            reply-to contains="noreply"
            sender contains="bounce"
        }
        not {
            subject contains="invoice"
        }
        not {
            body contains="unsubscribe"
            list-id
        }
        query "has:attachment is:unread"
        attachment
        attachment {
            name ends-with=".pdf"
            type starts-with="application/"
        }
        size larger="5MB"
        size smaller=1024
        date before="2026-01-01"
        date after="2025-06-30"
        age older-than=30
        age newer-than=2
        flag "seen" set=#false
        flag "flagged"
        tag "work"
        account "work"
        folder "Lists/Rust" account="work"
        folder role="inbox"
        dkim "pass"
        spf "softfail"
        dmarc "fail"
        list-id contains="rust-lang"
    }
}
"#;

#[test]
fn every_condition_kind_parses() {
    let rules = parse(ALL_CONDITIONS);
    let Condition::All { conditions } = &rules[0].conditions else {
        panic!("root is not `all`");
    };
    let expected = vec![
        Condition::All {
            conditions: vec![
                Condition::Not {
                    condition: Box::new(Condition::Header {
                        name: "X-Spam".into(),
                        matches: None,
                    }),
                },
                Condition::Header {
                    name: "Subject".into(),
                    matches: Some(TextMatch {
                        op: TextOp::Regex,
                        value: r"^\[list\]".into(),
                        case_sensitive: true,
                    }),
                },
            ],
        },
        Condition::Any {
            conditions: vec![
                Condition::Address {
                    field: AddressField::From,
                    matches: tm(TextOp::Contains, "@example.com"),
                },
                Condition::Address {
                    field: AddressField::To,
                    matches: tm(TextOp::Equals, "me@example.com"),
                },
                Condition::Address {
                    field: AddressField::Cc,
                    matches: tm(TextOp::StartsWith, "team"),
                },
                Condition::Address {
                    field: AddressField::Recipients,
                    matches: tm(TextOp::EndsWith, ".org"),
                },
                Condition::Address {
                    field: AddressField::ReplyTo,
                    matches: tm(TextOp::Contains, "noreply"),
                },
                Condition::Address {
                    field: AddressField::Sender,
                    matches: tm(TextOp::Contains, "bounce"),
                },
            ],
        },
        Condition::Not {
            condition: Box::new(Condition::Subject {
                matches: tm(TextOp::Contains, "invoice"),
            }),
        },
        Condition::Not {
            condition: Box::new(Condition::All {
                conditions: vec![
                    Condition::Body {
                        matches: tm(TextOp::Contains, "unsubscribe"),
                    },
                    Condition::ListId { matches: None },
                ],
            }),
        },
        Condition::Query {
            query: "has:attachment is:unread".into(),
        },
        Condition::Attachment {
            name: None,
            mime: None,
        },
        Condition::Attachment {
            name: Some(tm(TextOp::EndsWith, ".pdf")),
            mime: Some(tm(TextOp::StartsWith, "application/")),
        },
        Condition::Size {
            cmp: Comparison::Greater,
            bytes: 5 * 1024 * 1024,
        },
        Condition::Size {
            cmp: Comparison::Less,
            bytes: 1024,
        },
        Condition::Date {
            cmp: DateComparison::Before,
            date: "2026-01-01".into(),
        },
        Condition::Date {
            cmp: DateComparison::After,
            date: "2025-06-30".into(),
        },
        Condition::Age {
            cmp: Comparison::Greater,
            days: 30,
        },
        Condition::Age {
            cmp: Comparison::Less,
            days: 2,
        },
        Condition::Flag {
            flag: MessageFlag::Seen,
            set: false,
        },
        Condition::Flag {
            flag: MessageFlag::Flagged,
            set: true,
        },
        Condition::Tag { tag: "work".into() },
        Condition::Account {
            account_id: "work".into(),
        },
        Condition::Folder {
            folder: FolderRef::Path {
                path: "Lists/Rust".into(),
                account_id: Some("work".into()),
            },
        },
        Condition::Folder {
            folder: FolderRef::Role {
                role: FolderRole::Inbox,
                account_id: None,
            },
        },
        Condition::Auth {
            method: AuthMethod::Dkim,
            result: AuthResult::Pass,
        },
        Condition::Auth {
            method: AuthMethod::Spf,
            result: AuthResult::Softfail,
        },
        Condition::Auth {
            method: AuthMethod::Dmarc,
            result: AuthResult::Fail,
        },
        Condition::ListId {
            matches: Some(tm(TextOp::Contains, "rust-lang")),
        },
    ];
    assert_eq!(conditions, &expected);
    // A rule without triggers, scope or actions is valid.
    assert!(rules[0].triggers.is_empty() && rules[0].scope.is_empty());
    assert!(rules[0].actions.is_empty());
}

/// Every action, trigger and scope kind.
const ALL_ACTIONS: &str = r#"
rule "Actions" id="all-actions" enabled=#false {
    trigger "arrival"
    trigger "on-send"
    trigger "manual"
    trigger "schedule" every="2h"
    trigger "older-than" days=30
    scope "account:work"
    scope "folder:Lists/Rust" account="work"
    scope "role:inbox"
    then {
        move "Reading"
        move role="archive" account="home"
        copy "Backup/2026"
        delete
        delete permanent=#true
        archive
        tag "newsletter"
        untag "todo"
        flag
        flag #false
        mark-read
        mark-read #false
        forward "a@example.com" "b@example.com" identity="work-main"
        redirect "c@example.com"
        reply "Thanks, I am away until Monday." identity="work-main"
        notify
        notify #false
        stop
    }
}
"#;

#[test]
fn every_action_trigger_and_scope_parses() {
    let rule = parse(ALL_ACTIONS).remove(0);
    assert_eq!(rule.id.as_str(), "all-actions");
    assert!(!rule.enabled);
    assert_eq!(
        rule.triggers,
        [
            RuleTrigger::Arrival,
            RuleTrigger::OnSend,
            RuleTrigger::Manual,
            RuleTrigger::Schedule {
                interval_minutes: 120
            },
            RuleTrigger::OlderThan { days: 30 },
        ]
    );
    assert_eq!(
        rule.scope,
        [
            RuleScope::Account {
                account_id: "work".into()
            },
            RuleScope::Folder {
                folder: FolderRef::Path {
                    path: "Lists/Rust".into(),
                    account_id: Some("work".into())
                }
            },
            RuleScope::Folder {
                folder: FolderRef::Role {
                    role: FolderRole::Inbox,
                    account_id: None
                }
            },
        ]
    );
    assert_eq!(rule.conditions, Condition::always());
    assert_eq!(
        rule.actions,
        [
            RuleAction::Move {
                folder: path("Reading")
            },
            RuleAction::Move {
                folder: FolderRef::Role {
                    role: FolderRole::Archive,
                    account_id: Some("home".into())
                }
            },
            RuleAction::Copy {
                folder: path("Backup/2026")
            },
            RuleAction::Delete { permanent: false },
            RuleAction::Delete { permanent: true },
            RuleAction::Archive,
            RuleAction::Tag {
                tag: "newsletter".into()
            },
            RuleAction::Untag { tag: "todo".into() },
            RuleAction::Flag { flagged: true },
            RuleAction::Flag { flagged: false },
            RuleAction::MarkRead { read: true },
            RuleAction::MarkRead { read: false },
            RuleAction::Forward {
                to: vec!["a@example.com".into(), "b@example.com".into()],
                identity_id: Some("work-main".into())
            },
            RuleAction::Redirect {
                to: vec!["c@example.com".into()]
            },
            RuleAction::Reply {
                template: "Thanks, I am away until Monday.".into(),
                identity_id: Some("work-main".into())
            },
            RuleAction::Notify { notify: true },
            RuleAction::Notify { notify: false },
            RuleAction::Stop,
        ]
    );
}

#[test]
fn schedule_intervals() {
    let every = |v: &str| {
        let rule = parse(&format!(
            "rule \"s\" {{\n    trigger \"schedule\" every={v}\n}}\n"
        ))
        .remove(0);
        match rule.triggers.as_slice() {
            [RuleTrigger::Schedule { interval_minutes }] => *interval_minutes,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(every("15"), 15);
    assert_eq!(every("\"30m\""), 30);
    assert_eq!(every("\"1d\""), 1440);
    let e = one_error("rule \"s\" {\n    trigger \"schedule\" every=\"soon\"\n}\n");
    assert!(e.starts_with("rules.kdl:2:24:"), "{e}");
    let e = one_error("rule \"s\" {\n    trigger \"schedule\" every=0\n}\n");
    assert!(e.contains("at least 1"), "{e}");
}

#[test]
fn ids_are_derived_unique_and_explicit_ids_win() {
    let rules = parse(
        r#"
rule "Work mail" {}
rule "Work mail" {}
rule "Other" id="work-mail-2" {}
rule "Äpfel & Birnen!" {}
"#,
    );
    let ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        ["work-mail", "work-mail-3", "work-mail-2", "pfel-birnen"]
    );
    let e = one_error("rule \"a\" id=\"x\" {}\nrule \"b\" id=\"x\" {}\n");
    assert!(
        e.starts_with("rules.kdl:2:10:") && e.contains("used twice"),
        "{e}"
    );
}

#[test]
fn every_error_of_a_file_is_reported_with_its_position() {
    let text = r#"rule "One" {
    when {
        frobnicate "x"
        subject contains="ok"
        size larger="lots"
    }
    then {
        move "A"
        explode
        tag "fine"
    }
}
rule "Two" trigger="sometimes" {}
rule "Three" {
    then { stop }
}
banana
"#;
    let errors = errors(text);
    assert_eq!(errors.len(), 5, "{errors:#?}");
    assert!(
        errors[0].starts_with("rules.kdl:3:9: unknown condition `frobnicate`"),
        "{}",
        errors[0]
    );
    assert!(errors[1].starts_with("rules.kdl:5:14:"), "{}", errors[1]);
    assert!(
        errors[2].starts_with("rules.kdl:9:9: unknown action `explode`"),
        "{}",
        errors[2]
    );
    assert!(
        errors[3].starts_with("rules.kdl:13:12: unknown trigger \"sometimes\""),
        "{}",
        errors[3]
    );
    assert!(
        errors[4].starts_with("rules.kdl:17:1: unknown node `banana`"),
        "{}",
        errors[4]
    );
}

#[test]
fn syntax_errors_are_a_single_error() {
    let e = one_error("rule \"a\" {\n    when {\n");
    assert!(e.contains("syntax error"), "{e}");
}

#[test]
fn specific_errors() {
    let cases: &[(&str, &str)] = &[
        ("rule {}", "needs a name"),
        ("rule \"\" {}", "must not be empty"),
        ("rule \"a\" \"b\" {}", "too many arguments"),
        ("rule \"a\" color=\"red\" {}", "unknown property `color`"),
        ("rule \"a\" id=\"Bad Id\" {}", "invalid rule id"),
        ("rule \"a\" enabled=1 {}", "must be #true or #false"),
        ("rule \"a\" scope=\"planet:earth\" {}", "unknown scope kind"),
        ("rule \"a\" scope=\"account:\" {}", "invalid scope"),
        ("rule \"a\" scope=\"role:attic\" {}", "unknown folder role"),
        ("rule \"a\" { when {} \n when {} }", "only one `when`"),
        ("rule \"a\" { then {} \n then {} }", "only one `then`"),
        ("rule \"a\" { when some {} }", "`all` or `any`"),
        ("rule \"a\" { if {} }", "unknown node `if`"),
        (
            "rule \"a\" { trigger \"arrival\"\n trigger \"arrival\" }",
            "listed twice",
        ),
        ("rule \"a\" trigger=\"schedule\" {}", "needs parameters"),
        ("rule \"a\" { trigger \"older-than\" }", "needs days"),
        ("rule \"a\" { when { subject } }", "needs a comparison"),
        (
            "rule \"a\" { when { subject contains=\"a\" equals=\"b\" } }",
            "only one comparison",
        ),
        (
            "rule \"a\" { when { subject contains=\"\" } }",
            "must not be empty",
        ),
        (
            "rule \"a\" { when { header \"X\" exists=#true contains=\"a\" } }",
            "not both",
        ),
        (
            "rule \"a\" { when { header \"X: y\" } }",
            "not a header name",
        ),
        (
            "rule \"a\" { when { list-id case-sensitive=#true } }",
            "needs a comparison to apply",
        ),
        (
            "rule \"a\" { when { date before=\"2026-02-30\" } }",
            "is not a date",
        ),
        ("rule \"a\" { when { size } }", "needs larger="),
        (
            "rule \"a\" { when { size larger=1 smaller=2 } }",
            "only one of",
        ),
        ("rule \"a\" { when { age older-than=-1 } }", "whole number"),
        ("rule \"a\" { when { flag \"red\" } }", "unknown flag"),
        ("rule \"a\" { when { dkim \"great\" } }", "unknown result"),
        (
            "rule \"a\" { when { tag \"two words\" } }",
            "cannot be a tag",
        ),
        (
            "rule \"a\" { when { attachment { size larger=1 } } }",
            "expected name or type",
        ),
        (
            "rule \"a\" { when { attachment { name contains=\"a\"\n name contains=\"b\" } } }",
            "given twice",
        ),
        ("rule \"a\" { when { folder } }", "needs a folder"),
        (
            "rule \"a\" { when { folder \"A\" role=\"inbox\" } }",
            "not both",
        ),
        (
            "rule \"a\" { then { move role=\"attic\" } }",
            "unknown folder role",
        ),
        ("rule \"a\" { then { forward } }", "at least one address"),
        (
            "rule \"a\" { then { forward \"not an address\" } }",
            "not an email address",
        ),
        (
            "rule \"a\" { then { redirect \"a@b\" identity=\"x\" } }",
            "unknown property `identity`",
        ),
        (
            "rule \"a\" { then { flag \"yes\" } }",
            "must be #true or #false",
        ),
        (
            "rule \"a\" { then { archive { stop } } }",
            "does not take a children block",
        ),
        ("rule \"a\" { then { stop 1 } }", "takes no arguments"),
        ("rule \"a\" { then { reply } }", "needs a template"),
    ];
    for (text, needle) in cases {
        let errors = errors(text);
        assert!(
            errors.iter().any(|e| e.contains(needle)),
            "{text:?}: expected {needle:?} in {errors:#?}"
        );
    }
}

#[test]
fn canonical_text_round_trips() {
    for text in [DESIGN_EXAMPLE, ALL_CONDITIONS, ALL_ACTIONS] {
        let rules = parse(text);
        let rendered = render_rules(&rules);
        let again = parse(&rendered);
        let strip =
            |rules: Vec<Rule>| -> Vec<Rule> { rules.into_iter().map(normalize_rule).collect() };
        assert_eq!(strip(again), strip(rules), "{rendered}");
    }
}

#[test]
fn canonical_form_of_the_design_example() {
    let rule = parse(DESIGN_EXAMPLE).remove(0);
    assert_eq!(
        rule_to_text(&rule),
        r#"rule "Newsletters" trigger="arrival" scope="account:Everything" {
    when any {
        header "List-Id" exists=#true
        query "from:*@substack.com"
    }
    then {
        tag "newsletter"
        move "Reading"
        notify #false
    }
}
"#
    );
}

#[test]
fn text_form_of_one_rule() {
    let rule = parse(ALL_ACTIONS).remove(0);
    let text = rule_to_text(&rule);
    let back = parse_rule_text(&text).unwrap();
    assert_eq!(normalize_rule(back), normalize_rule(rule));

    // A bare condition is wrapped into `all` by the text form.
    let bare = Rule {
        id: RuleId::new("x"),
        name: "X".into(),
        enabled: true,
        triggers: vec![RuleTrigger::Schedule {
            interval_minutes: 5,
        }],
        scope: vec![],
        conditions: Condition::Not {
            condition: Box::new(Condition::All {
                conditions: vec![Condition::Tag { tag: "a".into() }],
            }),
        },
        actions: vec![],
        source: None,
    };
    let text = rule_to_text(&bare);
    assert!(text.contains("trigger \"schedule\" every=5"), "{text}");
    assert_eq!(parse_rule_text(&text).unwrap(), {
        let mut n = normalize_rule(bare);
        n.source = parse_rule_text(&text).unwrap().source;
        n
    });

    let errors = parse_rule_text("").unwrap_err();
    assert!(errors[0].message.contains("found none"));
    let errors = parse_rule_text("rule \"a\" {}\nrule \"b\" {}").unwrap_err();
    assert!(errors[0].message.contains("found 2"));
    let errors = parse_rule_text("rule \"a\" {\n  when { nope }\n}").unwrap_err();
    assert_eq!(errors[0].file, RULE_TEXT_SOURCE);
    assert_eq!((errors[0].line, errors[0].column), (2, 10));
}

#[test]
fn an_empty_file_has_no_rules() {
    assert!(parse("").is_empty());
    assert!(parse("// only a comment\n").is_empty());
    assert_eq!(render_rules(&[]), "");
}

#[test]
fn special_characters_survive_the_writer() {
    let rule = Rule {
        id: RuleId::new("quotes"),
        name: "Quotes \"and\" \\ back\nslash".into(),
        enabled: true,
        triggers: vec![],
        scope: vec![],
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Regex,
                    value: r#"^\s*"(re|fwd)":"#.into(),
                    case_sensitive: false,
                },
            }],
        },
        actions: vec![RuleAction::Reply {
            template: "Line 1\nLine \"2\"\n\n— {{sender}}".into(),
            identity_id: None,
        }],
        source: None,
    };
    let back = parse_rule_text(&rule_to_text(&rule)).unwrap();
    assert_eq!(normalize_rule(back), normalize_rule(rule));
}
