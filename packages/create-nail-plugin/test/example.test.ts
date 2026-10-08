/**
 * The "Follow up" example (examples/follow-up) at run time against the
 * SDK's test host: the dialog, the task it creates (tasks.write, linked to
 * the message), denied and expired confirmations, missing permissions,
 * the panel and the reminder job.
 */
import type { PluginUiNode } from "@nail/plugin-api";
import { testMessage } from "@nail/plugin-api/testing";
import { afterEach, describe, expect, it } from "vitest";

import { pluginLoader, useExample } from "./helpers";

const message = testMessage("m1", {
  subject: "Contract renewal",
  from: { name: "Ada Lovelace", email: "ada@example.org" },
  date: "2026-09-29T10:00:00Z",
  flags: { seen: true, flagged: false, answered: false, draft: false },
});

const lists = [
  { id: "work", name: "Work", default: true, readOnly: false },
  { id: "home", name: "Home", default: false, readOnly: false },
  { id: "holidays", name: "Holidays", default: false, readOnly: true },
];

const followUp = useExample("follow-up");

const pad = (n: number) => String(n).padStart(2, "0");
const isoDate = (d: Date) =>
  `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

describe("the follow-up plug-in at run time", () => {
  const answers: { button: string | null; values: Record<string, unknown> }[] = [];
  afterEach(() => {
    answers.length = 0;
  });

  const load = pluginLoader(
    () => followUp.example.dir,
    () => ({
      messages: [message],
      taskLists: lists,
      settings: { "default-days": 1, remind: true },
      dialog: () => {
        const next = answers.shift();
        if (!next) throw new Error("unexpected dialog");
        return next as never;
      },
    }),
  );

  const stackOf = (nodes: PluginUiNode[] | undefined) =>
    (nodes?.[0] as Extract<PluginUiNode, { type: "stack" }>).children;

  it("registers its command, panel and reminder job", async () => {
    const host = await load();
    expect(host.commandNames()).toEqual(["follow-up"]);
    expect(host.panelNames()).toEqual(["due"]);
    expect(host.extensionNames().jobs).toEqual(["remind"]);
  });

  it("creates a task linked to the message and remembers the follow-up", async () => {
    const host = await load();
    answers.push({
      button: "create",
      values: { due: "2999-12-31", list: ["home"], note: "  Call Ada  ", remind: true },
    });
    await host.runCommand("follow-up", {
      surface: "reader",
      selection: { messageIds: ["m1"], openMessageId: "m1" },
    });

    const dialog = host.dialogs[0];
    expect(dialog?.title).toBe("Follow up");
    expect(dialog?.buttons?.map((b) => b.id)).toEqual(["create", "cancel"]);
    expect(stackOf(dialog?.body)).toEqual([
      { type: "text", style: "strong", value: "Follow up on “Contract renewal”" },
      { type: "text", style: "muted", value: "From Ada Lovelace <ada@example.org>" },
      { type: "date", id: "due", label: "Due", default: "tomorrow", required: true },
      {
        type: "select",
        id: "list",
        label: "Task list",
        // The read-only list is left out.
        options: [
          { value: "work", label: "Work" },
          { value: "home", label: "Home" },
        ],
        default: ["work"],
        required: true,
      },
      {
        type: "textarea",
        id: "note",
        label: "Note",
        placeholder: "What needs doing?",
        default: "",
        rows: 3,
        maxLength: 500,
      },
      { type: "checkbox", id: "remind", label: "Remind me when it is due", default: true },
    ]);

    // The task: in the chosen list, linked to the message, no confirmation by default.
    expect([...host.tasks.values()]).toEqual([
      {
        id: "task-1",
        list: "home",
        title: "Follow up: Contract renewal",
        due: "2999-12-31",
        notes: "Call Ada",
        links: ["nail://messages/m1"],
        completed: false,
      },
    ]);
    expect(host.writes.map((w) => `${w.operation} ${w.kind} ${w.result.status}`)).toEqual([
      "nail.tasks.create task done",
    ]);
    expect(host.confirmations).toEqual([]);
    expect(host.storage.get("follow-up:m1")).toMatchObject({
      messageId: "m1",
      messageUri: "nail://messages/m1",
      subject: "Contract renewal",
      from: "Ada Lovelace <ada@example.org>",
      taskId: "task-1",
      due: "2999-12-31",
      note: "Call Ada",
      remind: true,
    });
    expect(host.notifications).toEqual([
      { title: "Follow-up task created, due 2999-12-31", level: "success" },
    ]);
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

    // Next time the dialog says a follow-up exists.
    answers.push({ button: "cancel", values: {} });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(stackOf(host.dialogs[1]?.body)[2]).toEqual({
      type: "text",
      style: "muted",
      value: "A follow-up task for this message is due 2999-12-31.",
    });
  });

  it("respects a denied or expired confirmation", async () => {
    let decision: "deny" | "expire" = "deny";
    const host = await load({ policies: { task: "ask" }, confirm: () => decision });
    for (const d of ["deny", "expire"] as const) {
      decision = d;
      answers.push({ button: "create", values: { due: "2999-01-01", note: "" } });
      await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    }
    expect(host.confirmations.map((c) => [c.kind, c.summary, c.decision])).toEqual([
      ["task", "Create the task “Follow up: Contract renewal”", "deny"],
      ["task", "Create the task “Follow up: Contract renewal”", "expire"],
    ]);
    expect(host.tasks.size).toBe(0);
    expect(host.storage.size).toBe(0);
    expect(host.notifications).toEqual([
      { title: "The follow-up task was not created.", level: "info" },
      {
        title: "The follow-up task was not created: nobody confirmed it in time.",
        level: "info",
      },
    ]);
  });

  it("uses the default list with one list and the settings for the dialog", async () => {
    const host = await load({
      taskLists: [{ id: "only", name: "Tasks", default: true, readOnly: false }],
      settings: { "default-days": 0, remind: false },
    });
    answers.push({ button: "create", values: { due: "2999-02-03", note: "", remind: false } });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    const children = stackOf(host.dialogs[0]?.body);
    expect(children.find((c) => c.type === "select")).toBeUndefined();
    expect(children.find((c) => c.type === "date")).toMatchObject({ default: "today" });
    expect(children.find((c) => c.type === "checkbox")).toMatchObject({ default: false });
    expect([...host.tasks.values()][0]).toMatchObject({ list: "only", due: "2999-02-03" });
    expect(host.storage.get("follow-up:m1")).toMatchObject({ remind: false });
  });

  it("does nothing on cancel", async () => {
    const host = await load();
    answers.push({ button: "cancel", values: { due: "2999-12-31" } });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.tasks.size).toBe(0);
    expect(host.storage.size).toBe(0);
    expect(host.notifications).toEqual([]);
  });

  it("explains a missing selection, message or permission instead of crashing", async () => {
    let host = await load();
    await host.runCommand("follow-up", { selection: { messageIds: [] } });
    expect(host.notifications.at(-1)).toEqual({
      title: "Select a message to follow up on",
      level: "warning",
    });

    host = await load({ permissions: ["ui.dialogs", "notifications", "storage", "tasks.write"] });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.notifications).toEqual([
      { title: "This message cannot be read by the Follow up plug-in", level: "error" },
    ]);
    expect(host.logs[0]?.level).toBe("warn");

    host = await load({ permissions: ["mail.read", "ui.dialogs", "notifications", "storage"] });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.dialogs).toEqual([]);
    expect(host.notifications).toEqual([
      {
        title: "Follow up may not create tasks. Allow “tasks.write” in the plug-in manager.",
        level: "error",
      },
    ]);

    host = await load({ taskLists: lists.slice(2, 3) });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.notifications).toEqual([
      { title: "There is no task list to add the follow-up to.", level: "warning" },
    ]);
  });

  it("keeps the task when the reminder cannot be saved", async () => {
    const host = await load({ storageQuota: 10 });
    answers.push({ button: "create", values: { due: "2999-12-31", note: "" } });
    await host.runCommand("follow-up", { selection: { messageIds: ["m1"] } });
    expect(host.tasks.size).toBe(1);
    expect(host.notifications).toEqual([
      { title: "The task was created, but the reminder could not be saved", level: "warning" },
    ]);
  });

  it("reminds once when a follow-up is due, and not when switched off", async () => {
    const host = await load();
    const today = isoDate(new Date());
    const base = { messageUri: "", from: "", note: "", taskId: "t", remind: true, createdAt: "" };
    host.storage.set("follow-up:a", { ...base, messageId: "a", subject: "Old", due: "2000-01-01" });
    host.storage.set("follow-up:b", { ...base, messageId: "b", subject: "Now", due: today });
    host.storage.set("follow-up:c", {
      ...base,
      messageId: "c",
      subject: "Later",
      due: "2999-01-01",
    });
    host.storage.set("follow-up:d", {
      ...base,
      messageId: "d",
      subject: "Quiet",
      due: "2000-01-01",
      remind: false,
    });
    expect(
      await host.runJob("remind", { kind: "schedule", scheduledFor: `${today}T10:00:00Z` }),
    ).toEqual({ outcome: "ok" });
    expect(host.notifications).toEqual([
      { title: "Overdue follow-up (2000-01-01): Old", level: "info", system: true },
      { title: "Follow up today: Now", level: "info", system: true },
    ]);
    expect(host.storage.get("follow-up:a")).toMatchObject({ notifiedOn: today });
    // Run again (e.g. at the next start): nothing new.
    await host.runJob("remind", { kind: "event", event: "app.started", messageIds: [] });
    expect(host.notifications).toHaveLength(2);

    host.changeSettings({ remind: false });
    host.storage.set("follow-up:e", { ...base, messageId: "e", subject: "Off", due: "2000-01-01" });
    await host.runJob("remind");
    expect(host.notifications).toHaveLength(2);
  });

  it("lists follow-ups in its panel and forgets past ones", async () => {
    const host = await load();
    expect(await host.renderPanel("due")).toEqual([
      {
        type: "text",
        style: "muted",
        value: "No follow-ups. Use “Follow up…” on a message to add one.",
      },
    ]);
    const base = { messageUri: "", from: "", note: "", taskId: "t", remind: true, createdAt: "" };
    host.storage.set("follow-up:m2", {
      ...base,
      messageId: "m2",
      subject: "Old",
      due: "2000-01-01",
    });
    host.storage.set("follow-up:m1", {
      ...base,
      messageId: "m1",
      subject: "New",
      due: "2999-01-01",
    });
    host.storage.set("unrelated", 1);
    const stack = stackOf(await host.renderPanel("due"));
    expect(stack[0]).toEqual({ type: "text", style: "error", value: "One follow-up is overdue" });
    expect(stack[1]).toMatchObject({
      rows: [
        ["2000-01-01", "Old", ""],
        ["2999-01-01", "New", ""],
      ],
    });
    expect(stack[2]).toEqual({
      type: "button",
      id: "remove-past",
      label: "Forget past follow-ups",
    });
    await host.clickPanelButton("due", "remove-past");
    expect([...host.storage.keys()].sort()).toEqual(["follow-up:m1", "unrelated"]);
  });
});
