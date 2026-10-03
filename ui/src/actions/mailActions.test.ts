import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../api/client";
import { MockTransport } from "../api/mock";
import { createMockData, defaultLayout } from "../api/mockData";
import { queryKeys } from "../state/queries";
import { createUiStore } from "../state/uiStore";
import { contextsOf, isFieldCombo } from "./ActionsProvider";
import { createDispatcher, moveTargetOf, type ActionContext } from "./registry";

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
    const changes = ctx.transport.requests
      .filter((r) => r.type === "UpdateFlags")
      .map((r) => r.type === "UpdateFlags" && r.change);
    expect(changes).toEqual([
      expect.objectContaining({ seen: true, flagged: null }),
      expect.objectContaining({ seen: null, flagged: false }),
    ]);
  });

  it("archive, move and delete the selection and select the next message", async () => {
    const ctx = await context();
    const { dispatch } = createDispatcher(ctx);
    ctx.store.getState().selectMessage(`${INBOX}#1`);
    await dispatch("mail.archive");
    expect(ctx.store.getState().selectedMessageId).toBe(`${INBOX}#2`);
    await dispatch("mail.move-to", ["acc-personal/Inbox/Receipts"]);
    await dispatch("mail.delete");
    await dispatch("mail.delete-permanently");
    const writes = ctx.transport.requests.filter((r) => r.type !== "ListMessages");
    expect(writes).toEqual([
      { type: "MoveMessages", message_ids: [`${INBOX}#1`], target: { role: "archive" } },
      {
        type: "MoveMessages",
        message_ids: [`${INBOX}#2`],
        target: { folder_id: "acc-personal/Inbox/Receipts" },
      },
      { type: "DeleteMessages", message_ids: [`${INBOX}#3`], permanent: false },
      { type: "DeleteMessages", message_ids: [`${INBOX}#4`], permanent: true },
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
    expect(ctx.store.getState().windows).toEqual(["composer"]);
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
});
