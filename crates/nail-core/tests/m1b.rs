//! M1b core integration (ADR 0023–0026) against the `demo-multi` mock
//! servers: the rich composer (quoting, HTML / Markdown / attachments on
//! the wire, server Drafts autosave, address suggestions), rules (text
//! form, dry run, arrival rules through the op queue, undo), customization
//! (layout write-back, merge + backups + revert, every config error), and
//! the MCP server started with the core as its data provider.

use std::future::Future;
use std::time::Duration;

use base64::Engine as _;
use nail_api::accounts::ComposerMode;
use nail_api::rules::{
    Condition, FolderRef, Rule, RuleAction, RuleRunState, RuleTrigger, TextMatch, TextOp,
};
use nail_api::{
    AccountId, Address, ConfigFile, Draft, DraftContext, DraftContextMode, ErrorCode, Event,
    Folder, FolderId, FolderRole, LayoutNodeRef, LayoutPatch, MessageSummary, Request, Response,
};
use nail_core::demo::{
    DemoMulti, MULTI_HOME_ID, MULTI_WORK_EMAIL, MULTI_WORK_ID, start_demo_multi,
};
use nail_core::{Service, ServiceOptions};
use serde_json::json;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(20);

struct Fixture {
    service: Service,
    demo: DemoMulti,
    config_dir: TempDir,
    _data_dir: TempDir,
}

async fn setup() -> Fixture {
    let demo = start_demo_multi().await.unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    demo.write_config(config_dir.path()).unwrap();
    let options = demo.service_options(ServiceOptions::new(data_dir.path(), config_dir.path()));
    let service = Service::open(options).unwrap();
    Fixture {
        service,
        demo,
        config_dir,
        _data_dir: data_dir,
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn request(service: &Service, request: Request) -> Response {
    service.handle(request).await
}

async fn try_folder(service: &Service, account: &str, role: FolderRole) -> Option<Folder> {
    let Response::Folders { folders } = request(
        service,
        Request::ListFolders {
            account_id: AccountId::new(account),
        },
    )
    .await
    else {
        panic!()
    };
    folders.into_iter().find(|f| f.role == role)
}

async fn folder(service: &Service, account: &str, role: FolderRole) -> Folder {
    try_folder(service, account, role)
        .await
        .unwrap_or_else(|| panic!("no {role:?} folder"))
}

async fn messages(service: &Service, folder: &FolderId) -> Vec<MessageSummary> {
    match request(
        service,
        Request::ListMessages {
            folder_id: folder.clone(),
            offset: 0,
            limit: 500,
        },
    )
    .await
    {
        Response::Messages { messages, .. } => messages,
        other => panic!("unexpected {other:?}"),
    }
}

async fn subjects(service: &Service, account: &str, role: FolderRole) -> Vec<String> {
    let Some(folder) = try_folder(service, account, role).await else {
        return Vec::new();
    };
    messages(service, &folder.id)
        .await
        .into_iter()
        .map(|m| m.subject)
        .collect()
}

async fn synced(f: &Fixture) {
    eventually("the work inbox synced", || async {
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
            .await
            .len()
            == 15
    })
    .await;
}

async fn find(service: &Service, role: FolderRole, subject: &str) -> MessageSummary {
    let folder = folder(service, MULTI_WORK_ID, role).await;
    messages(service, &folder.id)
        .await
        .into_iter()
        .find(|m| m.subject.contains(subject))
        .unwrap_or_else(|| panic!("no message {subject:?}"))
}

fn control(f: &Fixture, command: &str, args: serde_json::Value) -> serde_json::Value {
    f.demo.control(command, &args).unwrap()
}

fn smtp_data(f: &Fixture) -> Vec<String> {
    control(f, "smtp-messages", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["data"].as_str().unwrap().to_owned())
        .collect()
}

fn server_subjects(f: &Fixture, mailbox: &str) -> Vec<String> {
    control(f, "imap-messages", json!({ "mailbox": mailbox }))
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["subject"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn blank_draft(subject: &str) -> Draft {
    let mut draft = Draft::new(MULTI_WORK_EMAIL);
    draft.to = vec![Address {
        name: Some("Bob".into()),
        email: "bob@example.org".into(),
    }];
    draft.subject = subject.into();
    draft
}

async fn save(service: &Service, draft: Draft) -> nail_api::DraftId {
    match request(service, Request::SaveDraft { draft }).await {
        Response::DraftSaved { draft_id } => draft_id,
        other => panic!("unexpected {other:?}"),
    }
}

async fn send_now(f: &Fixture, draft: Draft) {
    let draft_id = save(&f.service, draft).await;
    assert!(matches!(
        request(
            &f.service,
            Request::SendDraft {
                draft_id,
                delay_secs: Some(0),
                send_at: None,
                crypto: None,
            },
        )
        .await,
        Response::SendQueued { .. }
    ));
}

async fn sent_with(f: &Fixture, subject: &str) -> String {
    let wanted = format!("Subject: {subject}");
    eventually("the SMTP server received it", || async {
        smtp_data(f).iter().any(|d| d.contains(&wanted))
    })
    .await;
    smtp_data(f)
        .into_iter()
        .find(|d| d.contains(&wanted))
        .unwrap()
}

#[tokio::test]
async fn replies_and_forwards_are_quoted_in_every_mode() {
    let f = setup().await;
    synced(&f).await;
    let original = find(&f.service, FolderRole::Inbox, "Welcome to Nail").await;
    let context = |mode| DraftContext {
        mode,
        message_id: original.id.clone(),
    };
    let new_draft = |mode, context| Request::NewDraft {
        identity_id: None,
        account_id: Some(MULTI_WORK_ID.into()),
        mode: Some(mode),
        context: Some(context),
    };

    let Response::Draft { draft } = request(
        &f.service,
        new_draft(ComposerMode::Plain, context(DraftContextMode::Reply)),
    )
    .await
    else {
        panic!()
    };
    assert_eq!(draft.identity_id.as_str(), MULTI_WORK_EMAIL);
    assert!(draft.subject.starts_with("Re: "), "{}", draft.subject);
    assert_eq!(draft.to.len(), 1);
    assert!(draft.text.contains("wrote:"), "{}", draft.text);
    assert!(
        draft.text.lines().any(|l| l.starts_with('>')),
        "{}",
        draft.text
    );
    assert_eq!(
        draft.context.as_ref().map(|c| c.mode),
        Some(DraftContextMode::Reply)
    );

    let Response::Draft { draft } = request(
        &f.service,
        new_draft(ComposerMode::Markdown, context(DraftContextMode::Reply)),
    )
    .await
    else {
        panic!()
    };
    let markdown = draft.markdown.unwrap();
    assert!(markdown.lines().any(|l| l.starts_with("> ")), "{markdown}");

    let Response::Draft { draft } = request(
        &f.service,
        new_draft(ComposerMode::Html, context(DraftContextMode::Forward)),
    )
    .await
    else {
        panic!()
    };
    assert!(draft.subject.starts_with("Fwd: "), "{}", draft.subject);
    assert!(draft.to.is_empty());
    let html = draft.html.unwrap();
    assert!(html.contains("Forwarded message"), "{html}");

    // An unknown original is not found.
    assert!(matches!(
        request(
            &f.service,
            Request::NewDraft {
                identity_id: None,
                account_id: None,
                mode: None,
                context: Some(DraftContext {
                    mode: DraftContextMode::Reply,
                    message_id: "nope".into(),
                }),
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    f.service.shutdown().await;
}

#[tokio::test]
async fn html_markdown_and_attachments_reach_the_smtp_server() {
    let f = setup().await;
    synced(&f).await;

    let mut html = blank_draft("HTML mail");
    html.mode = ComposerMode::Html;
    html.html = Some("<p>Hello <strong>Bob</strong></p>".into());
    send_now(&f, html).await;
    let data = sent_with(&f, "HTML mail").await;
    assert!(data.contains("multipart/alternative"), "{data}");
    assert!(data.contains("text/html"), "{data}");
    assert!(data.contains("<strong>Bob</strong>"), "{data}");

    let Response::AttachmentUploaded { attachment } = request(
        &f.service,
        Request::UploadAttachment {
            filename: "notes.txt".into(),
            mime: "text/plain".into(),
            data: base64::engine::general_purpose::STANDARD.encode("attached text"),
            content_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(attachment.size, 13);

    // A saved draft serves the content of its own attachments (inline
    // image previews after reopening), nothing else.
    let mut with_file = blank_draft("Draft with a file");
    with_file.attachments = vec![attachment.clone()];
    let draft_id = save(&f.service, with_file).await;
    let Response::AttachmentData { mime, data } = request(
        &f.service,
        Request::GetDraftAttachment {
            draft_id: draft_id.clone(),
            blob: attachment.blob.clone(),
        },
    )
    .await
    else {
        panic!("no attachment data")
    };
    assert_eq!(mime, "text/plain");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap(),
        b"attached text"
    );
    let other = save(&f.service, blank_draft("Draft without files")).await;
    assert!(matches!(
        request(
            &f.service,
            Request::GetDraftAttachment {
                draft_id: other,
                blob: attachment.blob.clone(),
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    let mut markdown = blank_draft("Markdown mail");
    markdown.mode = ComposerMode::Markdown;
    markdown.markdown = Some("# Title\n\n*emphasis* and `code`".into());
    markdown.markdown_part = true;
    markdown.attachments = vec![attachment];
    send_now(&f, markdown).await;
    let data = sent_with(&f, "Markdown mail").await;
    for part in [
        "multipart/mixed",
        "multipart/alternative",
        "text/markdown",
        "text/html",
        "notes.txt",
    ] {
        assert!(data.contains(part), "{part} missing:\n{data}");
    }

    // Recipients are suggested afterwards.
    let Response::AddressSuggestions { addresses } = request(
        &f.service,
        Request::SuggestAddresses {
            prefix: "bo".into(),
            limit: 5,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(
        addresses.iter().any(|a| a.email == "bob@example.org"),
        "{addresses:?}"
    );

    // Bad uploads are refused.
    assert!(matches!(
        request(
            &f.service,
            Request::UploadAttachment {
                filename: "x".into(),
                mime: String::new(),
                data: "not base64!".into(),
                content_id: None,
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    f.service.shutdown().await;
}

#[tokio::test]
async fn preview_and_mode_conversion_come_from_the_core() {
    let f = setup().await;
    let Response::PreviewRendered { html } = request(
        &f.service,
        Request::RenderPreview {
            markdown: "# Hi\n\n<script>x()</script>".into(),
            identity_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(html.contains("<h1"), "{html}");
    assert!(!html.contains("<script"), "{html}");

    let mut draft = blank_draft("x");
    draft.mode = ComposerMode::Html;
    draft.html = Some("<p><strong>bold</strong></p>".into());
    let Response::DraftConverted { draft, lossy } = request(
        &f.service,
        Request::ConvertDraft {
            draft: Box::new(draft),
            mode: ComposerMode::Plain,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(lossy);
    assert_eq!(draft.mode, ComposerMode::Plain);
    assert!(draft.text.contains("bold"));
    let Response::DraftConverted { lossy, draft } = request(
        &f.service,
        Request::ConvertDraft {
            draft,
            mode: ComposerMode::Markdown,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(!lossy, "plain → Markdown is lossless");
    assert!(draft.markdown.unwrap().contains("bold"));
    f.service.shutdown().await;
}

#[tokio::test]
async fn drafts_are_autosaved_to_the_server_and_removed_after_sending() {
    let f = setup().await;
    synced(&f).await;
    let mut draft = blank_draft("Autosaved v1");
    draft.text = "first".into();
    let id = save(&f.service, draft.clone()).await;
    eventually("the draft on the server", || async {
        server_subjects(&f, "Drafts").contains(&"Autosaved v1".to_owned())
    })
    .await;
    // A later save replaces the copy.
    draft.id = Some(id.clone());
    draft.subject = "Autosaved v2".into();
    save(&f.service, draft).await;
    eventually("the copy replaced", || async {
        let on_server = server_subjects(&f, "Drafts");
        on_server.contains(&"Autosaved v2".to_owned())
            && !on_server.contains(&"Autosaved v1".to_owned())
    })
    .await;
    assert_eq!(
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Drafts).await,
        ["Autosaved v2"]
    );
    // It is listed for continuing after a restart.
    let Response::Drafts { drafts } = request(&f.service, Request::ListDrafts).await else {
        panic!()
    };
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].id.as_ref(), Some(&id));
    // Sending removes it.
    request(
        &f.service,
        Request::SendDraft {
            draft_id: id,
            delay_secs: Some(0),
            send_at: None,
            crypto: None,
        },
    )
    .await;
    sent_with(&f, "Autosaved v2").await;
    eventually("the copy deleted after sending", || async {
        server_subjects(&f, "Drafts").is_empty()
    })
    .await;
    let Response::Drafts { drafts } = request(&f.service, Request::ListDrafts).await else {
        panic!()
    };
    assert!(drafts.is_empty(), "{drafts:?}");
    f.service.shutdown().await;
}

fn invoice_rule() -> Rule {
    Rule {
        id: "".into(),
        name: "Invoices to Archive".into(),
        enabled: true,
        triggers: vec![RuleTrigger::Arrival, RuleTrigger::Manual],
        scope: Vec::new(),
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: "old invoice".into(),
                    case_sensitive: false,
                },
            }],
        },
        actions: vec![
            RuleAction::Move {
                folder: FolderRef::Role {
                    role: FolderRole::Archive,
                    account_id: None,
                },
            },
            RuleAction::Flag { flagged: true },
        ],
        source: None,
    }
}

#[tokio::test]
async fn arrival_rules_move_new_mail_and_runs_can_be_undone() {
    let f = setup().await;
    synced(&f).await;
    let mut events = f.service.subscribe();
    let Response::RuleSaved { rule } = request(
        &f.service,
        Request::SaveRule {
            rule: Box::new(invoice_rule()),
            position: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(rule.id.as_str(), "invoices-to-archive");
    let text = std::fs::read_to_string(f.config_dir.path().join("rules.kdl")).unwrap();
    assert!(text.contains("rule \"Invoices to Archive\""), "{text}");
    let Response::Rules { rules } = request(&f.service, Request::ListRules).await else {
        panic!()
    };
    assert_eq!(rules.len(), 1);

    // A dry run explains its matches and changes nothing.
    control(
        &f,
        "imap-deliver",
        json!({"raw": "From: Shop <shop@example.org>\r\nSubject: Old invoice 1\r\nMessage-ID: <old1@example.org>\r\n\r\nPay.\r\n"}),
    );
    eventually("new mail arrived", || async {
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Archive)
            .await
            .iter()
            .any(|s| s == "Old invoice 1")
    })
    .await;
    let mut archived = false;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !archived {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("a RuleRunFinished event")
            .unwrap();
        if let Event::RuleRunFinished { run } = event {
            assert_eq!(run.rule_id.as_str(), "invoices-to-archive");
            assert_eq!(run.state, RuleRunState::Done);
            assert_eq!(run.matched, 1);
            assert!(run.undoable);
            archived = true;
        }
    }
    eventually("moved on the server", || async {
        server_subjects(&f, "Archive").contains(&"Old invoice 1".to_owned())
    })
    .await;
    let moved = find(&f.service, FolderRole::Archive, "Old invoice 1").await;
    assert!(moved.flags.flagged);

    // Undo puts it back as one unit.
    let Response::RuleRuns { runs } = request(
        &f.service,
        Request::ListRuleRuns {
            rule_id: None,
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(runs.len(), 1, "{runs:#?}");
    let Response::Queued { op_ids } = request(
        &f.service,
        Request::UndoRuleRun {
            run_id: runs[0].id.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(op_ids.len(), 2);
    eventually("back in the inbox, unflagged", || async {
        let folder = folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await;
        messages(&f.service, &folder.id)
            .await
            .iter()
            .any(|m| m.subject == "Old invoice 1" && !m.flags.flagged)
    })
    .await;
    eventually("back in the server's inbox", || async {
        server_subjects(&f, "INBOX").contains(&"Old invoice 1".to_owned())
    })
    .await;
    assert!(matches!(
        request(
            &f.service,
            Request::UndoRuleRun {
                run_id: runs[0].id.clone(),
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    // The dry run over the inbox now finds it again, with the reason.
    let Response::RuleTested { scanned, matches } = request(
        &f.service,
        Request::TestRule {
            rule: Box::new(invoice_rule()),
            folder_id: None,
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    assert!(scanned >= 15);
    assert_eq!(matches.len(), 1, "{matches:#?}");
    assert_eq!(matches[0].message.subject, "Old invoice 1");
    assert!(
        matches[0]
            .explanation
            .iter()
            .any(|h| h.matched && h.path == [0] && h.value.as_deref() == Some("Old invoice 1"))
    );
    assert!(
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
            .await
            .contains(&"Old invoice 1".to_owned()),
        "a dry run changes nothing"
    );

    // A manual run moves it again.
    let Response::RuleRan { run } = request(
        &f.service,
        Request::RunRule {
            rule_id: rule.id.clone(),
            message_ids: Vec::new(),
            folder_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(run.matched, 1);
    eventually("moved by the manual run", || async {
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Archive)
            .await
            .contains(&"Old invoice 1".to_owned())
    })
    .await;

    // Text form round trip, and positioned errors.
    let Response::RuleText { text } = request(
        &f.service,
        Request::RuleToText {
            rule: Box::new(rule.as_ref().clone()),
        },
    )
    .await
    else {
        panic!()
    };
    let Response::RuleParsed {
        rule: parsed,
        errors,
    } = request(&f.service, Request::RuleFromText { text }).await
    else {
        panic!()
    };
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(parsed.unwrap().name, "Invoices to Archive");
    let Response::RuleParsed { rule, errors } = request(
        &f.service,
        Request::RuleFromText {
            text: "rule \"x\" {\n    when {\n        subject regex=\"(\"\n    }\n}\n".into(),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(!errors.is_empty(), "{rule:?}");

    // Delete.
    assert!(matches!(
        request(&f.service, Request::DeleteRule { rule_id: rule_id() }).await,
        Response::Done
    ));
    assert!(matches!(
        request(&f.service, Request::DeleteRule { rule_id: rule_id() }).await,
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
    f.service.shutdown().await;
}

fn rule_id() -> nail_api::RuleId {
    "invoices-to-archive".into()
}

#[tokio::test]
async fn invalid_rules_are_refused_and_hand_edits_reported() {
    let f = setup().await;
    let mut bad = invoice_rule();
    bad.conditions = Condition::Subject {
        matches: TextMatch {
            op: TextOp::Regex,
            value: "(".into(),
            case_sensitive: false,
        },
    };
    assert!(matches!(
        request(
            &f.service,
            Request::SaveRule {
                rule: Box::new(bad),
                position: None,
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    // A hand-edited broken rules.kdl is reported with every error.
    let mut events = f.service.subscribe();
    std::fs::write(
        f.config_dir.path().join("rules.kdl"),
        "rule \"a\" trigger=\"never\" { }\nrule \"b\" { then { move } }\n",
    )
    .unwrap();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("a ConfigErrors event for rules.kdl")
            .unwrap();
        if let Event::ConfigErrors { file, errors } = event
            && file == "rules.kdl"
        {
            assert!(errors.len() >= 2, "{errors:?}");
            break;
        }
    }
    let on_connect = f.service.config_error_events();
    assert!(on_connect.iter().any(|e| matches!(
        e,
        Event::ConfigErrors { file, errors } if file == "rules.kdl" && errors.len() >= 2
    )));
    f.service.shutdown().await;
}

#[tokio::test]
async fn layout_write_back_merge_backups_and_every_error() {
    let f = setup().await;
    let mut events = f.service.subscribe();
    let patch = LayoutPatch::SplitSizes {
        node: LayoutNodeRef {
            window: "main".into(),
            breakpoint: 0,
            path: vec![2],
        },
        sizes: vec!["30%".into(), "1fr".into(), "1.4fr".into()],
    };
    let response = request(
        &f.service,
        Request::UpdateLayout {
            patches: vec![patch],
        },
    )
    .await;
    assert!(matches!(response, Response::Done), "{response:?}");
    let text = std::fs::read_to_string(f.config_dir.path().join("layout.kdl")).unwrap();
    assert!(text.contains("30%"), "{text}");
    let changed = tokio::time::timeout(TIMEOUT, async {
        loop {
            if let Ok(Event::ConfigChanged { .. }) = events.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(changed.is_ok(), "ConfigChanged after the write-back");

    // Merge a theme: preview, apply (with a backup), revert.
    let incoming = "theme \"Shared\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#ff0000\"\n    }\n}\n";
    let Response::ConfigMergePreview { preview } = request(
        &f.service,
        Request::PreviewConfigMerge {
            file: ConfigFile::Theme,
            incoming: incoming.into(),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(!preview.sections.is_empty(), "{preview:#?}");
    let Response::ConfigMergeApplied { backup } = request(
        &f.service,
        Request::ApplyConfigMerge {
            file: ConfigFile::Theme,
            incoming: incoming.into(),
            choices: Vec::new(),
            default: nail_api::customize::MergeTake::Theirs,
        },
    )
    .await
    else {
        panic!()
    };
    let theme = std::fs::read_to_string(f.config_dir.path().join("theme.kdl")).unwrap();
    assert!(theme.contains("#ff0000"), "{theme}");
    let Response::ConfigBackups { backups } = request(
        &f.service,
        Request::ListConfigBackups {
            file: ConfigFile::Theme,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(backups[0].id, backup.id);
    assert!(matches!(
        request(
            &f.service,
            Request::RevertConfig {
                file: ConfigFile::Theme,
                backup_id: None,
            },
        )
        .await,
        Response::Done
    ));
    let theme = std::fs::read_to_string(f.config_dir.path().join("theme.kdl")).unwrap_or_default();
    assert!(!theme.contains("#ff0000"), "{theme}");

    // Opening an editor is disabled for this (test) session.
    assert!(matches!(
        request(
            &f.service,
            Request::OpenConfigFile {
                file: ConfigFile::Layout,
                line: 1,
                column: 1,
            },
        )
        .await,
        Response::Error {
            code: ErrorCode::Unsupported,
            ..
        }
    ));

    // Every error of a broken accounts.kdl is reported at once.
    let mut events = f.service.subscribe();
    std::fs::write(
        f.config_dir.path().join("accounts.kdl"),
        "account \"a\" email=\"x\" { imap host=\"h\" }\naccount \"b\" email=\"y\" { imap host=\"h\" }\n",
    )
    .unwrap();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("a ConfigErrors event for accounts.kdl")
            .unwrap();
        if let Event::ConfigErrors { file, errors } = event
            && file == "accounts.kdl"
        {
            assert_eq!(errors.len(), 2, "{errors:?}");
            break;
        }
    }
    f.service.shutdown().await;
}

#[tokio::test]
async fn the_mcp_server_runs_with_the_core_as_data_provider() {
    let f = setup().await;
    let Response::McpServer { info } = request(&f.service, Request::GetMcpServerInfo).await else {
        panic!()
    };
    assert!(!info.running);
    let endpoint = f.config_dir.path().join("mcp-endpoint.json");
    let running = f
        .service
        .start_mcp_server(0, Some(endpoint.clone()))
        .await
        .unwrap();
    assert!(running.url().starts_with("http://127.0.0.1:"));
    assert!(endpoint.exists());
    let Response::McpServer { info } = request(&f.service, Request::GetMcpServerInfo).await else {
        panic!()
    };
    assert!(info.running);
    assert_eq!(info.url.as_deref(), Some(running.url().as_str()));

    let Response::McpClientCreated { client, token } = request(
        &f.service,
        Request::CreateMcpClient {
            name: "Agent".into(),
            grant: nail_api::mcp::McpGrant::default(),
        },
    )
    .await
    else {
        panic!()
    };
    assert!(token.starts_with("nail_mcp_"));
    let Response::McpClients { clients } = request(&f.service, Request::ListMcpClients).await
    else {
        panic!()
    };
    assert_eq!(clients[0].id, client.id);

    running.shutdown().await;
    let Response::McpServer { info } = request(&f.service, Request::GetMcpServerInfo).await else {
        panic!()
    };
    assert!(!info.running);
    let _ = MULTI_HOME_ID;
    f.service.shutdown().await;
}

fn subject_rule(name: &str, trigger: RuleTrigger, subject: &str, action: RuleAction) -> Rule {
    Rule {
        id: "".into(),
        name: name.into(),
        enabled: true,
        triggers: vec![trigger],
        scope: Vec::new(),
        conditions: Condition::All {
            conditions: vec![Condition::Subject {
                matches: TextMatch {
                    op: TextOp::Contains,
                    value: subject.into(),
                    case_sensitive: false,
                },
            }],
        },
        actions: vec![action],
        source: None,
    }
}

#[tokio::test]
async fn on_send_and_scheduled_rules_run() {
    let f = setup().await;
    synced(&f).await;
    for rule in [
        subject_rule(
            "Tag what I send",
            RuleTrigger::OnSend,
            "tag me",
            RuleAction::Tag {
                tag: "sent-by-rule".into(),
            },
        ),
        subject_rule(
            "Flag welcomes",
            RuleTrigger::Schedule {
                interval_minutes: 60,
            },
            "Welcome to Nail",
            RuleAction::Flag { flagged: true },
        ),
    ] {
        assert!(matches!(
            request(
                &f.service,
                Request::SaveRule {
                    rule: Box::new(rule),
                    position: None,
                },
            )
            .await,
            Response::RuleSaved { .. }
        ));
    }
    // The scheduled rule runs on the next tick.
    eventually("the scheduled rule flagged the welcome", || async {
        find(&f.service, FolderRole::Inbox, "Welcome to Nail")
            .await
            .flags
            .flagged
    })
    .await;
    // The on-send rule tags the Sent copy.
    send_now(&f, blank_draft("Please tag me")).await;
    eventually("the Sent copy tagged", || async {
        let Some(sent) = try_folder(&f.service, MULTI_WORK_ID, FolderRole::Sent).await else {
            return false;
        };
        messages(&f.service, &sent.id)
            .await
            .iter()
            .any(|m| m.subject == "Please tag me" && m.tags.iter().any(|t| t == "sent-by-rule"))
    })
    .await;
    let Response::RuleRuns { runs } = request(
        &f.service,
        Request::ListRuleRuns {
            rule_id: None,
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    let causes: Vec<_> = runs.iter().map(|r| r.cause).collect();
    assert!(
        causes.contains(&nail_api::rules::RuleRunCause::OnSend),
        "{causes:?}"
    );
    assert!(
        causes.contains(&nail_api::rules::RuleRunCause::Schedule),
        "{causes:?}"
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn rules_forward_redirect_reply_and_copy() {
    let f = setup().await;
    synced(&f).await;
    let rule = Rule {
        actions: vec![
            RuleAction::Copy {
                folder: FolderRef::Role {
                    role: FolderRole::Archive,
                    account_id: None,
                },
            },
            RuleAction::Forward {
                to: vec!["boss@example.org".into()],
                identity_id: None,
            },
            RuleAction::Redirect {
                to: vec!["desk@example.org".into()],
            },
            RuleAction::Reply {
                template: "Hello {sender.name}, thanks for “{subject}”.".into(),
                identity_id: None,
            },
        ],
        triggers: vec![RuleTrigger::Manual],
        ..subject_rule(
            "Everything",
            RuleTrigger::Manual,
            "Welcome to Nail",
            RuleAction::Stop,
        )
    };
    let Response::RuleSaved { rule } = request(
        &f.service,
        Request::SaveRule {
            rule: Box::new(rule),
            position: None,
        },
    )
    .await
    else {
        panic!()
    };
    let Response::RuleRan { run } = request(
        &f.service,
        Request::RunRule {
            rule_id: rule.id.clone(),
            message_ids: Vec::new(),
            folder_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(run.matched, 1);
    assert_eq!(run.state, RuleRunState::Done, "{run:?}");
    // Nothing was queued that could be undone.
    assert!(!run.undoable);

    let forwarded = sent_with(&f, "Fwd: Welcome to Nail").await;
    assert!(forwarded.contains("boss@example.org"), "{forwarded}");
    // Rule mail is marked as automatic (RFC 3834) and with the loop marker.
    assert!(
        forwarded.contains("Auto-Submitted: auto-generated"),
        "{forwarded}"
    );
    assert!(forwarded.contains("X-Nail-Loop: "), "{forwarded}");
    let replied = sent_with(&f, "Re: Welcome to Nail").await;
    assert!(
        replied.contains("Hello Ada Lovelace, thanks for"),
        "{replied}"
    );
    assert!(
        replied.contains("Auto-Submitted: auto-replied"),
        "{replied}"
    );
    eventually("the redirect", || async {
        control(&f, "smtp-messages", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .any(|m| {
                m["to"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t == "desk@example.org")
                    && m["data"]
                        .as_str()
                        .unwrap()
                        .contains("Resent-To: desk@example.org")
                    && m["data"].as_str().unwrap().contains("X-Nail-Loop: ")
            })
    })
    .await;
    // The redirect went through the outbox: it has a Sent copy (the
    // original subject; the forward and reply have Fwd: / Re:).
    eventually("the redirect's Sent copy", || async {
        server_subjects(&f, "Sent").contains(&"Welcome to Nail".to_owned())
    })
    .await;
    eventually("the copy in Archive", || async {
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Archive)
            .await
            .contains(&"Welcome to Nail".to_owned())
    })
    .await;
    // The original stays where it was.
    assert!(
        subjects(&f.service, MULTI_WORK_ID, FolderRole::Inbox)
            .await
            .contains(&"Welcome to Nail".to_owned())
    );
    f.service.shutdown().await;
}

async fn save_rule(f: &Fixture, rule: Rule) -> Rule {
    match request(
        &f.service,
        Request::SaveRule {
            rule: Box::new(rule),
            position: None,
        },
    )
    .await
    {
        Response::RuleSaved { rule } => *rule,
        other => panic!("unexpected {other:?}"),
    }
}

fn smtp_subjects(f: &Fixture) -> Vec<String> {
    smtp_data(f)
        .iter()
        .filter_map(|d| {
            d.lines()
                .find_map(|l| l.strip_prefix("Subject: ").map(str::to_owned))
        })
        .collect()
}

#[tokio::test]
async fn on_send_forwards_do_not_forward_their_own_forwards() {
    let f = setup().await;
    synced(&f).await;
    let rule = Rule {
        conditions: Condition::All {
            conditions: Vec::new(),
        },
        ..subject_rule(
            "Archive what I send",
            RuleTrigger::OnSend,
            "",
            RuleAction::Forward {
                to: vec!["archive@example.org".into()],
                identity_id: None,
            },
        )
    };
    save_rule(&f, rule).await;
    send_now(&f, blank_draft("Loop check")).await;
    sent_with(&f, "Fwd: Loop check").await;
    // The forward's own Sent copy is filed without running on-send rules.
    eventually("the forward's Sent copy", || async {
        server_subjects(&f, "Sent").contains(&"Fwd: Loop check".to_owned())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let subjects = smtp_subjects(&f);
    assert_eq!(
        subjects.iter().filter(|s| s.contains("Loop check")).count(),
        2,
        "{subjects:?}"
    );
    assert!(
        !subjects.iter().any(|s| s.contains("Fwd: Fwd:")),
        "{subjects:?}"
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn arrival_rules_leave_rule_mail_alone_and_redirects_survive_a_delete() {
    let f = setup().await;
    synced(&f).await;
    let rule = Rule {
        actions: vec![
            RuleAction::Redirect {
                to: vec!["desk@example.org".into()],
            },
            RuleAction::Reply {
                template: "Away. About **{subject}**: see [help](https://help.example).".into(),
                identity_id: None,
            },
            RuleAction::Delete { permanent: true },
        ],
        ..subject_rule(
            "Redirect everything",
            RuleTrigger::Arrival,
            "incoming",
            RuleAction::Stop,
        )
    };
    save_rule(&f, rule).await;
    // Mail a rule sent (here: another Nail's redirect) is left alone …
    control(
        &f,
        "imap-deliver",
        json!({"raw": "From: Bob <bob@example.org>\r\nSubject: incoming loop\r\nX-Nail-Loop: bob@example.org\r\nMessage-ID: <loop1@example.org>\r\n\r\nHi.\r\n"}),
    );
    // … ordinary mail is redirected (through the outbox, after the
    // message was already deleted by the next action) and answered.
    control(
        &f,
        "imap-deliver",
        json!({"raw": "From: Bob <bob@example.org>\r\nSubject: incoming plain [Verify](https://evil.example) <img src=https://evil.example/t.png>\r\nMessage-ID: <plain1@example.org>\r\n\r\nHi.\r\n"}),
    );
    eventually("the plain message redirected", || async {
        control(&f, "smtp-messages", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .any(|m| {
                let data = m["data"].as_str().unwrap();
                data.contains("Resent-To: desk@example.org")
                    && data.contains("Subject: incoming plain")
            })
    })
    .await;
    // The sender's subject is text in the reply, never a link or image.
    eventually("the reply", || async {
        smtp_data(&f)
            .iter()
            .any(|d| d.contains("Auto-Submitted: auto-replied"))
    })
    .await;
    let reply = smtp_data(&f)
        .into_iter()
        .find(|d| d.contains("Auto-Submitted: auto-replied"))
        .unwrap();
    let html = reply[reply.find("Content-Type: text/html").unwrap()..]
        .replace("=\r\n", "")
        .replace("=\n", "")
        .replace("=3D", "=");
    assert!(html.contains("href=\"https://help.example\""), "{reply}");
    assert!(!html.contains("href=\"https://evil.example\""), "{reply}");
    assert!(!html.contains("<img"), "{reply}");
    eventually("both processed and deleted", || async {
        let inbox = subjects(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await;
        !inbox.iter().any(|s| s.starts_with("incoming"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let subjects = smtp_subjects(&f);
    assert!(
        !subjects.iter().any(|s| s.contains("incoming loop")),
        "{subjects:?}"
    );
    f.service.shutdown().await;
}

#[tokio::test]
async fn autosaves_racing_uploads_leave_no_stale_server_drafts() {
    let f = setup().await;
    synced(&f).await;
    for (round, finish) in [("sent", true), ("deleted", false)] {
        let mut draft = blank_draft(&format!("Racing {round} 0"));
        let id = save(&f.service, draft.clone()).await;
        draft.id = Some(id.clone());
        // Saves around the upload delay (300 ms in tests), so later saves
        // land while earlier uploads run.
        for n in 1..8 {
            tokio::time::sleep(Duration::from_millis(40 * n)).await;
            draft.subject = format!("Racing {round} {n}");
            save(&f.service, draft.clone()).await;
        }
        tokio::time::sleep(Duration::from_millis(320)).await;
        if finish {
            assert!(matches!(
                request(
                    &f.service,
                    Request::SendDraft {
                        draft_id: id,
                        delay_secs: Some(0),
                        send_at: None,
                        crypto: None,
                    },
                )
                .await,
                Response::SendQueued { .. }
            ));
            sent_with(&f, &format!("Racing {round} 7")).await;
        } else {
            assert!(matches!(
                request(&f.service, Request::DeleteDraft { draft_id: id }).await,
                Response::Done
            ));
        }
        eventually("no copy left on the server", || async {
            server_subjects(&f, "Drafts").is_empty()
                && subjects(&f.service, MULTI_WORK_ID, FolderRole::Drafts)
                    .await
                    .is_empty()
        })
        .await;
        // … and none appears later.
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(server_subjects(&f, "Drafts").is_empty(), "{round}");
        assert!(
            subjects(&f.service, MULTI_WORK_ID, FolderRole::Drafts)
                .await
                .is_empty(),
            "{round}"
        );
    }
    f.service.shutdown().await;
}

#[tokio::test]
async fn undo_restores_what_it_can_when_a_message_was_moved_back_by_hand() {
    let f = setup().await;
    synced(&f).await;
    let rule = save_rule(
        &f,
        Rule {
            triggers: vec![RuleTrigger::Manual],
            ..invoice_rule()
        },
    )
    .await;
    for n in ["A", "B"] {
        control(
            &f,
            "imap-deliver",
            json!({"raw": format!("From: Shop <shop@example.org>\r\nSubject: Old invoice {n}\r\nMessage-ID: <inv{n}@example.org>\r\n\r\nPay.\r\n")}),
        );
    }
    eventually("both invoices arrived", || async {
        let inbox = subjects(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await;
        inbox
            .iter()
            .filter(|s| s.starts_with("Old invoice"))
            .count()
            == 2
    })
    .await;
    let Response::RuleRan { run } = request(
        &f.service,
        Request::RunRule {
            rule_id: rule.id.clone(),
            message_ids: Vec::new(),
            folder_id: None,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(run.matched, 2);
    assert!(run.undoable);
    // The user moves A back by hand.
    let a = find(&f.service, FolderRole::Archive, "Old invoice A").await;
    assert!(matches!(
        request(
            &f.service,
            Request::MoveMessages {
                message_ids: vec![a.id.clone()],
                target: nail_api::MoveTarget::Role(FolderRole::Inbox),
            },
        )
        .await,
        Response::Queued { .. }
    ));
    // Undo still restores B (and unflags both); A's move back is skipped.
    let Response::Queued { op_ids } = request(
        &f.service,
        Request::UndoRuleRun {
            run_id: run.id.clone(),
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(op_ids.len(), 3, "{op_ids:?}");
    eventually("both back in the inbox, unflagged", || async {
        let folder = folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await;
        messages(&f.service, &folder.id)
            .await
            .iter()
            .filter(|m| m.subject.starts_with("Old invoice") && !m.flags.flagged)
            .count()
            == 2
    })
    .await;
    let Response::RuleRuns { runs } = request(
        &f.service,
        Request::ListRuleRuns {
            rule_id: None,
            limit: 10,
        },
    )
    .await
    else {
        panic!()
    };
    assert_eq!(runs[0].state, RuleRunState::Undone);
    assert!(!runs[0].undoable);
    f.service.shutdown().await;
}

/// Records `notifications/resources/updated`.
#[derive(Clone)]
struct UpdateRecorder(tokio::sync::mpsc::UnboundedSender<String>);

impl rmcp::ClientHandler for UpdateRecorder {
    async fn on_resource_updated(
        &self,
        params: rmcp::model::ResourceUpdatedNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let _ = self.0.send(params.uri);
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        rmcp::model::ClientConfig::default()
    }
}

async fn wait_for_update(rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>, uri: &str) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let got = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("no update of {uri}"))
            .expect("the client is connected");
        if got == uri {
            return;
        }
    }
}

#[tokio::test]
#[allow(deprecated)]
async fn the_core_feeds_new_mail_and_ui_reports_to_mcp_subscriptions() {
    use rmcp::ServiceExt as _;

    let f = setup().await;
    synced(&f).await;
    let running = f.service.start_mcp_server(0, None).await.unwrap();
    let Response::McpClientCreated { token, .. } = request(
        &f.service,
        Request::CreateMcpClient {
            name: "Watcher".into(),
            grant: nail_api::mcp::McpGrant {
                scopes: vec![
                    nail_api::mcp::McpScope::Read,
                    nail_api::mcp::McpScope::Context,
                ],
                accounts: vec![AccountId::new(MULTI_WORK_ID)],
                ..nail_api::mcp::McpGrant::default()
            },
        },
    )
    .await
    else {
        panic!()
    };
    let (tx, mut updates) = tokio::sync::mpsc::unbounded_channel();
    let transport = rmcp::transport::StreamableHttpClientTransport::with_client(
        nail_mcp::bridge::loopback_http_client().unwrap(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            running.url(),
        )
        .auth_header(token),
    );
    let client = UpdateRecorder(tx).serve(transport).await.unwrap();
    let inbox = folder(&f.service, MULTI_WORK_ID, FolderRole::Inbox).await;
    let messages_uri = nail_mcp::uri::folder_messages(&inbox.id);
    for uri in [messages_uri.as_str(), "nail://ui/context"] {
        client
            .subscribe(rmcp::model::SubscribeRequestParams::new(uri))
            .await
            .unwrap_or_else(|e| panic!("{uri}: {e}"));
    }

    // New mail synced by the core reaches the folder's subscribers.
    control(
        &f,
        "imap-deliver",
        json!({"raw": "From: Bob <bob@example.org>\r\nSubject: For the agent\r\nMessage-ID: <agent1@example.org>\r\n\r\nHi.\r\n"}),
    );
    wait_for_update(&mut updates, &messages_uri).await;

    // A UI context report wakes the context subscribers.
    assert!(matches!(
        request(
            &f.service,
            Request::ReportUiContext {
                context: nail_api::UiContext {
                    folder_id: Some(inbox.id.clone()),
                    ..nail_api::UiContext::default()
                },
            },
        )
        .await,
        Response::Done
    ));
    wait_for_update(&mut updates, "nail://ui/context").await;

    let _ = client.cancel().await;
    running.shutdown().await;
    f.service.shutdown().await;
}

/// Recipients of sent mail that are in no address book land in the
/// Collected book: contact search (the in-memory PIM index, also behind
/// MCP) finds them at once, and open contact views hear ContactsChanged.
#[tokio::test]
async fn sent_mail_fills_the_collected_book_and_contact_search() {
    let f = setup().await;
    synced(&f).await;
    let search = |query: &'static str| {
        let service = f.service.clone();
        async move {
            match request(
                &service,
                Request::SearchContacts {
                    query: query.into(),
                    address_book_ids: None,
                    limit: 10,
                },
            )
            .await
            {
                Response::ContactMatches { matches } => matches,
                other => panic!("unexpected {other:?}"),
            }
        }
    };
    // The index is built before the mail goes out.
    assert!(search("zephyrine").await.is_empty());
    let mut events = f.service.subscribe();
    let mut draft = blank_draft("Collected recipients");
    draft.to = vec![Address {
        name: Some("Zephyrine Quandt".into()),
        email: "zephyrine@example.net".into(),
    }];
    send_now(&f, draft).await;
    sent_with(&f, "Collected recipients").await;
    eventually("the recipient is found", || async {
        search("zephyrine")
            .await
            .iter()
            .any(|m| m.contact.display_name == "Zephyrine Quandt")
    })
    .await;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut announced = false;
    while !announced && tokio::time::Instant::now() < deadline {
        if let Ok(Ok(Event::ContactsChanged { address_book_ids })) =
            tokio::time::timeout(Duration::from_secs(1), events.recv()).await
        {
            announced = address_book_ids
                .iter()
                .any(|b| b.as_str() == nail_store::pim::COLLECTED_BOOK_ID);
        }
    }
    assert!(announced, "ContactsChanged for the Collected book");
}
