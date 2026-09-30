/**
 * The "Follow up" example (examples/follow-up): it is built from the same
 * scripts as the template, type-checks, bundles, packs into
 * follow-up.nailplugin and behaves as documented against the SDK's test
 * host (read-only mail, dialog, storage, notification, panel).
 */
import { readFileSync } from "node:fs";
import path from "node:path";

import type { PluginMessage, PluginUiNode } from "@nail/plugin-api";
import { checkManifest, readZip } from "@nail/plugin-api/tools";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import { TEMPLATE_DIR } from "../scaffold.mjs";
import {
  copyPlugin,
  linkDependencies,
  loadBundle,
  node,
  readManifest,
  REPO,
  tempDir,
  typecheck,
  type LoadedPlugin,
} from "./helpers";

const EXAMPLE = path.join(REPO, "examples", "follow-up");

const message: PluginMessage = {
  id: "m1",
  uri: "nail://messages/m1",
  accountId: "work",
  folderId: "INBOX",
  subject: "Contract renewal",
  from: { name: "Ada Lovelace", email: "ada@example.org" },
  to: [],
  cc: [],
  date: "2026-09-29T10:00:00Z",
  flags: { seen: true, flagged: false, answered: false, draft: false },
  tags: [],
  hasAttachments: false,
  encrypted: false,
};
const other: PluginMessage = { ...message, id: "m2", uri: "nail://messages/m2", subject: "Old" };

let dir = "";
let cleanup = () => {};
let bundle = "";

beforeAll(() => {
  const t = tempDir("nail-follow-up-");
  cleanup = t.cleanup;
  dir = path.join(t.dir, "follow-up");
  copyPlugin(EXAMPLE, dir);
  linkDependencies(dir);
  typecheck(dir);
  node(dir, "scripts/build.mjs");
  bundle = path.join(dir, "dist", "index.js");
  return () => cleanup();
});

describe("the follow-up example", () => {
  it("uses the template's build and pack scripts and tsconfig", () => {
    for (const file of ["scripts/build.mjs", "scripts/pack.mjs", "tsconfig.json"]) {
      expect(readFileSync(path.join(EXAMPLE, file), "utf8"), file).toBe(
        readFileSync(path.join(TEMPLATE_DIR, file), "utf8"),
      );
    }
  });

  it("has a valid manifest matching the design's example, read-only", () => {
    const manifest = readManifest(EXAMPLE);
    expect(checkManifest(manifest)).toEqual([]);
    expect(manifest.id).toBe("follow-up");
    expect(manifest.permissions).toEqual(["mail.read", "ui.dialogs", "storage", "notifications"]);
    expect(manifest.contributes?.placements).toEqual([
      { toolbar: "reader", after: "mail.forward", command: "follow-up" },
    ]);
    const catalog = readFileSync(path.join(EXAMPLE, "locales", "en.ftl"), "utf8");
    const texts = JSON.stringify(manifest).match(/%[a-z-]+%/g) ?? [];
    expect(texts.length).toBeGreaterThan(0);
    for (const t of texts) expect(catalog, t).toMatch(new RegExp(`^${t.slice(1, -1)} = `, "m"));
  });

  it("builds into one self-contained module and packs follow-up.nailplugin", () => {
    const code = readFileSync(bundle, "utf8");
    expect(code).not.toMatch(/^\s*import\s/m);
    expect(code).not.toMatch(/\brequire\(/);
    const out = node(dir, "scripts/pack.mjs");
    expect(out).toMatch(/follow-up\.nailplugin/);
    const zip = readZip(readFileSync(path.join(dir, "dist", "follow-up.nailplugin")));
    expect(zip.map((e) => e.name)).toEqual(["manifest.json", "dist/index.js", "locales/en.ftl"]);
    expect(zip[1]?.data.toString("utf8")).toBe(code);
  });
});

describe("the follow-up plug-in at run time", () => {
  let loaded: LoadedPlugin | undefined;
  afterEach(() => {
    loaded?.restore();
    loaded = undefined;
    answers.length = 0;
  });

  const answers: { button: string | null; values: Record<string, unknown> }[] = [];
  async function load(options: Parameters<typeof loadBundle>[1] = {}) {
    loaded = await loadBundle(bundle, {
      manifest: readManifest(EXAMPLE),
      messages: [message, other],
      settings: { "default-days": 1 },
      dialog: () => {
        const next = answers.shift();
        if (!next) throw new Error("unexpected dialog");
        return next as never;
      },
      ...options,
    });
    return loaded.host;
  }

  it("registers its command and panel", async () => {
    const host = await load();
    expect(host.commandNames()).toEqual(["follow-up"]);
    expect(host.panelNames()).toEqual(["due"]);
  });

  it("asks for a date and note, stores the reminder and notifies", async () => {
    const host = await load();
    answers.push({ button: "save", values: { due: "2999-12-31", note: "  Call Ada  " } });
    await host.runCommand("follow-up", {
      surface: "reader",
      selection: { messageIds: ["m1"], openMessageId: "m1" },
    });

    const dialog = host.dialogs[0];
    expect(dialog?.title).toBe("Follow up");
    expect(dialog?.buttons?.map((b) => b.id)).toEqual(["save", "cancel"]);
    const stack = dialog?.body[0] as Extract<PluginUiNode, { type: "stack" }>;
    expect(stack.children).toEqual([
      { type: "text", style: "strong", value: "Remind me about “Contract renewal”" },
      { type: "text", style: "muted", value: "From Ada Lovelace <ada@example.org>" },
      { type: "date", id: "due", label: "Due", default: "tomorrow", required: true },
      {
        type: "textarea",
        id: "note",
        label: "Note",
        placeholder: "What needs doing?",
        default: "",
        rows: 3,
        maxLength: 500,
      },
    ]);

    expect(host.storage.get("reminder:m1")).toMatchObject({
      messageId: "m1",
      messageUri: "nail://messages/m1",
      subject: "Contract renewal",
      from: "Ada Lovelace <ada@example.org>",
      due: "2999-12-31",
      note: "Call Ada",
    });
    expect(host.notifications).toEqual([
      { title: "Follow-up reminder set for 2999-12-31", level: "success" },
    ]);
    // The panel (registered) was redrawn with the new reminder.
    expect(host.panelUpdates.at(-1)?.tree).toEqual([
      {
        type: "stack",
        children: [
          {
            type: "table",
            columns: [{ label: "Due" }, { label: "Subject" }, { label: "Note" }],
            rows: [["2999-12-31", "Contract renewal", "Call Ada"]],
          },
        ],
      },
    ]);
  });

  it("edits and removes an existing reminder", async () => {
    const host = await load();
    host.storage.set("reminder:m1", {
      messageId: "m1",
      messageUri: "nail://messages/m1",
      subject: "Contract renewal",
      from: "",
      due: "2999-01-02",
      note: "old note",
      createdAt: "2026-01-01T00:00:00.000Z",
    });
    answers.push({ button: "remove", values: {} });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    const stack = host.dialogs[0]?.body[0] as Extract<PluginUiNode, { type: "stack" }>;
    expect(stack.children[2]).toMatchObject({ id: "due", default: "2999-01-02" });
    expect(stack.children[3]).toMatchObject({ id: "note", default: "old note" });
    expect(host.dialogs[0]?.buttons?.map((b) => b.id)).toEqual(["save", "remove", "cancel"]);
    expect(host.storage.has("reminder:m1")).toBe(false);
    expect(host.notifications).toEqual([{ title: "Follow-up reminder removed", level: "info" }]);
  });

  it("does nothing on cancel and uses the default-days setting", async () => {
    const host = await load({ settings: { "default-days": 0 } });
    answers.push({ button: "cancel", values: { due: "2999-12-31" } });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    const stack = host.dialogs[0]?.body[0] as Extract<PluginUiNode, { type: "stack" }>;
    expect(stack.children[2]).toMatchObject({ default: "today" });
    expect(host.storage.size).toBe(0);
    expect(host.notifications).toEqual([]);
  });

  it("warns when nothing is selected", async () => {
    const host = await load();
    await host.runCommand("follow-up", { selection: { messageIds: [] } });
    expect(host.dialogs).toEqual([]);
    expect(host.notifications).toEqual([
      { title: "Select a message to follow up on", level: "warning" },
    ]);
  });

  it("reports a missing mail.read grant instead of crashing", async () => {
    const host = await load({ permissions: ["ui.dialogs", "storage", "notifications"] });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.dialogs).toEqual([]);
    expect(host.notifications).toEqual([
      { title: "This message cannot be read by the Follow up plug-in", level: "error" },
    ]);
    expect(host.logs[0]?.level).toBe("warn");
  });

  it("reports a full storage", async () => {
    const host = await load({ storageQuota: 10 });
    answers.push({ button: "save", values: { due: "2999-12-31", note: "" } });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.storage.size).toBe(0);
    expect(host.notifications).toEqual([
      { title: "The reminder could not be saved", level: "error" },
    ]);
  });

  it("lists reminders in its panel and removes past ones", async () => {
    const host = await load();
    expect(await host.renderPanel("due")).toEqual([
      {
        type: "text",
        style: "muted",
        value: "No follow-ups. Use “Follow up…” on a message to add one.",
      },
    ]);
    const base = { messageUri: "", from: "", note: "", createdAt: "" };
    host.storage.set("reminder:m2", {
      ...base,
      messageId: "m2",
      subject: "Old",
      due: "2000-01-01",
    });
    host.storage.set("reminder:m1", {
      ...base,
      messageId: "m1",
      subject: "New",
      due: "2999-01-01",
    });
    host.storage.set("unrelated", 1);
    const tree = await host.renderPanel("due");
    const stack = tree[0] as Extract<PluginUiNode, { type: "stack" }>;
    expect(stack.children[0]).toEqual({ type: "text", style: "error", value: "1 overdue" });
    expect(stack.children[1]).toMatchObject({
      rows: [
        ["2000-01-01", "Old", ""],
        ["2999-01-01", "New", ""],
      ],
    });
    expect(stack.children[2]).toEqual({
      type: "button",
      id: "remove-past",
      label: "Remove past reminders",
    });
    await host.clickPanelButton("due", "remove-past");
    expect([...host.storage.keys()].sort()).toEqual(["reminder:m1", "unrelated"]);
  });
});
