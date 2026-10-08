import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockTransport } from "../../api/mock";
import { createDispatcher } from "../../actions/registry";
import { appRegistry } from "../registry";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { queryKeys } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import {
  fromLocalInput,
  overrideOf,
  presetTime,
  toLocalInput,
  toRfc3339,
  withOverride,
  defaultNotificationSettings,
} from "./model";
import { decorateJunk, snoozedFolderId, wakeDue } from "./mock";
import { SnoozeDialog, SNOOZE_DIALOG } from "./Snooze";
import { WebNotifications, notificationTexts } from "./Notifications";
import { triageStoreFor } from "./store";
import { snoozeForADay, requestsOf, actionContext } from "../../test/env";
import { INBOX } from "../../test/mockIds";

function dispatcher(env: Env) {
  return createDispatcher(actionContext(env), appRegistry.actions);
}

describe("triage model", () => {
  it("computes snooze presets in local time", () => {
    // Wednesday 30 Sep 2026, 10:15.
    const now = new Date(2026, 8, 30, 10, 15);
    expect(presetTime("later-today", now)).toEqual(new Date(2026, 8, 30, 13, 0));
    expect(presetTime("tomorrow", now)).toEqual(new Date(2026, 9, 1, 8, 0));
    expect(presetTime("weekend", now)).toEqual(new Date(2026, 9, 3, 8, 0));
    expect(presetTime("next-week", now)).toEqual(new Date(2026, 9, 5, 8, 0));
    // Late evening: no "later today"; on a Sunday: no "this weekend".
    expect(presetTime("later-today", new Date(2026, 8, 30, 20, 0))).toBeNull();
    const sunday = new Date(2026, 9, 4, 9, 0);
    expect(presetTime("weekend", sunday)).toBeNull();
    expect(presetTime("next-week", sunday)).toEqual(new Date(2026, 9, 5, 8, 0));
  });

  it("formats and parses times", () => {
    const date = new Date(2026, 9, 1, 8, 5);
    expect(toRfc3339(date)).toMatch(/^2026-10-01T08:05:00[+-]\d{2}:\d{2}$/);
    expect(Date.parse(toRfc3339(date))).toBe(date.getTime());
    expect(toLocalInput(date)).toBe("2026-10-01T08:05");
    expect(fromLocalInput("2026-10-01T08:05")).toEqual(date);
    expect(fromLocalInput("2026-02-30T08:05")).toBeNull();
    expect(fromLocalInput("tomorrow")).toBeNull();
  });

  it("edits notification overrides", () => {
    const base = defaultNotificationSettings();
    const account = { type: "account" as const, account_id: "a" };
    const folder = { type: "folder" as const, folder_id: "f" };
    const one = withOverride(withOverride(base, account, "off"), folder, "on");
    expect(overrideOf(one, account)).toBe("off");
    expect(overrideOf(one, folder)).toBe("on");
    const two = withOverride(one, account, "default");
    expect(two.overrides).toEqual([{ scope: folder, notify: true }]);
  });
});

describe("triage mock (as the core after WP55)", () => {
  it("marks junk: trains, tags and moves to Junk and back", async () => {
    const t = new MockTransport();
    const message = t.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    expect(await t.send({ type: "MarkJunk", message_ids: [message.id], junk: true })).toMatchObject(
      { type: "Queued" },
    );
    expect(message.folder_id).toBe("acc-personal/Junk");
    expect(message.tags).toContain("$Junk");
    const status = await t.send({ type: "GetJunkStatus", account_id: "acc-personal" });
    expect(status).toMatchObject({ status: { trained_junk: 1, trained_ham: 0, ready: false } });
    const body = t.data.bodies.get(message.id);
    if (!body) throw new Error("no body");
    expect(decorateJunk(t, body).junk).toEqual({ junk: true, score: null, source: "user" });

    await t.send({ type: "MarkJunk", message_ids: [message.id], junk: false });
    expect(message.folder_id).toBe(INBOX);
    expect(message.tags).toContain("$NotJunk");
    expect(message.tags).not.toContain("$Junk");
    expect(await t.send({ type: "GetJunkStatus", account_id: "acc-personal" })).toMatchObject({
      status: { trained_junk: 0, trained_ham: 1 },
    });
    await t.send({ type: "ResetJunkClassifier", account_id: "acc-personal" });
    expect(await t.send({ type: "GetJunkStatus", account_id: "acc-personal" })).toMatchObject({
      status: { trained_junk: 0, trained_ham: 0, last_trained_at: null },
    });
  });

  it("snoozes into the hidden folder and wakes up", async () => {
    const t = new MockTransport();
    const message = t.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    const past = await t.send({
      type: "SnoozeMessages",
      message_ids: [message.id],
      until: "2000-01-01T00:00:00Z",
      wake_folder_id: null,
    });
    expect(past).toMatchObject({ code: "invalid_request" });
    const until = new Date(Date.now() + 3_600_000).toISOString();
    await t.send({
      type: "SnoozeMessages",
      message_ids: [message.id],
      until,
      wake_folder_id: null,
    });
    expect(message.folder_id).toBe(snoozedFolderId("acc-personal"));
    const listed = await t.send({ type: "ListSnoozed", account_id: "acc-personal" });
    expect(listed).toMatchObject({
      messages: [{ message: { id: message.id }, until, wake_folder_id: INBOX }],
    });
    expect(wakeDue(t, Date.now())).toEqual([]);
    expect(wakeDue(t, Date.now() + 7_200_000)).toEqual([message.id]);
    expect(message.folder_id).toBe(INBOX);
    expect(message.tags).not.toContain("$Snoozed");
    expect(await t.send({ type: "UnsnoozeMessages", message_ids: [message.id] })).toMatchObject({
      code: "not_found",
    });
  });
});

describe("junk and snooze actions", () => {
  it("marks the selected message as junk and takes it out of the list", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    env.store.getState().selectMessage(message.id);
    env.store.getState().setMessageOrder([message.id]);
    const actions = dispatcher(env);
    expect(actions.isEnabled("junk.mark")).toBe(true);
    await actions.dispatch("junk.mark");
    // P3: one mail action (one batch with one undo) on the selection.
    expect(requestsOf(env, "ApplyMailAction")).toEqual([
      {
        type: "ApplyMailAction",
        targets: [{ kind: "message", message_id: message.id }],
        action: { type: "junk", junk: true },
      },
    ]);
    expect(env.store.getState().selectedMessageId).toBeNull();
    expect(env.store.getState().pendingOps.length).toBe(1);
  });

  it("opens the snooze picker and snoozes with a quick choice", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    env.store.getState().selectMessage(message.id);
    await dispatcher(env).dispatch("snooze.open");
    expect(env.store.getState().windows).toEqual([SNOOZE_DIALOG]);
    expect(triageStoreFor(env.store).getState().snoozing).toEqual([message.id]);
    const close = vi.fn();
    renderWithProviders(<SnoozeDialog name={SNOOZE_DIALOG} close={close} />, env);
    await user.click(screen.getByRole("button", { name: /Tomorrow/ }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const [request] = requestsOf(env, "ApplyMailAction");
    const expected = presetTime("tomorrow", new Date());
    expect(request?.action.type).toBe("snooze");
    expect(Date.parse(request?.action.type === "snooze" ? request.action.until : "")).toBe(
      expected?.getTime(),
    );
    expect(request?.targets).toEqual([{ kind: "message", message_id: message.id }]);
  });

  it("rejects a custom time in the past", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    triageStoreFor(env.store).getState().setSnoozing(["x"]);
    renderWithProviders(<SnoozeDialog name={SNOOZE_DIALOG} close={vi.fn()} />, env);
    const field = screen.getByLabelText("Date and time");
    fireEvent.change(field, { target: { value: "2020-01-01T08:00" } });
    await user.click(screen.getByRole("button", { name: "Snooze" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Choose a time in the future.");
    expect(requestsOf(env, "SnoozeMessages")).toEqual([]);
  });
});

describe("triage widgets", () => {
  it("lists snoozed messages and wakes one", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    await snoozeForADay(env, message.id);
    renderWithProviders(<NodeView node={widget("snooze-list")} path="t" />, env);
    const list = await screen.findByRole("region", { name: "Snoozed messages" });
    const item = await within(list).findByText(message.subject);
    expect(item).toHaveAttribute("data-part", "snooze-list.subject");
    await user.click(within(list).getByRole("button", { name: /Wake up/ }));
    await waitFor(() => expect(within(list).getByText("No snoozed messages.")).toBeVisible());
    expect(message.folder_id).toBe(INBOX);
  });

  it("shows the junk verdict and un-junks from the reader", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    await env.client.request({ type: "MarkJunk", message_ids: [message.id], junk: true }, "Queued");
    act(() => {
      env.store.getState().selectMessage(message.id);
    });
    renderWithProviders(<NodeView node={widget("junk-verdict")} path="t" />, env);
    const bar = await screen.findByRole("status", { name: "Junk mail" });
    expect(bar).toHaveTextContent("This message is junk.");
    expect(bar).toHaveTextContent("Marked by you");
    await user.click(within(bar).getByRole("button", { name: "Not junk" }));
    await waitFor(() => {
      expect(requestsOf(env, "ApplyMailAction").at(-1)).toEqual({
        type: "ApplyMailAction",
        targets: [{ kind: "message", message_id: message.id }],
        action: { type: "junk", junk: false },
      });
    });
  });

  it("edits the junk filter with validation", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<NodeView node={widget("junk-settings")} path="t" />, env);
    await screen.findByText(/Learning/);
    await user.click(screen.getByLabelText(/Classify arriving mail/));
    const threshold = screen.getByLabelText("Junk threshold (percent)");
    await user.clear(threshold);
    await user.type(threshold, "20");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("between 50 and 99");
    expect(requestsOf(env, "UpdateJunkSettings")).toEqual([]);
    await user.clear(threshold);
    await user.type(threshold, "80");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Saved.");
    expect(requestsOf(env, "UpdateJunkSettings")[0]?.settings).toMatchObject({
      enabled: true,
      threshold: 80,
    });
    await user.click(screen.getByRole("button", { name: "Reset the filter…" }));
    await user.click(screen.getByRole("button", { name: "Reset" }));
    expect(await screen.findByText("The filter forgot everything it learned.")).toBeVisible();
    expect(requestsOf(env, "ResetJunkClassifier")).toHaveLength(1);
  });

  it("edits notification settings: quiet hours, overrides, test", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<NodeView node={widget("notification-settings")} path="t" />, env);
    await user.click(await screen.findByLabelText("No notifications during quiet hours"));
    const from = screen.getByLabelText("From");
    expect(from).toHaveValue("22:00");
    await user.click(screen.getByLabelText("Saturday"));
    await user.selectOptions(await screen.findByLabelText("Personal"), "off");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved.");
    const [update] = requestsOf(env, "UpdateNotificationSettings");
    expect(update?.settings.quiet_hours).toEqual({ start: "22:00", end: "07:00", days: ["sat"] });
    expect(update?.settings.overrides).toContainEqual({
      scope: { type: "account", account_id: "acc-personal" },
      notify: false,
    });
    await user.click(screen.getByRole("button", { name: "Send a test notification" }));
    expect(await screen.findByText("A test notification is on its way.")).toBeVisible();
    // Invalid quiet hours are caught before sending.
    await user.clear(screen.getByLabelText("Until"));
    await user.type(screen.getByLabelText("Until"), "22:00");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("must not start and end");
  });
});

describe("web notifications", () => {
  const original = globalThis.Notification;
  afterEach(() => {
    Object.defineProperty(globalThis, "Notification", { configurable: true, value: original });
  });

  it("shows NotificationRequested in the browser once allowed", async () => {
    const shown: { title: string; options: NotificationOptions }[] = [];
    class FakeNotification {
      static permission: NotificationPermission = "granted";
      static requestPermission = vi.fn(() => Promise.resolve("granted"));
      onclick: (() => void) | null = null;
      constructor(title: string, options: NotificationOptions) {
        shown.push({ title, options });
      }
      close = vi.fn();
    }
    Object.defineProperty(globalThis, "Notification", {
      configurable: true,
      value: FakeNotification,
    });
    const env = createEnv();
    function Probe() {
      useUiStoreApi();
      return <WebNotifications />;
    }
    renderWithProviders(<Probe />, env);
    await env.client.request({ type: "TestNotification" }, "Done");
    await waitFor(() => {
      expect(shown).toHaveLength(1);
    });
    expect(shown[0]).toEqual({
      title: "Notifications work",
      options: { body: "This is how Nail tells you about new mail.", tag: "test", silent: false },
    });
  });

  it("renders localized titles and bodies", () => {
    const texts = notificationTexts(
      {
        id: "n",
        tag: "f",
        kind: "new-mail",
        account_id: "a",
        folder_id: "f",
        message_ids: ["m"],
        title: { id: "title", args: { count: 2 } },
        body: null,
        sound: true,
      },
      (id, args) => `${id}:${String(args?.count ?? "")}`,
    );
    expect(texts).toEqual({ title: "title:2", body: undefined });
  });
});

describe("junk actions apply only where they make sense (M5b review)", () => {
  it("offers Not junk only in Junk or for mail the classifier flagged, Mark as junk outside Junk", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const d = dispatcher(env);
    const folders = await env.client.request(
      { type: "ListFolders", account_id: "acc-personal" },
      "Folders",
    );
    env.queryClient.setQueryData(queryKeys.folders("acc-personal"), folders.folders);
    const inboxMessage = env.transport.data.messages.find((m) => m.folder_id === INBOX)?.id ?? "";
    act(() => {
      env.store.getState().selectMessage(inboxMessage);
    });
    expect(d.isEnabled("junk.mark")).toBe(true);
    expect(d.isEnabled("junk.not-junk")).toBe(false);
    // The classifier's verdict on the loaded message.
    env.queryClient.setQueryData(queryKeys.message(inboxMessage), {
      id: inboxMessage,
      junk: { junk: true, score: 0.97, source: "classifier" },
    });
    expect(d.isEnabled("junk.not-junk")).toBe(true);

    const junkFolder = folders.folders.find((f) => f.role === "junk")?.id ?? "";
    const junkMessage =
      env.transport.data.messages.find((m) => m.folder_id === junkFolder)?.id ?? "";
    act(() => {
      env.store.getState().selectFolder(junkFolder);
      env.store.getState().selectMessage(junkMessage);
    });
    expect(d.isEnabled("junk.not-junk")).toBe(true);
    expect(d.isEnabled("junk.mark")).toBe(false);
  });
});
