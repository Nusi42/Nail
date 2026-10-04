//! M4a plug-in writes through a running host: every write needs its
//! permission, reaches the core only through `PluginServices::write` (kind,
//! operation, attribution, account scope), names only messages the plug-in
//! may read / organize, and answers the core's outcome.

#![cfg(test)]

#[expect(
    dead_code,
    reason = "the shared test helpers serve several test crates; each uses a subset"
)]
mod common;

use common::*;
use nail_api::agents::{ActionActor, ActionKind};
use nail_api::mcp::tools::McpWriteResult;
use nail_api::{ApiError, ErrorCode};
use nail_plugins::{AccountScope, PluginWrite};
use serde_json::json;

/// Runs `body` (an async function body using `nail`) at activation and
/// logs `R <json>` with its result or `E <code>` with its error.
fn script(body: &str) -> String {
    format!(
        r#"
        export default {{ async activate() {{
          try {{
            const r = await (async () => {{ {body} }})();
            nail.log.info("R " + JSON.stringify(r === undefined ? null : r));
          }} catch (e) {{
            nail.log.info("E " + e.code);
          }}
        }} }};
        "#
    )
}

async fn outcome(fake: &Fake, plugin: &str) -> Result<serde_json::Value, String> {
    let line = wait_for(plugin, || {
        fake.results(plugin, "")
            .into_iter()
            .find(|l| l.starts_with("R ") || l.starts_with("E "))
    })
    .await;
    match line.split_once(' ') {
        Some(("R", json)) => Ok(serde_json::from_str(json).unwrap()),
        Some((_, code)) => Err(code.to_owned()),
        None => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_follow_up_example_creates_a_task_through_the_core() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = script(
        r#"const msg = await nail.mail.get("m1");
           const lists = await nail.tasks.lists();
           return await nail.tasks.create({
             list: lists[0].id, title: "Follow up: " + msg.subject, due: "2026-10-03",
             links: [msg.uri],
           });"#,
    );
    host.start(spec("follow", &["mail.read", "tasks.write"], &js))
        .await
        .unwrap();
    let result = outcome(&fake, "follow").await.unwrap();
    assert_eq!(
        result,
        json!({"status": "done", "opIds": ["op-1"], "created": ["new-1"]})
    );
    let writes = fake.writes();
    assert_eq!(writes.len(), 1);
    let w = &writes[0];
    assert_eq!(w.kind, ActionKind::Task);
    assert_eq!(w.operation, "nail.tasks.create");
    assert_eq!(w.attribution.actor, ActionActor::plugin("follow"));
    assert_eq!(w.attribution.name, "Test follow");
    assert_eq!(w.attribution.on_behalf_of, None);
    assert_eq!(w.scope, AccountScope::All);
    let PluginWrite::UpsertTask(args) = &w.write else {
        panic!("{:?}", w.write)
    };
    assert_eq!(args.list_id.as_deref(), Some("list-1"));
    assert_eq!(args.title.as_deref(), Some("Follow up: Subject m1"));
    assert_eq!(args.related_message_id.as_deref(), Some("m1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn every_write_needs_its_permission() {
    let fake = Fake::new();
    let host = host(&fake);
    let cases = [
        (
            "w1",
            r#"return await nail.mail.update(["m1"], {flagged: true});"#,
            "mail.organize",
        ),
        (
            "w2",
            r#"return await nail.mail.delete("m1");"#,
            "mail.organize",
        ),
        (
            "w3",
            r#"return await nail.mail.createDraft({to: ["a@b.c"], body: "x"});"#,
            "mail.send",
        ),
        ("w4", r#"return await nail.mail.send("d1");"#, "mail.send"),
        (
            "w5",
            r#"return await nail.calendar.createEvent({title: "T", start: "2026-10-05", end: "2026-10-06"});"#,
            "calendar.write",
        ),
        (
            "w6",
            r#"return await nail.calendar.deleteEvent("e1");"#,
            "calendar.write",
        ),
        (
            "w7",
            r#"return await nail.contacts.upsert({fullName: "Ada"});"#,
            "contacts.write",
        ),
        (
            "w8",
            r#"return await nail.contacts.delete("c1");"#,
            "contacts.write",
        ),
        (
            "w9",
            r#"return await nail.tasks.create({title: "T"});"#,
            "tasks.write",
        ),
        (
            "w10",
            r#"return await nail.tasks.delete("t1");"#,
            "tasks.write",
        ),
        ("w11", r"return await nail.tasks.lists();", "tasks.write"),
        (
            "w12",
            r#"return await nail.contacts.find("ada@example.com");"#,
            "contacts.write",
        ),
        (
            "w13",
            r"return await nail.calendar.calendars();",
            "calendar.write",
        ),
    ];
    for (id, body, permission) in cases {
        // Declares mail.read only: every write is refused.
        host.start(spec(id, &["mail.read"], &script(body)))
            .await
            .unwrap();
        assert_eq!(outcome(&fake, id).await, Err("forbidden".into()), "{id}");
        let denied = fake.localized(id, "plugin-host-api-denied");
        assert_eq!(
            denied[0].localized.as_ref().unwrap().args["permission"].to_string(),
            permission,
            "{id}"
        );
    }
    assert!(fake.writes().is_empty());

    // Invitations need mail.send next to calendar.write.
    let body = r#"return await nail.calendar.createEvent({title: "T", start: "a", end: "b",
                    attendees: ["bob@example.com"], sendInvitations: true});"#;
    host.start(spec("invite", &["calendar.write"], &script(body)))
        .await
        .unwrap();
    assert_eq!(outcome(&fake, "invite").await, Err("forbidden".into()));
    // calendar.read is enough to list calendars.
    host.start(spec(
        "reader",
        &["calendar.read"],
        &script("return await nail.calendar.calendars();"),
    ))
    .await
    .unwrap();
    assert_eq!(
        outcome(&fake, "reader").await.unwrap()[0]["id"],
        json!("cal-1")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn message_writes_stay_inside_the_granted_accounts() {
    let fake = Fake::new();
    let host = host(&fake);
    // m1 is in `work`, m2 in `home`; organize is granted for `work` only.
    let js = script(
        r#"const ok = await nail.mail.update(["m1"], {addTags: ["crm"], read: true});
           let other;
           try { await nail.mail.update(["m1", "m2"], {flagged: true}); } catch (e) { other = e.code; }
           let missing;
           try { await nail.mail.delete(["nope"], {permanent: true}); } catch (e) { missing = e.code; }
           return {ok, other, missing};"#,
    );
    let mut s = spec("organizer", &["mail.read", "mail.organize"], &js);
    s.grants = vec![
        grant("mail.read", None),
        grant("mail.organize", Some(&["work"])),
    ];
    host.start(s).await.unwrap();
    let result = outcome(&fake, "organizer").await.unwrap();
    assert_eq!(result["ok"]["status"], "done");
    assert_eq!(result["other"], "not_found");
    assert_eq!(result["missing"], "not_found");
    let writes = fake.writes();
    assert_eq!(writes.len(), 1, "refused writes never reach the core");
    assert_eq!(writes[0].kind, ActionKind::Organize);
    assert_eq!(writes[0].scope, AccountScope::Only(vec!["work".into()]));
    let PluginWrite::UpdateMessages(args) = &writes[0].write else {
        panic!()
    };
    assert_eq!(args.add_tags, ["crm"]);
    assert_eq!(args.read, Some(true));
}

#[tokio::test(flavor = "multi_thread")]
async fn deletes_and_sends_carry_their_kind_and_the_core_decides() {
    let fake = Fake::new();
    let host = host(&fake);
    *fake.write_result.lock().unwrap() = Some(Ok(McpWriteResult::Denied {
        confirmation_id: "conf-1".into(),
    }));
    let js = script(
        r#"const trash = await nail.mail.delete("m1");
           const gone = await nail.mail.delete(["m1"], {permanent: true});
           const sent = await nail.mail.send("d1", {sendAt: "2026-10-03T08:00:00Z"});
           return {trash, gone, sent};"#,
    );
    let mut s = spec("deleter", &["mail.read", "mail.organize", "mail.send"], &js);
    s.grants = vec![
        grant("mail.read", None),
        grant("mail.organize", None),
        grant("mail.send", Some(&["home"])),
    ];
    host.start(s).await.unwrap();
    let result = outcome(&fake, "deleter").await.unwrap();
    assert_eq!(
        result["trash"],
        json!({"status": "denied", "confirmationId": "conf-1"})
    );
    let kinds: Vec<ActionKind> = fake.writes().iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        [
            ActionKind::Trash,
            ActionKind::PermanentDelete,
            ActionKind::Send
        ]
    );
    // Sending is limited to the accounts of the mail.send grant.
    assert_eq!(
        fake.writes()[2].scope,
        AccountScope::Only(vec!["home".into()])
    );

    // Errors of the core reach the plug-in.
    *fake.write_result.lock().unwrap() = Some(Err(ApiError::new(
        ErrorCode::RateLimited,
        "too many pending confirmations",
    )));
    host.start(spec(
        "limited",
        &["tasks.write"],
        &script(r#"return await nail.tasks.create({title: "x"});"#),
    ))
    .await
    .unwrap();
    assert_eq!(outcome(&fake, "limited").await, Err("rate_limited".into()));
}

#[tokio::test(flavor = "multi_thread")]
async fn drafts_take_bytes_and_copy_only_readable_attachments() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = script(
        r#"const bytes = new Uint8Array([104, 105]);
           const made = await nail.mail.createDraft({
             to: ["ada@example.com"], subject: "Hi", body: "**Hello**", replyToMessageId: "m1",
             attachments: [{filename: "hi.txt", mimeType: "text/plain", data: bytes},
                           {filename: "buf.bin", data: bytes.buffer}],
           });
           let encrypted;
           try {
             await nail.mail.updateDraft("d1", {addAttachments: [
               {filename: "x", fromMessageId: "m3", fromAttachmentIndex: 0}]});
           } catch (e) { encrypted = e.code; }
           return {made, encrypted};"#,
    );
    host.start(spec(
        "drafter",
        &["mail.read", "mail.read.body", "mail.send"],
        &js,
    ))
    .await
    .unwrap();
    let result = outcome(&fake, "drafter").await.unwrap();
    assert_eq!(result["made"]["status"], "done");
    assert_eq!(result["encrypted"], "forbidden");
    let writes = fake.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].kind, ActionKind::Draft);
    let PluginWrite::CreateDraft(args) = &writes[0].write else {
        panic!()
    };
    assert_eq!(args.attachments[0].data_base64.as_deref(), Some("aGk="));
    assert_eq!(args.attachments[1].data_base64.as_deref(), Some("aGk="));
    assert_eq!(args.reply_to_message_id.as_deref(), Some("m1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_write_arguments_are_refused_before_the_core() {
    let fake = Fake::new();
    let host = host(&fake);
    for (id, body) in [
        ("bad1", r#"return await nail.mail.update(["m1"], {});"#),
        ("bad2", r"return await nail.mail.update([], {read: true});"),
        (
            "bad3",
            r#"return await nail.tasks.create({title: "x", priority: 42});"#,
        ),
        (
            "bad4",
            r#"return await nail.tasks.create({title: "x", lsit: "typo"});"#, // spellchecker:disable-line
        ),
        (
            "bad5",
            r#"return await nail.calendar.respond({response: "maybe", eventId: "e"});"#,
        ),
    ] {
        host.start(spec(
            id,
            &[
                "mail.read",
                "mail.organize",
                "tasks.write",
                "calendar.write",
            ],
            &script(body),
        ))
        .await
        .unwrap();
        assert_eq!(
            outcome(&fake, id).await,
            Err("invalid_request".into()),
            "{id}"
        );
    }
    assert!(fake.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn collections_are_listed_with_the_write_permissions() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = script(
        r#"return {
             books: await nail.contacts.addressBooks(),
             found: await nail.contacts.find("ada@example.com"),
             lists: await nail.tasks.lists(),
             calendars: await nail.calendar.calendars(),
           };"#,
    );
    host.start(spec(
        "pim",
        &["contacts.write", "tasks.write", "calendar.write"],
        &js,
    ))
    .await
    .unwrap();
    let result = outcome(&fake, "pim").await.unwrap();
    assert_eq!(result["books"][0]["id"], "book-1");
    assert_eq!(result["found"][0]["fullName"], "Ada");
    assert_eq!(result["lists"][0]["name"], "Inbox");
    assert_eq!(result["calendars"][0]["readOnly"], false);
}
