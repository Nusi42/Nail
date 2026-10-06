//! The round-trip harness (plan M2c WP51): every Sieve-expressible rule
//! compiles to a script the mock server accepts, which parses and
//! decompiles back to the rule's canonical form; everything else gets the
//! right placement and reasons.
#![cfg(test)]

use nail_api::domain::FolderRole;
use nail_api::localized::MessageArg;
use nail_api::rules::{
    AddressField, AuthMethod, AuthResult, Comparison, Condition, DateComparison, FolderRef, Rule,
    RuleAction, RuleScope, RuleTrigger, TextMatch, TextOp,
};
use nail_api::sieve::{RulePlacement, RuleSieveStatus, VacationSettings};
use nail_sieve::{
    COMMON_EXTENSIONS, CompileOptions, FolderMap, VacationState, compile, decompile,
    normalize_rule, parse, reasons, to_crlf,
};
use pretty_assertions::assert_eq;

fn folders() -> FolderMap {
    FolderMap::new(
        Some('.'),
        &[
            (FolderRole::Archive, "Archive"),
            (FolderRole::Trash, "Trash"),
            (FolderRole::Junk, "Spam"),
        ],
    )
}

fn options<'a>(folders: &'a FolderMap, extensions: &[&str]) -> CompileOptions<'a> {
    CompileOptions {
        account_id: "work".into(),
        account_aliases: vec!["all-mail".into()],
        extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
        folders,
        own_addresses: vec!["ada@example.org".into()],
        utc_offset_minutes: 120,
        today: Some("2026-09-30".into()),
        max_redirects: Some(4),
        previous_active: None,
    }
}

fn rule(id: &str, conditions: Condition, actions: Vec<RuleAction>) -> Rule {
    Rule {
        id: id.into(),
        name: id.into(),
        enabled: true,
        triggers: vec![RuleTrigger::Arrival, RuleTrigger::Manual],
        scope: Vec::new(),
        conditions,
        actions,
        source: None,
    }
}

fn m(op: TextOp, value: &str) -> TextMatch {
    TextMatch {
        op,
        value: value.into(),
        case_sensitive: false,
    }
}

fn cs(op: TextOp, value: &str) -> TextMatch {
    TextMatch {
        case_sensitive: true,
        ..m(op, value)
    }
}

fn path(p: &str) -> FolderRef {
    FolderRef::Path {
        path: p.into(),
        account_id: None,
    }
}

fn all(conditions: Vec<Condition>) -> Condition {
    Condition::All { conditions }
}

fn any(conditions: Vec<Condition>) -> Condition {
    Condition::Any { conditions }
}

fn tag(t: &str) -> RuleAction {
    RuleAction::Tag { tag: t.into() }
}

/// Rules covering every expressible condition and action.
fn corpus() -> Vec<Rule> {
    vec![
        rule(
            "subject-ops",
            any(vec![
                Condition::Subject {
                    matches: m(TextOp::Equals, "  Hello  "),
                },
                Condition::Subject {
                    matches: m(TextOp::Contains, "50% off"),
                },
                Condition::Subject {
                    matches: cs(TextOp::StartsWith, "[list*]"),
                },
                Condition::Subject {
                    matches: m(TextOp::EndsWith, "?\\"),
                },
                Condition::Subject {
                    matches: m(TextOp::Regex, r"invoice\s+#\d+"),
                },
                Condition::Subject {
                    matches: m(TextOp::StartsWith, ""),
                },
                Condition::Subject {
                    matches: m(TextOp::StartsWith, "C:\\"),
                },
                Condition::Subject {
                    matches: m(TextOp::StartsWith, "a*"),
                },
            ]),
            vec![RuleAction::Move {
                folder: path("Lists/Rust"),
            }],
        ),
        rule(
            "headers",
            all(vec![
                Condition::Header {
                    name: "X-Spam-Flag".into(),
                    matches: None,
                },
                Condition::Header {
                    name: " X-Priority ".into(),
                    matches: Some(m(TextOp::Equals, "1")),
                },
                Condition::Header {
                    name: "Subject".into(),
                    matches: Some(m(TextOp::Contains, "urgent")),
                },
                Condition::Not {
                    condition: Box::new(Condition::Header {
                        name: "list-id".into(),
                        matches: Some(m(TextOp::Equals, "x")),
                    }),
                },
            ]),
            vec![
                RuleAction::Flag { flagged: true },
                RuleAction::MarkRead { read: false },
                tag("urgent"),
            ],
        ),
        rule(
            "addresses",
            any(vec![
                Condition::Address {
                    field: AddressField::From,
                    matches: m(TextOp::EndsWith, "@substack.com"),
                },
                Condition::Address {
                    field: AddressField::Recipients,
                    matches: cs(TextOp::Equals, "Team@Example.org"),
                },
                Condition::Address {
                    field: AddressField::ReplyTo,
                    matches: m(TextOp::Contains, "noreply"),
                },
                Condition::Address {
                    field: AddressField::Sender,
                    matches: m(TextOp::Regex, "^bounce-.*@"),
                },
                Condition::Address {
                    field: AddressField::To,
                    matches: m(TextOp::StartsWith, "ada+"),
                },
                Condition::Address {
                    field: AddressField::Cc,
                    matches: m(TextOp::Contains, "bob"),
                },
            ]),
            vec![
                RuleAction::Archive,
                RuleAction::Untag {
                    tag: "inbox".into(),
                },
                RuleAction::Stop,
                // Ignored after `stop`, like the rule engine does.
                tag("never"),
            ],
        ),
        rule(
            "lists",
            any(vec![
                Condition::ListId { matches: None },
                Condition::ListId {
                    matches: Some(m(TextOp::Equals, "rust.lists.example.org")),
                },
                Condition::ListId {
                    matches: Some(m(TextOp::StartsWith, "rust.")),
                },
                Condition::ListId {
                    matches: Some(m(TextOp::EndsWith, ".example.org")),
                },
                Condition::ListId {
                    matches: Some(m(TextOp::Contains, "rust")),
                },
            ]),
            vec![
                RuleAction::Copy {
                    folder: FolderRef::Role {
                        role: FolderRole::Junk,
                        account_id: Some("work".into()),
                    },
                },
                RuleAction::Delete { permanent: false },
            ],
        ),
        rule(
            "size-date-auth",
            all(vec![
                Condition::Size {
                    cmp: Comparison::Greater,
                    bytes: 10 * 1024 * 1024,
                },
                Condition::Size {
                    cmp: Comparison::Less,
                    bytes: 20_000_000,
                },
                Condition::Date {
                    cmp: DateComparison::Before,
                    date: "2026-10-01".into(),
                },
                Condition::Date {
                    cmp: DateComparison::After,
                    date: "2025-01-31".into(),
                },
                Condition::Auth {
                    method: AuthMethod::Dkim,
                    result: AuthResult::Fail,
                },
                Condition::Auth {
                    method: AuthMethod::Spf,
                    result: AuthResult::Softfail,
                },
            ]),
            vec![RuleAction::Delete { permanent: true }],
        ),
        rule(
            "body-and-accounts",
            all(vec![
                Condition::Body {
                    matches: m(TextOp::Contains, "unsubscribe"),
                },
                Condition::Account {
                    account_id: "work".into(),
                },
                Condition::Account {
                    account_id: "all-mail".into(),
                },
                Condition::Not {
                    condition: Box::new(Condition::Account {
                        account_id: "home".into(),
                    }),
                },
                Condition::Folder {
                    folder: FolderRef::Role {
                        role: FolderRole::Inbox,
                        account_id: None,
                    },
                },
                Condition::always(),
            ]),
            vec![
                RuleAction::Redirect {
                    to: vec!["archive@example.org".into(), " backup@example.org ".into()],
                },
                RuleAction::Redirect {
                    to: vec!["third@example.org".into()],
                },
                RuleAction::Redirect { to: vec![] },
            ],
        ),
        rule(
            "unicode-and-variables",
            any(vec![
                Condition::Subject {
                    matches: m(TextOp::Contains, "Grüße"),
                },
                Condition::Subject {
                    matches: cs(TextOp::Equals, "Straße"),
                },
                Condition::Header {
                    name: "x-template".into(),
                    matches: Some(m(TextOp::Contains, "${name} costs $5 \"quoted\"")),
                },
            ]),
            vec![
                RuleAction::Move {
                    folder: path("Price ${x}"),
                },
                RuleAction::MarkRead { read: true },
            ],
        ),
        rule("always", Condition::always(), vec![tag("$Label1")]),
        rule(
            "never",
            any(vec![]),
            vec![RuleAction::Flag { flagged: false }, RuleAction::Stop],
        ),
    ]
}

#[test]
fn every_expressible_rule_round_trips() {
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    let rules = corpus();
    let compiled = compile(&rules, &VacationSettings::default(), &opts);
    for status in &compiled.rules {
        assert_eq!(
            status.placement,
            RulePlacement::Server,
            "{}: {:?}",
            status.rule_id,
            status.reasons
        );
    }
    assert_eq!(compiled.server_rules().len(), rules.len());

    // The mock server's validation accepts it (CRLF, as uploaded).
    let wire = to_crlf(&compiled.script);
    let extensions: Vec<String> = COMMON_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect();
    nail_testkit_net::validate_script(&wire, &extensions).unwrap();

    // It parses, every used extension is required, and it decompiles to the
    // canonical form of each rule.
    let parsed = parse(&wire).unwrap();
    parsed.check_requires().unwrap();
    let back = decompile(&wire).unwrap();
    assert!(back.managed);
    assert!(back.deferred_delivery);
    assert_eq!(back.vacation, None);
    assert_eq!(
        back.required.iter().cloned().collect::<Vec<_>>(),
        compiled.required
    );
    assert_eq!(back.rules.len(), rules.len());
    for (original, decompiled) in rules.iter().zip(&back.rules) {
        assert_eq!(decompiled.id, original.id.as_str());
        let (condition, actions) = normalize_rule(original, &opts);
        assert_eq!(decompiled.condition, condition, "{}", original.id);
        assert_eq!(decompiled.actions, actions, "{}", original.id);
    }
}

#[test]
fn generated_script_shape() {
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    let rules = vec![rule(
        "news",
        Condition::Subject {
            matches: m(TextOp::StartsWith, "[news]"),
        },
        vec![
            RuleAction::Move {
                folder: path("Reading"),
            },
            tag("newsletter"),
        ],
    )];
    let compiled = compile(&rules, &VacationSettings::default(), &opts);
    let expected = "\
# nail-managed-script
require [\"fileinto\", \"imap4flags\", \"variables\"];
set \"nail_dollar\" \"$\";
# rule \"news\"
if header :matches \"subject\" \"[news]*\" {
    set \"nail_deliver\" \"fileinto\";
    set \"nail_folder\" \"Reading\";
    addflag \"newsletter\";
}
# nail:deliver
if string :is \"${nail_deliver}\" \"fileinto\" {
    fileinto \"${nail_folder}\";
} elsif string :is \"${nail_deliver}\" \"discard\" {
    discard;
}
";
    assert_eq!(compiled.script, expected);
    assert_eq!(compiled.required, ["fileinto", "imap4flags", "variables"]);

    // Flags only: no variables, no delivery block; `stop` stays plain.
    let flags_only = vec![rule(
        "flag",
        Condition::Size {
            cmp: Comparison::Greater,
            bytes: 100,
        },
        vec![RuleAction::Flag { flagged: true }, RuleAction::Stop],
    )];
    let compiled = compile(&flags_only, &VacationSettings::default(), &opts);
    assert_eq!(
        compiled.script,
        "# nail-managed-script\nrequire \"imap4flags\";\n# rule \"flag\"\n\
         if size :over 100 {\n    addflag \"\\\\Flagged\";\n    stop;\n}\n"
    );
    // Nothing to do: just the marker (implicit keep).
    let empty = compile(&[], &VacationSettings::default(), &opts);
    assert_eq!(empty.script, "# nail-managed-script\n");
    assert!(empty.rules.is_empty() && empty.required.is_empty());
}

#[test]
fn stop_flushes_the_pending_delivery() {
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    let rules = vec![
        rule("first", Condition::always(), vec![RuleAction::Archive]),
        rule("second", Condition::always(), vec![RuleAction::Stop]),
    ];
    let compiled = compile(&rules, &VacationSettings::default(), &opts);
    let second = compiled.script.split("# rule \"second\"").nth(1).unwrap();
    let before_stop = second.split("stop;").next().unwrap();
    assert!(
        before_stop.contains("fileinto \"${nail_folder}\""),
        "{second}"
    );
    let back = decompile(&compiled.script).unwrap();
    assert_eq!(back.rules[1].actions, [RuleAction::Stop]);
}

fn reason_ids(status: &RuleSieveStatus) -> Vec<&str> {
    status.reasons.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn not_expressible_rules_say_why() {
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    let cases: Vec<(Rule, Vec<&str>)> = vec![
        (
            rule(
                "q",
                all(vec![
                    Condition::always(),
                    Condition::Query {
                        query: "from:ada".into(),
                    },
                ]),
                vec![],
            ),
            vec![reasons::QUERY],
        ),
        (
            rule(
                "many",
                any(vec![
                    Condition::Attachment {
                        name: None,
                        mime: None,
                    },
                    Condition::Age {
                        cmp: Comparison::Greater,
                        days: 3,
                    },
                    Condition::Flag {
                        flag: nail_api::rules::MessageFlag::Seen,
                        set: true,
                    },
                    Condition::Tag { tag: "x".into() },
                    Condition::Folder {
                        folder: path("Lists"),
                    },
                    Condition::Subject {
                        matches: m(TextOp::Regex, r"\bword\b"),
                    },
                    Condition::Subject {
                        matches: m(TextOp::Regex, "(unclosed"),
                    },
                    Condition::Subject {
                        matches: m(TextOp::Regex, "grüße"),
                    },
                    Condition::Header {
                        name: "Bad Name".into(),
                        matches: None,
                    },
                    Condition::Date {
                        cmp: DateComparison::Before,
                        date: "soon".into(),
                    },
                ]),
                vec![
                    tag("two words"),
                    RuleAction::Forward {
                        to: vec!["a@b.c".into()],
                        identity_id: None,
                    },
                    RuleAction::Reply {
                        template: "Hi".into(),
                        identity_id: None,
                    },
                    RuleAction::Notify { notify: false },
                    RuleAction::Move {
                        folder: FolderRef::Path {
                            path: "X".into(),
                            account_id: Some("home".into()),
                        },
                    },
                    RuleAction::Redirect {
                        to: vec!["not an address".into()],
                    },
                ],
            ),
            vec![
                reasons::ATTACHMENT,
                reasons::AGE,
                reasons::FLAG,
                reasons::TAG,
                reasons::FOLDER,
                reasons::REGEX,
                reasons::INVALID,
                reasons::REGEX,
                reasons::HEADER_NAME,
                reasons::INVALID,
                reasons::KEYWORD,
                reasons::FORWARD,
                reasons::REPLY,
                reasons::NOTIFY,
                reasons::OTHER_ACCOUNT,
                reasons::ADDRESS,
            ],
        ),
    ];
    for (rule, expected) in cases {
        let compiled = compile(
            std::slice::from_ref(&rule),
            &VacationSettings::default(),
            &opts,
        );
        let status = &compiled.rules[0];
        assert_eq!(status.placement, RulePlacement::NotExpressible);
        assert_eq!(reason_ids(status), expected, "{}", rule.id);
        assert_eq!(compiled.script, "# nail-managed-script\n");
    }
    // Positions: the query is the second child of the root group.
    let compiled = compile(
        &[rule(
            "q",
            all(vec![
                Condition::always(),
                Condition::Not {
                    condition: Box::new(Condition::Query { query: "x".into() }),
                },
            ]),
            vec![],
        )],
        &VacationSettings::default(),
        &opts,
    );
    assert_eq!(
        compiled.rules[0].reasons[0].args["position"],
        MessageArg::from("1.2.1")
    );
    let compiled = compile(
        &[rule(
            "a",
            Condition::always(),
            vec![tag("ok"), RuleAction::Notify { notify: true }],
        )],
        &VacationSettings::default(),
        &opts,
    );
    assert_eq!(
        compiled.rules[0].reasons[0].args["action"],
        MessageArg::Number(2)
    );
}

#[test]
fn client_rules_and_scope() {
    use RulePlacement::{Client, Server};
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    let base = || rule("r", Condition::always(), vec![tag("x")]);
    let mut disabled = base();
    disabled.enabled = false;
    let mut scheduled = base();
    scheduled.triggers.push(RuleTrigger::Schedule {
        interval_minutes: 5,
    });
    let mut manual = base();
    manual.triggers = vec![RuleTrigger::Manual];
    let mut other_folder = base();
    other_folder.scope = vec![RuleScope::Folder {
        folder: path("Lists"),
    }];
    let mut inbox_only = base();
    inbox_only.scope = vec![RuleScope::Folder {
        folder: path("INBOX"),
    }];
    let mut account_and_folder = base();
    account_and_folder.scope = vec![
        RuleScope::Folder {
            folder: path("Lists"),
        },
        RuleScope::Account {
            account_id: "all-mail".into(),
        },
    ];
    let mut elsewhere = base();
    elsewhere.scope = vec![
        RuleScope::Account {
            account_id: "home".into(),
        },
        RuleScope::Folder {
            folder: FolderRef::Path {
                path: "INBOX".into(),
                account_id: Some("home".into()),
            },
        },
    ];
    let unknown_folder = rule(
        "u",
        Condition::always(),
        vec![RuleAction::Move {
            folder: FolderRef::Role {
                role: FolderRole::Sent,
                account_id: None,
            },
        }],
    );
    let too_many = rule(
        "redirects",
        Condition::always(),
        vec![RuleAction::Redirect {
            to: (0..5).map(|i| format!("r{i}@example.org")).collect(),
        }],
    );
    let rules = vec![
        disabled,
        scheduled,
        manual,
        other_folder,
        inbox_only,
        account_and_folder,
        elsewhere,
        unknown_folder,
        too_many,
    ];
    let compiled = compile(&rules, &VacationSettings::default(), &opts);
    // `elsewhere` does not apply to this account at all.
    assert_eq!(compiled.rules.len(), rules.len() - 1);
    let got: Vec<(RulePlacement, Vec<&str>)> = compiled
        .rules
        .iter()
        .map(|s| (s.placement, reason_ids(s)))
        .collect();
    assert_eq!(
        got,
        [
            (Client, vec![reasons::DISABLED]),
            (Client, vec![reasons::TRIGGER]),
            (Client, vec![reasons::NO_ARRIVAL]),
            (Client, vec![reasons::SCOPE]),
            (Server, vec![]),
            (Server, vec![]),
            (Client, vec![reasons::FOLDER_UNKNOWN]),
            (Client, vec![reasons::REDIRECTS]),
        ]
    );
    let unknown = &compiled.rules[6].reasons[0];
    assert_eq!(unknown.args["folder"], MessageArg::from("sent"));
    let redirects = &compiled.rules[7].reasons[0];
    assert_eq!(redirects.args["count"], MessageArg::Number(5));
    assert_eq!(redirects.args["max"], MessageArg::Number(4));
}

#[test]
fn missing_extensions_keep_rules_on_the_client() {
    use RulePlacement::{Client, Server};
    let folders = folders();
    let rules = vec![
        rule(
            "move",
            Condition::always(),
            vec![RuleAction::Move { folder: path("A") }],
        ),
        rule(
            "body",
            Condition::Body {
                matches: m(TextOp::Contains, "x"),
            },
            vec![tag("b")],
        ),
        rule(
            "regex",
            Condition::Subject {
                matches: m(TextOp::Regex, "a+"),
            },
            vec![tag("r")],
        ),
        rule(
            "copy",
            Condition::always(),
            vec![RuleAction::Copy { folder: path("B") }],
        ),
        rule(
            "unicode",
            Condition::Subject {
                matches: m(TextOp::Contains, "ä"),
            },
            vec![RuleAction::Flag { flagged: true }],
        ),
        rule(
            "size",
            Condition::Size {
                cmp: Comparison::Less,
                bytes: 5,
            },
            vec![RuleAction::Stop],
        ),
    ];
    // A minimal RFC 5228 server: only fileinto.
    let opts = options(&folders, &["fileinto"]);
    let compiled = compile(&rules, &VacationSettings::default(), &opts);
    let got: Vec<(&str, RulePlacement, Vec<String>)> = compiled
        .rules
        .iter()
        .map(|s| {
            (
                s.rule_id.as_str(),
                s.placement,
                s.reasons
                    .iter()
                    .map(|r| r.args["extension"].to_string())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("move", Client, vec!["variables".to_owned()]),
            ("body", Client, vec!["body".into(), "imap4flags".into()]),
            ("regex", Client, vec!["imap4flags".into(), "regex".into()]),
            ("copy", Client, vec!["copy".into()]),
            (
                "unicode",
                Client,
                vec!["comparator-i;unicode-casemap".into(), "imap4flags".into()]
            ),
            ("size", Server, vec![]),
        ]
    );
    let wire = to_crlf(&compiled.script);
    nail_testkit_net::validate_script(&wire, &["fileinto".to_owned()]).unwrap();
    assert!(compiled.required.is_empty(), "{:?}", compiled.required);
}

#[test]
fn previous_active_script_is_included_and_recorded() {
    let folders = folders();
    let mut opts = options(&folders, COMMON_EXTENSIONS);
    opts.previous_active = Some("roundcube".into());
    let compiled = compile(&[], &VacationSettings::default(), &opts);
    assert_eq!(compiled.included.as_deref(), Some("roundcube"));
    assert!(compiled.script.contains("# nail:previous \"roundcube\"\n"));
    assert!(compiled.script.contains("include :personal \"roundcube\";"));
    let back = decompile(&compiled.script).unwrap();
    assert_eq!(back.included.as_deref(), Some("roundcube"));
    assert_eq!(back.previous_active.as_deref(), Some("roundcube"));
    // Without `include` the name is only recorded.
    let mut opts = options(&folders, &["fileinto"]);
    opts.previous_active = Some("roundcube".into());
    let compiled = compile(&[], &VacationSettings::default(), &opts);
    assert_eq!(compiled.included, None);
    assert!(!compiled.script.contains("include"));
    assert_eq!(
        nail_sieve::recorded_previous(&compiled.script).as_deref(),
        Some("roundcube")
    );
}

fn away() -> VacationSettings {
    VacationSettings {
        enabled: true,
        subject: "Out of office \"${x}\"".into(),
        body: "I am away.\n.\n..and back soon.\nCosts ${nothing}.".into(),
        start_date: Some("2026-10-01".into()),
        end_date: Some("2026-10-10".into()),
        days: 3,
        addresses: vec!["ada@example.org".into(), "lovelace@example.org".into()],
        from: Some("ada@example.org".into()),
        skip_lists: true,
    }
}

#[test]
fn vacation_round_trips() {
    let folders = folders();
    let opts = options(&folders, COMMON_EXTENSIONS);
    // With a rule that needs variables, so `${` must be protected.
    let rules = vec![rule("m", Condition::always(), vec![RuleAction::Archive])];
    let compiled = compile(&rules, &away(), &opts);
    assert_eq!(compiled.vacation.state, VacationState::Active);
    assert!(compiled.vacation.in_script());
    assert_eq!(compiled.vacation.recompile_on, None);
    for ext in ["vacation", "date", "relational", "variables"] {
        assert!(compiled.required.iter().any(|r| r == ext), "{ext}");
    }
    let wire = to_crlf(&compiled.script);
    let extensions: Vec<String> = COMMON_EXTENSIONS.iter().map(|e| (*e).to_owned()).collect();
    nail_testkit_net::validate_script(&wire, &extensions).unwrap();
    let back = decompile(&wire).unwrap();
    let expected = VacationSettings {
        // Identities are added, without duplicates.
        addresses: vec!["ada@example.org".into(), "lovelace@example.org".into()],
        ..away()
    };
    assert_eq!(back.vacation, Some(expected));
    assert!(compiled.script.contains(":zone \"+0200\""));

    // Without list filtering and dates, no identities: an unconditional
    // reply.
    let simple = VacationSettings {
        enabled: true,
        body: "Away".into(),
        ..VacationSettings::default()
    };
    let mut opts = options(&folders, COMMON_EXTENSIONS);
    opts.own_addresses.clear();
    let plain = VacationSettings {
        skip_lists: false,
        ..simple.clone()
    };
    let compiled = compile(&[], &plain, &opts);
    assert_eq!(
        compiled.script,
        "# nail-managed-script\nrequire \"vacation\";\n# nail:vacation\n\
         vacation :days 7 text:\nAway\n.\n;\n"
    );
    assert_eq!(decompile(&compiled.script).unwrap().vacation, Some(plain));
    let compiled = compile(&[], &simple, &opts);
    assert_eq!(decompile(&compiled.script).unwrap().vacation, Some(simple));
}

#[test]
fn vacation_without_server_support() {
    let folders = folders();
    // Disabled or invalid settings: nothing in the script.
    let opts = options(&folders, COMMON_EXTENSIONS);
    let off = compile(&[], &VacationSettings::default(), &opts);
    assert_eq!(off.vacation.state, VacationState::Off);
    let invalid = VacationSettings {
        body: String::new(),
        ..away()
    };
    assert_eq!(
        compile(&[], &invalid, &opts).vacation.state,
        VacationState::Off
    );

    // No `vacation` extension.
    let opts = options(&folders, &["fileinto"]);
    let compiled = compile(&[], &away(), &opts);
    assert_eq!(compiled.vacation.state, VacationState::Unsupported);
    assert_eq!(compiled.script, "# nail-managed-script\n");

    // `vacation` without `date`: Nail applies the dates.
    let mut opts = options(&folders, &["vacation"]);
    opts.today = Some("2026-09-30".into());
    let waiting = compile(&[], &away(), &opts);
    assert_eq!(waiting.vacation.state, VacationState::Waiting);
    assert_eq!(waiting.vacation.recompile_on.as_deref(), Some("2026-10-01"));
    assert!(!waiting.script.contains("vacation"));
    opts.today = Some("2026-10-05".into());
    let active = compile(&[], &away(), &opts);
    assert_eq!(active.vacation.state, VacationState::Active);
    assert_eq!(active.vacation.recompile_on.as_deref(), Some("2026-10-11"));
    assert!(!active.script.contains("currentdate"));
    let back = decompile(&active.script).unwrap().vacation.unwrap();
    assert_eq!((back.start_date, back.end_date), (None, None));
    opts.today = Some("2026-10-11".into());
    let ended = compile(&[], &away(), &opts);
    assert_eq!(ended.vacation.state, VacationState::Ended);
    assert!(!ended.script.contains("vacation"));
}

#[test]
fn decompile_rejects_foreign_scripts() {
    assert!(decompile("keep").is_err());
    assert!(
        decompile("if true { keep; }")
            .unwrap_err()
            .contains("marker")
    );
    assert!(decompile("require \"fileinto\";\nfileinto \"x\";").is_err());
    assert!(
        decompile("fileinto \"x\";")
            .unwrap_err()
            .contains("needs fileinto")
    );
    let user = decompile("# my rules\nrequire \"fileinto\";").unwrap();
    assert!(!user.managed);
}
