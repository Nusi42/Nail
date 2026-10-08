/**
 * ADR 0190: the connection log viewer (filters, pause, live entries, copy,
 * export, clear), the error details window, "Show details" in the status
 * bar and the settings (account problems, disk logging, application log).
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ConnectionLog, ConnectionLogEntry, ConnectionLogScope } from "../../api/generated";
import { WindowLayer } from "../../layout/WindowLayer";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  createEnv,
  renderWithProviders,
  settleQueries,
  widget,
  type Env,
} from "../../test/harness";
import { must } from "../../test/must";
import { appendMockLines, failSyncWith, selectFailure } from "./mock";
import {
  ALL_ENTRIES,
  WIRE_PROTOCOLS,
  entryAt,
  filterEntries,
  mergeLog,
  protocolsIn,
  scopeOfViewerWindow,
  viewerWindowOf,
} from "./model";
import { handleConnlogEvent } from "./queries";
import { connlogStoreFor, openConnectionLog, openErrorDetails } from "./store";

const WORK: ConnectionLogScope = { kind: "account", account_id: "acc-work" };

function entry(seq: number, extra: Partial<ConnectionLogEntry> = {}): ConnectionLogEntry {
  return {
    seq,
    at: "2026-10-08T09:12:01.123Z",
    protocol: "imap",
    connection: 1,
    direction: "sent",
    level: "info",
    text: `A${String(seq)} NOOP`,
    ...extra,
  };
}

function page(
  entries: ConnectionLogEntry[],
  lastSeq: number,
  dropped = 0,
  capacity = 2000,
): ConnectionLog {
  return { scope: WORK, entries, last_seq: lastSeq, dropped, capacity, keep_on_disk: false };
}

/** Routes the mock core's events to the feature like `App.tsx` does. */
function wireEvents(env: Env): () => void {
  return env.client.subscribe((event) => {
    handleConnlogEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    });
  });
}

function renderApp(env: Env) {
  return renderWithProviders(
    <>
      <NodeView node={widget("status-bar", { options: { items: "sync-progress" } })} path="s" />
      <WindowLayer layout={env.transport.data.layout} />
    </>,
    env,
  );
}

function windowOf(id: string): HTMLElement {
  return must(document.querySelector<HTMLElement>(`[data-part="window"][data-window="${id}"]`));
}

describe("connection log model", () => {
  it("merges pages: appends, drops what the core dropped, caps the size", () => {
    const held = [entry(1), entry(2)];
    expect(mergeLog(held, page([entry(2), entry(3)], 3)).map((e) => e.seq)).toEqual([1, 2, 3]);
    expect(mergeLog(held, page([], 2, 2))).toEqual([]);
    const many = Array.from({ length: 15 }, (_, i) => entry(i + 1));
    const merged = mergeLog([], page(many, many.length, 0, 10));
    expect(merged).toHaveLength(10);
    expect(merged[0]?.seq).toBe(6);
  });

  it("filters, lists present protocols and finds an error's entry", () => {
    const entries = [
      entry(1),
      entry(2, { protocol: "smtp", level: "warning" }),
      entry(3, { level: "error" }),
    ];
    expect(filterEntries(entries, ALL_ENTRIES)).toHaveLength(3);
    expect(filterEntries(entries, { protocol: "imap", level: "info" }).map((e) => e.seq)).toEqual([
      1, 3,
    ]);
    expect(filterEntries(entries, { protocol: null, level: "warning" })).toHaveLength(2);
    expect(filterEntries(entries, ALL_ENTRIES, 2)).toHaveLength(2);
    expect(protocolsIn(entries)).toEqual(["imap", "smtp"]);
    expect(WIRE_PROTOCOLS).toContain("managesieve");
    expect(entryAt(entries, 2)).toBe(2);
    expect(entryAt(entries, 99)).toBe(3);
    expect(entryAt(entries, null)).toBeNull();
  });

  it("names viewer windows after their scope", () => {
    const id = must(viewerWindowOf(WORK));
    expect(id).toBe("connection-log:account-acc-work");
    expect(scopeOfViewerWindow(id)).toEqual(WORK);
    expect(scopeOfViewerWindow(must(viewerWindowOf({ kind: "setup" })))).toEqual({
      kind: "setup",
    });
    expect(viewerWindowOf({ kind: "account", account_id: "a b" })).toBeNull();
  });
});

describe("connection log viewer", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("shows redacted lines, filters them and follows new ones unless paused", async () => {
    const env = createEnv();
    const unwire = wireEvents(env);
    const user = userEvent.setup({ delay: null });
    renderApp(env);
    act(() => {
      openConnectionLog(env.store, WORK);
    });
    const viewer = windowOf("connection-log:account-acc-work");
    const list = await within(viewer).findByRole("list", { name: "Recorded lines" });
    await waitFor(() => {
      expect(list).toHaveTextContent('A0001 LOGIN "a***@work.example" <redacted>');
    });
    expect(list).toHaveTextContent('A0003 EXAMINE "INBOX"');
    expect(list).toHaveTextContent("BODY[] {4211 bytes}");
    expect(within(viewer).getByRole("heading", { name: "Connection log of Work" })).toBeVisible();
    // Only the protocols present are offered.
    const protocol = within(viewer).getByRole("combobox", { name: "Protocol" });
    expect(
      within(protocol)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["All protocols", "IMAP"]);
    // A new line arrives live.
    act(() => {
      appendMockLines(env.transport, WORK, "smtp", [["sent", "EHLO [127.0.0.1]"]]);
    });
    await waitFor(() => {
      expect(list).toHaveTextContent("EHLO [127.0.0.1]");
    });
    // Warnings and errors only.
    await user.selectOptions(within(viewer).getByRole("combobox", { name: "Show" }), "warning");
    expect(within(viewer).getByText("No lines match the filters.")).toBeVisible();
    await user.selectOptions(within(viewer).getByRole("combobox", { name: "Show" }), "info");
    // Paused: new lines wait.
    await user.click(within(viewer).getByRole("button", { name: "Pause" }));
    expect(within(viewer).getByRole("button", { name: "Pause" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    act(() => {
      appendMockLines(env.transport, WORK, "imap", [["sent", "A0009 NOOP"]]);
    });
    await waitFor(() => {
      expect(within(viewer).getByRole("status")).toHaveTextContent(
        "Paused — one new line is waiting.",
      );
    });
    expect(within(viewer).queryByText("A0009 NOOP")).toBeNull();
    await user.click(within(viewer).getByRole("button", { name: "Pause" }));
    expect(await within(viewer).findByText("A0009 NOOP")).toBeVisible();
    unwire();
    await settleQueries(env);
  });

  it("copies and exports the core's text, and clears the log", async () => {
    const env = createEnv();
    const unwire = wireEvents(env);
    const user = userEvent.setup({ delay: null });
    renderApp(env);
    act(() => {
      openConnectionLog(env.store, WORK);
    });
    const viewer = windowOf("connection-log:account-acc-work");
    await within(viewer).findByText(/A0001 LOGIN/);
    await user.click(within(viewer).getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(within(viewer).getByRole("status")).toHaveTextContent(
        "The connection log was copied.",
      );
    });
    const copied = await navigator.clipboard.readText();
    expect(copied).toMatch(/^\S+ imap #\d+ -- connecting to imap\.example\.org:993$/m);
    expect(copied).toContain('C: A0001 LOGIN "a***@work.example" <redacted>');
    // Without the desktop app: a browser download of the same text.
    const created = vi.fn(() => "blob:log");
    Object.defineProperty(URL, "createObjectURL", { value: created, configurable: true });
    Object.defineProperty(URL, "revokeObjectURL", { value: vi.fn(), configurable: true });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    await user.click(within(viewer).getByRole("button", { name: "Export…" }));
    await waitFor(() => {
      expect(within(viewer).getByRole("status")).toHaveTextContent(
        "The connection log was downloaded.",
      );
    });
    expect(created).toHaveBeenCalledOnce();
    // The desktop app: the Save dialog, then the core writes the file.
    env.transport.data.features.portability.desktop = true;
    await user.click(within(viewer).getByRole("button", { name: "Export…" }));
    await waitFor(() => {
      expect(within(viewer).getByRole("status")).toHaveTextContent("The connection log was saved.");
    });
    const [path, text] = must(Object.entries(env.transport.data.features.connlog.exports)[0]);
    expect(path).toMatch(/nail-connection-log-account-acc-work\.txt$/);
    expect(text).toContain("<redacted>");
    // Clear.
    await user.click(within(viewer).getByRole("button", { name: "Clear" }));
    expect(await within(viewer).findByText(/^Nothing recorded yet/)).toBeVisible();
    unwire();
    await settleQueries(env);
  });
});

describe("error details", () => {
  it("opens from the status bar and points into the connection log", async () => {
    const env = createEnv();
    const unwire = wireEvents(env);
    const user = userEvent.setup({ delay: null });
    renderApp(env);
    const details = selectFailure("Archive", 7);
    act(() => {
      env.store.getState().setSyncError({
        accountId: "acc-work",
        code: "not_found",
        localized: { id: "sync-error-folders", args: { count: 1, folders: "Archive" } },
        details,
      });
    });
    const bar = screen.getByRole("status", { name: "Status" });
    await user.click(within(bar).getByRole("button", { name: "Show details" }));
    const window = windowOf("error-details:w1");
    expect(window).toHaveTextContent("The folder Archive could not be synchronized.");
    expect(window).toHaveTextContent("What Nail was doingOpening the folder");
    expect(window).toHaveTextContent("FolderArchive");
    expect(window).toHaveTextContent('CommandA0005 EXAMINE "Archive" (CONDSTORE)');
    expect(window).toHaveTextContent(
      "Server's answerNO [NONEXISTENT] Mailbox doesn't exist: Archive",
    );
    await user.click(within(window).getByRole("button", { name: "Copy details" }));
    expect(await navigator.clipboard.readText()).toContain("Folder: Archive");
    await user.click(within(window).getByRole("button", { name: "Show in connection log" }));
    expect(
      connlogStoreFor(env.store).getState().viewers["connection-log:account-acc-work"],
    ).toEqual({
      scope: WORK,
      highlight: 7,
    });
    const viewer = windowOf("connection-log:account-acc-work");
    await waitFor(() => {
      expect(viewer.querySelector('[data-highlight][data-seq="7"]')).not.toBeNull();
    });
    unwire();
    await settleQueries(env);
  });

  it("says when a details window has nothing to show", () => {
    const env = createEnv();
    renderApp(env);
    act(() => {
      env.store.getState().openWindow("error-details:w9");
    });
    expect(windowOf("error-details:w9")).toHaveTextContent(
      "The details of this error are no longer available.",
    );
  });

  it("the mock core fails a sync with details like the core", async () => {
    const env = createEnv();
    const events: string[] = [];
    const off = env.client.subscribe((event) => {
      if (event.type === "SyncFailed") {
        events.push(`${event.details?.step ?? ""} ${event.details?.folder ?? ""}`);
      }
    });
    failSyncWith(env.transport, "acc-work", "Archive");
    await env.client.request({ type: "SyncNow", account_id: "acc-work" }, "SyncStarted");
    await waitFor(() => {
      expect(events).toEqual(["select Archive"]);
    });
    const answer = await env.client.request(
      { type: "ListSyncErrors", account_id: "acc-work" },
      "SyncErrors",
    );
    expect(answer.errors[0]?.details.server_text).toContain("NONEXISTENT");
    expect(answer.errors[0]?.details.log_seq).toBeGreaterThan(0);
    off();
  });
});

describe("connection log settings", () => {
  it("lists an account's problems with details and opens its log", async () => {
    const env = createEnv();
    const unwire = wireEvents(env);
    const user = userEvent.setup({ delay: null });
    failSyncWith(env.transport, "acc-work", "Archive");
    await env.client.request({ type: "SyncNow", account_id: "acc-work" }, "SyncStarted");
    await waitFor(() => {
      expect(env.transport.data.features.connlog.errors).toHaveLength(1);
    });
    env.transport.data.features.connlog.skipped.push({
      account_id: "acc-work",
      folder_id: "acc-work/INBOX",
      uid: 151,
      attempts: 2,
      details: { ...selectFailure("INBOX", null), step: "fetch" },
    });
    renderWithProviders(
      <>
        <NodeView node={widget("account-settings")} path="a" />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    await user.click(await screen.findByRole("button", { name: /^Work/ }));
    const section = await screen.findByRole("region", { name: "Connection" });
    await within(section).findByText(/^Opening the folder failed for Archive/);
    expect(
      within(section).getByText(
        "One message in INBOX could not be downloaded; Nail will try again.",
      ),
    ).toBeVisible();
    expect(within(section).getAllByRole("button", { name: "Show details" })).toHaveLength(2);
    await user.click(within(section).getByRole("button", { name: "Connection log" }));
    expect(windowOf("connection-log:account-acc-work")).toBeVisible();
    unwire();
    await settleQueries(env);
  });

  it("switches disk logging and shows the application log", async () => {
    const env = createEnv();
    const user = userEvent.setup({ delay: null });
    renderWithProviders(
      <>
        <NodeView node={widget("diagnostics")} path="d" />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    const toggle = await screen.findByRole("switch", { name: "Keep connection log on disk" });
    await waitFor(() => {
      expect(toggle).toBeEnabled();
    });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });
    expect(env.transport.data.features.connlog.settings.keep_on_disk).toBe(true);
    expect(screen.getByText("This Nail writes no application log file.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Show recent lines" }));
    const lines = await within(windowOf("application-log")).findByLabelText(
      "Recent application log lines",
    );
    expect(lines).toHaveTextContent("engine started");
    await settleQueries(env);
  });

  it("opens a details window per error", () => {
    const env = createEnv();
    renderApp(env);
    act(() => {
      openErrorDetails(env.store, {
        accountId: null,
        code: "network",
        localized: null,
        details: selectFailure("INBOX", null),
      });
      openErrorDetails(env.store, {
        accountId: null,
        code: "network",
        localized: null,
        details: selectFailure("Sent", null),
      });
    });
    expect(env.store.getState().windows).toEqual(["error-details:w1", "error-details:w2"]);
    expect(windowOf("error-details:w2")).toHaveTextContent("FolderSent");
    // Without an account there is no connection log to show.
    expect(
      within(windowOf("error-details:w2")).queryByRole("button", {
        name: "Show in connection log",
      }),
    ).toBeNull();
  });
});
