/**
 * Several composers and non-modal windows (M5c WP172, ADR 0116): one window
 * per draft, the focused-composer mirror, editing two composers at once,
 * closing that asks only when something is unsaved, the discard
 * confirmation, each composer's sign / encrypt choice, window titles, and
 * the modality rules of every registered window.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionContext } from "../../actions/registry";
import { NodeView } from "../../layout/LayoutRenderer";
import type { ComposeContext } from "../../state/compose";
import { emptyComposer } from "../../state/compose";
import { useUi, type UiState } from "../../state/uiStore";
import {
  CONFIRMATION_WINDOW_SPEC,
  WindowInstanceContext,
  resolveWindowSpec,
  windowKind,
} from "../../state/windows";
import { createEnv, Providers, renderWithProviders, widget, type Env } from "../../test/harness";
import { appRegistry } from "../registry";
import { composerCryptoStore } from "../security";
import { ComposerWindowTitle, composerSubjectTitle } from "./Composer";
import { CloseConfirmDialog, DiscardConfirmDialog } from "./ConfirmDialogs";
import {
  closeComposerNow,
  openRichDraft,
  openComposer,
  requestClose,
  requestDiscard,
  saveRich,
  sendRich,
  startRichCompose,
} from "./flows";
import { windowTitles } from "./index";
import { acceptRichDrafts } from "./mock";
import {
  activeComposer,
  CLOSE_CONFIRM_KIND,
  composerOf,
  composerOfConfirm,
  composerSessions,
  composerTarget,
  confirmWindowFor,
  cryptoFor,
  DISCARD_CONFIRM_KIND,
  findComposerByDraft,
  isUnsaved,
  newComposerWindow,
} from "./sessions";
import { draftKey } from "./draft";
import {
  autosaveDelay,
  AUTOSAVE_MS,
  instanceRichStore,
  richStoreFor,
  setAutosaveDelay,
} from "./store";

function setup(state: Partial<UiState> = {}): Env & { ctx: ComposeContext } {
  const env = createEnv({ state: { activeAccountId: "acc-personal", ...state } });
  acceptRichDrafts(env.transport);
  const ctx: ComposeContext = {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    t: (id) => (id === "composer-attachment-keywords" ? "attached" : id),
    locale: "en",
  };
  return { ...env, ctx };
}

const ui = (env: Env) => env.store.getState();

/**
 * The open composer windows and their confirmations, each in its window
 * instance (as the window layer renders them; the frame itself is WP171's).
 */
function ComposerWindows() {
  const windows = useUi((s) => s.windows);
  const closeWindow = useUi((s) => s.closeWindow);
  return (
    <>
      {windows.map((id) => {
        const kind = windowKind(id);
        const Dialog = kind === "composer" ? null : appRegistry.dialogs[kind];
        return (
          <WindowInstanceContext.Provider key={id} value={id}>
            <section aria-label={id}>
              {Dialog ? (
                <Dialog
                  name={id}
                  close={() => {
                    closeWindow(id);
                  }}
                />
              ) : (
                <NodeView node={widget("composer")} path={id} />
              )}
            </section>
          </WindowInstanceContext.Provider>
        );
      })}
    </>
  );
}

function renderWindows(env: Env) {
  return renderWithProviders(<ComposerWindows />, env);
}

const region = (id: string) => within(screen.getByRole("region", { name: id }));
const formOf = (id: string) => region(id).getByRole("form", { name: "New message" });

afterEach(() => {
  act(() => {
    composerCryptoStore.getState().reset();
  });
});

describe("composer windows", () => {
  // The closing flows check "unsaved changes": the autosave (real timers)
  // must not save in between, however slow the run is.
  beforeEach(() => {
    setAutosaveDelay(10 * 60 * 1000);
  });
  afterEach(() => {
    setAutosaveDelay(null);
  });

  it("opens one window per message and mirrors the focused one", async () => {
    const env = setup();
    await startRichCompose(env.ctx);
    await startRichCompose(env.ctx);
    expect(ui(env).windows).toEqual(["composer:w1", "composer:w2"]);
    expect(Object.keys(ui(env).composers)).toEqual(["composer:w1", "composer:w2"]);
    expect(activeComposer(ui(env).windows)).toBe("composer:w2");
    expect(ui(env).composer).toBe(ui(env).composers["composer:w2"]);

    // Edits of a composer in the background stay its own.
    act(() => {
      ui(env).updateComposerOf("composer:w1", { subject: "First" });
    });
    expect(ui(env).composer?.subject).toBe("");
    // Focusing it (the window manager raises it) moves the mirror.
    act(() => {
      ui(env).focusWindow("composer:w1");
    });
    expect(ui(env).composer?.subject).toBe("First");
    // A write to the mirror (an older flow) goes to the focused composer.
    act(() => {
      ui(env).updateComposer({ to: "grace@navy.example" });
    });
    expect(ui(env).composers["composer:w1"]?.to).toBe("grace@navy.example");
    expect(ui(env).composers["composer:w2"]?.to).toBe("");
    // Other windows on top do not change which composer is focused.
    act(() => {
      ui(env).openWindow("settings");
    });
    expect(composerTarget(ui(env))).toBe("composer:w1");
    expect(ui(env).composer?.subject).toBe("First");
  });

  it("keys windows by draft id and finds an open draft", () => {
    const env = setup();
    const id = openComposer(env.ctx, { ...emptyComposer(), draftId: "d42" });
    expect(id).toBe("composer:d42");
    expect(findComposerByDraft(ui(env), "d42")).toBe("composer:d42");
    // The same draft again: the open window, raised.
    act(() => {
      ui(env).openWindow("settings");
    });
    expect(openComposer(env.ctx, { ...emptyComposer(), draftId: "d42" })).toBe("composer:d42");
    expect(ui(env).windows).toEqual(["settings", "composer:d42"]);
    // Ids that are no instance keys get a generated one.
    expect(newComposerWindow(ui(env), "bad id!")).toBe("composer:w1");
    expect(newComposerWindow(ui(env), null)).toBe("composer:w1");
    expect(confirmWindowFor(CLOSE_CONFIRM_KIND, "composer:d42")).toBe("composer-close:d42");
    expect(composerOfConfirm("composer-discard:d42")).toBe("composer:d42");
    expect(composerOfConfirm(DISCARD_CONFIRM_KIND)).toBe("composer");
  });

  it("opens a draft in a named window (a pop-out's route)", async () => {
    const env = setup();
    act(() => {
      ui(env).setComposerOf("composer:w1", emptyComposer());
    });
    const { draft_id } = await env.client.request(
      {
        type: "SaveDraft",
        draft: {
          id: null,
          identity_id: "ada@example.org",
          to: [],
          cc: [],
          bcc: [],
          subject: "Popped",
          mode: "plain",
          text: "",
          html: null,
          markdown: null,
          attachments: [],
          context: null,
          signature: "identity",
          markdown_part: false,
        },
      },
      "DraftSaved",
    );
    await act(() => openRichDraft(env.ctx, draft_id, { window: "composer:w7" }));
    expect(ui(env).windows).toEqual(["composer:w7"]);
    expect(ui(env).composer?.subject).toBe("Popped");
    // An open window without a composer (the pop-out page opened its route's
    // window first) takes the draft.
    act(() => {
      closeComposerNow(env.ctx, "composer:w7");
    });
    act(() => {
      ui(env).openWindow("composer:w7");
    });
    await act(() => openRichDraft(env.ctx, draft_id, { window: "composer:w7" }));
    expect(ui(env).windows).toEqual(["composer:w7"]);
    expect(ui(env).composers["composer:w7"]?.subject).toBe("Popped");
    // A window id with a composer is not reused.
    act(() => {
      ui(env).setComposerOf("composer:w8", emptyComposer());
    });
    act(() => {
      closeComposerNow(env.ctx, "composer:w7");
    });
    await act(() => openRichDraft(env.ctx, draft_id, { window: "composer:w8" }));
    expect(ui(env).windows).toContain(`composer:${draft_id}`);
  });

  it("edits two composers at once", async () => {
    const user = userEvent.setup();
    const env = setup();
    await startRichCompose(env.ctx);
    await startRichCompose(env.ctx);
    renderWindows(env);
    const first = formOf("composer:w1");
    const second = formOf("composer:w2");

    await user.type(within(first).getByRole("textbox", { name: "Subject" }), "Lab photos");
    await user.type(within(second).getByRole("textbox", { name: "Subject" }), "Budget");
    await user.type(within(first).getByRole("textbox", { name: "To" }), "grace@navy.example");
    fireEvent.change(within(second).getByRole("textbox", { name: "Message" }), {
      target: { value: "Numbers attached below." },
    });
    expect(ui(env).composers["composer:w1"]).toMatchObject({
      subject: "Lab photos",
      to: "grace@navy.example",
    });
    expect(ui(env).composers["composer:w2"]).toMatchObject({ subject: "Budget", to: "" });
    expect(ui(env).composers["composer:w2"]?.text).toContain("Numbers attached below.");

    // Each composer has its own format.
    await user.click(within(second).getByRole("radio", { name: "Markdown" }));
    await waitFor(() => {
      expect(richStoreFor(env.store, "composer:w2").getState().mode).toBe("markdown");
    });
    expect(richStoreFor(env.store, "composer:w1").getState().mode).toBe("plain");
    expect(within(first).getByRole("radio", { name: "Plain text" })).toBeChecked();

    // Sending the first closes only that one.
    await user.click(within(first).getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(ui(env).windows).toEqual(["composer:w2"]);
    });
    expect(ui(env).lastSend).toMatchObject({ subject: "Lab photos" });
    expect(ui(env).composers["composer:w1"]).toBeUndefined();
    expect(ui(env).composer?.subject).toBe("Budget");
    expect(env.transport.requests.filter((r) => r.type === "SendDraft")).toHaveLength(1);
  });

  it("saves each composer as its own draft", async () => {
    const env = setup();
    await startRichCompose(env.ctx);
    await startRichCompose(env.ctx);
    act(() => {
      ui(env).updateComposerOf("composer:w1", { subject: "One" });
      ui(env).updateComposerOf("composer:w2", { subject: "Two" });
    });
    const [one, two] = await Promise.all([
      saveRich(env.ctx, { window: "composer:w1" }),
      saveRich(env.ctx, { window: "composer:w2" }),
    ]);
    expect(one).not.toBe(two);
    expect(env.transport.data.drafts.get(one ?? "")?.subject).toBe("One");
    expect(env.transport.data.drafts.get(two ?? "")?.subject).toBe("Two");
    expect(findComposerByDraft(ui(env), one ?? "")).toBe("composer:w1");
    // Without a window the flows work on the focused composer.
    act(() => {
      ui(env).updateComposerOf("composer:w2", { subject: "Two, again" });
    });
    await saveRich(env.ctx);
    expect(env.transport.data.drafts.get(two ?? "")?.subject).toBe("Two, again");
  });

  it("closes at once when everything is saved", async () => {
    const env = setup();
    await startRichCompose(env.ctx);
    renderWindows(env);
    // The composer took what it opened with as its baseline.
    await waitFor(() => {
      expect(richStoreFor(env.store, "composer:w1").getState().savedKey).not.toBeNull();
    });
    expect(isUnsaved(env.store, "composer:w1")).toBe(false);
    act(() => {
      ui(env).closeWindow("composer:w1");
    });
    expect(ui(env).windows).toEqual([]);
    expect(ui(env).composers).toEqual({});
    expect(ui(env).composer).toBeNull();
    expect(instanceRichStore(env.store, "composer:w1")).toBeUndefined();
  });

  it("asks before closing a composer with unsaved changes", async () => {
    const user = userEvent.setup();
    const env = setup();
    await startRichCompose(env.ctx);
    renderWindows(env);
    await user.type(region("composer:w1").getByRole("textbox", { name: "Subject" }), "Unsent");
    expect(isUnsaved(env.store, "composer:w1")).toBe(true);

    // The title bar's close button (closeWindow): it stays, the confirmation asks.
    act(() => {
      ui(env).closeWindow("composer:w1");
    });
    expect(ui(env).windows).toEqual(["composer:w1", "composer-close:w1"]);
    expect(ui(env).composers["composer:w1"]?.subject).toBe("Unsent");
    const confirm = region("composer-close:w1");
    expect(confirm.getByText(/“Unsent” has changes that are not saved yet/)).toBeInTheDocument();
    await waitFor(() =>
      expect(confirm.getByRole("button", { name: "Keep editing" })).toHaveFocus(),
    );
    await user.click(confirm.getByRole("button", { name: "Keep editing" }));
    expect(ui(env).windows).toEqual(["composer:w1"]);
    // The composer kept its state (and its form).
    expect(region("composer:w1").getByRole("textbox", { name: "Subject" })).toHaveValue("Unsent");

    // Escape (window.close closes the topmost window): asked again.
    act(() => {
      ui(env).closeWindow();
    });
    expect(ui(env).windows).toEqual(["composer:w1", "composer-close:w1"]);
    await user.click(
      region("composer-close:w1").getByRole("button", { name: "Close without saving" }),
    );
    expect(ui(env).windows).toEqual([]);
    expect(ui(env).composers).toEqual({});
    // Nothing was saved: no autosave in between, and closing does not save.
    expect(env.transport.requests.filter((r) => r.type === "SaveDraft")).toEqual([]);
  });

  it("saves and closes from the confirmation", async () => {
    const user = userEvent.setup();
    const env = setup();
    await startRichCompose(env.ctx);
    renderWindows(env);
    await user.type(region("composer:w1").getByRole("textbox", { name: "Subject" }), "Keep me");
    act(() => {
      requestClose(env.ctx, "composer:w1");
    });
    expect(ui(env).windows).toEqual(["composer:w1", "composer-close:w1"]);
    await user.click(region("composer-close:w1").getByRole("button", { name: "Save draft" }));
    await waitFor(() => {
      expect(ui(env).windows).toEqual([]);
    });
    const saved = [...env.transport.data.drafts.values()];
    expect(saved.map((d) => d.subject)).toEqual(["Keep me"]);
  });

  it("asks before discarding a saved draft, not an untouched new message", async () => {
    const user = userEvent.setup();
    const env = setup();
    await startRichCompose(env.ctx);
    renderWindows(env);
    await waitFor(() => {
      expect(richStoreFor(env.store, "composer:w1").getState().savedKey).not.toBeNull();
    });
    await user.click(region("composer:w1").getByRole("button", { name: "Discard" }));
    expect(ui(env).windows).toEqual([]);

    await act(() => startRichCompose(env.ctx));
    const id = activeComposer(ui(env).windows) ?? "";
    await user.type(region(id).getByRole("textbox", { name: "Subject" }), "Draft to drop");
    await user.click(region(id).getByRole("button", { name: "Save draft" }));
    await waitFor(() => {
      expect(ui(env).composers[id]?.draftId).not.toBeNull();
    });
    const draftId = ui(env).composers[id]?.draftId ?? "";
    await user.click(region(id).getByRole("button", { name: "Discard" }));
    const confirm = confirmWindowFor(DISCARD_CONFIRM_KIND, id);
    expect(ui(env).windows).toEqual([id, confirm]);
    const dialog = region(confirm);
    expect(
      dialog.getByText("Discard “Draft to drop”? Its saved draft is deleted too."),
    ).toBeVisible();
    await user.click(dialog.getByRole("button", { name: "Discard" }));
    await waitFor(() => {
      expect(env.transport.data.drafts.has(draftId)).toBe(false);
    });
    expect(ui(env).windows).toEqual([]);
    expect(env.transport.requests).toContainEqual({ type: "DeleteDraft", draft_id: draftId });
    await act(() => requestDiscard(env.ctx, id));
    expect(ui(env).windows).toEqual([]);
  });

  it.each([
    ["composer-close", CloseConfirmDialog],
    ["composer-discard:gone", DiscardConfirmDialog],
  ])("shows a note when %s has no composer left", (name, Dialog) => {
    const env = setup();
    const close = vi.fn();
    render(
      <Providers env={env}>
        <Dialog name={name} close={close} />
      </Providers>,
    );
    expect(screen.getByText("No message is being written.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "OK" }));
    expect(close).toHaveBeenCalled();
  });

  it("keeps each composer's sign / encrypt choice", async () => {
    const env = setup();
    const choice = { sign: true, encrypt: true, technology: null, protect_headers: null };
    const first = openComposer(env.ctx, { ...emptyComposer(), identityId: "ada@example.org" });
    act(() => {
      composerCryptoStore.getState().setChoice(choice);
    });
    const second = openComposer(
      env.ctx,
      { ...emptyComposer(), identityId: "ada@example.org", draftId: "d2" },
      {},
      { crypto: null },
    );
    // The new composer starts from the defaults; the first keeps its choice.
    expect(composerCryptoStore.getState().choice).toBeNull();
    expect(cryptoFor(env.store, first)).toEqual(choice);
    expect(cryptoFor(env.store, second)).toBeUndefined();
    act(() => {
      ui(env).focusWindow(first);
    });
    expect(composerCryptoStore.getState().choice).toEqual(choice);
    expect(cryptoFor(env.store, second)).toBeNull();
    // A composer in the background saves its own choice, not the focused one's.
    const saved = await saveRich(env.ctx, { window: second, quiet: true });
    expect(env.transport.data.drafts.get(saved ?? "")?.crypto ?? null).toBeNull();
    await saveRich(env.ctx, { window: first });
    const own = [...env.transport.data.drafts.values()].find((d) => d.id !== saved);
    expect(own?.crypto).toEqual(choice);
    // A saved draft's choice comes back with it.
    const third = openComposer(
      env.ctx,
      { ...emptyComposer(), draftId: "d3" },
      {},
      { crypto: choice },
    );
    expect(activeComposer(ui(env).windows)).toBe(third);
    expect(composerCryptoStore.getState().choice).toEqual(choice);
  });

  it("sends the focused composer's protection from a background window", async () => {
    const env = setup();
    const first = openComposer(env.ctx, {
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "grace@navy.example",
      subject: "From behind",
    });
    openComposer(env.ctx, { ...emptyComposer(), identityId: "ada@example.org" });
    expect(await sendRich(env.ctx, null, { window: first })).toBe(true);
    // Sending focused it first, then it closed.
    expect(ui(env).windows).toEqual(["composer:w2"]);
    expect(ui(env).lastSend).toMatchObject({ subject: "From behind" });
  });

  it("moves a plain composer into a window of its own when another opens", async () => {
    const env = setup({
      windows: ["composer"],
      composer: { ...emptyComposer(), identityId: "ada@example.org", subject: "Old style" },
    });
    composerSessions(env.store);
    richStoreFor(env.store).setState({ mode: "markdown", markdown: "# kept" });
    await startRichCompose(env.ctx);
    const ids = ui(env).windows;
    expect(ids).toHaveLength(2);
    expect(ids).not.toContain("composer");
    const moved = ids.find((id) => ui(env).composers[id]?.subject === "Old style") ?? "";
    expect(richStoreFor(env.store, moved).getState()).toMatchObject({
      mode: "markdown",
      markdown: "# kept",
    });
    expect(activeComposer(ids)).not.toBe(moved);
  });

  it("runs the composer actions on the focused composer", async () => {
    const env = setup();
    await startRichCompose(env.ctx);
    await startRichCompose(env.ctx);
    act(() => {
      ui(env).focusWindow("composer:w1");
    });
    const actionCtx = { ...env.ctx, layout: env.transport.data.layout } as unknown as ActionContext;
    const setMode = appRegistry.actions["composer.set-mode"];
    expect(setMode?.enabled?.(actionCtx, ["html"])).toBe(true);
    await act(() => setMode?.run(actionCtx, ["html"]));
    expect(richStoreFor(env.store, "composer:w1").getState().mode).toBe("html");
    expect(richStoreFor(env.store, "composer:w2").getState().mode).toBe("plain");
    await appRegistry.actions["composer.attach"]?.run(actionCtx, []);
    expect(richStoreFor(env.store, "composer:w1").getState().attachRequest).toBe(1);
    act(() => {
      closeComposerNow(env.ctx, "composer:w1");
    });
    act(() => {
      closeComposerNow(env.ctx, "composer:w2");
    });
    expect(appRegistry.actions["composer.attach"]?.enabled?.(actionCtx, [])).toBe(false);
  });

  it("titles a composer window with its subject", () => {
    const env = setup();
    const id = openComposer(env.ctx, { ...emptyComposer(), subject: "  " });
    expect(composerSubjectTitle(ui(env), id)).toBeNull();
    expect(windowTitles.composer?.(ui(env), id)).toBeNull();
    renderWithProviders(<ComposerWindowTitle id={id} />, env);
    expect(screen.getByText("New message")).toBeInTheDocument();
    act(() => {
      ui(env).updateComposerOf(id, { subject: "Quarterly report" });
    });
    expect(screen.getByText("Quarterly report")).toBeInTheDocument();
    expect(composerSubjectTitle(ui(env), id)).toBe("Quarterly report");
    // The plain composer's title comes from `UiState.composer`.
    expect(composerSubjectTitle({ composer: null, composers: {} }, "composer")).toBeNull();
  });

  it("compares drafts with each composer's own choice", () => {
    const env = setup();
    const id = openComposer(env.ctx, { ...emptyComposer(), identityId: "ada@example.org" });
    const composer = composerOf(ui(env), id);
    if (!composer) throw new Error("no composer");
    const rich = richStoreFor(env.store, id).getState();
    const choice = { sign: true, encrypt: false, technology: null, protect_headers: null };
    expect(draftKey(composer, rich, choice)).not.toBe(draftKey(composer, rich, null));
    expect(draftKey(composer, rich, null)).toBe(draftKey(composer, rich));
  });
});

describe("window model", () => {
  /** Confirmations: the only modal windows. */
  const CONFIRMATIONS = new Set([
    "composer-close",
    "composer-discard",
    "exchange-mailbox-remove",
    "exchange-category-delete",
    "gmail-label-delete",
    "mail-delete-permanently",
    "theme-delete",
    "plugin-modal-small",
    "plugin-modal-medium",
    "plugin-modal-large",
    "plugin-send-veto",
    "remote-reauth",
    "pim-edit-scope",
    "pim-remove-calendar",
    "scheduling-cancel-meeting",
    "smartcard-pin",
  ]);

  it("makes only confirmations modal", () => {
    const env = setup();
    const layout = env.transport.data.layout;
    const kinds = [
      ...Object.keys(appRegistry.dialogs),
      ...Object.keys(layout.windows).filter((w) => w !== "main"),
    ];
    for (const kind of kinds) {
      const spec = resolveWindowSpec(kind, layout, appRegistry.windows);
      expect({ kind, modal: spec.modal }).toEqual({ kind, modal: CONFIRMATIONS.has(kind) });
    }
    for (const kind of CONFIRMATIONS) {
      expect(appRegistry.windows[kind]?.confirmation).toBe(true);
      expect(resolveWindowSpec(kind, null, appRegistry.windows).pop_out).toBe(
        CONFIRMATION_WINDOW_SPEC.pop_out,
      );
    }
  });

  it("declares a size for every feature dialog", () => {
    for (const kind of Object.keys(appRegistry.dialogs)) {
      const registration = appRegistry.windows[kind];
      expect({ kind, registered: registration !== undefined }).toEqual({
        kind,
        registered: true,
      });
      if (registration?.confirmation) continue;
      const spec = resolveWindowSpec(kind, null, appRegistry.windows);
      expect(spec.size.width).toBeGreaterThanOrEqual(spec.min_size.width);
      expect(spec.size.height).toBeGreaterThanOrEqual(spec.min_size.height);
      expect(registration?.size).toBeDefined();
      expect(registration?.min_size).toBeDefined();
    }
  });

  it("lets several composers open, each resizable to its minimum", () => {
    const env = setup();
    const spec = resolveWindowSpec("composer:w3", env.transport.data.layout, appRegistry.windows);
    expect(spec).toMatchObject({
      modal: false,
      resizable: true,
      instances: "multiple",
      min_size: { width: 520, height: 400 },
    });
    // Without a layout window the registration gives the same.
    expect(resolveWindowSpec("composer", null, appRegistry.windows)).toMatchObject({
      modal: false,
      instances: "multiple",
      size: { width: 820, height: 720 },
    });
    // Its confirmations open per composer too.
    expect(resolveWindowSpec("composer-close:w3", null, appRegistry.windows)).toMatchObject({
      modal: true,
      instances: "multiple",
    });
  });
});

describe("the autosave delay", () => {
  afterEach(() => {
    setAutosaveDelay(null);
  });

  it("is AUTOSAVE_MS unless a test overrides it", () => {
    expect(autosaveDelay()).toBe(AUTOSAVE_MS);
    setAutosaveDelay(5000);
    expect(autosaveDelay()).toBe(5000);
    setAutosaveDelay(-1);
    expect(autosaveDelay()).toBe(AUTOSAVE_MS);
    setAutosaveDelay(null);
    expect(autosaveDelay()).toBe(AUTOSAVE_MS);
  });
});
