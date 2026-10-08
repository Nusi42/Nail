import { describe, expect, it } from "vitest";
import type {
  AutomaticRepliesSettings,
  Calendar,
  DiscoveredAccount,
  ExchangeCategorySettings,
  ExchangeMailboxSettings,
} from "../../api/generated";
import exchangeVectors from "../../../../crates/nail-api/fixtures/ts-vectors/exchange.json";
import { describeVectors } from "../../test/vectors";
import {
  addRoom,
  automaticRepliesIssues,
  categoryIssues,
  categoryRgb,
  categoryTextRgb,
  defaultWindowsLogin,
  exchangeCandidate,
  exchangeCredentialsReady,
  exchangeSecretsFor,
  exchangeSignInOf,
  exchangeSignIns,
  exchangeWarnings,
  focusOfQuery,
  focusQuery,
  groupTaskLists,
  isExchangeOnlineHost,
  isPlausibleEmail,
  mailboxIssues,
  mailboxOfFolder,
  mailboxOfPath,
  newMailboxSettings,
  overlapping,
  redirectedHost,
  removeRoom,
  repliesActive,
  repliesUpcoming,
  roomState,
  tagQuery,
  withEngine,
  withExchangeSignIn,
} from "./model";

const replies = (patch: Partial<AutomaticRepliesSettings> = {}): AutomaticRepliesSettings => ({
  status: "always-enabled",
  start: null,
  end: null,
  internal_reply: "Away",
  external_reply: "Away",
  external_audience: "all",
  ...patch,
});

describe("categories", () => {
  it("maps Outlook's presets to their colours", () => {
    expect(categoryRgb("none")).toBeNull();
    expect(categoryRgb("preset0")).toBe("#e7a1a2");
    expect(categoryRgb("preset24")).toBe("#93446b");
    // Light presets get black text, dark ones white.
    expect(categoryTextRgb("preset3")).toBe("#000000");
    expect(categoryTextRgb("preset14")).toBe("#ffffff");
    expect(categoryTextRgb("none")).toBeNull();
  });

  it("mirrors the contract's category issues plus duplicates", () => {
    expect(categoryIssues({ name: "  ", color: "none" }, [])).toEqual(["missing-name"]);
    expect(categoryIssues({ name: "a,b", color: "none" }, [])).toEqual(["invalid-name"]);
    expect(categoryIssues({ name: "a;b", color: "none" }, [])).toEqual(["invalid-name"]);
    expect(categoryIssues({ name: "x".repeat(256), color: "none" }, [])).toEqual(["name-too-long"]);
    expect(categoryIssues({ name: "urgent", color: "preset0" }, [{ name: "Urgent" }])).toEqual([
      "duplicate-name",
    ]);
    expect(categoryIssues({ name: "Fine", color: "preset0" }, [{ name: "Other" }])).toEqual([]);
  });

  it("quotes tag views", () => {
    expect(tagQuery("Project Phoenix")).toBe('tag:"Project Phoenix"');
    expect(tagQuery('say "hi"')).toBe('tag:"say \\"hi\\""');
  });
});

describe("Focused Inbox queries", () => {
  it("round-trips the tab queries", () => {
    expect(focusQuery("focused")).toBe("focus:focused");
    expect(focusOfQuery("focus:other")).toBe("other");
    expect(focusOfQuery(" focus:focused ")).toBe("focused");
    expect(focusOfQuery("focus:other from:x")).toBeNull();
    expect(focusOfQuery(null)).toBeNull();
  });
});

describe("mailboxes", () => {
  it("mirrors the contract's mailbox issues", () => {
    const base = { ...newMailboxSettings(), address: "support@x.example" };
    expect(mailboxIssues(base, "me@x.example", [])).toEqual([]);
    expect(mailboxIssues({ ...base, address: "" }, "me@x.example", [])).toEqual([
      "missing-address",
    ]);
    expect(mailboxIssues({ ...base, address: "nope" }, "me@x.example", [])).toEqual([
      "invalid-address",
    ]);
    expect(mailboxIssues({ ...base, address: "ME@x.example" }, "me@x.example", [])).toEqual([
      "own-mailbox",
    ]);
    expect(
      mailboxIssues(base, "me@x.example", [
        { settings: { ...base, address: "SUPPORT@x.example" } },
      ]),
    ).toEqual(["duplicate"]);
    expect(mailboxIssues({ ...base, name: " " }, "me@x.example", [])).toEqual(["empty-name"]);
    expect(mailboxIssues({ ...base, mail: false }, "me@x.example", [])).toEqual(["nothing-shown"]);
  });

  it("checks addresses like the core", () => {
    expect(isPlausibleEmail("a@b")).toBe(true);
    expect(isPlausibleEmail("@b")).toBe(false);
    expect(isPlausibleEmail("a@.b")).toBe(false);
    expect(isPlausibleEmail("a b@c")).toBe(false);
  });

  it("finds the mailbox of a folder path", () => {
    expect(mailboxOfPath("@Support@X.example/Inbox")).toBe("support@x.example");
    expect(mailboxOfPath("@support@x.example")).toBe("support@x.example");
    expect(mailboxOfPath("Inbox")).toBeNull();
    const mailbox = {
      account_id: "a",
      settings: { ...newMailboxSettings(), address: "support@x.example" },
      origin: "user" as const,
      state: "synced" as const,
      root_folder_id: null,
      calendar_ids: [],
      address_book_ids: [],
      error: null,
    };
    expect(mailboxOfFolder({ path: "@support@x.example/Inbox" }, [mailbox])).toBe(mailbox);
    expect(mailboxOfFolder({ path: "Inbox" }, [mailbox])).toBeNull();
  });
});

describe("automatic replies", () => {
  it("mirrors the contract's issues", () => {
    expect(automaticRepliesIssues(replies())).toEqual([]);
    expect(automaticRepliesIssues(replies({ status: "disabled", internal_reply: "" }))).toEqual([]);
    expect(automaticRepliesIssues(replies({ internal_reply: " " }))).toEqual([
      "empty-internal-reply",
    ]);
    expect(automaticRepliesIssues(replies({ external_reply: "" }))).toEqual([
      "empty-external-reply",
    ]);
    expect(
      automaticRepliesIssues(replies({ external_reply: "", external_audience: "none" })),
    ).toEqual([]);
    expect(automaticRepliesIssues(replies({ internal_reply: "x".repeat(32_001) }))).toEqual([
      "reply-too-long",
    ]);
    expect(automaticRepliesIssues(replies({ status: "scheduled" }))).toEqual(["missing-schedule"]);
    expect(
      automaticRepliesIssues(
        replies({
          status: "scheduled",
          start: "2026-10-02T08:00:00Z",
          end: "2026-10-01T08:00:00Z",
        }),
      ),
    ).toEqual(["end-before-start"]);
    expect(automaticRepliesIssues(replies({ start: "2026-10-02 08:00" }))).toEqual([
      "invalid-time",
    ]);
  });

  it("knows when replies go out", () => {
    const at = Date.parse("2026-10-01T12:00:00Z");
    expect(repliesActive(replies(), at)).toBe(true);
    expect(repliesActive(replies({ status: "disabled" }), at)).toBe(false);
    const scheduled = replies({
      status: "scheduled",
      start: "2026-10-01T08:00:00Z",
      end: "2026-10-02T08:00:00Z",
    });
    expect(repliesActive(scheduled, at)).toBe(true);
    expect(repliesActive(scheduled, Date.parse("2026-10-02T08:00:00Z"))).toBe(false);
    expect(repliesUpcoming(scheduled, Date.parse("2026-09-30T08:00:00Z"))).toBe(true);
    expect(repliesUpcoming(scheduled, at)).toBe(false);
  });
});

describe("account setup", () => {
  const ews = (host: string, url: string | null = null): DiscoveredAccount => {
    const c = exchangeCandidate("ada@contoso.example", "ews", host);
    return {
      ...c,
      source: "autodiscover",
      incoming:
        c.incoming.protocol === "exchange"
          ? { ...c.incoming, options: { ...c.incoming.options, url } }
          : c.incoming,
    };
  };

  it("builds candidates per engine", () => {
    const graph = exchangeCandidate("ada@contoso.example", "graph");
    expect(graph.incoming).toMatchObject({
      protocol: "exchange",
      server: { host: "graph.microsoft.com", port: 443, tls: "implicit" },
      options: { engine: "graph", url: null },
    });
    expect(graph.incoming.server.auth).toMatchObject({ method: "oauth2", provider: "microsoft" });
    expect(graph.outgoing).toBeNull();
    const server = exchangeCandidate("ada@contoso.example", "ews");
    expect(server.incoming.server.host).toBe("mail.contoso.example");
    expect(server.incoming.server.auth).toEqual({
      method: "windows",
      username: null,
      secret: null,
      package: null,
    });
  });

  it("warns about redirects outside the domain, not about Microsoft or sub-domains", () => {
    expect(redirectedHost("ada@contoso.example", ews("mail.contoso.example"))).toBeNull();
    expect(
      redirectedHost(
        "ada@contoso.example",
        ews("x", "https://outlook.office365.com/EWS/Exchange.asmx"),
      ),
    ).toBeNull();
    expect(
      redirectedHost(
        "ada@contoso.example",
        ews("x", "https://mail.hoster.example/EWS/Exchange.asmx"),
      ),
    ).toBe("mail.hoster.example");
    expect(
      redirectedHost("ada@contoso.example", {
        ...ews("mail.contoso.example"),
        warnings: ["foreign-hosts"],
      }),
    ).toBe("mail.contoso.example");
    const imap = { ...ews("x"), incoming: { ...exchangeCandidate("a@b", "graph").incoming } };
    expect(redirectedHost("ada@contoso.example", imap)).toBeNull();
  });

  it("warns about EWS against Exchange Online", () => {
    expect(isExchangeOnlineHost("outlook.office365.com")).toBe(true);
    expect(isExchangeOnlineHost("mail.contoso.example")).toBe(false);
    expect(exchangeWarnings(ews("outlook.office365.com").incoming)).toEqual(["ews-retired-online"]);
    expect(exchangeWarnings(ews("mail.contoso.example").incoming)).toEqual([]);
  });

  it("switches engines with the sign-in each needs", () => {
    const server = ews("mail.contoso.example", "https://mail.contoso.example/EWS/Exchange.asmx");
    const graph = withEngine(server, "graph", "ada@contoso.example");
    expect(graph.incoming).toMatchObject({
      server: { host: "graph.microsoft.com", auth: { method: "oauth2" } },
      options: { engine: "graph", url: null },
    });
    const back = withEngine(graph, "ews", "ada@contoso.example");
    expect(back.incoming).toMatchObject({
      server: { host: "mail.contoso.example", auth: { method: "windows" } },
      options: { engine: "ews" },
    });
    expect(withEngine(graph, "graph", "x")).toBe(graph);
  });

  it("offers the sign-ins of each engine and applies them", () => {
    expect(exchangeSignIns("graph")).toEqual(["oauth2"]);
    expect(exchangeSignIns("ews")).toContain("windows-current");
    const current = { method: "windows" as const, username: null, secret: null, package: null };
    expect(exchangeSignInOf(current)).toBe("windows-current");
    const account = withExchangeSignIn(current, "windows-account", "ada@corp.example");
    expect(account).toEqual({
      method: "windows",
      username: "CORP\\ada",
      secret: null,
      package: null,
    });
    expect(exchangeSignInOf(account)).toBe("windows-account");
    expect(withExchangeSignIn(account, "password", "ada@corp.example")).toMatchObject({
      method: "password",
      username: "CORP\\ada",
    });
    expect(withExchangeSignIn(account, "oauth2", "ada@corp.example")).toMatchObject({
      method: "oauth2",
      provider: "microsoft",
      username: "ada@corp.example",
    });
    expect(defaultWindowsLogin("ada")).toBe("ada");
  });

  it("knows when credentials are ready and which secrets to send", () => {
    const current = { method: "windows" as const, username: null, secret: null, package: null };
    const account = { ...current, username: "CORP\\ada" };
    const oauth = exchangeCandidate("a@b", "graph").incoming.server.auth;
    expect(exchangeCredentialsReady(current, "", false)).toBe(true);
    expect(exchangeCredentialsReady(account, "", false)).toBe(false);
    expect(exchangeCredentialsReady(account, "pw", false)).toBe(true);
    expect(exchangeCredentialsReady(oauth, "", true)).toBe(true);
    const config = (auth: typeof current | typeof account | typeof oauth) => ({
      incoming: {
        ...exchangeCandidate("a@b", "ews").incoming,
        server: { host: "h", port: 443, tls: "implicit" as const, auth },
      },
    });
    expect(exchangeSecretsFor(config(current), "pw", null)).toEqual([]);
    expect(exchangeSecretsFor(config(account), "pw", null)).toEqual([
      { endpoint: "incoming", value: { kind: "password", password: "pw" } },
    ]);
    expect(exchangeSecretsFor(config(oauth), "", "flow-1")).toEqual([
      { endpoint: "incoming", value: { kind: "oauth2", flow_id: "flow-1" } },
    ]);
    expect(exchangeSecretsFor(config(oauth), "", null)).toEqual([]);
  });
});

describe("rooms", () => {
  const room = { address: "Room@x.example", name: "Room" };

  it("adds a room once and removes it", () => {
    const once = addRoom([], room);
    expect(once).toHaveLength(1);
    expect(once[0]).toMatchObject({
      email: "Room@x.example",
      kind: "room",
      role: "non-participant",
    });
    expect(addRoom(once, { ...room, address: "room@x.example" })).toHaveLength(1);
    expect(removeRoom(once, "ROOM@x.example")).toEqual([]);
  });

  it("reads availability", () => {
    const full = {
      room: {
        ...room,
        capacity: null,
        building: null,
        floor: null,
        accessible: null,
        features: [],
        room_list: null,
      },
    };
    expect(roomState({ ...full, free: true, periods: [], error: null })).toBe("free");
    expect(roomState({ ...full, free: false, periods: [], error: null })).toBe("busy");
    expect(roomState({ ...full, free: null, periods: [], error: null })).toBe("unknown");
    expect(roomState({ ...full, free: true, periods: [], error: "network" })).toBe("unknown");
    const periods = [
      { start: "2026-10-01T08:00:00Z", end: "2026-10-01T09:00:00Z", kind: "busy" as const },
      { start: "2026-10-01T10:00:00Z", end: "2026-10-01T11:00:00Z", kind: "busy" as const },
    ];
    expect(overlapping(periods, "2026-10-01T08:30:00Z", "2026-10-01T10:00:00Z")).toHaveLength(1);
  });
});

describe("task lists", () => {
  const list = (
    id: string,
    exchange?: Calendar["exchange"],
    account: string | null = "acc",
  ): Calendar => ({
    id,
    source_id: exchange ? `exchange:${String(account)}` : "local",
    account_id: exchange ? account : null,
    settings: {
      name: id,
      description: null,
      color: null,
      visible: true,
      reminders: true,
      default_alarms: [],
      order: 0,
    },
    components: ["task"],
    read_only: false,
    timezone: null,
    ...(exchange ? { exchange } : {}),
  });

  it("groups To Do lists per account, mailboxes, then the rest", () => {
    const groups = groupTaskLists([
      list("local"),
      list("flagged", { mailbox: null, role: "flagged-emails", todo: true }),
      list("groceries", { mailbox: null, role: null, todo: true }),
      list("tasks", { mailbox: null, role: "default-tasks", todo: true }),
      list("boss", { mailbox: "boss@x.example", role: "default-tasks", todo: false }),
    ]);
    expect(groups.map((g) => [g.kind, g.key, g.lists.map((l) => l.id)])).toEqual([
      ["todo", "acc", ["tasks", "groceries", "flagged"]],
      ["mailbox", "boss@x.example", ["boss"]],
      ["other", null, ["local"]],
    ]);
  });
});

describeVectors("exchange", exchangeVectors, {
  categoryIssues: (settings: ExchangeCategorySettings) => categoryIssues(settings, []),
  mailboxIssues: (settings: ExchangeMailboxSettings, own: string) =>
    mailboxIssues(settings, own, []),
  isPlausibleEmail,
  automaticRepliesIssues,
});
