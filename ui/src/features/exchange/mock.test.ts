import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type {
  AutomaticRepliesSettings,
  Event as NailEvent,
  ExchangeCategory,
  ExchangeMailbox,
  Response,
} from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { queryKeys } from "../../state/queries";
import type { FeatureEventContext } from "../types";
import { EXCHANGE_EVENTS, exchangeKeys, handleExchangeEvent } from ".";
import { CATEGORY_COLORS, notMessages, runtimeIds } from "./messages";
import { EXCHANGE_REQUESTS, isExchangeRequest } from "./mock";
import {
  ADDABLE_MAILBOX,
  DENIED_MAILBOX,
  EXCHANGE_ACCOUNT,
  EXCHANGE_FOLDERS,
  SHARED_MAILBOX,
  SHARED_ROOT,
  seedExchangeDemo,
} from "./mockSeed";

function seeded(): MockTransport {
  const t = new MockTransport();
  seedExchangeDemo(t.data);
  t.data.behaviour.workMs = 0;
  return t;
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 5));

function events(t: MockTransport): NailEvent[] {
  const out: NailEvent[] = [];
  t.onEvent((e) => out.push(e));
  return out;
}

async function categories(t: MockTransport): Promise<ExchangeCategory[]> {
  const r = await t.send({ type: "ListExchangeCategories", account_id: EXCHANGE_ACCOUNT });
  if (r.type !== "ExchangeCategories") throw new Error(JSON.stringify(r));
  return r.categories;
}

async function mailboxes(t: MockTransport): Promise<ExchangeMailbox[]> {
  const r = await t.send({ type: "ListExchangeMailboxes", account_id: EXCHANGE_ACCOUNT });
  if (r.type !== "ExchangeMailboxes") throw new Error(JSON.stringify(r));
  return r.mailboxes;
}

function category(r: Response): ExchangeCategory {
  if (r.type !== "ExchangeCategorySaved") throw new Error(JSON.stringify(r));
  return r.category;
}

const replies = (patch: Partial<AutomaticRepliesSettings> = {}): AutomaticRepliesSettings => ({
  status: "always-enabled",
  start: null,
  end: null,
  internal_reply: "Out",
  external_reply: "Out",
  external_audience: "all",
  ...patch,
});

describe("exchange mock: routing and accounts", () => {
  it("routes exactly its requests", () => {
    expect(new Set(EXCHANGE_REQUESTS).size).toBe(EXCHANGE_REQUESTS.length);
    expect(EXCHANGE_REQUESTS).toHaveLength(18);
    expect(isExchangeRequest({ type: "GetExchangeStatus", account_id: "work" })).toBe(true);
    expect(
      isExchangeRequest({
        type: "ClassifyFocused",
        message_ids: ["m1"],
        classification: "other",
        always: false,
      }),
    ).toBe(true);
    expect(isExchangeRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(false);
  });

  it("answers unsupported like the core before WP95 when switched to", async () => {
    const t = seeded();
    t.data.features.exchange.unsupported = true;
    expect(await t.send({ type: "GetExchangeStatus", account_id: EXCHANGE_ACCOUNT })).toMatchObject(
      { type: "Error", code: "unsupported" },
    );
  });

  it("refuses unknown and non-Exchange accounts", async () => {
    const t = seeded();
    expect(await t.send({ type: "GetExchangeStatus", account_id: "nope" })).toMatchObject({
      code: "not_found",
    });
    expect(await t.send({ type: "ListExchangeCategories", account_id: "acc-work" })).toMatchObject({
      code: "invalid_request",
    });
    // An unseeded mock knows no Exchange account.
    const bare = new MockTransport();
    expect(bare.data.features.exchange.categories).toEqual({});
    expect(await bare.send({ type: "GetExchangeStatus", account_id: "work" })).toMatchObject({
      code: "not_found",
    });
  });

  it("reports the status with queued operations", async () => {
    const t = seeded();
    t.data.behaviour.workMs = 60_000;
    await t.send({
      type: "CreateExchangeCategory",
      account_id: EXCHANGE_ACCOUNT,
      settings: { name: "Later", color: "none" },
    });
    expect(await t.send({ type: "GetExchangeStatus", account_id: EXCHANGE_ACCOUNT })).toMatchObject(
      { status: { engine: "graph", pending_ops: 1, push: "polling" } },
    );
    t.close();
  });
});

describe("exchange mock: categories", () => {
  it("lists the master list with counts from the tags (case-insensitive)", async () => {
    const t = seeded();
    const list = await categories(t);
    expect(list.map((c) => c.name)).toEqual(["Customers", "Personal", "Project Phoenix", "Urgent"]);
    expect(list.find((c) => c.name === "Customers")).toMatchObject({ total: 1, unread: 1 });
    expect(list.find((c) => c.name === "Urgent")).toMatchObject({ total: 2, unread: 1 });
  });

  it("creates with a local id the replay replaces, and validates", async () => {
    const t = seeded();
    const seen = events(t);
    const created = category(
      await t.send({
        type: "CreateExchangeCategory",
        account_id: EXCHANGE_ACCOUNT,
        settings: { name: "Travel", color: "preset5" },
      }),
    );
    expect(created).toMatchObject({ name: "Travel", sync: "pending" });
    expect(created.id.startsWith("local-")).toBe(true);
    await tick();
    const after = (await categories(t)).find((c) => c.name === "Travel");
    expect(after?.sync).toBe("synced");
    expect(after?.id.startsWith("local-")).toBe(false);
    expect(seen.some((e) => e.type === "ExchangeChanged" && e.areas.includes("categories"))).toBe(
      true,
    );
    expect(seen.some((e) => e.type === "OpStatus" && e.status.state === "done")).toBe(true);
    expect(
      await t.send({
        type: "CreateExchangeCategory",
        account_id: EXCHANGE_ACCOUNT,
        settings: { name: "urgent", color: "none" },
      }),
    ).toMatchObject({
      code: "invalid_request",
      localized: { id: "exchange-category-issue-duplicate-name" },
    });
  });

  it("renames the tag on messages, and deleting leaves the tags", async () => {
    const t = seeded();
    const seen = events(t);
    const urgent = (await categories(t)).find((c) => c.name === "Urgent");
    if (!urgent) throw new Error("demo category missing");
    category(
      await t.send({
        type: "UpdateExchangeCategory",
        account_id: EXCHANGE_ACCOUNT,
        category_id: urgent.id,
        settings: { name: "Important", color: "preset1" },
      }),
    );
    const tagged = t.data.messages.filter((m) => m.tags.includes("Important"));
    expect(tagged).toHaveLength(2);
    expect(t.data.messages.some((m) => m.tags.includes("Urgent"))).toBe(false);
    expect(seen.some((e) => e.type === "MailChanged")).toBe(true);
    expect(
      await t.send({
        type: "DeleteExchangeCategory",
        account_id: EXCHANGE_ACCOUNT,
        category_id: urgent.id,
      }),
    ).toMatchObject({ type: "Queued" });
    expect((await categories(t)).some((c) => c.name === "Important")).toBe(false);
    expect(t.data.messages.filter((m) => m.tags.includes("Important"))).toHaveLength(2);
  });
});

describe("exchange mock: mailboxes", () => {
  it("adds a mailbox that syncs into a folder tree under @address", async () => {
    const t = seeded();
    const r = await t.send({
      type: "AddExchangeMailbox",
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: ADDABLE_MAILBOX,
        name: null,
        kind: "shared",
        mail: true,
        calendar: false,
        contacts: false,
        tasks: false,
      },
    });
    expect(r).toMatchObject({ type: "ExchangeMailboxSaved", mailbox: { state: "pending" } });
    await tick();
    const sales = (await mailboxes(t)).find((m) => m.settings.address === ADDABLE_MAILBOX);
    expect(sales).toMatchObject({ state: "synced", settings: { name: "Sales" } });
    const folders = t.data.folders.filter((f) => f.path.startsWith(`@${ADDABLE_MAILBOX}`));
    expect(folders.map((f) => f.path)).toEqual([
      `@${ADDABLE_MAILBOX}`,
      `@${ADDABLE_MAILBOX}/Inbox`,
      `@${ADDABLE_MAILBOX}/Leads`,
    ]);
    expect(sales?.root_folder_id).toBe(folders[0]?.id);
  });

  it("reports unknown mailboxes and validates", async () => {
    const t = seeded();
    const base = {
      name: null,
      kind: "shared" as const,
      mail: true,
      calendar: false,
      contacts: false,
      tasks: false,
    };
    await t.send({
      type: "AddExchangeMailbox",
      account_id: EXCHANGE_ACCOUNT,
      settings: { ...base, address: "ghost@contoso.example" },
    });
    await tick();
    expect(
      (await mailboxes(t)).find((m) => m.settings.address === "ghost@contoso.example"),
    ).toMatchObject({
      state: "not-found",
      error: "not_found",
    });
    expect(
      await t.send({
        type: "AddExchangeMailbox",
        account_id: EXCHANGE_ACCOUNT,
        settings: { ...base, address: SHARED_MAILBOX.toUpperCase() },
      }),
    ).toMatchObject({ localized: { id: "exchange-mailbox-issue-duplicate" } });
    expect(
      await t.send({
        type: "AddExchangeMailbox",
        account_id: EXCHANGE_ACCOUNT,
        settings: { ...base, address: "ada@contoso.example" },
      }),
    ).toMatchObject({ localized: { id: "exchange-mailbox-issue-own-mailbox" } });
  });

  it("hides an automapped mailbox and removes an added one", async () => {
    const t = seeded();
    expect(
      await t.send({
        type: "RemoveExchangeMailbox",
        account_id: EXCHANGE_ACCOUNT,
        address: SHARED_MAILBOX,
      }),
    ).toMatchObject({ type: "Done" });
    expect((await mailboxes(t)).some((m) => m.settings.address === SHARED_MAILBOX)).toBe(false);
    expect(t.data.folders.some((f) => f.id === SHARED_ROOT)).toBe(false);
    expect(t.data.features.exchange.hiddenMailboxes[EXCHANGE_ACCOUNT]).toEqual([SHARED_MAILBOX]);
    await t.send({
      type: "RemoveExchangeMailbox",
      account_id: EXCHANGE_ACCOUNT,
      address: DENIED_MAILBOX,
    });
    expect(
      t.data.features.exchange.mailboxes[EXCHANGE_ACCOUNT]?.some(
        (m) => m.settings.address === DENIED_MAILBOX,
      ),
    ).toBe(false);
    // Adding the hidden one shows it again.
    await t.send({
      type: "AddExchangeMailbox",
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: SHARED_MAILBOX,
        name: "Support",
        kind: "shared",
        mail: true,
        calendar: false,
        contacts: false,
        tasks: false,
      },
    });
    await tick();
    expect((await mailboxes(t)).find((m) => m.settings.address === SHARED_MAILBOX)?.state).toBe(
      "synced",
    );
  });

  it("updates what a mailbox shows", async () => {
    const t = seeded();
    const r = await t.send({
      type: "UpdateExchangeMailbox",
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: SHARED_MAILBOX,
        name: "Helpdesk",
        kind: "shared",
        mail: false,
        calendar: true,
        contacts: false,
        tasks: false,
      },
    });
    expect(r).toMatchObject({ mailbox: { settings: { name: "Helpdesk", mail: false } } });
    expect(t.data.folders.some((f) => f.id === SHARED_ROOT)).toBe(false);
  });
});

describe("exchange mock: Focused Inbox, replies, rules, rooms", () => {
  it("moves messages between Focused and Other, with sender overrides", async () => {
    const t = seeded();
    const digest = t.data.messages.find((m) => m.subject === "Your weekly digest");
    if (!digest) throw new Error("demo message missing");
    expect(
      await t.send({
        type: "ClassifyFocused",
        message_ids: [digest.id],
        classification: "focused",
        always: true,
      }),
    ).toMatchObject({ type: "Queued" });
    expect(digest.exchange?.inference).toBe("focused");
    expect(t.data.features.exchange.focusOverrides["news@digest.example"]).toBe("focused");
    const other = t.data.messages.find((m) => m.account_id === "acc-work");
    expect(
      await t.send({
        type: "ClassifyFocused",
        message_ids: [other?.id ?? ""],
        classification: "other",
        always: false,
      }),
    ).toMatchObject({ code: "invalid_request" });
  });

  it("queues automatic replies and validates them", async () => {
    const t = seeded();
    const r = await t.send({
      type: "SetAutomaticReplies",
      account_id: EXCHANGE_ACCOUNT,
      settings: replies(),
    });
    expect(r).toMatchObject({ type: "AutomaticReplies", replies: { sync: "pending" } });
    await tick();
    expect(
      await t.send({ type: "GetAutomaticReplies", account_id: EXCHANGE_ACCOUNT }),
    ).toMatchObject({ replies: { sync: "synced", settings: { status: "always-enabled" } } });
    expect(
      await t.send({
        type: "SetAutomaticReplies",
        account_id: EXCHANGE_ACCOUNT,
        settings: replies({ status: "scheduled" }),
      }),
    ).toMatchObject({ localized: { id: "exchange-oof-issue-missing-schedule" } });
  });

  it("places expressible Nail rules and switches / deletes server rules", async () => {
    const t = seeded();
    const r = await t.send({ type: "GetExchangeRules", account_id: EXCHANGE_ACCOUNT });
    if (r.type !== "ExchangeRules") throw new Error(JSON.stringify(r));
    // The sample rule has a query condition: it stays in Nail.
    expect(r.status.rules).toEqual([
      {
        rule_id: "rust-announcements",
        placement: "client",
        server_rule_id: null,
        reasons: [{ id: "sieve-reason-query", args: { position: 2 } }],
      },
    ]);
    expect(
      await t.send({
        type: "SetExchangeInboxRuleEnabled",
        account_id: EXCHANGE_ACCOUNT,
        server_rule_id: "inbox-rule-3",
        enabled: true,
      }),
    ).toMatchObject({ type: "Queued" });
    await t.send({
      type: "DeleteExchangeInboxRule",
      account_id: EXCHANGE_ACCOUNT,
      server_rule_id: "inbox-rule-2",
    });
    const sync = await t.send({ type: "SyncExchangeRules", account_id: EXCHANGE_ACCOUNT });
    expect(sync).toMatchObject({ status: { state: "syncing" } });
    await tick();
    const after = await t.send({ type: "GetExchangeRules", account_id: EXCHANGE_ACCOUNT });
    if (after.type !== "ExchangeRules") throw new Error(JSON.stringify(after));
    expect(after.status.state).toBe("in-sync");
    expect(after.status.server_rules.map((s) => [s.id, s.enabled])).toEqual([
      ["inbox-rule-1", true],
      ["inbox-rule-3", true],
    ]);
  });

  it("lists room lists and finds rooms with availability, free first", async () => {
    const t = seeded();
    expect(
      await t.send({ type: "ListRoomLists", account_id: EXCHANGE_ACCOUNT, refresh: false }),
    ).toMatchObject({ lists: [{ name: "Building 1" }, { name: "Building 2" }] });
    const r = await t.send({
      type: "FindRooms",
      account_id: EXCHANGE_ACCOUNT,
      room_list: null,
      from: "2026-10-01T09:00:00Z",
      to: "2026-10-01T09:30:00Z",
      min_capacity: null,
      refresh: false,
    });
    if (r.type !== "Rooms") throw new Error(JSON.stringify(r));
    expect(r.rooms.map((x) => [x.room.name, x.free])).toEqual([
      ["Turing room", true],
      ["Ada room", false],
      ["Babbage hall", false],
      ["Hopper lab", null],
    ]);
    expect(r.rooms[1]?.periods).toEqual([
      { start: "2026-10-01T09:00:00Z", end: "2026-10-01T09:30:00Z", kind: "busy" },
    ]);
    const big = await t.send({
      type: "FindRooms",
      account_id: EXCHANGE_ACCOUNT,
      room_list: "building1@contoso.example",
      from: "2026-10-01T12:00:00Z",
      to: "2026-10-01T13:00:00Z",
      min_capacity: 10,
      refresh: false,
    });
    expect(big).toMatchObject({ rooms: [{ room: { name: "Babbage hall" }, free: false }] });
    expect(
      await t.send({
        type: "FindRooms",
        account_id: EXCHANGE_ACCOUNT,
        room_list: null,
        from: "2026-10-01T12:00:00Z",
        to: "2026-10-01T11:00:00Z",
        min_capacity: null,
        refresh: false,
      }),
    ).toMatchObject({ localized: { id: "freebusy-issue-invalid-range" } });
    t.data.features.exchange.roomsForbidden.push(EXCHANGE_ACCOUNT);
    expect(
      await t.send({ type: "ListRoomLists", account_id: EXCHANGE_ACCOUNT, refresh: true }),
    ).toMatchObject({
      code: "forbidden",
      localized: { id: "exchange-warning-rooms-need-consent" },
    });
  });

  it("seeds folders, messages and the PIM source", () => {
    const t = seeded();
    expect(t.data.folders.find((f) => f.id === EXCHANGE_FOLDERS.inbox)).toMatchObject({
      total: 5,
      unread: 3,
    });
    expect(t.data.features.pim.sources.some((s) => s.id === "exchange:acc-exchange")).toBe(true);
    expect(
      t.data.features.pim.calendars.filter((c) => c.exchange?.todo).map((c) => c.settings.name),
    ).toEqual(["Tasks", "Groceries", "Flagged email"]);
  });
});

describe("exchange events and texts", () => {
  it("refetches the changed areas of one account, everything on mail events", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleExchangeEvent(
      { type: "ExchangeChanged", account_id: "work", areas: ["categories", "rules"] },
      ctx,
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: exchangeKeys.area("work", "categories"),
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: exchangeKeys.area("work", "rules") });
    invalidate.mockClear();
    handleExchangeEvent({ type: "ExchangeChanged", account_id: "work", areas: ["mailboxes"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.folders("work") });
    invalidate.mockClear();
    handleExchangeEvent({ type: "MailChanged", folder_ids: ["f1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: exchangeKeys.all });
    invalidate.mockClear();
    handleExchangeEvent({ type: "PimSourcesChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(EXCHANGE_EVENTS).toContain("ExchangeChanged");
  });

  it("lists the core's and the UI's runtime texts", () => {
    expect(CATEGORY_COLORS).toHaveLength(26);
    expect(CATEGORY_COLORS[25]).toBe("preset24");
    const ids = runtimeIds();
    expect(ids).toContain("exchange-warning-ews-retired-online");
    expect(ids).toContain("exchange-focus-other");
    expect(ids).toContain("exchange-color-preset24");
    expect(ids).toContain("exchange-oof-issue-end-before-start");
    expect(ids).toContain("exchange-room-response-needs-action");
    expect(ids).toContain("window-exchange-automatic-replies");
    expect(notMessages).toContain("exchange-mailbox-editor");
  });
});
