import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { GmailLabel, GmailLabelSettings, Response } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { GMAIL_EVENTS, gmailKeys, handleGmailEvent } from ".";
import { runtimeIds } from "./messages";
import { GMAIL_REQUESTS, isGmailRequest } from "./mock";
import { GMAIL_ACCOUNT, seedGmailDemo } from "./mockSeed";

function seeded(): MockTransport {
  const t = new MockTransport();
  seedGmailDemo(t.data);
  t.data.behaviour.workMs = 0;
  return t;
}

async function labels(t: MockTransport): Promise<GmailLabel[]> {
  const r = await t.send({ type: "ListGmailLabels", account_id: GMAIL_ACCOUNT });
  if (r.type !== "GmailLabels") throw new Error(JSON.stringify(r));
  return r.labels;
}

const settings = (name: string, extra: Partial<GmailLabelSettings> = {}): GmailLabelSettings => ({
  name,
  color: null,
  list_visibility: "show",
  message_visibility: "show",
  ...extra,
});

function saved(r: Response): GmailLabel {
  if (r.type !== "GmailLabelSaved") throw new Error(JSON.stringify(r));
  return r.label;
}

describe("gmail mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(GMAIL_REQUESTS).size).toBe(GMAIL_REQUESTS.length);
    expect(GMAIL_REQUESTS).toHaveLength(4);
    expect(isGmailRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(true);
    expect(isGmailRequest({ type: "GetSchedulingSettings" })).toBe(false);
  });

  it("answers unsupported like the core before WP85 when switched to", async () => {
    const t = seeded();
    t.data.features.gmail.unsupported = true;
    expect(await t.send({ type: "ListGmailLabels", account_id: GMAIL_ACCOUNT })).toMatchObject({
      type: "Error",
      code: "unsupported",
    });
  });

  it("refuses unknown and non-Gmail accounts", async () => {
    const t = seeded();
    expect(await t.send({ type: "ListGmailLabels", account_id: "gm" })).toMatchObject({
      code: "not_found",
    });
    expect(await t.send({ type: "ListGmailLabels", account_id: "acc-work" })).toMatchObject({
      code: "invalid_request",
    });
  });

  it("refetches one account's labels on label events, everything on mail events", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleGmailEvent({ type: "GmailLabelsChanged", account_id: "gm" }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: gmailKeys.labels("gm") });
    invalidate.mockClear();
    handleGmailEvent({ type: "MailChanged", folder_ids: ["f1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: gmailKeys.all });
    invalidate.mockClear();
    handleGmailEvent({ type: "PimSourcesChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(GMAIL_EVENTS).toContain("GmailLabelsChanged");
  });

  it("lists the core's label and category texts", () => {
    expect(runtimeIds()).toContain("gmail-label-issue-duplicate-name");
    expect(runtimeIds()).toContain("gmail-category-promotions");
    expect(runtimeIds()).toContain("gmail-color-light-blue");
  });

  it("lists system labels first with counts from the messages", async () => {
    const t = seeded();
    const list = await labels(t);
    expect(list.slice(0, 5).map((l) => l.id)).toEqual(["INBOX", "SENT", "DRAFT", "SPAM", "TRASH"]);
    expect(list[5]?.id).toBe("CATEGORY_PERSONAL");
    const user = list.filter((l) => l.kind === "user").map((l) => l.name);
    expect(user).toEqual([...user].sort((a, b) => a.localeCompare(b)));
    expect(list.find((l) => l.id === "INBOX")).toMatchObject({ total: 8, unread: 5 });
    expect(list.find((l) => l.name === "Travel")).toMatchObject({ total: 2, unread: 1 });
    expect(list.find((l) => l.id === "CATEGORY_SOCIAL")).toMatchObject({ total: 1, unread: 1 });
  });

  it("creates a label with a local id that the replay replaces", async () => {
    const t = seeded();
    const events: string[] = [];
    t.onEvent((e) => events.push(e.type));
    const label = saved(
      await t.send({
        type: "CreateGmailLabel",
        account_id: GMAIL_ACCOUNT,
        settings: settings("Family", { color: { background: "#fb4c2f", text: "#ffffff" } }),
      }),
    );
    expect(label).toMatchObject({ name: "Family", tag: "Family", sync: "pending" });
    expect(label.id.startsWith("local-")).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 5));
    const after = (await labels(t)).find((l) => l.name === "Family");
    expect(after?.id.startsWith("Label_")).toBe(true);
    expect(after?.sync).toBe("synced");
    expect(events.filter((e) => e === "GmailLabelsChanged").length).toBe(2);
    // The old local id still finds the label.
    expect(
      await t.send({
        type: "UpdateGmailLabel",
        account_id: GMAIL_ACCOUNT,
        label_id: label.id,
        settings: settings("Family"),
      }),
    ).toMatchObject({ type: "GmailLabelSaved" });
  });

  it("validates like the core", async () => {
    const t = seeded();
    const create = (s: GmailLabelSettings) =>
      t.send({ type: "CreateGmailLabel", account_id: GMAIL_ACCOUNT, settings: s });
    expect(await create(settings(" "))).toMatchObject({
      localized: { id: "gmail-label-issue-missing-name" },
    });
    expect(await create(settings("travel"))).toMatchObject({
      localized: { id: "gmail-label-issue-duplicate-name" },
    });
    expect(await create(settings("INBOX"))).toMatchObject({
      localized: { id: "gmail-label-issue-invalid-name" },
    });
    expect(await create(settings("a//b"))).toMatchObject({
      localized: { id: "gmail-label-issue-invalid-name" },
    });
    expect(await create(settings("x".repeat(226)))).toMatchObject({
      localized: { id: "gmail-label-issue-name-too-long" },
    });
    expect(
      await create(settings("Red", { color: { background: "red", text: "#ffffff" } })),
    ).toMatchObject({ localized: { id: "gmail-label-issue-invalid-color" } });
  });

  it("renaming moves nested labels and renames the tags on messages", async () => {
    const t = seeded();
    const work = (await labels(t)).find((l) => l.name === "Work");
    if (!work) throw new Error("no Work label");
    const events: string[] = [];
    t.onEvent((e) => events.push(e.type));
    saved(
      await t.send({
        type: "UpdateGmailLabel",
        account_id: GMAIL_ACCOUNT,
        label_id: work.id,
        settings: settings("Office"),
      }),
    );
    const names = (await labels(t)).filter((l) => l.kind === "user").map((l) => l.name);
    expect(names).toContain("Office/Projects");
    expect(names).not.toContain("Work");
    const tags = t.data.messages
      .filter((m) => m.account_id === GMAIL_ACCOUNT)
      .flatMap((m) => m.tags);
    expect(tags).toContain("Office/Projects");
    expect(tags).not.toContain("Work/Projects");
    expect(events).toContain("MailChanged");
  });

  it("deleting removes the tag but keeps nested labels; system labels stay", async () => {
    const t = seeded();
    const list = await labels(t);
    const work = list.find((l) => l.name === "Work");
    if (!work) throw new Error("no Work label");
    expect(
      await t.send({ type: "DeleteGmailLabel", account_id: GMAIL_ACCOUNT, label_id: work.id }),
    ).toEqual({ type: "Done" });
    const after = await labels(t);
    expect(after.some((l) => l.name === "Work")).toBe(false);
    expect(after.some((l) => l.name === "Work/Projects")).toBe(true);
    expect(t.data.messages.some((m) => m.tags.includes("Work"))).toBe(false);
    expect(
      await t.send({ type: "DeleteGmailLabel", account_id: GMAIL_ACCOUNT, label_id: "INBOX" }),
    ).toMatchObject({ code: "invalid_request" });
    expect(
      await t.send({
        type: "UpdateGmailLabel",
        account_id: GMAIL_ACCOUNT,
        label_id: "nope",
        settings: settings("x"),
      }),
    ).toMatchObject({ code: "not_found" });
  });

  it("a new tag on a Gmail message becomes a label", async () => {
    const t = seeded();
    const message = t.data.messages.find((m) => m.account_id === GMAIL_ACCOUNT);
    if (!message) throw new Error("no message");
    await t.send({
      type: "UpdateFlags",
      message_ids: [message.id],
      change: {
        seen: null,
        flagged: null,
        answered: null,
        draft: null,
        add_tags: ["Ideas"],
        remove_tags: [],
      },
    });
    expect((await labels(t)).find((l) => l.name === "Ideas")).toMatchObject({
      kind: "user",
      total: 1,
    });
  });
});
