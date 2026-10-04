/**
 * M5c integration (WP174): every window of the app — each layout window and
 * each feature dialog — opens through the window manager with its declared
 * modality and minimum size; window titles from the UI state (a composer's
 * subject); "Open in new window" in the title bar on the desktop only;
 * composers popping out with their draft; the per-window readers of
 * several composers (security toggles, program-draft notice).
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ResolvedLayout } from "./api/generated";
import { enableMockPopOuts } from "./api/windowStateMock";
import { emptyComposer } from "./state/compose";
import { appRegistry } from "./features/registry";
import { effects as composerEffects } from "./features/composer";
import { openComposer } from "./features/composer/flows";
import { draftOfPopOutContext, prepareComposerPopOut } from "./features/composer/popOut";
import { syncStoreFor } from "./features/sync/store";
import { triageStoreFor } from "./features/triage/store";
import { PopOutHost } from "./features/popout";
import { systemStoreFor } from "./features/system/store";
import { POP_OUT_CLOSE_EVENT, WINDOW_KEYBOARD_STEP, resolveWindowSpec } from "./state/windows";
import { setAutosaveDelay } from "./features/composer/store";
import { WindowLayer } from "./layout/WindowLayer";
import { applyPopOuts } from "./state/windowState";
import { createEnv, renderWithProviders, stubElementSizes, type Env } from "./test/harness";
import { must } from "./test/must";

const INBOX = "acc-personal/Inbox";
const PAGE = { width: 1600, height: 1200 };

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  Object.defineProperty(window, "innerWidth", { configurable: true, value: PAGE.width });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: PAGE.height });
  window.dispatchEvent(new Event("resize"));
});
afterEach(() => {
  restoreSizes();
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1024 });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 768 });
  window.location.hash = "";
  document.title = "";
});

function ctxOf(env: Env) {
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    t: (id: string) => id,
    locale: "en",
  };
}

function mailEnv(): Env {
  const env = createEnv({
    state: {
      activeAccountId: "acc-personal",
      activeFolderId: INBOX,
      composer: { ...emptyComposer(), identityId: "ada@example.org" },
    },
  });
  const first = env.transport.data.messages.find((m) => m.folder_id === INBOX);
  if (first) env.store.getState().selectMessage(first.id);
  triageStoreFor(env.store)
    .getState()
    .setSnoozing(first ? [first.id] : []);
  syncStoreFor(env.store).getState().setFolder(INBOX);
  return env;
}

/** The declarations of an element's geometry rule. */
function geometry(element: Element): Record<string, string> {
  const id = element.getAttribute("data-geom") ?? "";
  const rule = [
    ...(document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ??
      []),
  ].find(
    (r): r is CSSStyleRule => r instanceof CSSStyleRule && r.selectorText === `[data-geom="${id}"]`,
  );
  const out: Record<string, string> = {};
  if (!rule) return out;
  for (let i = 0; i < rule.style.length; i++) {
    const property = rule.style.item(i);
    out[property] = rule.style.getPropertyValue(property);
  }
  return out;
}

const sizeOf = (element: Element) => ({
  width: parseFloat(geometry(element).width ?? "NaN"),
  height: parseFloat(geometry(element).height ?? "NaN"),
});

describe("every window opens through the window manager", () => {
  const layout = createEnv().transport.data.layout;
  const dialogs = Object.keys(appRegistry.dialogs).map((name) => ["dialog", name] as const);
  const windows = Object.keys(layout.windows)
    .filter((name) => name !== "main")
    .map((name) => ["layout window", name] as const);
  const cases = [...windows, ...dialogs].sort((a, b) => a[1].localeCompare(b[1]));

  it("covers the layout windows and the feature dialogs", () => {
    expect(windows.map(([, name]) => name)).toEqual(
      expect.arrayContaining(["composer", "outbox", "move", "settings", "rules"]),
    );
    expect(dialogs.length).toBeGreaterThan(20);
  });

  it.each(cases)(
    "%s %s: declared modality, default size, never below its minimum",
    async (what, name) => {
      const env = mailEnv();
      const base = env.transport.data.layout;
      // A feature dialog renders only without a layout window of its name.
      const used: ResolvedLayout =
        what === "dialog"
          ? {
              ...base,
              windows: Object.fromEntries(Object.entries(base.windows).filter(([n]) => n !== name)),
            }
          : base;
      const spec = resolveWindowSpec(name, used, appRegistry.windows);
      act(() => {
        env.store.getState().openWindow(name);
      });
      const { container } = renderWithProviders(<WindowLayer layout={used} />, env, {
        layout: used,
      });
      const frame = await waitFor(() => {
        const found = container.querySelector(`[data-part="window"][data-window="${name}"]`);
        if (!found) throw new Error(`${name} is not a window`);
        return found;
      });
      expect(frame).toHaveAttribute("role", "dialog");
      expect(frame.getAttribute("aria-modal")).toBe(spec.modal ? "true" : null);
      expect(container.querySelector('[data-part="window-scrim"]') !== null).toBe(spec.modal);
      // Opens at its declared size (the page is larger than every default).
      expect(sizeOf(frame)).toEqual(spec.size);
      const header = must(within(frame as HTMLElement).getAllByRole("group")[0]);
      expect(header).toHaveAttribute("data-part", "window.header");
      const steps =
        Math.ceil(
          Math.max(spec.size.width - spec.min_size.width, spec.size.height - spec.min_size.height) /
            WINDOW_KEYBOARD_STEP,
        ) + 2;
      act(() => {
        header.focus();
      });
      for (let i = 0; i < steps; i++) {
        fireEvent.keyDown(header, { key: "ArrowLeft", shiftKey: true });
        fireEvent.keyDown(header, { key: "ArrowUp", shiftKey: true });
      }
      const shrunk = sizeOf(frame);
      if (spec.resizable) {
        expect(shrunk).toEqual(spec.min_size);
      } else {
        expect(shrunk).toEqual(spec.size);
      }
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 10));
      });
    },
    60_000,
  );
});

describe("window titles and the title bar", () => {
  it("shows a composer's subject as its window title", async () => {
    const env = createEnv();
    const id = openComposer(ctxOf(env), { ...emptyComposer(), identityId: "ada@example.org" });
    renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
    expect(await screen.findByRole("dialog", { name: "New message" })).toBeInTheDocument();
    act(() => {
      env.store.getState().updateComposerOf(id, { subject: "Quarterly report" });
    });
    expect(await screen.findByRole("dialog", { name: "Quarterly report" })).toBeInTheDocument();
  });

  it("offers Open in new window only in the desktop app, never for confirmations", async () => {
    const browser = createEnv();
    act(() => {
      browser.store.getState().openWindow("rules");
    });
    const first = renderWithProviders(
      <WindowLayer layout={browser.transport.data.layout} />,
      browser,
    );
    await screen.findByRole("dialog", { name: "Filter rules" });
    await waitFor(() => {
      expect(browser.queryClient.isFetching()).toBe(0);
    });
    expect(first.container.querySelector('[data-part="window.pop-out"]')).toBeNull();
    first.unmount();

    const desktop = createEnv();
    enableMockPopOuts(desktop.transport);
    act(() => {
      desktop.store.getState().openWindow("rules");
    });
    renderWithProviders(<WindowLayer layout={desktop.transport.data.layout} />, desktop);
    const rules = await screen.findByRole("dialog", { name: "Filter rules" });
    expect(
      await within(rules).findByRole("button", { name: "Open in new window" }),
    ).toBeInTheDocument();
    act(() => {
      desktop.store.getState().openWindow("mail-delete-permanently");
    });
    const confirm = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(within(confirm).queryByRole("button", { name: "Open in new window" })).toBeNull();
  });

  it("offers Open in new window only for windows that work in another page", async () => {
    // Every webview has its own UI state: a window may pop out only when it
    // shows core data alone or hands its state over (a pop-out preparer).
    const layout = createEnv().transport.data.layout;
    const kinds = [
      ...new Set([...Object.keys(layout.windows), ...Object.keys(appRegistry.dialogs)]),
    ].filter((name) => name !== "main");
    const popping = kinds
      .filter((name) => resolveWindowSpec(name, layout, appRegistry.windows).pop_out)
      .sort();
    expect(popping).toEqual(["composer", "outbox", "rules", "settings", "snoozed"]);

    // A stateful editor (it edits the main page's selection) stays in the page.
    const desktop = createEnv();
    enableMockPopOuts(desktop.transport);
    act(() => {
      desktop.store.getState().openWindow("pim-event-editor");
    });
    renderWithProviders(<WindowLayer layout={desktop.transport.data.layout} />, desktop);
    const editor = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('[data-window="pim-event-editor"]');
      if (found === null) throw new Error("no editor");
      return found;
    });
    await waitFor(() => {
      expect(desktop.queryClient.isFetching()).toBe(0);
    });
    expect(editor.querySelector('[data-part="window.pop-out"]')).toBeNull();
  });
});

describe("composers in pop-out windows", () => {
  it("saves the draft before popping out and opens it in the pop-out page", async () => {
    const user = userEvent.setup();
    const main = createEnv();
    enableMockPopOuts(main.transport);
    const id = openComposer(ctxOf(main), {
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "grace@navy.example",
      subject: "Popped",
    });
    // The composer's effects register its pop-out preparer, as in the app.
    renderWithProviders(
      <>
        {composerEffects.map((Effect, index) => (
          <Effect key={index} />
        ))}
        <WindowLayer layout={main.transport.data.layout} />
      </>,
      main,
    );
    const dialog = await screen.findByRole("dialog", { name: "Popped" });
    await user.click(await within(dialog).findByRole("button", { name: "Open in new window" }));
    await waitFor(() => {
      expect(main.store.getState().windows).not.toContain(id);
    });
    const listed = await main.client.call({ type: "ListPopOuts" });
    if (listed.type !== "PopOuts") throw new Error("no pop-outs");
    const entry = listed.pop_outs.find((p) => p.window === id);
    const draft = draftOfPopOutContext(entry?.context ?? null);
    expect(draft).not.toBeNull();
    expect(main.transport.data.drafts.get(draft ?? "")?.subject).toBe("Popped");

    // The pop-out page: its own UI store on the same core.
    const page = createEnv();
    page.transport = main.transport;
    page.client = main.client;
    window.location.hash = `#/window/${id}`;
    renderWithProviders(
      <>
        {composerEffects.map((Effect, index) => (
          <Effect key={index} />
        ))}
        <PopOutHost layout={main.transport.data.layout} window={id} />
      </>,
      page,
    );
    const subject = await screen.findByRole("textbox", { name: "Subject" });
    await waitFor(() => expect(subject).toHaveValue("Popped"));
    await waitFor(() => {
      expect(document.title).toBe("Popped");
    });
    expect(page.store.getState().windows).toEqual([id]);

    // Edited in the pop-out, then "Return to main window": the draft is
    // saved first, and the main page opens the window again with it.
    await user.clear(subject);
    await user.type(subject, "Popped back");
    await user.click(screen.getByRole("button", { name: "Return to main window" }));
    await waitFor(() => {
      expect(main.transport.data.drafts.get(draft ?? "")?.subject).toBe("Popped back");
    });
    // What the main page does on `PopOutChanged returned`: the list without
    // it, and the window back in the page.
    act(() => {
      applyPopOuts(main.queryClient, []);
      main.store.getState().openWindow(id);
    });
    await waitFor(() => {
      expect(main.store.getState().composers[id]?.subject).toBe("Popped back");
    });
  });

  /**
   * A composer popped out (saved draft, pop-out page on the same core) and
   * its page rendered; the autosave is lengthened so "unsaved" holds.
   */
  async function poppedOutComposer() {
    setAutosaveDelay(10 * 60 * 1000);
    const main = createEnv();
    enableMockPopOuts(main.transport);
    const id = openComposer(ctxOf(main), {
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "grace@navy.example",
      subject: "Popped",
    });
    const preparer = renderWithProviders(
      <>
        {composerEffects.map((Effect, index) => (
          <Effect key={index} />
        ))}
        <WindowLayer layout={main.transport.data.layout} />
      </>,
      main,
    );
    const dialog = await screen.findByRole("dialog", { name: "Popped" });
    await userEvent
      .setup()
      .click(await within(dialog).findByRole("button", { name: "Open in new window" }));
    await waitFor(() => {
      expect(main.store.getState().windows).not.toContain(id);
    });
    preparer.unmount();

    const page = createEnv();
    page.transport = main.transport;
    page.client = main.client;
    window.location.hash = `#/window/${id}`;
    renderWithProviders(
      <>
        {composerEffects.map((Effect, index) => (
          <Effect key={index} />
        ))}
        <PopOutHost layout={main.transport.data.layout} window={id} />
      </>,
      page,
    );
    const subject = await screen.findByRole("textbox", { name: "Subject" });
    await waitFor(() => expect(subject).toHaveValue("Popped"));
    const closes = () => main.transport.requests.filter((r) => r.type === "ClosePopOut");
    return { main, page, id, key: id.split(":")[1] ?? "", subject, closes };
  }

  const windowEl = (id: string) =>
    document.querySelector<HTMLElement>(`[data-part="window"][data-window="${id}"]`);

  async function findWindow(id: string): Promise<HTMLElement> {
    return waitFor(() => {
      const found = windowEl(id);
      if (found === null) throw new Error(`no window ${id}`);
      return found;
    });
  }

  it("shows the composer's Discard question in the pop-out page", async () => {
    try {
      const user = userEvent.setup();
      const { page, id, key, closes } = await poppedOutComposer();
      await user.click(screen.getByRole("button", { name: "Discard" }));
      const question = await findWindow(`composer-discard:${key}`);
      // The pop-out page is the main window behind it.
      expect(question).toHaveAttribute("data-owner", "main");
      expect(page.store.getState().windows).toEqual([id, `composer-discard:${key}`]);
      await user.click(within(question).getByRole("button", { name: "Discard" }));
      // Discarded: the composer closes, and with it the OS window.
      await waitFor(() => {
        expect(closes()).toEqual([{ type: "ClosePopOut", window: id }]);
      });
    } finally {
      setAutosaveDelay(null);
    }
  });

  it("asks before closing unsaved changes on Escape and on the OS close button", async () => {
    try {
      const user = userEvent.setup();
      const { page, id, key, subject, closes } = await poppedOutComposer();
      await user.type(subject, " again");
      await user.keyboard("{Escape}");
      const question = await findWindow(`composer-close:${key}`);
      expect(within(question).getByText(/has changes that are not saved yet/)).toBeVisible();
      await user.click(within(question).getByRole("button", { name: "Keep editing" }));
      await waitFor(() => {
        expect(windowEl(`composer-close:${key}`)).toBeNull();
      });
      expect(page.store.getState().windows).toEqual([id]);
      expect(closes()).toEqual([]);

      // The OS window's close button (Alt+F4): the shell asks the page.
      act(() => {
        globalThis.dispatchEvent(new Event(POP_OUT_CLOSE_EVENT));
      });
      const again = await findWindow(`composer-close:${key}`);
      // Asked again while the question is open: nothing more happens.
      act(() => {
        globalThis.dispatchEvent(new Event(POP_OUT_CLOSE_EVENT));
      });
      expect(page.store.getState().windows).toEqual([id, `composer-close:${key}`]);
      expect(closes()).toEqual([]);
      // "Save draft": saved, then the composer and the OS window close.
      await user.click(within(again).getByRole("button", { name: "Save draft" }));
      await waitFor(() => {
        expect(closes()).toEqual([{ type: "ClosePopOut", window: id }]);
      });
      const saved = [...page.transport.data.drafts.values()].map((d) => d.subject);
      expect(saved).toContain("Popped again");
    } finally {
      setAutosaveDelay(null);
    }
  });

  it("closes a saved pop-out at once when its OS window is closed", async () => {
    try {
      const { closes, id } = await poppedOutComposer();
      act(() => {
        globalThis.dispatchEvent(new Event(POP_OUT_CLOSE_EVENT));
      });
      await waitFor(() => {
        expect(closes()).toEqual([{ type: "ClosePopOut", window: id }]);
      });
    } finally {
      setAutosaveDelay(null);
    }
  });

  it("refuses to pop out the plain composer window and reads only draft contexts", async () => {
    const env = createEnv();
    await expect(prepareComposerPopOut(ctxOf(env), "composer")).rejects.toThrow();
    expect(draftOfPopOutContext({ draft: "d1" })).toBe("d1");
    expect(draftOfPopOutContext({ draft: 3 })).toBeNull();
    expect(draftOfPopOutContext(["d1"])).toBeNull();
    expect(draftOfPopOutContext(null)).toBeNull();
  });
});

describe("several composers: per-window readers", () => {
  it("shows the program-draft notice only in the composer of that draft", async () => {
    const env = createEnv();
    const ctx = ctxOf(env);
    openComposer(ctx, { ...emptyComposer(), identityId: "ada@example.org", draftId: "d-prog" });
    openComposer(ctx, { ...emptyComposer(), identityId: "ada@example.org", draftId: "d-mine" });
    systemStoreFor(env.store).getState().noteProgramDraft("d-prog", "Report tool");
    renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
    await waitFor(() => {
      expect(screen.getAllByRole("dialog")).toHaveLength(2);
    });
    const notes = await screen.findAllByRole("note", { name: /program/i });
    expect(notes).toHaveLength(1);
    const owner = notes[0]?.closest('[data-part="window"]');
    expect(owner?.getAttribute("data-window")).toBe("composer:d-prog");
  });
});
