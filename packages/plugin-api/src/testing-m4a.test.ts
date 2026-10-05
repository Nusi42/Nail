/**
 * The test host's M4a behaviour: writes with policies and confirmations,
 * account-limited grants, hook budgets and answer checks, previews,
 * jobs, MCP tools, `nail.l10n` and binary HTTP bodies.
 */
import { describe, expect, it } from "vitest";

import { encodeUtf8 } from "./bytes";
import { veto } from "./extensions";
import type { PluginManifest, PluginMessage, PluginOutgoingMessage } from "./generated";
import { createTestHost } from "./testing";

const msg = (
  id: string,
  accountId = "work",
  extra: Partial<PluginMessage> = {},
): PluginMessage => ({
  id,
  uri: `nail://messages/${id}`,
  accountId,
  folderId: "INBOX",
  subject: `Subject ${id}`,
  to: [],
  cc: [],
  date: "2026-10-01T10:00:00Z",
  flags: { seen: false, flagged: false, answered: false, draft: false },
  tags: [],
  hasAttachments: false,
  encrypted: false,
  ...extra,
});

const outgoing: PluginOutgoingMessage = {
  draftId: "d1",
  accountId: "work",
  identityId: "work-main",
  from: { name: null, email: "me@example.org" },
  to: [{ name: "Ada", email: "ada@example.org" }],
  cc: [],
  bcc: [],
  subject: "Report",
  text: "Hi",
  attachments: [],
  encrypted: false,
};

describe("writes", () => {
  it("need their permission", async () => {
    const host = createTestHost({ permissions: ["mail.read"], messages: [msg("m1")] });
    const { api } = host;
    const calls: (() => Promise<unknown>)[] = [
      () => api.mail.update(["m1"], { read: true }),
      () => api.mail.delete(["m1"]),
      () => api.mail.createDraft({ subject: "x" }),
      () => api.mail.send("d1"),
      () => api.calendar.calendars(),
      () => api.calendar.createEvent({ title: "x", start: "2026-10-01", end: "2026-10-02" }),
      () => api.contacts.addressBooks(),
      () => api.contacts.upsert({ fullName: "Ada" }),
      () => api.tasks.lists(),
      () => api.tasks.create({ title: "x" }),
      () => api.composer.insert("x"),
    ];
    for (const call of calls) await expect(call()).rejects.toMatchObject({ code: "forbidden" });
    expect(host.writes).toEqual([]);
  });

  it("run organize, draft and task writes without asking by default", async () => {
    const host = createTestHost({ messages: [msg("m1")] });
    const update = await host.api.mail.update(["m1"], {
      read: true,
      flagged: true,
      addTags: ["crm"],
      moveToFolderId: "Projects",
    });
    expect(update).toMatchObject({ status: "done", created: [] });
    expect(host.messages.get("m1")).toMatchObject({
      folderId: "Projects",
      tags: ["crm"],
      flags: { seen: true, flagged: true },
    });
    const draft = await host.api.mail.createDraft({
      to: ["ada@example.org"],
      subject: "Hi",
      body: "**Hello**",
    });
    expect(draft).toMatchObject({ status: "done", created: ["draft-2"] });
    const task = await host.api.tasks.create({
      title: "Call Ada",
      due: "2026-10-05",
      links: ["nail://messages/m1"],
    });
    expect(task.status === "done" && task.created).toEqual(["task-4"]);
    expect(host.tasks.get("task-4")).toMatchObject({
      list: "tasks",
      title: "Call Ada",
      completed: false,
    });
    expect(host.confirmations).toEqual([]);
    expect(host.writes.map((w) => `${w.operation} ${w.kind}`)).toEqual([
      "nail.mail.update organize",
      "nail.mail.createDraft draft",
      "nail.tasks.create task",
    ]);
  });

  it("asks before sending, and before every delete even when allowed", async () => {
    const answers = ["deny", "approve", "expire", "approve"] as const;
    let n = 0;
    const host = createTestHost({
      messages: [msg("m1"), msg("m2")],
      policies: { trash: "allow", send: "ask" } as never,
      confirm: () => answers[n++] ?? "approve",
    });
    const draft = await host.api.mail.createDraft({ to: ["ada@example.org"], subject: "Hi" });
    const id = draft.status === "done" ? (draft.created[0] ?? "") : "";
    expect(await host.api.mail.send(id)).toEqual({
      status: "denied",
      confirmationId: "confirmation-3",
    });
    expect(host.drafts.get(id)?.sent).toBe(false);
    const sent = await host.api.mail.send(id);
    expect(sent).toMatchObject({ status: "done", confirmationId: "confirmation-4" });
    expect(host.drafts.get(id)?.sent).toBe(true);
    // A policy cannot relax deletions: Trash still asks.
    expect(await host.api.mail.delete(["m1"])).toMatchObject({ status: "expired" });
    expect(host.messages.get("m1")?.folderId).toBe("INBOX");
    expect(await host.api.mail.delete(["m2"], { permanent: true })).toMatchObject({
      status: "done",
    });
    expect(host.messages.has("m2")).toBe(false);
    expect(host.confirmations.map((c) => [c.kind, c.decision, c.canAlwaysAllow])).toEqual([
      ["send", "deny", true],
      ["send", "approve", true],
      ["trash", "expire", false],
      ["permanent-delete", "approve", false],
    ]);
  });

  it("lets the user allow sending, and make tasks ask", async () => {
    const host = createTestHost({
      policies: { send: "allow", task: "ask" },
      confirm: () => "deny",
    });
    const draft = await host.api.mail.createDraft({ subject: "x" });
    const id = draft.status === "done" ? (draft.created[0] ?? "") : "";
    expect(await host.api.mail.send(id)).toMatchObject({ status: "done" });
    expect(await host.api.tasks.create({ title: "x" })).toMatchObject({ status: "denied" });
    expect(host.tasks.size).toBe(0);
  });

  it("reports other plug-ins' vetoes", async () => {
    const host = createTestHost({ sendVetoes: () => ["Checker: no attachment"] });
    const draft = await host.api.mail.createDraft({ subject: "x" });
    const id = draft.status === "done" ? (draft.created[0] ?? "") : "";
    expect(await host.api.mail.send(id)).toEqual({
      status: "vetoed",
      reasons: ["Checker: no attachment"],
    });
    expect(host.confirmations).toEqual([]);
  });

  it("checks task, event and contact input", async () => {
    const host = createTestHost({
      messages: [msg("m1", "home")],
      accounts: ["work"],
      taskLists: [
        { id: "personal", name: "Personal", default: false, readOnly: false },
        { id: "work", name: "Work", default: true, readOnly: false },
      ],
    });
    const { tasks, calendar, contacts } = host.api;
    await expect(tasks.create({ title: " " })).rejects.toMatchObject({ code: "invalid_request" });
    await expect(tasks.create({ title: "x", priority: 12 })).rejects.toMatchObject({
      code: "invalid_request",
    });
    await expect(tasks.create({ title: "x", list: "work", listId: "work" })).rejects.toMatchObject({
      code: "invalid_request",
    });
    await expect(
      tasks.create({ title: "x", links: ["nail://messages/a", "nail://messages/b"] }),
    ).rejects.toMatchObject({ code: "invalid_request" });
    await expect(tasks.create({ title: "x", list: "nope" })).rejects.toMatchObject({
      code: "not_found",
    });
    await expect(tasks.create({ title: "x", links: ["https://x"] })).rejects.toMatchObject({
      code: "invalid_request",
    });
    // Linking a message needs mail.read for its account.
    await expect(tasks.create({ title: "x", links: ["nail://messages/m1"] })).rejects.toMatchObject(
      {
        code: "forbidden",
      },
    );
    const made = await tasks.create({ title: "Report", due: "2026-10-03" });
    const taskId = made.status === "done" ? (made.created[0] ?? "") : "";
    expect(host.tasks.get(taskId)?.list).toBe("work");
    await tasks.update(taskId, { completed: true, due: "" });
    expect(host.tasks.get(taskId)).toMatchObject({ completed: true });
    expect(host.tasks.get(taskId)?.due).toBeUndefined();
    expect(await tasks.delete(taskId)).toMatchObject({ status: "done" });
    expect(host.confirmations.at(-1)?.kind).toBe("pim-delete");

    await expect(
      calendar.createEvent({ title: "x", start: "2026-10-02", end: "2026-10-01" }),
    ).rejects.toMatchObject({
      code: "invalid_request",
    });
    const invite = await calendar.createEvent({
      title: "Sync",
      start: "2026-10-02T10:00:00Z",
      end: "2026-10-02T11:00:00Z",
      attendees: ["ada@example.org"],
      sendInvitations: true,
    });
    expect(invite).toMatchObject({ status: "done" });
    expect(host.writes.at(-1)?.kind).toBe("invite");
    expect(host.confirmations.at(-1)?.kind).toBe("invite");

    expect(await contacts.find("ADA@example.org")).toEqual([]);
    await contacts.upsert({ fullName: "Ada", emails: ["ada@example.org"] });
    expect(await contacts.find("ADA@example.org")).toMatchObject([
      { fullName: "Ada", addressBookId: "contacts" },
    ]);
    const readOnly = createTestHost({
      calendars: [{ id: "holidays", name: "Holidays", default: true, readOnly: true }],
    });
    await expect(
      readOnly.api.calendar.createEvent({ title: "x", start: "2026-10-01", end: "2026-10-02" }),
    ).rejects.toMatchObject({ code: "forbidden" });
  });

  it("acts on whole conversations", async () => {
    const host = createTestHost({
      messages: [
        msg("a1", "work", { threadId: "t1" }),
        msg("a2", "work", { threadId: "t1", folderId: "Sent" }),
        msg("b1", "work", { threadId: "t2" }),
      ],
    });
    await host.api.mail.update(
      { conversations: [{ threadId: "t1", folderId: "INBOX" }] },
      {
        moveToFolderId: "Archive",
      },
    );
    expect(host.messages.get("a1")?.folderId).toBe("Archive");
    expect(host.messages.get("a2")?.folderId).toBe("Sent");
    await host.api.mail.delete({ messageIds: "b1", conversations: [{ threadId: "t1" }] });
    expect([...host.messages.values()].map((m) => m.folderId)).toEqual(["Trash", "Trash", "Trash"]);
  });

  it("keeps mail writes to the granted accounts", async () => {
    const host = createTestHost({
      messages: [msg("w", "work"), msg("h", "home")],
      accounts: ["work"],
    });
    await expect(host.api.mail.update(["w", "h"], { read: true })).rejects.toMatchObject({
      code: "forbidden",
    });
    await expect(host.api.mail.get("h")).rejects.toMatchObject({ code: "forbidden" });
    expect((await host.api.mail.search("")).map((m) => m.id)).toEqual(["w"]);
    expect(await host.api.mail.update(["w"], { read: true })).toMatchObject({ status: "done" });
  });

  it("records composer edits and refuses unknown ones", async () => {
    const host = createTestHost();
    // No composer action ran: there is no draft to edit.
    await expect(host.api.composer.insert("Hello")).rejects.toMatchObject({ code: "not_found" });
    host.activationContext().composer.registerAction("act", async () => {
      await host.api.composer.insert("Hello", { format: "markdown" });
      await host.api.composer.replaceAll("<p>x</p>", { draftId: "d1", format: "html" });
    });
    await host.runComposerAction("act", { draftId: "d1" });
    expect(host.composerEdits).toEqual([
      { draftId: "d1", edit: { op: "insert", text: "Hello", format: "markdown" } },
      { draftId: "d1", edit: { op: "replace-all", text: "<p>x</p>", format: "html" } },
    ]);
    // Only the draft the action ran on.
    await expect(host.api.composer.insert("x", { draftId: "other" })).rejects.toMatchObject({
      code: "not_found",
    });
    await expect(host.api.composer.insert("x", { format: "rtf" as never })).rejects.toMatchObject({
      code: "invalid_request",
    });
  });
});

const hookManifest = (hooks: PluginManifest["contributes"]): PluginManifest => ({
  id: "hooks",
  name: "Hooks",
  version: "1.0.0",
  apiVersion: "1",
  main: "dist/index.js",
  permissions: ["mail.read", "hooks.arrival", "hooks.beforeSend", "hooks.open"],
  contributes: hooks ?? {},
});

describe("hooks in the test host", () => {
  it("refuses hooks the manifest does not declare", () => {
    const host = createTestHost({ manifest: hookManifest({ hooks: [{ hook: "open" }] }) });
    expect(() => {
      host.activationContext().hooks.register("beforeSend", () => null);
    }).toThrow(/not in the manifest/);
    host.activationContext().hooks.register("open", () => null);
    expect(() => {
      host.activationContext().hooks.register("open", () => null);
    }).toThrow(/registered/);
  });

  it("skips unregistered and unpermitted hooks and other accounts' mail", async () => {
    const host = createTestHost({
      permissions: ["mail.read", "hooks.open"],
      accounts: ["work"],
    });
    expect((await host.runBeforeSend(outgoing)).outcome).toBe("skipped");
    host.activationContext().hooks.register("beforeSend", () => veto("x"));
    expect(await host.runBeforeSend(outgoing)).toMatchObject({
      outcome: "skipped",
      error: /hooks.beforeSend/,
    });
    host.activationContext().hooks.register("open", () => ({ badges: [{ text: "ok" }] }));
    expect((await host.runOpen(msg("h", "home"))).outcome).toBe("skipped");
    expect((await host.runOpen(msg("w", "work"))).outcome).toBe("ok");
  });

  it("counts throws, invalid answers and timeouts as failures that never block", async () => {
    const host = createTestHost({
      manifest: hookManifest({
        hooks: [{ hook: "beforeSend", timeoutMs: 50 }, { hook: "arrival" }, { hook: "open" }],
      }),
      messages: [msg("m1")],
    });
    const ctx = host.activationContext();
    let mode: "throw" | "slow" | "bad" | "ok" = "throw";
    ctx.hooks.register("beforeSend", async () => {
      if (mode === "throw") throw new Error("boom");
      if (mode === "slow") await new Promise((r) => setTimeout(r, 200));
      if (mode === "bad") return { headers: [{ name: "Subject", value: "x" }] };
      return { headers: [{ name: "X-Checked", value: "yes" }] };
    });
    expect(await host.runBeforeSend(outgoing)).toMatchObject({
      outcome: "failed",
      error: "Error: boom",
    });
    mode = "slow";
    expect(await host.runBeforeSend(outgoing)).toMatchObject({ outcome: "timed-out" });
    mode = "bad";
    expect(await host.runBeforeSend(outgoing)).toEqual({
      outcome: "failed",
      problems: ["invalid header"],
    });
    mode = "ok";
    expect((await host.runBeforeSend(outgoing)).result).toEqual({
      headers: [{ name: "X-Checked", value: "yes" }],
    });

    // Arrival actions need mail.organize, which this manifest lacks.
    ctx.hooks.register("arrival", () => ({ actions: [{ type: "archive" }] }));
    expect(await host.runArrival(msg("m1"))).toEqual({
      outcome: "failed",
      problems: ["actions need mail.organize"],
    });
    expect(host.messages.get("m1")?.folderId).toBe("INBOX");
    ctx.hooks.register("open", () => "banner" as never);
    expect(await host.runOpen(msg("m1"))).toEqual({
      outcome: "failed",
      problems: ["not an object"],
    });
  });

  it("gives bodies only with mail.read.body and never of encrypted mail", async () => {
    const seen: (string | undefined)[] = [];
    const bodies = {
      plain: { id: "plain", text: "hello", attachments: [] },
      secret: { id: "secret", text: "decrypted", attachments: [] },
    };
    for (const permissions of [
      ["mail.read", "hooks.open"],
      ["mail.read", "mail.read.body", "hooks.open"],
    ] as const) {
      const host = createTestHost({ permissions: [...permissions], bodies });
      host.activationContext().hooks.register("open", (mail) => {
        seen.push(mail.body?.text);
        return null;
      });
      await host.runOpen(msg("plain"));
      await host.runOpen(msg("secret", "work", { encrypted: true }));
    }
    expect(seen).toEqual([undefined, undefined, "hello", undefined]);
  });
});

describe("extensions in the test host", () => {
  const manifest: PluginManifest = {
    id: "ext",
    name: "Ext",
    version: "1.0.0",
    apiVersion: "1",
    main: "dist/index.js",
    permissions: ["attachments", "background", "mcp.expose", "composer"],
    contributes: {
      attachmentPreviews: [{ id: "small", title: "Small", extensions: ["txt"], maxBytes: 4 }],
      jobs: [{ id: "slow", events: ["app.started"] }],
      mcpTools: [{ id: "t", description: "T" }],
      markdownBlocks: [{ id: "b", title: "B" }],
    },
  };

  it("checks preview sizes and answers", async () => {
    const host = createTestHost({ manifest });
    let answer: unknown = { kind: "image", mimeType: "image/svg+xml", data: "" };
    host.activationContext().attachments.registerPreview("small", () => answer as never);
    await expect(host.renderPreview("small", { name: "a.txt" }, "12345")).rejects.toMatchObject({
      code: "too_large",
    });
    await expect(host.renderPreview("small", { name: "a.txt" }, "1234")).rejects.toMatchObject({
      code: "invalid_request",
      message: "image type not allowed",
    });
    answer = { kind: "tree", tree: [{ type: "text", value: "ok" }] };
    expect(await host.renderPreview("small", { name: "a.txt" }, encodeUtf8("1234"))).toEqual(
      answer,
    );
    await expect(host.renderPreview("other", { name: "a.txt" }, "")).rejects.toMatchObject({
      code: "not_found",
    });
  });

  it("limits job runs and reports errors", async () => {
    const host = createTestHost({ manifest, jobTimeoutMs: 30 });
    let mode = "slow";
    host.activationContext().jobs.register("slow", async () => {
      if (mode === "slow") await new Promise((r) => setTimeout(r, 200));
      if (mode === "throw") throw new TypeError("nope");
    });
    expect(
      await host.runJob("slow", { kind: "event", event: "app.started", messageIds: [] }),
    ).toMatchObject({
      outcome: "timeout",
    });
    mode = "throw";
    expect(await host.runJob("slow")).toEqual({ outcome: "error", error: "TypeError: nope" });
    mode = "ok";
    expect(await host.runJob("slow")).toEqual({ outcome: "ok" });
  });

  it("refuses oversized Markdown blocks and needs mcp.expose for tools", async () => {
    const host = createTestHost({ manifest });
    host
      .activationContext()
      .composer.registerBlock("b", () => ({ html: "x".repeat(300_000), text: "" }));
    await expect(host.renderMarkdownBlock("b", "src")).rejects.toMatchObject({
      code: "invalid_request",
    });
    const noExpose = createTestHost({ manifest, permissions: ["attachments"] });
    noExpose.activationContext().mcp.registerTool("t", () => 1);
    await expect(noExpose.callMcpTool("t")).rejects.toMatchObject({ code: "forbidden" });
  });
});

describe("l10n and binary HTTP in the test host", () => {
  it("formats the plug-in's catalogs in the chosen language", () => {
    const host = createTestHost({
      locale: "de",
      catalogs: { en: "greet = Hello { $name }\nonly = English", de: "greet = Hallo { $name }" },
    });
    expect(host.api.l10n.locale).toBe("de");
    expect(host.api.l10n.t("greet", { name: "Ada" })).toBe("Hallo Ada");
    expect(host.api.l10n.t("only")).toBe("English");
    expect(host.api.l10n.has("missing")).toBe(false);
    expect(createTestHost().api.l10n.t("x")).toBe("x");
  });

  it("sends and receives bytes", async () => {
    const seen: unknown[] = [];
    const host = createTestHost({
      permissions: ["network:api.example.com"],
      fetch: (_url, request) => {
        seen.push(request.body);
        return { body: Uint8Array.from([0x89, 0x50, 0xc3, 0xa9]) };
      },
    });
    const res = await host.api.http.fetch("https://api.example.com/upload", {
      method: "POST",
      body: Uint8Array.from([1, 2, 3]),
    });
    expect(seen).toEqual([Uint8Array.from([1, 2, 3])]);
    expect(await res.bytes()).toEqual(Uint8Array.from([0x89, 0x50, 0xc3, 0xa9]));
    expect(new Uint8Array(await res.arrayBuffer())).toEqual(
      Uint8Array.from([0x89, 0x50, 0xc3, 0xa9]),
    );
    expect(res.body).toBe("�Pé");
    await expect(
      host.api.http.fetch("https://api.example.com/x", { method: "GET", body: "nope" }),
    ).rejects.toMatchObject({ code: "invalid_request" });
    const text = await createTestHost({
      permissions: ["network:api.example.com"],
      fetch: () => ({ body: '{"a":1}' }),
    }).api.http.fetch("https://api.example.com/j");
    expect(await text.json()).toEqual({ a: 1 });
    expect(await text.bytes()).toEqual(encodeUtf8('{"a":1}'));
  });
});
