//! `RulesDocument`: edits keep comments, blank lines and formatting of
//! everything they do not change.
#![cfg(test)]

use nail_api::RuleId;
use nail_api::rules::{Condition, FolderRef, Rule, RuleAction, RuleTrigger, TextMatch, TextOp};
use nail_config::rules::{RULES_FILE, RulesDocument, normalize_rule, parse_rules};
use pretty_assertions::assert_eq;

const FILE: &str = r#"// My rules.
// Keep newsletters out of the inbox.

// Newsletters first.
rule "Newsletters" trigger="arrival" {
    when any {
        // Lists announce themselves.
        header "List-Id" exists=#true
        query  "from:*@substack.com"   // Substack
    }
    then {
        tag "newsletter"
        move "Reading"
        notify #false
    }
}

rule "Boss"   trigger="arrival" {   // important
    when {
        from contains="boss@work.example"
    }
    then {
        flag
        stop
    }
}

// ----- archive -----

rule "Old mail" {
    trigger "older-than" days=90
    then { archive }
}
"#;

fn doc() -> RulesDocument {
    RulesDocument::parse(FILE, RULES_FILE).unwrap()
}

fn rule(doc: &RulesDocument, id: &str) -> Rule {
    doc.rule(&RuleId::new(id)).unwrap().clone()
}

/// The document text still means the document rules.
fn check(doc: &RulesDocument) {
    let reread = parse_rules(doc.text(), RULES_FILE).unwrap();
    assert_eq!(reread, doc.rules());
}

#[test]
fn unchanged_rules_are_not_touched() {
    let mut d = doc();
    d.set_rules(d.rules().to_vec()).unwrap();
    assert_eq!(d.text(), FILE);
}

#[test]
fn changing_an_action_keeps_the_rest_of_the_rule() {
    let mut d = doc();
    let mut r = rule(&d, "newsletters");
    r.actions[1] = RuleAction::Move {
        folder: FolderRef::Path {
            path: "Later".into(),
            account_id: None,
        },
    };
    let saved = d.upsert_rule(r, None).unwrap();
    assert_eq!(saved.id.as_str(), "newsletters");
    assert_eq!(saved.source.as_ref().map(|s| s.line), Some(5));
    let expected = FILE.replace("move \"Reading\"", "move \"Later\"");
    assert_eq!(d.text(), expected);
    check(&d);
}

#[test]
fn changing_a_condition_keeps_comments_of_the_others() {
    let mut d = doc();
    let mut r = rule(&d, "newsletters");
    let Condition::Any { conditions } = &mut r.conditions else {
        panic!()
    };
    conditions.push(Condition::Subject {
        matches: TextMatch {
            op: TextOp::StartsWith,
            value: "[news]".into(),
            case_sensitive: false,
        },
    });
    d.upsert_rule(r, None).unwrap();
    let expected = FILE.replace(
        "        query  \"from:*@substack.com\"   // Substack\n",
        "        query  \"from:*@substack.com\"   // Substack\n        subject starts-with=\"[news]\"\n",
    );
    assert_eq!(d.text(), expected);
    check(&d);
}

#[test]
fn changing_the_header_keeps_the_blocks() {
    let mut d = doc();
    let mut r = rule(&d, "boss");
    r.enabled = false;
    d.upsert_rule(r, None).unwrap();
    let expected = FILE.replace(
        "rule \"Boss\"   trigger=\"arrival\" {   // important",
        "rule \"Boss\" enabled=#false trigger=\"arrival\" {   // important",
    );
    assert_eq!(d.text(), expected);
    check(&d);
}

#[test]
fn adding_a_trigger_turns_the_property_into_children() {
    let mut d = doc();
    let mut r = rule(&d, "boss");
    r.triggers.push(RuleTrigger::Schedule {
        interval_minutes: 60,
    });
    d.upsert_rule(r, None).unwrap();
    assert!(
        d.text().contains(
            "rule \"Boss\" {   // important\n    trigger \"arrival\"\n    trigger \"schedule\" every=60\n    when {\n        from contains=\"boss@work.example\"\n    }"
        ),
        "{}",
        d.text()
    );
    check(&d);
}

#[test]
fn removing_a_rule_takes_its_comments_along() {
    let mut d = doc();
    assert!(d.remove_rule(&RuleId::new("newsletters")).unwrap());
    assert!(!d.remove_rule(&RuleId::new("nope")).unwrap());
    let start = FILE.find("// Newsletters first.").unwrap();
    let end = FILE.find("rule \"Boss\"").unwrap();
    let expected = format!("{}{}", &FILE[..start], &FILE[end..]);
    assert_eq!(d.text(), expected);
    check(&d);

    // Removing the last one.
    assert!(d.remove_rule(&RuleId::new("old-mail")).unwrap());
    assert!(!d.text().contains("Old mail"));
    assert!(d.text().contains("// ----- archive -----"));
    check(&d);
}

#[test]
fn reordering_moves_rules_with_their_comments() {
    let mut d = doc();
    d.reorder(&[
        RuleId::new("boss"),
        RuleId::new("newsletters"),
        RuleId::new("old-mail"),
    ])
    .unwrap();
    let ids: Vec<&str> = d.rules().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["boss", "newsletters", "old-mail"]);
    let text = d.text();
    // Free comments stay at the top; the attached one moves with its rule.
    assert!(
        text.starts_with("// My rules.\n// Keep newsletters out of the inbox.\n\nrule \"Boss\""),
        "{text}"
    );
    assert!(
        text.contains("}\n\n// Newsletters first.\nrule \"Newsletters\""),
        "{text}"
    );
    check(&d);

    assert!(d.reorder(&[RuleId::new("boss")]).is_err());
    assert!(
        d.reorder(&[
            RuleId::new("boss"),
            RuleId::new("boss"),
            RuleId::new("old-mail")
        ])
        .is_err()
    );
}

fn simple(name: &str) -> Rule {
    Rule {
        id: RuleId::new(""),
        name: name.into(),
        enabled: true,
        triggers: vec![RuleTrigger::Manual],
        scope: vec![],
        conditions: Condition::All {
            conditions: vec![Condition::Tag { tag: "x".into() }],
        },
        actions: vec![RuleAction::MarkRead { read: true }],
        source: None,
    }
}

#[test]
fn new_rules_go_to_their_position() {
    let mut d = doc();
    let first = d.upsert_rule(simple("First"), Some(0)).unwrap();
    assert_eq!(first.id.as_str(), "first");
    let middle = d.upsert_rule(simple("Boss"), Some(2)).unwrap();
    assert_eq!(middle.id.as_str(), "boss-2");
    let last = d.upsert_rule(simple("Last"), None).unwrap();
    let ids: Vec<&str> = d.rules().iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        ["first", "newsletters", "boss-2", "boss", "old-mail", "last"]
    );
    assert_eq!(last.source.unwrap().file, RULES_FILE);
    let text = d.text();
    assert!(
        text.contains(
            "rule \"First\" trigger=\"manual\" {\n    when {\n        tag \"x\"\n    }\n    then {\n        mark-read\n    }\n}\n\n// Newsletters first."
        ),
        "{text}"
    );
    assert!(
        text.contains("rule \"Boss\" id=\"boss-2\" trigger=\"manual\""),
        "{text}"
    );
    assert!(text.ends_with("then { archive }\n}\n\nrule \"Last\" trigger=\"manual\" {\n    when {\n        tag \"x\"\n    }\n    then {\n        mark-read\n    }\n}\n"), "{text}");
    check(&d);
}

#[test]
fn an_empty_document_gets_the_canonical_form() {
    let mut d = RulesDocument::empty(RULES_FILE);
    d.upsert_rule(simple("A"), None).unwrap();
    d.upsert_rule(simple("B"), None).unwrap();
    assert_eq!(
        d.text(),
        "rule \"A\" trigger=\"manual\" {\n    when {\n        tag \"x\"\n    }\n    then {\n        mark-read\n    }\n}\n\nrule \"B\" trigger=\"manual\" {\n    when {\n        tag \"x\"\n    }\n    then {\n        mark-read\n    }\n}\n"
    );
    check(&d);

    let mut commented = RulesDocument::parse("// nothing yet", RULES_FILE).unwrap();
    commented.upsert_rule(simple("A"), None).unwrap();
    assert!(
        commented.text().starts_with("// nothing yet\n\nrule \"A\""),
        "{}",
        commented.text()
    );
    check(&commented);
}

#[test]
fn line_endings_and_indentation_are_kept() {
    let text = "rule \"A\" {\r\n\twhen {\r\n\t\ttag \"x\"\r\n\t}\r\n}\r\n";
    let mut d = RulesDocument::parse(text, RULES_FILE).unwrap();
    let mut a = d.rules()[0].clone();
    a.actions.push(RuleAction::Stop);
    d.upsert_rule(a, None).unwrap();
    d.upsert_rule(simple("B"), None).unwrap();
    assert_eq!(
        d.text(),
        "rule \"A\" {\r\n\twhen {\r\n\t\ttag \"x\"\r\n\t}\r\n\tthen {\r\n\t\tstop\r\n\t}\r\n}\r\n\r\nrule \"B\" trigger=\"manual\" {\r\n\twhen {\r\n\t\ttag \"x\"\r\n\t}\r\n\tthen {\r\n\t\tmark-read\r\n\t}\r\n}\r\n"
    );
    check(&d);
}

#[test]
fn a_rule_can_be_renamed_and_keeps_its_id() {
    let mut d = doc();
    let mut r = rule(&d, "boss");
    r.name = "Manager".into();
    let saved = d.upsert_rule(r, None).unwrap();
    assert_eq!(saved.id.as_str(), "boss");
    assert!(
        d.text().contains("rule \"Manager\" id=\"boss\""),
        "{}",
        d.text()
    );
    check(&d);
}

#[test]
fn invalid_edits_leave_the_document_unchanged() {
    let mut d = doc();
    let before = d.clone();
    let mut bad = simple("Bad");
    bad.id = RuleId::new("Not Valid");
    assert!(d.upsert_rule(bad, None).is_err());
    assert!(d.upsert_rule(simple("   "), None).is_err());
    let mut dup = d.rules().to_vec();
    dup.push(dup[0].clone());
    assert!(d.set_rules(dup).is_err());
    assert_eq!(d, before);
}

#[test]
fn single_line_blocks_are_merged() {
    let text = "rule \"A\" { when { tag \"x\"; tag \"y\" }; then { stop } }\n";
    let mut d = RulesDocument::parse(text, RULES_FILE).unwrap();
    let mut a = d.rules()[0].clone();
    a.actions = vec![RuleAction::Archive];
    d.upsert_rule(a.clone(), None).unwrap();
    check(&d);
    assert_eq!(normalize_rule(d.rules()[0].clone()), normalize_rule(a));
    assert!(
        d.text().contains("when { tag \"x\"; tag \"y\" }"),
        "{}",
        d.text()
    );
}

#[test]
fn files_survive_a_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let missing = RulesDocument::load(dir.path()).unwrap();
    assert!(missing.rules().is_empty());
    let d = doc();
    d.save(dir.path()).unwrap();
    let loaded = RulesDocument::load(dir.path()).unwrap();
    assert_eq!(loaded.text(), FILE);
    assert_eq!(loaded.rules(), d.rules());
    std::fs::write(dir.path().join(RULES_FILE), "rule {}").unwrap();
    let errors = RulesDocument::load(dir.path()).unwrap_err();
    assert_eq!(errors.len(), 1);
}

/// Many random edits: the text always means the rules.
#[test]
fn random_edit_sequences_stay_consistent() {
    let mut seed = 0x2545_f491_u64;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % n).unwrap()
    };
    let mut d = doc();
    for step in 0..200 {
        let count = d.rules().len() as u64;
        match next(5) {
            0 => {
                let rule = simple(&format!("Rule {step}"));
                d.upsert_rule(rule, Some(next(count + 1))).unwrap();
            }
            1 if count > 0 => {
                let id = d.rules()[next(count)].id.clone();
                d.remove_rule(&id).unwrap();
            }
            2 if count > 1 => {
                let mut ids: Vec<RuleId> = d.rules().iter().map(|r| r.id.clone()).collect();
                let a = next(count);
                let b = next(count);
                ids.swap(a, b);
                d.reorder(&ids).unwrap();
            }
            3 if count > 0 => {
                let mut r = d.rules()[next(count)].clone();
                r.actions.push(RuleAction::Tag {
                    tag: format!("t{step}"),
                });
                if let Condition::All { conditions } | Condition::Any { conditions } =
                    &mut r.conditions
                {
                    if next(2) == 0 {
                        conditions.push(Condition::Tag {
                            tag: format!("c{step}"),
                        });
                    } else if !conditions.is_empty() {
                        conditions.remove(0);
                    }
                }
                d.upsert_rule(r, None).unwrap();
            }
            _ if count > 0 => {
                let mut r = d.rules()[next(count)].clone();
                r.enabled = !r.enabled;
                d.upsert_rule(r, None).unwrap();
            }
            _ => {}
        }
        check(&d);
        // The preserving path was used (the canonical fallback drops
        // comments).
        assert!(d.text().starts_with("// My rules.\n"), "{}", d.text());
        assert!(d.text().contains("// ----- archive -----"), "{}", d.text());
    }
}
