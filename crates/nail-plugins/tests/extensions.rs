//! M4a extensions through a running host: composer actions and composer
//! edits, attachment previews (trees or images, never HTML / SVG) and
//! actions, link actions, Markdown blocks, MCP tools, `nail.l10n`, binary
//! fetch bodies and panel actions checked against the rendered tree.

mod common;

use std::collections::BTreeMap;

use common::*;
use nail_api::agents::McpClientRef;
use nail_api::plugins::extensions::{ComposerEditOp, ComposerTextFormat};
use nail_api::plugins::{
    ComposerSelection, PluginActionTarget, PluginPreview, PluginRenderPurpose, PluginUiNode,
    PluginUiTarget,
};
use nail_api::{ErrorCode, Event, McpScope, UiContext};
use nail_plugins::{ClientLimit, PackageFiles, PluginSpec};

/// An MCP client that may reach everything.
fn full() -> ClientLimit {
    ClientLimit {
        scopes: McpScope::ALL.to_vec(),
        accounts: None,
        excluded_folders: Vec::new(),
    }
}
use serde_json::{Value, json};

fn add_files(mut spec: PluginSpec, files: &[(&str, &[u8])]) -> PluginSpec {
    let PackageFiles::Memory(map) = &spec.files else {
        panic!()
    };
    let mut map = (**map).clone();
    for (path, bytes) in files {
        map.insert((*path).to_owned(), bytes.to_vec());
    }
    spec.files = PackageFiles::from_map(map);
    spec
}

fn composer_edits(fake: &Fake) -> Vec<Event> {
    fake.events()
        .into_iter()
        .filter(|e| matches!(e, Event::ComposerEditRequested { .. }))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn composer_actions_edit_their_draft_live() {
    let fake = Fake::new();
    fake.drafts
        .lock()
        .unwrap()
        .insert("d1".into(), "markdown".into());
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.composer.registerAction("shout", async ({ action, composer }) => {
          nail.log.info("CTX " + JSON.stringify({ action, composer }));
          await nail.composer.replaceSelection(composer.selection.text.toUpperCase(), { format: "markdown" });
          await nail.composer.insert("!");
          try { await nail.composer.replaceAll("x", { draftId: "d2" }); }
          catch (e) { nail.log.info("OTHER " + e.code); }
        });
      } };
    "#;
    host.start(spec_with(
        "shout",
        &["composer"],
        json!({ "composerActions": [{ "id": "shout", "title": "Shout" }] }),
        js,
    ))
    .await
    .unwrap();
    let target = PluginActionTarget::Composer {
        draft_id: "d1".into(),
        selection: ComposerSelection {
            text: "hello".into(),
            start: 0,
            end: 5,
        },
    };
    host.run_action("shout.shout", &target, Some("ui-1".into()))
        .await
        .unwrap();
    let ctx = json_result(&fake, "shout", "CTX ").await;
    assert_eq!(
        ctx,
        json!({"action": "shout", "composer": {"draftId": "d1", "mode": "markdown",
               "selection": {"text": "hello", "start": 0, "end": 5}}})
    );
    assert_eq!(result(&fake, "shout", "OTHER ").await, "not_found");
    let edits = composer_edits(&fake);
    assert_eq!(edits.len(), 2);
    let Event::ComposerEditRequested {
        draft_id,
        client_id,
        by,
        edit,
    } = &edits[0]
    else {
        panic!()
    };
    assert_eq!(draft_id.as_str(), "d1");
    assert_eq!(client_id.as_deref(), Some("ui-1"));
    assert_eq!(by.name, "Test shout");
    assert_eq!(edit.op, ComposerEditOp::ReplaceSelection);
    assert_eq!(edit.text, "HELLO");
    assert_eq!(edit.format, ComposerTextFormat::Markdown);

    // Unknown drafts, actions and plug-ins.
    let missing = PluginActionTarget::Composer {
        draft_id: "nope".into(),
        selection: ComposerSelection::default(),
    };
    for (action, target) in [
        ("shout.shout", &missing),
        ("shout.other", &target),
        ("ghost.shout", &target),
    ] {
        let err = host.run_action(action, target, None).await.unwrap_err();
        assert!(matches!(err.code, ErrorCode::NotFound), "{action}: {err:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn composer_edits_need_a_composer_action_and_the_permission() {
    let fake = Fake::new();
    fake.drafts
        .lock()
        .unwrap()
        .insert("d1".into(), "plain".into());
    let host = host(&fake);
    // Edits outside a composer action: nothing to edit.
    host.start(spec_with(
        "eager",
        &["composer"],
        json!({ "composerActions": [{ "id": "go", "title": "Go" }] }),
        r#"export default { async activate() {
             try { await nail.composer.insert("x"); } catch (e) { nail.log.info("E " + e.code); }
           } };"#,
    ))
    .await
    .unwrap();
    assert_eq!(result(&fake, "eager", "E ").await, "not_found");
    // Declared but not granted: the action is refused.
    let mut s = spec_with(
        "ungranted",
        &["composer"],
        json!({ "composerActions": [{ "id": "go", "title": "Go" }] }),
        r#"export default { activate(ctx) { ctx.composer.registerAction("go", () => {}); } };"#,
    );
    s.grants = Vec::new();
    host.start(s).await.unwrap();
    let target = PluginActionTarget::Composer {
        draft_id: "d1".into(),
        selection: ComposerSelection::default(),
    };
    let err = host
        .run_action("ungranted.go", &target, None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(composer_edits(&fake).is_empty());
}

const PREVIEWS: &str = r#"
  export default { activate(ctx) {
    ctx.attachments.registerPreview("csv", (att, bytes) => {
      const text = String.fromCharCode(...bytes);
      const rows = text.trim().split("\n").map((l) => l.split(","));
      return { kind: "tree", tree: [
        { type: "text", value: att.name + " " + att.size },
        { type: "table", columns: rows[0].map((c) => ({ label: c })), rows: rows.slice(1) },
      ] };
    });
    ctx.attachments.registerPreview("png", (att, bytes) =>
      ({ kind: "image", mimeType: "image/png", data: bytes, alt: att.name }));
    ctx.attachments.registerPreview("svg", () =>
      ({ kind: "image", mimeType: "image/svg+xml", data: "PHN2Zz48L3N2Zz4=" }));
    ctx.attachments.registerPreview("html", () => ({ kind: "html", html: "<script>x</script>" }));
  } };
"#;

fn preview_contributions() -> Value {
    json!({ "attachmentPreviews": [
        { "id": "csv", "title": "Table", "mimeTypes": ["text/csv"], "extensions": ["csv"], "maxBytes": 64 },
        { "id": "png", "title": "Image", "mimeTypes": ["image/png"] },
        { "id": "svg", "title": "SVG", "extensions": ["svg"] },
        { "id": "html", "title": "HTML", "extensions": ["htm"] }
    ] })
}

#[tokio::test(flavor = "multi_thread")]
async fn attachment_previews_render_trees_and_images_only() {
    let fake = Fake::new();
    fake.add_attachment("m1", 0, "data.csv", "text/csv", b"a,b\n1,2\n3,4\n");
    fake.add_attachment(
        "m1",
        1,
        "pic.png",
        "image/png",
        &[0x89, b'P', b'N', b'G', 1, 2],
    );
    fake.add_attachment("m1", 2, "x.svg", "image/svg+xml", b"<svg/>");
    fake.add_attachment("m1", 3, "x.htm", "text/html", b"<p>");
    fake.add_attachment("m1", 4, "big.csv", "text/csv", &[b'a'; 100]);
    fake.add_attachment("m2", 0, "home.csv", "text/csv", b"a\n1\n");
    fake.add_attachment("m3", 0, "secret.csv", "text/csv", b"a\n1\n");
    let host = host(&fake);
    let mut s = spec_with(
        "viewer",
        &["attachments"],
        preview_contributions(),
        PREVIEWS,
    );
    s.grants = vec![grant("attachments", Some(&["work"]))];
    host.start(s).await.unwrap();

    let tree = host
        .render_attachment_preview(&"m1".into(), 0, "viewer.csv", None)
        .await
        .unwrap();
    let PluginPreview::Tree { tree } = tree else {
        panic!()
    };
    assert_eq!(
        tree[0],
        PluginUiNode::Text {
            value: "data.csv 12".into(),
            style: Default::default()
        }
    );
    let PluginUiNode::Table { rows, .. } = &tree[1] else {
        panic!("{tree:?}")
    };
    assert_eq!(rows.len(), 2);

    let image = host
        .render_attachment_preview(&"m1".into(), 1, "viewer.png", None)
        .await
        .unwrap();
    let PluginPreview::Image {
        mime_type,
        data,
        alt,
    } = image
    else {
        panic!()
    };
    assert_eq!(mime_type, "image/png");
    assert_eq!(data, "iVBORwEC");
    assert_eq!(alt.as_deref(), Some("pic.png"));

    for (message, index, renderer, code) in [
        // SVG and HTML answers are refused.
        ("m1", 2, "viewer.svg", ErrorCode::InvalidRequest),
        ("m1", 3, "viewer.html", ErrorCode::InvalidRequest),
        // A type the renderer does not render.
        ("m1", 1, "viewer.csv", ErrorCode::InvalidRequest),
        // Over the renderer's size.
        ("m1", 4, "viewer.csv", ErrorCode::TooLarge),
        // Outside the grant's accounts, unknown, encrypted.
        ("m2", 0, "viewer.csv", ErrorCode::NotFound),
        ("m1", 9, "viewer.csv", ErrorCode::NotFound),
        ("m3", 0, "viewer.csv", ErrorCode::Forbidden),
        ("m1", 0, "viewer.unknown", ErrorCode::NotFound),
    ] {
        let err = host
            .render_attachment_preview(&message.into(), index, renderer, None)
            .await
            .unwrap_err();
        assert_eq!(err.code, code, "{message} {index} {renderer}: {err:?}");
    }
    assert!(
        !fake
            .localized("viewer", "plugin-host-handler-failed")
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn attachment_and_link_actions_get_what_the_user_picked() {
    let fake = Fake::new();
    fake.add_attachment("m1", 0, "report.pdf", "application/pdf", b"%PDF-1");
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.attachments.registerAction("save", (c, bytes) =>
          nail.log.info("SAVE " + c.attachment.name + " " + bytes.length + " " + bytes[0]));
        ctx.links.registerAction("open-ticket", (c) => nail.log.info("LINK " + JSON.stringify(c.link)));
      } };
    "#;
    let mut s = spec_with(
        "acts",
        &["attachments"],
        json!({
            "attachmentActions": [{ "id": "save", "title": "Save to CRM", "mimeTypes": ["application/pdf"] }],
            "linkActions": [{ "id": "open-ticket", "title": "Open ticket", "hosts": ["*.atlassian.net"] }]
        }),
        js,
    );
    // No mail.read: link actions get no message id.
    s.grants = vec![grant("attachments", None)];
    host.start(s).await.unwrap();
    host.run_action(
        "acts.save",
        &PluginActionTarget::Attachment {
            message_id: "m1".into(),
            index: 0,
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(result(&fake, "acts", "SAVE ").await, "report.pdf 6 37");
    host.run_action(
        "acts.open-ticket",
        &PluginActionTarget::Link {
            url: "https://acme.atlassian.net/browse/X-1".into(),
            text: Some("X-1".into()),
            message_id: Some("m1".into()),
        },
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        json_result(&fake, "acts", "LINK ").await,
        json!({"url": "https://acme.atlassian.net/browse/X-1", "text": "X-1"})
    );
    for target in [
        PluginActionTarget::Link {
            url: "https://example.com/".into(),
            text: None,
            message_id: None,
        },
        PluginActionTarget::Link {
            url: "javascript:alert(1)".into(),
            text: None,
            message_id: None,
        },
    ] {
        let err = host
            .run_action("acts.open-ticket", &target, None)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn markdown_blocks_render_sanitized_html() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.composer.registerBlock("ticket", ({ block, source, purpose }) => ({
          html: "<p class=\"t\">Ticket " + source.trim() + "</p><img src=\"https://track.example/x.gif\"><script>alert(1)</script>",
          text: "Ticket " + source.trim() + " (" + purpose + ")",
        }));
        ctx.composer.registerBlock("huge", () => ({ html: "x".repeat(300 * 1024), text: "" }));
      } };
    "#;
    host.start(spec_with(
        "crm",
        &["composer"],
        json!({ "markdownBlocks": [{ "id": "ticket", "title": "Ticket" }, { "id": "huge", "title": "Huge" }] }),
        js,
    ))
    .await
    .unwrap();
    let block = host
        .render_markdown_block("crm.ticket", "4711\n", PluginRenderPurpose::Send)
        .await
        .unwrap();
    assert_eq!(block.text, "Ticket 4711 (send)");
    assert!(block.html.contains("Ticket 4711"), "{}", block.html);
    assert!(!block.html.contains("script") && !block.html.contains("track.example"));
    let err = host
        .render_markdown_block("crm.huge", "", PluginRenderPurpose::Preview)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    let err = host
        .render_markdown_block("crm.nope", "", PluginRenderPurpose::Preview)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_tools_are_exposed_and_act_on_behalf_of_the_client() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.mcp.registerTool("find-customer", async (args) => {
          const task = await nail.tasks.create({ title: "Call " + args.email });
          return { customer: args.email, task: task.status };
        });
        ctx.mcp.registerTool("fails", () => { throw new Error("no CRM"); });
      } };
    "#;
    let tools = json!({ "mcpTools": [
        { "id": "find-customer", "description": "Find a customer by address",
          "inputSchema": { "type": "object", "properties": { "email": { "type": "string" } },
                           "required": ["email"] } },
        { "id": "fails", "description": "Always fails", "scopes": ["read", "pim"] }
    ] });
    host.start(spec_with(
        "crm",
        &["mcp.expose", "tasks.write"],
        tools.clone(),
        js,
    ))
    .await
    .unwrap();
    // Not granted: not offered.
    let mut hidden = spec_with("hidden", &["mcp.expose"], tools, js);
    hidden.grants = Vec::new();
    host.start(hidden).await.unwrap();

    let listed = host.mcp_tools();
    let names: Vec<&str> = listed.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["crm__fails", "crm__find-customer"]);
    assert_eq!(listed[0].scopes, [McpScope::Read, McpScope::Pim]);
    assert_eq!(
        listed[0].input_schema,
        json!({"type": "object", "properties": {}})
    );
    assert_eq!(listed[1].plugin_name, "Test crm");

    let client = McpClientRef {
        client_id: "claude".into(),
        name: "Claude Desktop".into(),
    };
    let value = host
        .call_mcp_tool(
            "crm__find-customer",
            json!({"email": "ada@example.com"}),
            client.clone(),
            full(),
        )
        .await
        .unwrap();
    assert_eq!(
        value,
        json!({"customer": "ada@example.com", "task": "done"})
    );
    // The plug-in's write is attributed to it, on behalf of the client.
    let write = &fake.writes()[0];
    assert_eq!(write.attribution.on_behalf_of.as_ref(), Some(&client));

    let err = host
        .call_mcp_tool(
            "crm__find-customer",
            json!({"email": 7}),
            client.clone(),
            full(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    let err = host
        .call_mcp_tool("crm__fails", Value::Null, client.clone(), full())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    assert!(err.message.contains("no CRM"));
    for name in ["hidden__fails", "crm__unknown", "search_messages"] {
        let err = host
            .call_mcp_tool(name, json!({}), client.clone(), full())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "{name}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn l10n_uses_the_plugins_own_catalogs() {
    let fake = Fake::new();
    *fake.locale.lock().unwrap() = Some("de-AT".into());
    let host = host(&fake);
    let js = r#"
      export default { activate() {
        nail.log.info("L " + JSON.stringify([
          nail.l10n.locale,
          nail.l10n.t("greeting", { name: "Ada" }),
          nail.l10n.t("only-en"),
          nail.l10n.t("missing"),
          nail.l10n.t("count", { n: 3 }),
          nail.l10n.has("greeting"), nail.l10n.has("missing"),
        ]));
      } };
    "#;
    let mut s = spec_with(
        "words",
        &[],
        json!({ "locales": { "en": "locales/en.ftl", "de": "locales/de.ftl" } }),
        js,
    );
    s = add_files(
        s,
        &[
            (
                "locales/en.ftl",
                b"greeting = Hello { $name }\nonly-en = English\ncount = { $n } items\n",
            ),
            ("locales/de.ftl", b"greeting = Hallo { $name }\n"),
        ],
    );
    host.start(s).await.unwrap();
    assert_eq!(
        json_result(&fake, "words", "L ").await,
        json!([
            "de-AT",
            "Hallo Ada",
            "English",
            "missing",
            "3 items",
            true,
            false
        ])
    );
}

/// Post-M4a: an MCP tool's `%id%` description reaches MCP clients in the
/// core's language.
#[tokio::test(flavor = "multi_thread")]
async fn mcp_tool_descriptions_are_localized() {
    let fake = Fake::new();
    *fake.locale.lock().unwrap() = Some("de".into());
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.mcp.registerTool("lookup", async () => ({ ok: true }));
        ctx.mcp.registerTool("plain", async () => ({ ok: true }));
      } };
    "#;
    let mut s = spec_with(
        "crm",
        &["mcp.expose"],
        json!({
            "mcpTools": [
                { "id": "lookup", "description": "%lookup-description%" },
                { "id": "plain", "description": "Plain description" }
            ],
            "locales": { "en": "locales/en.ftl", "de": "locales/de.ftl" }
        }),
        js,
    );
    s = add_files(
        s,
        &[
            ("locales/en.ftl", b"lookup-description = Finds a customer\n"),
            (
                "locales/de.ftl",
                b"lookup-description = Sucht einen Kunden\n",
            ),
        ],
    );
    host.start(s).await.unwrap();
    let listed = host.mcp_tools();
    let described: Vec<(&str, &str)> = listed
        .iter()
        .map(|t| (t.name.as_str(), t.description.as_str()))
        .collect();
    assert_eq!(
        described,
        [
            ("crm__lookup", "Sucht einen Kunden"),
            ("crm__plain", "Plain description")
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_bodies_may_be_binary_both_ways() {
    let fake = Fake::new();
    *fake.fetch_body.lock().unwrap() = Some(vec![0, 159, 146, 150, 255]);
    let host = host(&fake);
    let js = r#"
      export default { async activate() {
        const r = await nail.http.fetch("https://api.example.com/up", {
          method: "POST", body: new Uint8Array([1, 2, 254]),
        });
        const bytes = await r.bytes();
        const buffer = await r.arrayBuffer();
        nail.log.info("B " + JSON.stringify([bytes instanceof Uint8Array, Array.from(bytes),
                                              buffer.byteLength, typeof r.body]));
        const r2 = await nail.http.fetch("https://api.example.com/view", {
          method: "PUT", body: new Uint16Array([258]).buffer,
        });
        nail.log.info("B2 " + r2.status);
      } };
    "#;
    host.start(spec("bin", &["network:api.example.com"], js))
        .await
        .unwrap();
    assert_eq!(
        json_result(&fake, "bin", "B ").await,
        json!([true, [0, 159, 146, 150, 255], 5, "string"])
    );
    assert_eq!(result(&fake, "bin", "B2 ").await, "200");
    let fetches = fake.fetches.lock().unwrap().clone();
    assert_eq!(fetches[0].body.as_deref(), Some(&[1u8, 2, 254][..]));
    assert_eq!(fetches[1].body.as_deref(), Some(&258u16.to_ne_bytes()[..]));
}

#[tokio::test(flavor = "multi_thread")]
async fn binary_bodies_are_limited() {
    let fake = Fake::new();
    *fake.fetch_body.lock().unwrap() = Some(vec![7; 2048]);
    let host = host_with(
        &fake,
        nail_plugins::HostLimits {
            http_max_request_bytes: 16,
            http_max_response_bytes: 1024,
            ..limits()
        },
    );
    let js = r#"
      export default { async activate() {
        try { await nail.http.fetch("https://api.example.com/", { method: "POST", body: new Uint8Array(17) }); }
        catch (e) { nail.log.info("UP " + e.code); }
        try { await nail.http.fetch("https://api.example.com/"); }
        catch (e) { nail.log.info("DOWN " + e.code); }
      } };
    "#;
    host.start(spec("limits", &["network:api.example.com"], js))
        .await
        .unwrap();
    assert_eq!(result(&fake, "limits", "UP ").await, "too_large");
    assert_eq!(result(&fake, "limits", "DOWN ").await, "too_large");
}

#[tokio::test(flavor = "multi_thread")]
async fn panel_actions_are_checked_against_the_rendered_tree() {
    let fake = Fake::new();
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.panels.register("side", {
          render: () => [
            { type: "input", id: "note", label: "Note" },
            { type: "button", id: "save", label: "Save" },
          ],
          onAction: async (a) => {
            nail.log.info("ACT " + JSON.stringify(a));
            await nail.ui.updatePanel("side", [{ type: "button", id: "undo", label: "Undo" }]);
          },
        });
      } };
    "#;
    host.start(spec("panel", &[], js)).await.unwrap();
    let target = PluginUiTarget::Panel {
        panel: "panel.side".into(),
    };
    // Nothing rendered yet.
    let err = host
        .ui_action(&target, "save", BTreeMap::new(), Some("c".into()))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    host.panel("panel.side", &UiContext::default(), Some("c".into()))
        .await
        .unwrap();
    let values: BTreeMap<String, Value> =
        serde_json::from_value(json!({"note": "hi", "evil": "x", "save": 1})).unwrap();
    host.ui_action(&target, "save", values, Some("c".into()))
        .await
        .unwrap();
    assert_eq!(
        json_result(&fake, "panel", "ACT ").await,
        json!({"componentId": "save", "values": {"note": "hi"}})
    );
    for button in ["delete", "note"] {
        let err = host
            .ui_action(&target, button, BTreeMap::new(), Some("c".into()))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "{button}");
    }
    // After the update only its buttons count.
    wait_for("update", || {
        fake.events()
            .iter()
            .any(|e| matches!(e, Event::PluginPanelChanged { .. }))
            .then_some(())
    })
    .await;
    let err = host
        .ui_action(&target, "save", BTreeMap::new(), Some("c".into()))
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    host.ui_action(&target, "undo", BTreeMap::new(), Some("c".into()))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn extension_handlers_have_wall_clock_and_size_limits() {
    let fake = Fake::new();
    fake.add_attachment("m1", 0, "data.csv", "text/csv", b"a\n1\n");
    let host = host_with(
        &fake,
        nail_plugins::HostLimits {
            mcp_tool_timeout: std::time::Duration::from_millis(200),
            reply_timeout: std::time::Duration::from_millis(300),
            max_tool_result_bytes: 64,
            max_blob_bytes: 8,
            ..limits()
        },
    );
    let js = r#"
      export default { activate(ctx) {
        ctx.mcp.registerTool("hang", () => new Promise(() => {}));
        ctx.mcp.registerTool("big", () => ({ text: "x".repeat(100) }));
        ctx.attachments.registerPreview("csv", () => new Promise(() => {}));
      } };
      export async function deactivate() {}
    "#;
    host.start(spec_with(
        "limited",
        &["mcp.expose", "attachments", "network:api.example.com"],
        json!({
            "mcpTools": [
                { "id": "hang", "description": "Never answers" },
                { "id": "big", "description": "Answers too much" }
            ],
            "attachmentPreviews": [{ "id": "csv", "title": "Table", "mimeTypes": ["text/csv"] }]
        }),
        js,
    ))
    .await
    .unwrap();
    let client = McpClientRef {
        client_id: "c".into(),
        name: "C".into(),
    };
    let started = std::time::Instant::now();
    let err = host
        .call_mcp_tool("limited__hang", json!({}), client.clone(), full())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let err = host
        .call_mcp_tool("limited__big", json!({}), client, full())
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::TooLarge);
    let err = host
        .render_attachment_preview(&"m1".into(), 0, "limited.csv", None)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);

    // Bytes handed over in one turn are capped.
    host.start(spec(
        "bytes",
        &["network:api.example.com"],
        r#"export default { async activate() {
             try { await nail.http.fetch("https://api.example.com/", { method: "POST", body: new Uint8Array(9) }); }
             catch (e) { nail.log.info("E " + /too large/.test(String(e))); }
           } };"#,
    ))
    .await
    .unwrap();
    assert_eq!(result(&fake, "bytes", "E ").await, "true");
    assert!(fake.fetches.lock().unwrap().is_empty());
}

/// Review M4a: a plug-in serving an MCP client reads and writes only what
/// the client's grant reaches too (its accounts, not its excluded folders,
/// its scopes).
#[tokio::test(flavor = "multi_thread")]
async fn mcp_tools_run_within_the_clients_grant() {
    let fake = Fake::new();
    {
        let mut messages = fake.messages.lock().unwrap();
        messages.push((message("m-work", "work", "t1", false), None));
        messages.push((message("m-home", "home", "t2", false), None));
        let mut hr = message("m-hr", "work", "t3", false);
        hr.folder_id = "hr".into();
        messages.push((hr, None));
    }
    let host = host(&fake);
    let js = r#"
      export default { activate(ctx) {
        ctx.mcp.registerTool("peek", async () => {
          const seen = {};
          for (const id of ["m-work", "m-home", "m-hr"]) {
            try { await nail.mail.get(id); seen[id] = "ok"; }
            catch (e) { seen[id] = e.code; }
          }
          try { await nail.tasks.create({ title: "x" }); seen.task = "ok"; }
          catch (e) { seen.task = e.code; }
          return seen;
        });
      } };
    "#;
    host.start(spec_with(
        "crm",
        &["mcp.expose", "mail.read", "tasks.write"],
        json!({ "mcpTools": [{ "id": "peek", "description": "Peek" }] }),
        js,
    ))
    .await
    .unwrap();
    let client = McpClientRef {
        client_id: "agent".into(),
        name: "Agent".into(),
    };
    let narrow = ClientLimit {
        scopes: vec![McpScope::Read],
        accounts: Some(vec!["work".into()]),
        excluded_folders: vec!["hr".into()],
    };
    let seen = host
        .call_mcp_tool("crm__peek", json!({}), client.clone(), narrow)
        .await
        .unwrap();
    assert_eq!(
        seen,
        json!({"m-work": "ok", "m-home": "not_found", "m-hr": "not_found", "task": "forbidden"})
    );
    assert!(fake.writes().is_empty(), "no task without the pim scope");

    // The same plug-in for a client with everything reaches everything.
    let seen = host
        .call_mcp_tool("crm__peek", json!({}), client, full())
        .await
        .unwrap();
    assert_eq!(
        seen,
        json!({"m-work": "ok", "m-home": "ok", "m-hr": "ok", "task": "ok"})
    );
    // Only the second call wrote.
    assert_eq!(fake.writes().len(), 1);
}
