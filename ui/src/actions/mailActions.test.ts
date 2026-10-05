import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../api/client";
import { MockTransport } from "../api/mock";
import { createMockData, defaultLayout } from "../api/mockData";
import { queryKeys } from "../state/queries";
import { createUiStore } from "../state/uiStore";
import { contextsOf, isActivation, isFieldCombo } from "./ActionsProvider";
import { createDispatcher, moveTargetOf, type ActionContext } from "./registry";
import { DELETE_PERMANENTLY_DIALOG, confirmDeletePermanently } from "../widgets/DeletePermanently";

const INBOX = "acc-personal/Inbox";

async function context(): Promise<ActionContext & { transport: MockTransport }> {
  const transport = new MockTransport({ data: createMockData(10) });
  const client = new TransportClient(transport);
  const queryClient = new QueryClient();
  const page = await client.request(
    { type: "ListMessages", folder_id: INBOX, offset: 0, limit: 100 },
    "Messages",
  );
  queryClient.setQueryData(queryKeys.messages(INBOX, 100), { pages: [page], pageParams: [0] });
  return {
    transport,
    client,
    store: createUiStore({
      activeAccountId: "acc-personal",
      activeFolderId: INBOX,
      messageOrder: page.messages.map((m) => m.id),
    }),
    queryClient,
    layout: defaultLayout(),
    t: (id) => id,
    locale: "en",
  };
}

describe("mail actions", () => {
  it("are disabled without a selection", async () => {
    const ctx = await context();
    const { isEnabled } = createDispatcher(ctx);
    for (const id of [
      "mail.archive",
      "mail.delete",
      "mail.toggle-read",
      "mail.toggle-flag",
      "mail.move",
      "mail.reply",
      "mail.forward",
    ]) {
      expect(isEnabled(id)).toBe(false);
    }
    expect(isEnabled("mail.compose")).toBe(true);
    ctx.store.getState().selectMessage(`${INBOX}#1`);
    expect(isEnabled("mail.archive")).toBe(true);
    expect(isEnabled("mail.move-to", ["trash"])).toBe(true);
    expect(isEnabled("mail.move-to", [])).toBe(false);
  });

  it("toggle flags from the message's current state", async () => {
    const ctx = await context();
    const { dispatch } = createDispatcher(ctx);
    ctx.store.getState().selectMessage(`${INBOX}#1`); // unread and flagged
    await dispatch("mail.toggle-read");
    await dispatch("mail.toggle-flag");
    const actions = ctx.transport.requests.flatMap((r) =>
      r.type === "ApplyMailAction" ? [{ targets: r.targets, action: r.action }] : [],
    );
    const target = [{ kind: "message", message_id: `${INBOX}#1` }];
    expect(actions).toEqual([
      { targets: target, action: { type: "mark-read", read: true } },
      { targets: target, action: { type: "flag", flagged: false } },
    ]);
  });

  it("archive, move and delete the selection and select the next message", async () => {
    const ctx = await context();
    const { dispatch } = createDispatcher(ctx);
    ctx.store.getState().selectMessage(`${INBOX}#1`);
    await dispatch("mail.archive");
    expect(ctx.store.getState().selectedMessageId).toBe(`${INBOX}#2`);
    await dispatch("mail.move-to", ["acc-personal/Inbox/Receipts"]);
    expect(ctx.store.getState().selectedMessageId).toBe(`${INBOX}#3`);
    await dispatch("mail.delete");
    expect(ctx.store.getState().selectedMessageId).toBe(`${INBOX}#4`);
    // Delete permanently only from Trash, and it asks first with the count:
    // nothing is written until confirmed.
    ctx.store.getState().selectMessage(`${INBOX}#3`);
    await dispatch("mail.delete-permanently");
    expect(ctx.store.getState().windows).toEqual([DELETE_PERMANENTLY_DIALOG]);
    expect(
      ctx.transport.requests.some(
        (r) => r.type === "ApplyMailAction" && r.action.type === "delete-permanently",
      ),
    ).toBe(false);
    ctx.store.getState().selectMessage(`${INBOX}#9`);
    await confirmDeletePermanently(ctx);
    expect(ctx.store.getState().windows).toEqual([]);
    const writes = ctx.transport.requests.flatMap((r) =>
      r.type === "ApplyMailAction" ? [[r.targets, r.action]] : [],
    );
    const one = (n: number) => [{ kind: "message", message_id: `${INBOX}#${String(n)}` }];
    expect(writes).toEqual([
      [one(1), { type: "archive" }],
      [one(2), { type: "move", target: { folder_id: "acc-personal/Inbox/Receipts" } }],
      [one(3), { type: "trash" }],
      [one(3), { type: "delete-permanently" }],
    ]);
  });

  it("open windows, the move picker, account setup and settings", async () => {
    const ctx = await context();
    const { dispatch, isEnabled } = createDispatcher(ctx);
    ctx.store.getState().selectMessage(`${INBOX}#1`);
    await dispatch("mail.move");
    expect(ctx.store.getState().windows).toEqual(["move"]);
    await dispatch("window.close");
    expect(ctx.store.getState().windows).toEqual([]);
    expect(isEnabled("window.close")).toBe(false);
    await dispatch("account.add");
    await dispatch("window.open", ["settings"]);
    expect(ctx.store.getState().windows).toEqual(["account-setup", "settings"]);
    // M5b: settings.open shows the settings view (an alias of view.show).
    expect(ctx.layout.actions["settings.open"]?.alias_of).toBe("view.show");
    expect(isEnabled("window.open", ["nope"])).toBe(false);
    await dispatch("window.close", ["account-setup"]);
    expect(ctx.store.getState().windows).toEqual(["settings"]);
  });

  it("compose, reply and focus the search", async () => {
    const ctx = await context();
    const { dispatch, isEnabled } = createDispatcher(ctx);
    expect(isEnabled("composer.send")).toBe(false);
    ctx.store.getState().selectMessage(`${INBOX}#2`);
    await dispatch("mail.reply");
    expect(ctx.store.getState().composer?.subject).toMatch(/^Re: /);
    expect(ctx.store.getState().windows).toEqual(["composer:w1"]);
    expect(isEnabled("composer.send")).toBe(true);
    await dispatch("composer.discard");
    expect(ctx.store.getState().composer).toBeNull();
    await dispatch("mail.compose");
    expect(ctx.store.getState().composer?.identityId).toBe("ada@example.org");
    await dispatch("search.focus");
    expect(ctx.store.getState().focusRequest).toEqual({ target: "search", seq: 1 });
    ctx.store.getState().setSearch({ text: "x", accountId: null, folderId: null, error: null });
    await dispatch("search.clear");
    expect(ctx.store.getState().search).toBeNull();
  });

  it("parse move targets", () => {
    expect(moveTargetOf("archive")).toEqual({ role: "archive" });
    expect(moveTargetOf("f-1")).toEqual({ folder_id: "f-1" });
    expect(moveTargetOf(3)).toBeNull();
  });
});

describe("key contexts", () => {
  it("collect nested contexts and allow only chords in text fields", () => {
    document.body.innerHTML =
      '<div data-key-context="window"><form data-key-context="composer"><textarea></textarea></form></div>';
    const textarea = document.querySelector("textarea");
    expect(contextsOf(textarea)).toEqual(["composer", "window"]);
    expect(contextsOf(document.body)).toEqual([]);
    expect(isFieldCombo("ctrl+enter")).toBe(true);
    expect(isFieldCombo("escape")).toBe(true);
    expect(isFieldCombo("j")).toBe(false);
    expect(isFieldCombo("shift+a")).toBe(false);
    document.body.innerHTML = "";
  });

  it("leave Enter and Space on buttons and links to the control itself", () => {
    document.body.innerHTML =
      '<div data-key-context="list" tabindex="0" id="list"><button id="b"><span id="in">x</span></button><a href="#" id="a">a</a></div>';
    const el = (id: string) => document.getElementById(id);
    expect(isActivation("enter", el("b"))).toBe(true);
    expect(isActivation("enter", el("in"))).toBe(true);
    expect(isActivation(" ", el("a"))).toBe(true);
    // The list itself (rows are its options): Enter is a binding there.
    expect(isActivation("enter", el("list"))).toBe(false);
    expect(isActivation("o", el("b"))).toBe(false);
    document.body.innerHTML = "";
  });
});
