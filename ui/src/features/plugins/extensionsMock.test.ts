import { describe, expect, it, vi } from "vitest";
import type { Draft, Event as NailEvent, PluginInfo } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import {
  answerSendDraft,
  CPU_QUOTA_MS_PER_HOUR,
  PLUGIN_EXTENSION_REQUESTS,
  isPluginExtensionRequest,
} from "./extensionsMock";
import { forgotAttachment, installDemoExtensions } from "./extensionsSeed";

function digest(): PluginInfo {
  return {
    id: "digest",
    manifest: {
      id: "digest",
      name: "Digest",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["background"],
      contributes: { jobs: [{ id: "daily", schedule: "0 8 * * *" }] },
    },
    source: { kind: "dev", folder: "/dev/digest" },
    enabled: true,
    state: "running",
    grants: [{ permission: "background", accounts: null }],
    storage: { keys: 0, used_bytes: 0, quota_bytes: 5_242_880 },
    package_sha256: null,
    installed_at: "2026-10-02T08:00:00Z",
    updated_at: "2026-10-02T08:00:00Z",
    error: null,
  };
}

function draft(text: string): Draft {
  return {
    id: null,
    identity_id: "ada@work.example",
    to: [{ name: null, email: "grace@example.org" }],
    cc: [],
    bcc: [],
    subject: "Figures",
    mode: "plain",
    text,
    html: null,
    markdown: null,
    attachments: [],
    context: null,
    signature: "none",
    markdown_part: false,
  };
}

async function saved(t: MockTransport, text: string): Promise<string> {
  const r = await t.send({ type: "SaveDraft", draft: draft(text) });
  if (r.type !== "DraftSaved") throw new Error(JSON.stringify(r));
  return r.draft_id;
}

function firstWithAttachments(t: MockTransport): string {
  const m = t.data.messages.find((x) => t.data.bodies.get(x.id)?.attachments.length);
  if (!m) throw new Error("no message with attachments");
  return m.id;
}

describe("plug-in extension mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(PLUGIN_EXTENSION_REQUESTS).size).toBe(PLUGIN_EXTENSION_REQUESTS.length);
    expect(isPluginExtensionRequest({ type: "OverrideSendVeto", veto_id: "v1" })).toBe(true);
    expect(isPluginExtensionRequest({ type: "ListPlugins" })).toBe(false);
  });

  it("answers for a plug-in without behaviour like a host with nothing to add", async () => {
    const t = new MockTransport();
    t.data.features.plugins.plugins.push(digest());
    const activity = await t.send({ type: "GetPluginActivity", plugin_id: "digest" });
    expect(activity).toMatchObject({
      type: "PluginActivity",
      activity: {
        jobs: [{ job: "daily", schedule: "0 8 * * *", run_count: 0 }],
        hooks: [],
        cpu_quota_ms_per_hour: CPU_QUOTA_MS_PER_HOUR,
      },
    });
    expect(await t.send({ type: "GetPluginActivity", plugin_id: "ghost" })).toMatchObject({
      code: "not_found",
    });
    const id = t.data.messages[0]?.id ?? "";
    expect(
      await t.send({ type: "GetMessagePluginAnnotations", message_id: id, client_id: null }),
    ).toEqual({ type: "MessagePluginAnnotations", message_id: id, banners: [], badges: [] });
    expect(await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "daily" })).toEqual({
      type: "Done",
    });
    expect(await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "nope" })).toMatchObject({
      code: "not_found",
    });
    expect(await t.send({ type: "OverrideSendVeto", veto_id: "v1" })).toMatchObject({
      code: "not_found",
    });
  });

  it("answers unsupported without a host (the contract stage)", async () => {
    const t = new MockTransport();
    t.data.features.plugins.host = null;
    t.data.features.plugins.plugins.push(digest());
    expect(await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "daily" })).toMatchObject(
      { code: "unsupported" },
    );
  });

  it("collects banners and badges of open hooks and counts the calls", async () => {
    const t = new MockTransport();
    installDemoExtensions(t);
    const id = t.data.messages[0]?.id ?? "";
    const r = await t.send({
      type: "GetMessagePluginAnnotations",
      message_id: id,
      client_id: null,
    });
    expect(r).toMatchObject({
      type: "MessagePluginAnnotations",
      banners: [{ plugin_id: "link-guard", plugin_name: "Link guard", banner: { id: "external" } }],
      badges: [{ plugin_id: "link-guard", badge: { text: "External" } }],
    });
    // Without the grant nothing shows.
    const guard = t.data.features.plugins.plugins.find((p) => p.id === "link-guard");
    if (guard) guard.grants = guard.grants.filter((g) => g.permission !== "hooks.open");
    const none = await t.send({
      type: "GetMessagePluginAnnotations",
      message_id: id,
      client_id: null,
    });
    expect(none).toMatchObject({ banners: [], badges: [] });
    expect(
      await t.send({ type: "GetMessagePluginAnnotations", message_id: "nope", client_id: null }),
    ).toMatchObject({ code: "not_found" });
  });

  it("vetoes a send, lets the user send anyway once, and sends unhooked mail at once", async () => {
    const t = new MockTransport();
    installDemoExtensions(t);
    const forgot = await saved(t, "Please find the figures attached.");
    const r = answerSendDraft(t, {
      type: "SendDraft",
      draft_id: forgot,
      delay_secs: 0,
      send_at: null,
    });
    expect(r).toMatchObject({
      type: "SendVetoed",
      draft_id: forgot,
      vetoes: [{ plugin_id: "attachment-check", plugin_name: "Attachment check" }],
    });
    expect(t.data.outbox).toHaveLength(0);
    if (r.type !== "SendVetoed") throw new Error("not vetoed");
    expect(await t.send({ type: "OverrideSendVeto", veto_id: r.veto_id })).toMatchObject({
      type: "SendQueued",
    });
    expect(await t.send({ type: "OverrideSendVeto", veto_id: r.veto_id })).toMatchObject({
      code: "not_found",
    });
    const fine = await saved(t, "See you tomorrow.");
    expect(
      answerSendDraft(t, { type: "SendDraft", draft_id: fine, delay_secs: 0, send_at: null }),
    ).toMatchObject({ type: "SendQueued" });
    const stats = await t.send({ type: "GetPluginActivity", plugin_id: "attachment-check" });
    expect(stats).toMatchObject({
      activity: { hooks: [{ hook: "beforeSend", calls: 42, vetoes: 2 }] },
    });
  });

  it("expires a veto after ten minutes", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      const t = new MockTransport();
      installDemoExtensions(t);
      const id = await saved(t, "attachment follows");
      const r = answerSendDraft(t, {
        type: "SendDraft",
        draft_id: id,
        delay_secs: 0,
        send_at: null,
      });
      if (r.type !== "SendVetoed") throw new Error("not vetoed");
      vi.setSystemTime(Date.now() + 11 * 60_000);
      expect(await t.send({ type: "OverrideSendVeto", veto_id: r.veto_id })).toMatchObject({
        code: "not_found",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("renders previews only for matching, granted, small enough attachments", async () => {
    const t = new MockTransport();
    installDemoExtensions(t);
    const id = firstWithAttachments(t);
    const csv = await t.send({
      type: "RenderAttachmentPreview",
      message_id: id,
      index: 1,
      renderer: "file-preview.csv",
      client_id: null,
    });
    expect(csv).toMatchObject({ type: "AttachmentPreview", preview: { kind: "tree" } });
    const pdf = await t.send({
      type: "RenderAttachmentPreview",
      message_id: id,
      index: 0,
      renderer: "file-preview.pdf",
      client_id: null,
    });
    expect(pdf).toMatchObject({ preview: { kind: "image", mimeType: "image/png" } });
    expect(
      await t.send({
        type: "RenderAttachmentPreview",
        message_id: id,
        index: 0,
        renderer: "file-preview.csv",
        client_id: null,
      }),
    ).toMatchObject({ code: "invalid_request" });
    const body = t.data.bodies.get(id);
    if (body?.attachments[0]) body.attachments[0].size = 6_000_000;
    expect(
      await t.send({
        type: "RenderAttachmentPreview",
        message_id: id,
        index: 0,
        renderer: "file-preview.pdf",
        client_id: null,
      }),
    ).toMatchObject({ code: "too_large" });
    expect(
      await t.send({
        type: "RenderAttachmentPreview",
        message_id: id,
        index: 0,
        renderer: "file-preview.nope",
        client_id: null,
      }),
    ).toMatchObject({ code: "not_found" });
  });

  it("runs composer actions, which edit the open draft through events", async () => {
    const t = new MockTransport();
    t.data.behaviour.workMs = 0;
    installDemoExtensions(t);
    const events: NailEvent[] = [];
    t.onEvent((e) => events.push(e));
    const context = {
      account_id: null,
      folder_id: null,
      message_id: null,
      thread_id: null,
      selected_message_ids: [],
      selected_text: null,
      draft_id: "d1",
      draft_cursor: null,
    };
    expect(
      await t.send({
        type: "RunPluginAction",
        action: "snippets.shout",
        target: { kind: "composer", draft_id: "d1", selection: { text: "hey", start: 0, end: 3 } },
        context,
        client_id: "ui-1",
      }),
    ).toEqual({ type: "Done" });
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "ComposerEditRequested",
        draft_id: "d1",
        edit: { op: "replace-selection", text: "HEY", format: "text" },
      }),
    );
    // A link action exists for every link; an attachment action needs a match.
    expect(
      await t.send({
        type: "RunPluginAction",
        action: "snippets.check",
        target: { kind: "link", url: "https://x.example", text: null, message_id: null },
        context,
        client_id: null,
      }),
    ).toMatchObject({ code: "not_found" });
    const snippets = t.data.features.plugins.plugins.find((p) => p.id === "snippets");
    if (snippets) snippets.enabled = false;
    expect(
      await t.send({
        type: "RunPluginAction",
        action: "snippets.greeting",
        target: { kind: "composer", draft_id: "d1", selection: { text: "", start: 0, end: 0 } },
        context,
        client_id: null,
      }),
    ).toMatchObject({ code: "unavailable" });
  });

  it("records job runs, skips them over the CPU quota and refuses unknown or stopped ones", async () => {
    const t = new MockTransport();
    installDemoExtensions(t);
    const events: NailEvent[] = [];
    t.onEvent((e) => events.push(e));
    expect(await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "morning" })).toEqual({
      type: "Done",
    });
    expect(events).toContainEqual({ type: "PluginActivityChanged", plugin_id: "digest" });
    const a = await t.send({ type: "GetPluginActivity", plugin_id: "digest" });
    expect(a).toMatchObject({
      activity: { jobs: [{ job: "morning", run_count: 15, last_outcome: "ok" }, { job: "count" }] },
    });
    const activity = t.data.features.plugins.extensions.activity.digest;
    if (activity) activity.cpuMs = CPU_QUOTA_MS_PER_HOUR;
    await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "morning" });
    expect(await t.send({ type: "GetPluginActivity", plugin_id: "digest" })).toMatchObject({
      activity: { jobs: [{ last_outcome: "cpu-quota", failure_count: 2 }, {}] },
    });
    const plugin = t.data.features.plugins.plugins.find((p) => p.id === "digest");
    if (plugin) plugin.enabled = false;
    expect(
      await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "morning" }),
    ).toMatchObject({ code: "unavailable" });
  });

  it("knows a forgotten attachment", () => {
    expect(forgotAttachment(draft("see the attached file"))).toBe(true);
    expect(forgotAttachment(draft("no files today"))).toBe(false);
    expect(
      forgotAttachment({
        ...draft("attached"),
        attachments: [
          { blob: "b", filename: "a.csv", mime: "text/csv", size: 1, content_id: null },
        ],
      }),
    ).toBe(false);
  });
});
