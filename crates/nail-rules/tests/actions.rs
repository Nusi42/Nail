//! Every action kind with its queued operation and undo, rule ordering and
//! stop semantics, triggers, schedules, dry runs and runs.
#![cfg(test)]

mod common;

use common::*;
use nail_api::rules::{Rule, RuleAction, RuleRunCause};
use nail_api::{FolderId, MessageId};
use nail_rules::{
    CompiledRule, Effect, MessageState, MessageView, PlannedAction, RuleSet, SkipReason,
    plan_actions, rule_from_message, run_rule, test_rule, validate,
};
use nail_store::{FlagChange, Operation};
use pretty_assertions::assert_eq;

/// Plans `then { … }` for a message.
fn plan(then: &str, message: &nail_rules::MessageData) -> (Vec<PlannedAction>, MessageState) {
    let m = mailbox();
    let rule = arrival("", then);
    let mut state = MessageState::of(message);
    let (planned, _) = plan_actions(&rule.actions, &mut state, message, &env(&m));
    (planned, state)
}

fn effects(then: &str, message: &nail_rules::MessageData) -> Vec<Effect> {
    plan(then, message)
        .0
        .into_iter()
        .map(|p| p.effect)
        .collect()
}

fn id(s: &str) -> MessageId {
    MessageId::new(s)
}

fn folder(s: &str) -> FolderId {
    FolderId::new(s)
}

fn moved(message: &str, to: &str, back: &str) -> Effect {
    Effect::Queue {
        op: Operation::MoveMessage {
            message_id: id(message),
            target_folder_id: folder(to),
            source: None,
        },
        undo: Some(Operation::MoveMessage {
            message_id: id(message),
            target_folder_id: folder(back),
            source: None,
        }),
    }
}

fn flags(message: &str, change: FlagChange, undo: FlagChange) -> Effect {
    Effect::Queue {
        op: Operation::UpdateFlags {
            message_id: id(message),
            change,
        },
        undo: Some(Operation::UpdateFlags {
            message_id: id(message),
            change: undo,
        }),
    }
}

#[test]
fn move_copy_and_archive() {
    let n = newsletter();
    assert_eq!(
        effects(r#"move "Reading""#, &n),
        [moved("m-news", "w-reading", "w-inbox")]
    );
    assert_eq!(
        effects(r#"move "Reading" account="home""#, &n),
        [moved("m-news", "h-reading", "w-inbox")]
    );
    assert_eq!(
        effects(r#"move "Nowhere""#, &n),
        [Effect::Skipped(SkipReason::FolderNotFound)]
    );
    assert_eq!(
        effects(r#"move role="inbox""#, &n),
        [Effect::Skipped(SkipReason::Unchanged)]
    );
    assert_eq!(
        effects("archive", &n),
        [moved("m-news", "w-archive", "w-inbox")]
    );
    assert_eq!(
        effects("archive", &at_home()),
        [Effect::Skipped(SkipReason::FolderNotFound)]
    );
    assert_eq!(
        effects(r#"copy "Lists/Rust""#, &n),
        [Effect::Copy {
            target: folder("w-rust")
        }]
    );
    // A second move goes on from where the first one left the message, and
    // its undo returns it there.
    let (planned, state) = plan("move \"Reading\"\n move \"Lists/Rust\"", &n);
    assert_eq!(planned[1].effect, moved("m-news", "w-rust", "w-reading"));
    assert_eq!(state.folder_id, folder("w-rust"));
    // A move into another account changes the message's account.
    let (_, state) = plan(r#"move "Reading" account="home""#, &n);
    assert_eq!(state.account_id.as_str(), "home");
}

#[test]
fn deletes() {
    let n = newsletter();
    let (planned, state) = plan("delete\n tag \"after\"\n notify #false", &n);
    assert_eq!(
        planned[0].effect,
        Effect::Queue {
            op: Operation::DeleteMessage {
                message_id: id("m-news"),
                permanent: false,
                source: None,
                trash_folder_id: None,
            },
            undo: Some(Operation::MoveMessage {
                message_id: id("m-news"),
                target_folder_id: folder("w-inbox"),
                source: None,
            }),
        }
    );
    assert!(!planned[0].is_irreversible());
    assert!(state.deleted);
    // Actions after a delete are skipped; notify still applies.
    assert_eq!(planned[1].effect, Effect::Skipped(SkipReason::Deleted));
    assert_eq!(planned[2].effect, Effect::Notify(false));

    let permanent = effects("delete permanent=#true", &n);
    assert!(matches!(
        &permanent[0],
        Effect::Queue {
            op: Operation::DeleteMessage {
                permanent: true,
                ..
            },
            undo: None
        }
    ));
    // Deleting in Trash (or without a Trash) expunges: no undo.
    let mut in_trash = newsletter();
    in_trash.folder_id = folder("w-trash");
    assert!(matches!(
        &effects("delete", &in_trash)[0],
        Effect::Queue { undo: None, .. }
    ));
    assert!(matches!(
        &effects("delete", &at_home())[0],
        Effect::Queue { undo: None, .. }
    ));
}

#[test]
fn tags_flags_and_read_state() {
    let n = newsletter(); // unread, unflagged, tag "Newsletter-Old"
    assert_eq!(
        effects(r#"tag "newsletter""#, &n),
        [flags(
            "m-news",
            FlagChange {
                add_tags: vec!["newsletter".into()],
                ..FlagChange::default()
            },
            FlagChange {
                remove_tags: vec!["newsletter".into()],
                ..FlagChange::default()
            }
        )]
    );
    assert_eq!(
        effects(r#"tag "newsletter-old""#, &n),
        [Effect::Skipped(SkipReason::Unchanged)]
    );
    // Untag removes the tag as the message spells it.
    assert_eq!(
        effects(r#"untag "newsletter-old""#, &n),
        [flags(
            "m-news",
            FlagChange {
                remove_tags: vec!["Newsletter-Old".into()],
                ..FlagChange::default()
            },
            FlagChange {
                add_tags: vec!["Newsletter-Old".into()],
                ..FlagChange::default()
            }
        )]
    );
    assert_eq!(
        effects(r#"untag "nope""#, &n),
        [Effect::Skipped(SkipReason::Unchanged)]
    );
    assert_eq!(
        effects("flag", &n),
        [flags(
            "m-news",
            FlagChange {
                flagged: Some(true),
                ..FlagChange::default()
            },
            FlagChange {
                flagged: Some(false),
                ..FlagChange::default()
            }
        )]
    );
    assert_eq!(
        effects("flag #false", &n),
        [Effect::Skipped(SkipReason::Unchanged)]
    );
    assert_eq!(
        effects("mark-read", &n),
        [flags(
            "m-news",
            FlagChange {
                seen: Some(true),
                ..FlagChange::default()
            },
            FlagChange {
                seen: Some(false),
                ..FlagChange::default()
            }
        )]
    );
    assert_eq!(
        effects("mark-read #false", &from_boss()),
        [flags(
            "m-boss",
            FlagChange {
                seen: Some(false),
                ..FlagChange::default()
            },
            FlagChange {
                seen: Some(true),
                ..FlagChange::default()
            }
        )]
    );
    // The state follows: tagging twice plans once.
    let (planned, state) = plan("tag \"x\"\n tag \"X\"\n mark-read\n mark-read", &n);
    assert!(matches!(
        planned[1].effect,
        Effect::Skipped(SkipReason::Unchanged)
    ));
    assert!(matches!(
        planned[3].effect,
        Effect::Skipped(SkipReason::Unchanged)
    ));
    assert!(state.flags.seen);
    assert!(state.tags.iter().any(|t| t == "x"));
}

#[test]
fn sending_actions() {
    let n = newsletter();
    let b = from_boss();
    assert_eq!(
        effects(
            r#"forward "a@x.example" "b@x.example" identity="work-main""#,
            &n
        ),
        [Effect::Forward {
            to: vec!["a@x.example".into(), "b@x.example".into()],
            identity_id: Some("work-main".into())
        }]
    );
    assert_eq!(
        effects(r#"redirect "a@x.example""#, &n),
        [Effect::Redirect {
            to: vec!["a@x.example".into()]
        }]
    );
    assert_eq!(
        effects(r#"reply "I am away.""#, &b),
        [Effect::Reply {
            template: "I am away.".into(),
            identity_id: None
        }]
    );
    // No automatic replies to list / bulk mail, automatic mail or bounces.
    assert_eq!(
        effects(r#"reply "I am away.""#, &n),
        [Effect::Skipped(SkipReason::AutomaticMail)]
    );
    let mut auto = from_boss();
    auto.headers
        .push(("Auto-Submitted".into(), "auto-replied".into()));
    assert_eq!(
        effects(r#"reply "x""#, &auto),
        [Effect::Skipped(SkipReason::AutomaticMail)]
    );
    let mut explicit_no = from_boss();
    explicit_no
        .headers
        .push(("Auto-Submitted".into(), "no".into()));
    assert!(matches!(
        effects(r#"reply "x""#, &explicit_no)[0],
        Effect::Reply { .. }
    ));
    let mut daemon = from_boss();
    daemon.from = vec![addr(None, "MAILER-DAEMON@work.example")];
    assert_eq!(
        effects(r#"reply "x""#, &daemon),
        [Effect::Skipped(SkipReason::AutomaticMail)]
    );
    let (planned, _) = plan(r#"forward "a@x.example""#, &n);
    assert!(planned[0].is_irreversible());
}

#[test]
fn rules_never_forward_redirect_or_answer_in_a_loop() {
    let skipped = [Effect::Skipped(SkipReason::AutomaticMail)];
    let sending = [
        r#"forward "a@x.example""#,
        r#"redirect "a@x.example""#,
        r#"reply "I am away.""#,
    ];
    // Mail a Nail rule sent (a forward coming back, a redirect of our own,
    // another Nail user's auto-reply) is never acted on again.
    let mut rule_mail = from_boss();
    rule_mail
        .headers
        .push((nail_api::LOOP_HEADER.into(), "boss@work.example".into()));
    assert!(nail_rules::is_rule_mail(&rule_mail));
    for then in sending {
        assert_eq!(effects(then, &rule_mail), skipped, "{then}");
    }
    // A message one of the user's accounts already redirected.
    let mut redirected = from_boss();
    redirected
        .headers
        .push(("Resent-From".into(), "Me <ME@work.example>".into()));
    for then in sending {
        assert_eq!(effects(then, &redirected), skipped, "{then}");
    }
    // Bounces and automatic responses: forwarding them to a dead address
    // would bounce again.
    let mut bounce = from_boss();
    bounce.from = vec![addr(None, "postmaster@work.example")];
    let mut report = from_boss();
    report.headers.push((
        "Content-Type".into(),
        "multipart/report; report-type=delivery-status".into(),
    ));
    let mut vacation = from_boss();
    vacation
        .headers
        .push(("Auto-Submitted".into(), "auto-replied (vacation)".into()));
    for message in [&bounce, &report, &vacation] {
        for then in sending {
            assert_eq!(effects(then, message), skipped, "{then}");
        }
    }
    // Ordinary automatic mail (notifications) and redirects by others can
    // still be forwarded; only the reply is refused.
    let mut notification = from_boss();
    notification
        .headers
        .push(("Auto-Submitted".into(), "auto-generated".into()));
    notification
        .headers
        .push(("Resent-From".into(), "someone@else.example".into()));
    assert!(matches!(
        effects(sending[0], &notification)[0],
        Effect::Forward { .. }
    ));
    assert!(matches!(
        effects(sending[1], &notification)[0],
        Effect::Redirect { .. }
    ));
    assert_eq!(effects(sending[2], &notification), skipped);
}

#[test]
fn notify_and_stop() {
    let n = newsletter();
    let m = mailbox();
    let rule = arrival("", "notify #false\n tag \"a\"\n stop\n tag \"b\"");
    let mut state = MessageState::of(&n);
    let (planned, flow) = plan_actions(&rule.actions, &mut state, &n, &env(&m));
    // Planning ends at `stop` (included).
    assert_eq!(planned.len(), 3);
    assert_eq!(planned[2].effect, Effect::Stop);
    assert!(flow.stopped);
    assert_eq!(flow.notify, Some(false));
    let (_, flow) = plan_actions(&arrival("", "notify").actions, &mut state, &n, &env(&m));
    assert_eq!(flow.notify, Some(true));
    assert!(!flow.stopped);
}

fn named(name: &str, trigger: &str, when: &str, then: &str) -> Rule {
    rule(&format!(
        "rule \"{name}\" {trigger} {{\n when {{\n{when}\n }}\n then {{\n{then}\n }}\n}}"
    ))
}

#[test]
fn rules_run_in_order_and_see_earlier_changes() {
    let m = mailbox();
    let set = RuleSet::new([
        named(
            "Tag lists",
            "trigger=\"arrival\"",
            "list-id",
            "tag \"list\"\n notify #false",
        ),
        named(
            "File tagged",
            "trigger=\"arrival\"",
            "tag \"list\"",
            "move \"Reading\"",
        ),
        // Sees the new folder, so it matches after the move.
        named(
            "In reading",
            "trigger=\"arrival\"",
            "folder \"Reading\"",
            "mark-read",
        ),
        named("Manual only", "trigger=\"manual\"", "", "flag"),
        named("Disabled", "trigger=\"arrival\" enabled=#false", "", "flag"),
    ]);
    assert!(set.broken().is_empty());
    let outcome = set.process(RuleRunCause::Arrival, &newsletter(), &env(&m));
    let names: Vec<&str> = outcome
        .applied
        .iter()
        .map(|a| a.rule_name.as_str())
        .collect();
    assert_eq!(names, ["Tag lists", "File tagged", "In reading"]);
    assert_eq!(outcome.notify, Some(false));
    assert!(!outcome.notify_or(true));
    assert!(!outcome.stopped);
    assert_eq!(outcome.state.folder_id, folder("w-reading"));
    assert!(outcome.state.flags.seen);
    assert_eq!(outcome.actions().count(), 4);

    // Nothing matches a personal mail.
    let outcome = set.process(RuleRunCause::Arrival, &from_boss(), &env(&m));
    assert!(outcome.applied.is_empty());
    assert!(outcome.notify_or(true));
}

#[test]
fn stop_ends_processing_for_the_message() {
    let m = mailbox();
    let set = RuleSet::new([
        named(
            "First",
            "trigger=\"arrival\"",
            "",
            "tag \"one\"\n stop\n tag \"never\"",
        ),
        named("Second", "trigger=\"arrival\"", "", "tag \"two\""),
    ]);
    let outcome = set.process(RuleRunCause::Arrival, &at_home(), &env(&m));
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(outcome.applied[0].actions.len(), 2);
    assert!(outcome.stopped);
    assert_eq!(outcome.state.tags, ["one"]);

    // A delete stops processing too.
    let set = RuleSet::new([
        named("Delete", "trigger=\"arrival\"", "", "delete"),
        named("After", "trigger=\"arrival\"", "", "tag \"two\""),
    ]);
    let outcome = set.process(RuleRunCause::Arrival, &newsletter(), &env(&m));
    assert_eq!(outcome.applied.len(), 1);
    assert!(outcome.stopped && outcome.state.deleted);
}

#[test]
fn triggers_decide_when_rules_run() {
    let r = |trigger: &str| CompiledRule::new(named("r", trigger, "", "stop")).unwrap();
    let arrival = r("trigger=\"arrival\"");
    assert!(arrival.runs_on(RuleRunCause::Arrival));
    assert!(!arrival.runs_on(RuleRunCause::OnSend));
    assert!(!arrival.runs_on(RuleRunCause::Schedule));
    assert!(arrival.runs_on(RuleRunCause::Manual));
    let on_send = r("trigger=\"on-send\"");
    assert!(on_send.runs_on(RuleRunCause::OnSend));
    assert!(!on_send.runs_on(RuleRunCause::Arrival));
    let manual = r("trigger=\"manual\"");
    assert!(manual.runs_on(RuleRunCause::Manual));
    assert!(!manual.runs_on(RuleRunCause::Arrival));
    let disabled = r("trigger=\"arrival\" enabled=#false");
    assert!(!disabled.runs_on(RuleRunCause::Arrival));
    assert!(disabled.runs_on(RuleRunCause::Manual));
    let none = r("");
    assert!(!none.runs_on(RuleRunCause::Arrival));
}

#[test]
fn schedules_and_older_than() {
    let m = mailbox();
    let e = env(&m);
    let scheduled = CompiledRule::new(rule(
        "rule \"s\" {\n trigger \"schedule\" every=30\n then { archive }\n}",
    ))
    .unwrap();
    assert!(scheduled.runs_on(RuleRunCause::Schedule));
    assert_eq!(scheduled.next_run_at(None, NOW), Some(NOW));
    assert_eq!(
        scheduled.next_run_at(Some(NOW - 60), NOW),
        Some(NOW + 29 * 60)
    );
    assert!(scheduled.is_due(Some(NOW - 30 * 60), NOW));
    assert!(!scheduled.is_due(Some(NOW - 60), NOW));

    let old = CompiledRule::new(rule(
        "rule \"old\" {\n trigger \"older-than\" days=30\n then { archive }\n}",
    ))
    .unwrap();
    assert!(old.runs_on(RuleRunCause::Schedule));
    assert_eq!(old.next_run_at(Some(NOW), NOW), Some(NOW + 3_600));
    // The age applies to scheduled runs and, for an older-than-only rule,
    // to dry runs and manual runs too.
    assert!(old.matches(&from_boss(), &e, Some(RuleRunCause::Schedule)));
    assert!(!old.matches(&newsletter(), &e, Some(RuleRunCause::Schedule)));
    assert!(!old.matches(&newsletter(), &e, None));
    assert!(!old.matches(&newsletter(), &e, Some(RuleRunCause::Manual)));
    // With an arrival trigger too, arrival ignores the age.
    // (A `trigger=` property and `trigger` children combine.)
    let both = CompiledRule::new(rule(
        "rule \"both\" trigger=\"arrival\" {\n trigger \"older-than\" days=30\n}",
    ))
    .unwrap();
    assert_eq!(both.rule().triggers.len(), 2);
    assert!(both.matches(&newsletter(), &e, Some(RuleRunCause::Arrival)));
    assert!(both.matches(&newsletter(), &e, None));
    assert!(!both.matches(&newsletter(), &e, Some(RuleRunCause::Schedule)));

    let set = RuleSet::new([scheduled.rule().clone(), old.rule().clone()]);
    let due = set.due(NOW, |id| (id.as_str() == "s").then_some(NOW - 60));
    let ids: Vec<&str> = due.iter().map(|r| r.rule().id.as_str()).collect();
    assert_eq!(ids, ["old"]);
    let manual = CompiledRule::new(named("m", "trigger=\"manual\"", "", "stop")).unwrap();
    assert_eq!(manual.next_run_at(None, NOW), None);
}

#[test]
fn dry_runs_explain_and_change_nothing() {
    let m = mailbox();
    let rule = CompiledRule::new(named(
        "Newsletters",
        "trigger=\"arrival\"",
        "list-id",
        "tag \"newsletter\"\n move \"Reading\"\n stop\n flag",
    ))
    .unwrap();
    let messages = [newsletter(), from_boss(), at_home()];
    let views: Vec<&dyn MessageView> = messages.iter().map(|m| -> &dyn MessageView { m }).collect();
    let report = test_rule(&rule, views.iter().copied(), &env(&m), 10);
    assert_eq!(report.scanned, 3);
    assert_eq!(report.matches.len(), 1);
    let hit = &report.matches[0];
    assert_eq!(hit.message_id, id("m-news"));
    assert_eq!(
        hit.actions,
        [
            RuleAction::Tag {
                tag: "newsletter".into()
            },
            RuleAction::Move {
                folder: nail_api::rules::FolderRef::Path {
                    path: "Reading".into(),
                    account_id: None
                }
            },
            RuleAction::Stop,
        ]
    );
    assert_eq!(hit.planned.len(), 3);
    assert!(hit.hits.iter().any(|h| h.path == [0] && h.matched));
    // The inputs are untouched.
    assert_eq!(messages[0], newsletter());

    // The limit stops scanning.
    let everything = CompiledRule::new(named("all", "", "", "stop")).unwrap();
    let report = test_rule(&everything, views.iter().copied(), &env(&m), 2);
    assert_eq!((report.scanned, report.matches.len()), (2, 2));
}

#[test]
fn a_manual_run_returns_what_to_queue_and_undo() {
    let m = mailbox();
    let rule = CompiledRule::new(named(
        "Archive read",
        "trigger=\"manual\"",
        "flag \"seen\"",
        "tag \"done\"\n archive",
    ))
    .unwrap();
    let messages = [newsletter(), from_boss()];
    let outcomes = run_rule(
        &rule,
        RuleRunCause::Manual,
        messages.iter().map(|m| -> &dyn MessageView { m }),
        &env(&m),
    );
    assert_eq!(outcomes.len(), 1);
    let o = &outcomes[0];
    assert_eq!(o.message_id, id("m-boss"));
    let ops: Vec<(&Operation, &Operation)> = o
        .actions()
        .filter_map(|(_, a)| match &a.effect {
            Effect::Queue {
                op,
                undo: Some(undo),
            } => Some((op, undo)),
            _ => None,
        })
        .collect();
    assert_eq!(ops.len(), 2);
    // Undoing in reverse order restores the message.
    assert!(
        matches!(ops[1].1, Operation::MoveMessage { target_folder_id, .. } if target_folder_id == &folder("w-inbox"))
    );
    assert!(
        matches!(ops[0].1, Operation::UpdateFlags { change, .. } if change.remove_tags == ["done"])
    );
}

#[test]
fn validation_lists_every_problem() {
    let mut rule = named("x", "", "", "stop");
    rule.name = "  ".into();
    rule.triggers = vec![
        nail_api::rules::RuleTrigger::Schedule {
            interval_minutes: 0,
        },
        nail_api::rules::RuleTrigger::OlderThan { days: 0 },
    ];
    rule.actions = vec![
        RuleAction::Tag { tag: " ".into() },
        RuleAction::Forward {
            to: vec![],
            identity_id: None,
        },
        RuleAction::Redirect {
            to: vec!["not-an-address".into()],
        },
        RuleAction::Reply {
            template: "\n".into(),
            identity_id: None,
        },
        RuleAction::Move {
            folder: nail_api::rules::FolderRef::Path {
                path: String::new(),
                account_id: None,
            },
        },
    ];
    let issues: Vec<String> = validate(&rule).iter().map(ToString::to_string).collect();
    assert_eq!(
        issues,
        [
            "the rule needs a name",
            "trigger 1: the schedule interval must be at least one minute",
            "trigger 2: the age must be at least one day",
            "action 1: the tag is empty",
            "action 2: there are no recipients",
            "action 3: a recipient is not an email address",
            "action 4: the reply template is empty",
            "action 5: the folder is empty",
        ]
    );
    // The localized texts read the same in English (core.ftl).
    let localized: Vec<String> = validate(&rule)
        .iter()
        .map(|issue| nail_config::l10n::english(&issue.localized))
        .collect();
    assert_eq!(localized, issues);
    let set = RuleSet::new([rule, named("ok", "", "", "stop")]);
    assert_eq!(set.rules().len(), 1);
    assert_eq!(set.broken().len(), 1);
    assert!(set.get(&"ok".into()).is_some());
}

#[test]
fn rules_from_messages() {
    let from_list = rule_from_message(&newsletter());
    assert_eq!(from_list.name, "Rust Weekly", "named after the list");
    assert_eq!(from_list.triggers, [nail_api::rules::RuleTrigger::Arrival]);
    let text = nail_config::rules::rule_to_text(&from_list);
    assert!(
        text.contains("list-id contains=\"rust-weekly.substack.com\""),
        "{text}"
    );
    let personal = rule_from_message(&from_boss());
    let text = nail_config::rules::rule_to_text(&personal);
    assert!(text.contains("from equals=\"boss@work.example\""), "{text}");
    // It matches the message it came from.
    let m = mailbox();
    let mut rule = personal;
    rule.id = "x".into();
    rule.actions = vec![RuleAction::Stop];
    assert!(
        CompiledRule::new(rule)
            .unwrap()
            .matches(&from_boss(), &env(&m), None)
    );
}

#[test]
fn the_design_example_runs() {
    let rules = nail_config::rules::parse_rules(
        r#"rule "Newsletters" trigger="arrival" scope="virtual:Everything" {
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
"#,
        "rules.kdl",
    )
    .unwrap();
    let m = mailbox();
    let set = RuleSet::new(rules);
    let outcome = set.process(RuleRunCause::Arrival, &newsletter(), &env(&m));
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(outcome.state.folder_id, folder("w-reading"));
    assert_eq!(outcome.notify, Some(false));
    assert!(outcome.state.tags.iter().any(|t| t == "newsletter"));
    let outcome = set.process(RuleRunCause::Arrival, &at_home(), &env(&m));
    assert!(outcome.applied.is_empty());
}
