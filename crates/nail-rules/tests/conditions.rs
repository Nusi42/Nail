//! Every condition kind, nesting, scope and explanations.
#![cfg(test)]

mod common;

use common::*;
use nail_api::rules::{Condition, Rule, RuleRunCause};
use nail_rules::{CompiledRule, ConditionHit, MessageData, MessageView, validate};
use pretty_assertions::assert_eq;

/// Whether a `when { … }` body matches a message.
fn when(body: &str, message: &MessageData) -> bool {
    let m = mailbox();
    let Ok(rule) = CompiledRule::new(arrival(body, "stop")) else {
        panic!("the rule does not compile: {body}");
    };
    rule.matches(message, &env(&m), Some(RuleRunCause::Arrival))
}

#[track_caller]
fn yes(body: &str, message: &MessageData) {
    assert!(when(body, message), "should match: {body}");
}

#[track_caller]
fn no(body: &str, message: &MessageData) {
    assert!(!when(body, message), "should not match: {body}");
}

#[test]
fn headers() {
    let n = newsletter();
    yes(r#"header "list-id" exists=#true"#, &n);
    yes(r#"header "X-Nope" exists=#false"#, &n);
    no(r#"header "X-Nope""#, &n);
    yes(r#"header "X-Mailer" equals="substack""#, &n);
    no(
        r#"header "X-Mailer" equals="substack" case-sensitive=#true"#,
        &n,
    );
    yes(r#"header "Precedence" regex="^(bulk|list)$""#, &n);
    // Any occurrence counts.
    yes(
        r#"header "Authentication-Results" starts-with="forged""#,
        &n,
    );
}

#[test]
fn addresses() {
    let n = newsletter();
    yes(r#"from ends-with="@substack.com""#, &n);
    yes(r#"from equals="ada writer""#, &n);
    yes(r#"from contains="Ada Writer <ada@""#, &n);
    no(r#"from contains="boss""#, &n);
    yes(r#"to equals="me@work.example""#, &n);
    yes(r#"cc starts-with="team@""#, &n);
    no(r#"cc equals="me@work.example""#, &n);
    yes(r#"recipients equals="team@work.example""#, &n);
    yes(r#"recipients equals="me@work.example""#, &n);
    yes(r#"reply-to starts-with="noreply""#, &n);
    yes(r#"sender regex="^bounce@""#, &n);
    no(r#"sender contains="ada""#, &from_boss());
}

#[test]
fn subject_and_body() {
    let n = newsletter();
    yes(r#"subject starts-with="[rust weekly]""#, &n);
    // A raw KDL string keeps backslashes as they are.
    yes(r##"subject regex=#"issue #\d+"#"##, &n);
    no(r#"subject regex="^issue""#, &n);
    yes(r#"body contains="async closures""#, &n);
    no(r#"body contains="python""#, &n);
    // No body downloaded: body conditions never match.
    no(r#"body contains="x""#, &at_home());
    yes(r#"not { body contains="x"; }"#, &at_home());
}

#[test]
fn query_language() {
    let n = newsletter();
    yes(r#"query "from:*@substack.com""#, &n);
    yes(
        r#"query "has:attachment is:unread folder:inbox account:work""#,
        &n,
    );
    yes(r#"query "subject:\"rust weekly\" OR from:nobody""#, &n);
    no(r#"query "from:*@substack.com is:read""#, &n);
    yes(
        r#"query "account:Everything size>100k after:2026-09-28""#,
        &n,
    );
    no(r#"query "tag:work""#, &n);
}

#[test]
fn attachments() {
    let n = newsletter();
    yes("attachment", &n);
    no("attachment", &from_boss());
    yes(r#"attachment { name ends-with=".pdf"; }"#, &n);
    yes(
        r#"attachment { name ends-with=".pdf"; type equals="application/pdf"; }"#,
        &n,
    );
    no(
        r#"attachment { name ends-with=".pdf"; type starts-with="image/"; }"#,
        &n,
    );
    no(
        r#"attachment { name ends-with=".PDF" case-sensitive=#false; type starts-with="image/"; }"#,
        &n,
    );
}

#[test]
fn size_date_and_age() {
    let n = newsletter(); // 150 000 bytes, one day old (2026-09-28 12:00Z)
    yes(r#"size larger="100k""#, &n);
    no(r#"size larger="1MB""#, &n);
    yes("size smaller=200000", &n);
    yes(r#"date before="2026-09-29""#, &n);
    no(r#"date before="2026-09-28""#, &n);
    yes(r#"date after="2026-09-27""#, &n);
    no(r#"date after="2026-09-28""#, &n);
    yes("age newer-than=2", &n);
    no("age older-than=2", &n);
    yes("age older-than=30", &from_boss());
}

#[test]
fn local_days_use_the_time_zone() {
    // 2026-09-28T23:30Z is already the 29th two hours east of UTC.
    let mut late = newsletter();
    late.date = NOW - 12 * 3_600 - 1_800;
    let m = mailbox();
    let rule = CompiledRule::new(arrival(r#"date after="2026-09-28""#, "stop")).unwrap();
    let utc = env(&m);
    assert!(!rule.matches(&late, &utc, None));
    let east = nail_rules::Env {
        utc_offset_secs: 2 * 3_600,
        ..utc
    };
    assert!(rule.matches(&late, &east, None));
}

#[test]
fn flags_and_tags() {
    let n = newsletter();
    yes(r#"flag "seen" set=#false"#, &n);
    no(r#"flag "seen""#, &n);
    yes(r#"flag "seen""#, &from_boss());
    no(r#"flag "flagged""#, &n);
    yes(r#"flag "answered" set=#false"#, &n);
    no(r#"flag "draft""#, &n);
    yes(r#"tag "newsletter-old""#, &n);
    no(r#"tag "newsletter""#, &n);
}

#[test]
fn accounts_and_folders() {
    let n = newsletter();
    yes(r#"account "work""#, &n);
    no(r#"account "home""#, &n);
    yes(r#"account "everything""#, &n);
    yes(r#"account "everything""#, &at_home());
    yes(r#"folder role="inbox""#, &n);
    yes(r#"folder "INBOX""#, &n);
    yes(r#"folder "inbox" account="work""#, &n);
    no(r#"folder "inbox" account="home""#, &n);
    no(r#"folder "Reading""#, &n);
    no(r#"folder role="archive""#, &n);
}

#[test]
fn authentication_results_come_from_the_topmost_header() {
    let n = newsletter();
    yes(r#"dkim "pass""#, &n);
    // The lower, possibly forged header is ignored.
    no(r#"dkim "fail""#, &n);
    yes(r#"spf "softfail""#, &n);
    yes(r#"dmarc "pass""#, &n);
    no(r#"spf "pass""#, &n);
    // No header: the result is "none".
    yes(r#"dkim "none""#, &at_home());
    no(r#"dkim "pass""#, &at_home());
}

#[test]
fn list_id() {
    let n = newsletter();
    yes("list-id", &n);
    no("list-id", &from_boss());
    yes(r#"list-id equals="rust-weekly.substack.com""#, &n);
    yes(r#"list-id starts-with="rust weekly""#, &n);
    no(r#"list-id contains="python""#, &n);
}

#[test]
fn nesting_all_any_not() {
    let n = newsletter();
    let b = from_boss();
    let body = r#"
        any {
            all {
                list-id
                not { from contains="boss" }
            }
            all {
                from contains="boss"
                not {
                    flag "seen" set=#false
                    subject contains="urgent"
                }
            }
        }
    "#;
    yes(body, &n);
    yes(body, &b);
    no(body, &at_home());
    // Empty groups: `all {}` matches everything, `any {}` nothing.
    yes("all {\n}", &at_home());
    no("any {\n}", &at_home());
    // An empty `when` matches every message.
    let m = mailbox();
    let always = CompiledRule::new(rule("rule \"x\" { then { stop } }")).unwrap();
    assert!(always.matches(&at_home(), &env(&m), None));
}

#[test]
fn scope_limits_the_rule() {
    let m = mailbox();
    let e = env(&m);
    let scoped = |scope: &str| {
        CompiledRule::new(rule(&format!(
            "rule \"s\" {{\n scope {scope}\n then {{ stop }}\n}}"
        )))
        .unwrap()
    };
    assert!(scoped("\"account:work\"").matches(&newsletter(), &e, None));
    assert!(!scoped("\"account:work\"").matches(&at_home(), &e, None));
    assert!(scoped("\"virtual:everything\"").matches(&at_home(), &e, None));
    assert!(scoped("\"virtual:Everything\"").matches(&at_home(), &e, None));
    assert!(scoped("\"account:all\"").matches(&at_home(), &e, None));
    assert!(scoped("\"role:inbox\"").matches(&at_home(), &e, None));
    assert!(!scoped("\"folder:Reading\"").matches(&at_home(), &e, None));
    assert!(scoped("\"folder:INBOX\" account=\"home\"").matches(&at_home(), &e, None));
    assert!(!scoped("\"folder:INBOX\" account=\"home\"").matches(&newsletter(), &e, None));
    let explanation = scoped("\"account:home\"").explain(&newsletter(), &e, None);
    assert!(!explanation.matched && !explanation.in_scope && explanation.hits.is_empty());
}

#[test]
fn explanations_name_each_condition_and_value() {
    let m = mailbox();
    let rule = CompiledRule::new(rule(
        r#"rule "Newsletters" trigger="arrival" {
            when any {
                header "List-Id" exists=#true
                query "from:*@substack.com"
                all {
                    subject contains="plan"
                    not { flag "seen" }
                }
            }
        }"#,
    ))
    .unwrap();
    let e = rule.explain(&newsletter(), &env(&m), None);
    assert!(e.matched && e.in_scope);
    let hit = |path: &[u32], matched: bool, value: Option<&str>| ConditionHit {
        path: path.to_vec(),
        matched,
        value: value.map(str::to_owned),
    };
    assert_eq!(
        e.hits,
        [
            hit(&[], true, None),
            hit(&[0], true, Some("Rust Weekly <rust-weekly.substack.com>")),
            hit(&[1], true, None),
            hit(&[2], false, None),
            hit(&[2, 0], false, Some("[Rust Weekly] Issue #42")),
            hit(&[2, 1], true, None),
            hit(&[2, 1, 0], false, None),
        ]
    );
    let e = rule.explain(&from_boss(), &env(&m), None);
    assert!(!e.matched);
    assert_eq!(e.hits[0], hit(&[], false, None));
    assert_eq!(e.hits[4], hit(&[2, 0], true, Some("Quarterly plan")));
    // Explanations serialize for the protocol.
    let json = serde_json::to_value(&e.hits[1]).unwrap();
    assert_eq!(json["path"], serde_json::json!([0]));
}

#[test]
fn broken_conditions_are_reported_with_their_path() {
    let mut r: Rule = arrival(r#"subject contains="x""#, "stop");
    r.conditions = Condition::All {
        conditions: vec![
            Condition::Any {
                conditions: vec![Condition::Subject {
                    matches: nail_api::rules::TextMatch {
                        op: nail_api::rules::TextOp::Regex,
                        value: "(".into(),
                        case_sensitive: false,
                    },
                }],
            },
            Condition::Query {
                query: "from:".into(),
            },
            Condition::Date {
                cmp: nail_api::rules::DateComparison::Before,
                date: "yesterday".into(),
            },
        ],
    };
    let issues = validate(&r);
    let text: Vec<String> = issues.iter().map(ToString::to_string).collect();
    assert_eq!(issues.len(), 3, "{text:#?}");
    assert!(
        text[0].starts_with("condition 1.1: invalid regular expression"),
        "{}",
        text[0]
    );
    assert!(
        text[1].starts_with("condition 2: invalid search query"),
        "{}",
        text[1]
    );
    assert!(
        text[2].starts_with("condition 3: \"yesterday\" is not a date"),
        "{}",
        text[2]
    );
    // The localized texts name the same conditions and read the same.
    let ids: Vec<&str> = issues.iter().map(|i| i.localized.id.as_str()).collect();
    assert_eq!(
        ids,
        ["rule-issue-regex", "rule-issue-query", "rule-issue-date"]
    );
    for (issue, text) in issues.iter().zip(&text) {
        assert_eq!(&nail_config::l10n::english(&issue.localized), text);
    }
    // An error of the whole tree says "conditions".
    let mut root = arrival(r#"subject contains="x""#, "stop");
    root.conditions = Condition::Header {
        name: " ".into(),
        matches: None,
    };
    let issue = &validate(&root)[0];
    assert_eq!(issue.to_string(), "conditions: the header name is empty");
    assert_eq!(
        nail_config::l10n::english(&issue.localized),
        issue.to_string()
    );
    assert!(CompiledRule::new(r).is_err());
}

#[test]
fn messages_can_be_trait_objects() {
    let m = mailbox();
    let messages: Vec<Box<dyn MessageView>> = vec![Box::new(newsletter()), Box::new(at_home())];
    let rule = CompiledRule::new(arrival("list-id", "stop")).unwrap();
    let matched: Vec<bool> = messages
        .iter()
        .map(|msg| rule.matches(msg.as_ref(), &env(&m), None))
        .collect();
    assert_eq!(matched, [true, false]);
}
