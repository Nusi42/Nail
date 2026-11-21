/**
 * R2 review: the rules window in a pop-out gets the mail and the rule the
 * main page's message menus asked for (every webview has its own UI
 * store) — when it pops out and when it is opened again while popped out.
 */
import { act, screen, waitFor } from "@testing-library/react";
import type { JsonValue, PopOut } from "../../api/generated";
import { enableMockPopOuts } from "../../api/windowStateMock";
import { applyPopOuts } from "../../state/windowState";
import { renderWithProviders, settleQueries, type Env } from "../../test/harness";
import { FIRST, INBOX } from "../../test/mockIds";
import { PopOutHost } from "../popout";
import { PopOutRouter } from "../popout/PopOutRouter";
import { effects, RULES_WINDOW } from ".";
import { newRule } from "./model";
import { applyHandoff, handoffOf, rulesHandoff, RulesPopOutPreparer } from "./popOut";
import { rulesStore } from "./store";
import { setup } from "./testing";

const ORIGIN = { messageId: FIRST, folderId: INBOX };

function desktop(): Env {
  const env = setup();
  enableMockPopOuts(env.transport);
  // As `App.tsx` does.
  env.client.subscribe((event) => {
    if (event.type === "PopOutChanged") applyPopOuts(env.queryClient, event.pop_outs);
  });
  return env;
}

async function rulesContext(env: Env): Promise<JsonValue | null> {
  const response = await env.client.call({ type: "ListPopOuts" });
  const list: readonly PopOut[] = response.type === "PopOuts" ? response.pop_outs : [];
  return list.find((p) => p.window === RULES_WINDOW)?.context ?? null;
}

afterEach(() => {
  window.location.hash = "";
});

describe("the handover", () => {
  it("hands over the origin, and the rule only when one was asked for since the last one", () => {
    const store = rulesStore(setup().client);
    const key = {};
    store.getState().setOrigin(ORIGIN);
    store.getState().select("rust-announcements");
    const first = rulesHandoff(store.getState(), key);
    expect(first).toMatchObject({
      origin: ORIGIN,
      editing: { kind: "saved", id: "rust-announcements" },
    });
    // Nothing asked for since: the pop-out keeps its rule.
    const second = rulesHandoff(store.getState(), key);
    expect(second.editing).toBeNull();
    expect(second.id).not.toBe(first.id);
    // Asking for the rule already shown counts too.
    store.getState().select("rust-announcements");
    expect(rulesHandoff(store.getState(), key).editing).toEqual({
      kind: "saved",
      id: "rust-announcements",
    });
    const rule = { ...newRule(), name: "From the mail" };
    store.getState().startNew(rule);
    expect(rulesHandoff(store.getState(), key).editing).toEqual({ kind: "new", rule });
  });

  it("reads a handover from a context and applies it", () => {
    expect(handoffOf(null)).toBeNull();
    expect(handoffOf({ draft: "d1" })).toBeNull();
    expect(handoffOf([1])).toBeNull();
    const handoff = { id: "h1", origin: ORIGIN, editing: null };
    expect(handoffOf(handoff)).toEqual(handoff);
    const store = rulesStore(setup().client);
    store.getState().select("rust-announcements");
    applyHandoff(store.getState(), handoff);
    expect(store.getState().origin).toEqual(ORIGIN);
    expect(store.getState().editing).toMatchObject({ id: "rust-announcements" });
    applyHandoff(store.getState(), {
      id: "h2",
      origin: null,
      editing: { kind: "saved", id: "tag-all" },
    });
    expect(store.getState().origin).toBeNull();
    expect(store.getState().editing).toMatchObject({ kind: "saved", id: "tag-all" });
  });
});

describe("the main page", () => {
  it("hands the mail and rule to a rules window that is popped out already", async () => {
    expect(effects).toContain(RulesPopOutPreparer);
    const env = desktop();
    await env.client.call({ type: "PopOutWindow", window: RULES_WINDOW });
    renderWithProviders(
      <>
        <PopOutRouter />
        <RulesPopOutPreparer />
      </>,
      env,
    );
    await settleQueries(env);
    const rules = rulesStore(env.client);
    // What "Add to rule… → Rust announcements" does in the main page.
    act(() => {
      rules.getState().setOrigin(ORIGIN);
      rules.getState().select("rust-announcements");
      env.store.getState().openWindow(RULES_WINDOW);
    });
    await waitFor(async () => {
      expect(await rulesContext(env)).toMatchObject({
        origin: ORIGIN,
        editing: { kind: "saved", id: "rust-announcements" },
      });
    });
    // Not shown twice.
    expect(env.store.getState().windows).toEqual([]);
    // "Filter rules" from a menu: no mail, the pop-out keeps its rule.
    act(() => {
      rules.getState().setOrigin(null);
      env.store.getState().openWindow(RULES_WINDOW);
    });
    await waitFor(async () => {
      expect(await rulesContext(env)).toMatchObject({ origin: null, editing: null });
    });
  });
});

describe("the pop-out page", () => {
  it("shows the handed-over rule with the mail, and each later handover once", async () => {
    const env = desktop();
    await env.client.call({
      type: "PopOutWindow",
      window: RULES_WINDOW,
      context: { id: "h1", origin: ORIGIN, editing: { kind: "saved", id: "rust-announcements" } },
    });
    window.location.hash = `#/window/${RULES_WINDOW}`;
    renderWithProviders(
      <PopOutHost layout={env.transport.data.layout} window={RULES_WINDOW} />,
      env,
    );
    await settleQueries(env);
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("Rust announcements");
    const rules = rulesStore(env.client);
    expect(rules.getState().origin).toEqual(ORIGIN);
    // The user opens no rule; a moved window (same handover) changes nothing.
    act(() => {
      rules.getState().select(null);
    });
    await act(async () => {
      await env.client.call({
        type: "PopOutWindow",
        window: RULES_WINDOW,
        context: { id: "h1", origin: ORIGIN, editing: { kind: "saved", id: "rust-announcements" } },
      });
    });
    expect(rules.getState().editing).toBeNull();
    // "Create rule from message" in the main page: a new handover.
    const rule = { ...newRule(), name: "From the mail" };
    await act(async () => {
      await env.client.call({
        type: "PopOutWindow",
        window: RULES_WINDOW,
        context: {
          id: "h2",
          origin: ORIGIN,
          editing: { kind: "new", rule },
        } as unknown as JsonValue,
      });
    });
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("From the mail");
  });
});
