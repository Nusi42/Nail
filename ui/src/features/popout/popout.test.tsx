/**
 * Pop-out windows (M5c WP173) against the mock core: the button only where
 * a desktop host exists and the spec allows it, popping out (preparer,
 * title, context, off the page), the pop-out page (title → document title
 * → taskbar, "Return to main window", closing from the content), the main
 * page following `PopOutChanged`, the router sending windows to their OS
 * window, the actions, and axe on the pop-out page.
 */
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Event as NailEvent, JsonValue, PopOut, Request } from "../../api/generated";
import { enableMockPopOuts } from "../../api/windowStateMock";
import type { DialogProps } from "../../layout/dialogRegistry";
import { DialogRegistryContext } from "../../layout/dialogRegistry";
import { DEFAULT_WINDOW_SPEC } from "../../state/windows";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../test/harness";
import { appRegistry } from "../registry";
import {
  POP_OUT_ACTION,
  POPOUT_BASE_CSS,
  PopOutButton,
  PopOutHost,
  RETURN_ACTION,
  actionInfo,
  actions,
  effects,
  events,
  registerPopOutPreparer,
  usePopOutContext,
  usePopOutTitle,
} from ".";
import {
  errorMessageId,
  isPopOutPage,
  isPoppedOut,
  pageEffectOf,
  popOutContextOf,
  popOutPageWindow,
  preparerOf,
  probeSaysAvailable,
} from "./model";
import { PopOutRouter } from "./PopOutRouter";
import { must } from "../../test/must";

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
  window.location.hash = "";
  document.title = "";
});

function desktopEnv(state: Parameters<typeof createEnv>[0] = {}): Env {
  const env = createEnv(state);
  enableMockPopOuts(env.transport);
  return env;
}

/** Records the requests the UI sends. */
function requests(env: Env): Request[] {
  const sent: Request[] = [];
  const send = env.transport.send.bind(env.transport);
  vi.spyOn(env.transport, "send").mockImplementation((request: Request) => {
    sent.push(request);
    return send(request);
  });
  return sent;
}

function popOuts(env: Env): Promise<readonly PopOut[]> {
  return env.client
    .call({ type: "ListPopOuts" })
    .then((r) => (r.type === "PopOuts" ? r.pop_outs : []));
}

const ctxOf = (env: Env) => ({
  client: env.client,
  store: env.store,
  queryClient: env.queryClient,
});

describe("pop-out model", () => {
  it("reads the page, the page effects, contexts and errors", () => {
    expect(popOutPageWindow("#/window/composer:d42")).toBe("composer:d42");
    expect(popOutPageWindow("#/inbox")).toBeNull();
    expect(popOutPageWindow("#/window/Bad Kind")).toBeNull();
    expect(isPopOutPage("#/window/settings")).toBe(true);
    expect(isPopOutPage("")).toBe(false);
    expect(pageEffectOf("opened")).toBe("close");
    expect(pageEffectOf("returned")).toBe("open");
    for (const change of ["moved", "focused", "closed"] as const) {
      expect(pageEffectOf(change)).toBeNull();
    }
    const list: PopOut[] = [
      { window: "composer:d1", geometry: null, context: { draft: "d1" }, focused: true },
      { window: "settings", geometry: null, context: null, focused: false },
    ];
    expect(popOutContextOf(list, "composer:d1")).toEqual({ draft: "d1" });
    expect(popOutContextOf(list, "settings")).toBeNull();
    expect(popOutContextOf(list, "rules")).toBeNull();
    expect(isPoppedOut(list, "settings")).toBe(true);
    expect(isPoppedOut(list, "composer")).toBe(false);
    expect(probeSaysAvailable({ type: "Error", code: "conflict", message: "" })).toBe(true);
    expect(probeSaysAvailable({ type: "Error", code: "unsupported", message: "" })).toBe(false);
    expect(probeSaysAvailable({ type: "Error", code: "forbidden", message: "" })).toBe(false);
    expect(errorMessageId({ localized: { id: "pop-out-issue-too-many" } })).toBe(
      "pop-out-issue-too-many",
    );
    expect(errorMessageId(new Error("x"))).toBe("popout-failed");
    expect(POPOUT_BASE_CSS).toContain('[data-part="pop-out"] { position: fixed; inset: 0;');
  });

  it("keeps one preparer per kind", () => {
    const first = vi.fn();
    const second = vi.fn();
    const undoFirst = registerPopOutPreparer("rules", first);
    expect(preparerOf("rules")).toBe(first);
    const undoSecond = registerPopOutPreparer("rules", second);
    expect(preparerOf("rules:w2")).toBe(second);
    undoFirst();
    expect(preparerOf("rules")).toBe(second);
    undoSecond();
    expect(preparerOf("rules")).toBeUndefined();
  });

  it("describes its actions for the palette", () => {
    expect(appRegistry.actions[POP_OUT_ACTION]).toBe(actions[POP_OUT_ACTION]);
    expect(appRegistry.actionInfo[RETURN_ACTION]?.title).toBe("popout-action-return");
    expect(actionInfo[POP_OUT_ACTION]?.availability).toEqual(["desktop"]);
  });
});

describe("the pop-out button", () => {
  it("is hidden without a desktop host (the browser UI)", async () => {
    const env = createEnv();
    renderWithProviders(<PopOutButton window="settings" />, env);
    await settleQueries(env);
    expect(document.querySelector('[data-part="window.pop-out"]')).toBeNull();
  });

  it("is hidden for windows that never pop out", async () => {
    const env = desktopEnv();
    renderWithProviders(
      <>
        <PopOutButton window="move" />
        <PopOutButton window="about-nail" spec={{ ...DEFAULT_WINDOW_SPEC, pop_out: false }} />
        <PopOutButton window="rules" />
      </>,
      env,
    );
    await settleQueries(env);
    const buttons = document.querySelectorAll('[data-part="window.pop-out"]');
    expect(buttons).toHaveLength(1);
  });

  it("prepares, pops the window out with its title and context, and takes it off the page", async () => {
    const env = desktopEnv();
    const sent = requests(env);
    act(() => {
      env.store.getState().openWindow("composer:d42");
    });
    const prepare = vi.fn(async (window: string): Promise<JsonValue> => {
      await Promise.resolve();
      return { draft: window.split(":")[1] ?? "" };
    });
    cleanups.push(registerPopOutPreparer("composer", prepare));
    renderWithProviders(<PopOutButton window="composer:d42" />, env);
    const button = await screen.findByRole("button", { name: "Open in new window" });
    await userEvent.click(button);
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(prepare).toHaveBeenCalledWith(
      "composer:d42",
      expect.objectContaining({ client: env.client }),
    );
    expect(sent).toContainEqual({
      type: "PopOutWindow",
      window: "composer:d42",
      title: "New message",
      context: { draft: "d42" },
      // The window spec's sizes: the OS window opens like the in-page one.
      size: { width: 820, height: 720 },
      min_size: { width: 520, height: 400 },
    });
    expect(await popOuts(env)).toMatchObject([
      { window: "composer:d42", context: { draft: "d42" }, focused: true },
    ]);
  });

  it("keeps the window in the page when popping out fails", async () => {
    const env = desktopEnv();
    act(() => {
      env.store.getState().openWindow("rules");
    });
    cleanups.push(
      registerPopOutPreparer("rules", () => {
        throw new Error("cannot save");
      }),
    );
    renderWithProviders(<PopOutButton window="rules" />, env);
    await userEvent.click(await screen.findByRole("button", { name: "Open in new window" }));
    const failed = await screen.findByRole("button", {
      name: /Open in new window \(.*This window could not be opened on its own\./,
    });
    expect(failed.getAttribute("data-state")).toBe("error");
    expect(env.store.getState().windows).toEqual(["rules"]);
    expect(await popOuts(env)).toEqual([]);
  });

  it("shows the core's reason (too large a context)", async () => {
    const env = desktopEnv();
    cleanups.push(registerPopOutPreparer("rules", () => ({ big: "x".repeat(20_000) })));
    renderWithProviders(<PopOutButton window="rules" />, env);
    await userEvent.click(await screen.findByRole("button", { name: "Open in new window" }));
    expect(
      await screen.findByRole("button", {
        name: /This window cannot be opened on its own \(too much data\)/,
      }),
    ).toBeTruthy();
  });
});

function TitledDialog({ close }: DialogProps) {
  usePopOutTitle("Re: Lunch on Friday");
  const context = usePopOutContext();
  return (
    <div>
      <p data-testid="context">{JSON.stringify(context)}</p>
      <button type="button" onClick={close}>
        done
      </button>
    </div>
  );
}

function PlainDialog() {
  return <p data-testid="plain">plain</p>;
}

function renderPopOutPage(env: Env, id: string, dialog = TitledDialog) {
  const layout = env.transport.data.layout;
  return renderWithProviders(
    <DialogRegistryContext.Provider value={{ ...appRegistry.dialogs, "about-nail": dialog }}>
      <PopOutHost layout={layout} window={id} />
    </DialogRegistryContext.Provider>,
    env,
  );
}

describe("the pop-out page", () => {
  it("names the OS window, reads its context and returns to the main window", async () => {
    const env = desktopEnv();
    await env.client.call({
      type: "PopOutWindow",
      window: "about-nail",
      context: { tab: "licences" },
    });
    window.location.hash = "#/window/about-nail";
    const sent = requests(env);
    renderPopOutPage(env, "about-nail");
    // The content's title wins over the window's (taskbar entry).
    await waitFor(() => {
      expect(document.title).toBe("Re: Lunch on Friday");
    });
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Re: Lunch on Friday");
    await waitFor(() => {
      expect(screen.getByTestId("context").textContent).toBe('{"tab":"licences"}');
    });
    // Open in this webview's own store.
    expect(env.store.getState().windows).toEqual(["about-nail"]);
    await userEvent.click(screen.getByRole("button", { name: "Return to main window" }));
    await waitFor(() => {
      expect(sent).toContainEqual({ type: "ReturnPopOut", window: "about-nail" });
    });
    expect(await popOuts(env)).toEqual([]);
  });

  it("uses the window's title without one from the content", async () => {
    const env = desktopEnv();
    renderPopOutPage(env, "about-nail", PlainDialog);
    await waitFor(() => {
      expect(document.title).toBe("About Nail");
    });
    expect(screen.getByTestId("plain")).toBeTruthy();
  });

  it("closes the OS window when the content closes the window", async () => {
    const env = desktopEnv();
    await env.client.call({ type: "PopOutWindow", window: "about-nail" });
    const sent = requests(env);
    renderPopOutPage(env, "about-nail");
    await userEvent.click(await screen.findByRole("button", { name: "done" }));
    await waitFor(() => {
      expect(sent).toContainEqual({ type: "ClosePopOut", window: "about-nail" });
    });
    expect(await popOuts(env)).toEqual([]);
  });

  it("shows why returning failed", async () => {
    const env = desktopEnv();
    // Not popped out (e.g. closed meanwhile): the core refuses.
    renderPopOutPage(env, "about-nail", PlainDialog);
    await userEvent.click(screen.getByRole("button", { name: "Return to main window" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "This window is not open on its own.",
    );
  });

  it("passes axe", async () => {
    const env = desktopEnv();
    const { container } = renderPopOutPage(env, "about-nail");
    await settleQueries(env);
    const results = await axe.run(container);
    expect(results.violations).toEqual([]);
  });
});

function popOutChanged(window: string, change: "opened" | "returned" | "closed"): NailEvent {
  return { type: "PopOutChanged", window, change, pop_outs: [] };
}

describe("the main page", () => {
  it("takes a popped-out window off the page and puts a returned one back", () => {
    const env = createEnv();
    const ctx = { client: env.client, queryClient: env.queryClient, store: env.store };
    const handle = (event: NailEvent) => {
      for (const handler of events) handler(event, ctx);
    };
    act(() => {
      env.store.getState().openWindow("settings");
    });
    act(() => {
      handle(popOutChanged("settings", "opened"));
    });
    expect(env.store.getState().windows).toEqual([]);
    act(() => {
      handle(popOutChanged("settings", "closed"));
    });
    expect(env.store.getState().windows).toEqual([]);
    act(() => {
      handle(popOutChanged("settings", "returned"));
    });
    expect(env.store.getState().windows).toEqual(["settings"]);
    // A pop-out page leaves its store alone.
    window.location.hash = "#/window/settings";
    act(() => {
      handle(popOutChanged("settings", "opened"));
    });
    expect(env.store.getState().windows).toEqual(["settings"]);
  });
});

describe("the router", () => {
  /** The router on a desktop where the window `kind` was popped out last time. */
  async function lastPoppedOut(kind: string) {
    const env = desktopEnv();
    await env.client.call({
      type: "SetWindowState",
      window: kind,
      state: { page: null, pop_out: null, popped_out: true },
    });
    const sent = requests(env);
    renderRouter(env);
    await settleQueries(env);
    return { env, sent };
  }

  function renderRouter(env: Env) {
    expect(effects).toContain(PopOutRouter);
    return renderWithProviders(<PopOutRouter />, env);
  }

  it("sends a window whose kind was last popped out to its OS window", async () => {
    const { env, sent } = await lastPoppedOut("rules");
    act(() => {
      env.store.getState().openWindow("rules");
    });
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(sent).toContainEqual({
      type: "PopOutWindow",
      window: "rules",
      title: "Filter rules",
      size: { width: 900, height: 640 },
      min_size: { width: 600, height: 420 },
    });
    expect(await popOuts(env)).toMatchObject([{ window: "rules" }]);
  });

  it("keeps a kind in the page that was popped out before popping out became opt-in", async () => {
    const { env, sent } = await lastPoppedOut("pim-event-editor");
    act(() => {
      env.store.getState().openWindow("pim-event-editor");
    });
    await settleQueries(env);
    expect(env.store.getState().windows).toEqual(["pim-event-editor"]);
    expect(sent.some((r) => r.type === "PopOutWindow")).toBe(false);
  });

  it("raises a window that is popped out instead of showing it twice", async () => {
    const env = desktopEnv();
    await env.client.call({ type: "PopOutWindow", window: "settings" });
    await env.client.call({ type: "PopOutWindow", window: "rules" });
    const sent = requests(env);
    renderRouter(env);
    await settleQueries(env);
    act(() => {
      env.store.getState().openWindow("settings");
    });
    await waitFor(() => {
      expect(sent).toContainEqual({ type: "FocusPopOut", window: "settings" });
    });
    expect(env.store.getState().windows).toEqual([]);
    expect((await popOuts(env)).find((p) => p.focused)?.window).toBe("settings");
  });

  it("leaves windows in the page in the browser and for kinds shown in the page", async () => {
    const browser = createEnv();
    renderRouter(browser);
    await settleQueries(browser);
    act(() => {
      browser.store.getState().openWindow("rules");
    });
    await settleQueries(browser);
    expect(browser.store.getState().windows).toEqual(["rules"]);

    const env = desktopEnv();
    const sent = requests(env);
    renderRouter(env);
    await settleQueries(env);
    act(() => {
      env.store.getState().openWindow("settings");
    });
    await settleQueries(env);
    expect(env.store.getState().windows).toEqual(["settings"]);
    expect(sent.some((r) => r.type === "PopOutWindow")).toBe(false);
  });
});

describe("the actions", () => {
  const layout = () => createEnv().transport.data.layout;

  it("pop out the focused window and return the pop-out page's window", async () => {
    const env = desktopEnv();
    const action = must(actions[POP_OUT_ACTION]);
    const ctx = { ...ctxOf(env), layout: layout(), t: (id: string) => id, locale: "en" };
    expect(action.enabled?.(ctx, [])).toBe(false);
    act(() => {
      env.store.getState().openWindow("rules");
      env.store.getState().openWindow("settings");
    });
    expect(action.enabled?.(ctx, [])).toBe(true);
    await act(async () => {
      await action.run(ctx, []);
    });
    expect(env.store.getState().windows).toEqual(["rules"]);
    expect((await popOuts(env)).map((p) => p.window)).toEqual(["settings"]);

    const back = must(actions[RETURN_ACTION]);
    expect(back.enabled?.(ctx, [])).toBe(false);
    expect(back.enabled?.(ctx, ["settings"])).toBe(true);
    window.location.hash = "#/window/settings";
    expect(back.enabled?.(ctx, [])).toBe(true);
    expect(action.enabled?.(ctx, [])).toBe(false);
    await act(async () => {
      await back.run(ctx, []);
    });
    expect(await popOuts(env)).toEqual([]);
  });

  it("never pops out a window whose spec keeps it in the page", async () => {
    window.location.hash = "";
    const env = desktopEnv();
    const action = must(actions[POP_OUT_ACTION]);
    const ctx = { ...ctxOf(env), layout: layout(), t: (id: string) => id, locale: "en" };
    // An editor of the main page's selection: its state would not travel.
    act(() => {
      env.store.getState().openWindow("pim-event-editor");
    });
    expect(action.enabled?.(ctx, [])).toBe(false);
    await act(async () => {
      await action.run(ctx, []);
    });
    expect(env.store.getState().windows).toEqual(["pim-event-editor"]);
    expect(await popOuts(env)).toEqual([]);
  });
});
