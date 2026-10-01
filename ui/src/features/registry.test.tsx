/**
 * The central registry merges the feature slots, refuses duplicates, and
 * the app shell uses what the slots contribute (dialogs, events, effects).
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { App } from "../App";
import { builtinActions } from "../actions/registry";
import type { Request } from "../api/generated";
import { COMPOSER_REQUESTS, isComposerRequest } from "./composer/mock";
import { CUSTOMIZATION_REQUESTS } from "./customization/mock";
import { MCP_REQUESTS } from "./mcp/mock";
import { REMOTE_REQUESTS } from "./remote/mock";
import { RULES_REQUESTS } from "./rules/mock";
import { SECURITY_REQUESTS } from "./security/mock";
import { builtinWidgets } from "../widgets";
import { appRegistry, features, mergeFeatures } from "./registry";
import type { Feature } from "./types";
import { createEnv, setWindowWidth, stubElementSizes } from "../test/harness";
import type { DialogProps } from "../layout/dialogRegistry";

const empty: Feature = { widgets: {}, actions: {}, dialogs: {}, events: [], effects: [] };

describe("feature registry", () => {
  it("has the M1b, M2a, M2b, M2c, M2d, M3a and M3b slots and contains every built-in", () => {
    expect(Object.keys(features)).toEqual([
      "composer",
      "rules",
      "mcp",
      "customization",
      "security",
      "remote",
      "launch",
      "sieve",
      "triage",
      "sync",
      "menus",
      "plugins",
      "pim",
      "scheduling",
      "gmail",
    ]);
    for (const kind of Object.keys(builtinWidgets)) {
      expect(appRegistry.widgets[kind]).toBe(builtinWidgets[kind]);
    }
    for (const id of Object.keys(builtinActions)) {
      expect(appRegistry.actions[id]).toBe(builtinActions[id]);
    }
  });

  it("merges feature contributions and refuses duplicates", () => {
    const Widget = () => null;
    const Dialog = () => null;
    const handler = { run: () => undefined };
    const merged = mergeFeatures(
      { widgets: { text: Widget }, actions: { "mail.next": handler } },
      {
        a: { ...empty, widgets: { "rule-list": Widget }, dialogs: { confirm: Dialog } },
        b: { ...empty, actions: { "rules.run": handler }, events: [() => undefined] },
      },
    );
    expect(Object.keys(merged.widgets).sort()).toEqual(["rule-list", "text"]);
    expect(Object.keys(merged.actions).sort()).toEqual(["mail.next", "rules.run"]);
    expect(Object.keys(merged.dialogs)).toEqual(["confirm"]);
    expect(merged.events).toHaveLength(1);

    expect(() =>
      mergeFeatures(
        { widgets: { text: Widget }, actions: {} },
        { a: { ...empty, widgets: { text: Widget } } },
      ),
    ).toThrow(/widget "text" is registered by both the built-ins and feature "a"/);
    expect(() =>
      mergeFeatures(
        { widgets: {}, actions: {} },
        {
          a: { ...empty, dialogs: { x: Dialog } },
          b: { ...empty, dialogs: { x: Dialog } },
        },
      ),
    ).toThrow(/dialog "x"/);
  });

  it("routes each feature request to exactly one feature mock", () => {
    const lists = [
      COMPOSER_REQUESTS,
      RULES_REQUESTS,
      MCP_REQUESTS,
      CUSTOMIZATION_REQUESTS,
      SECURITY_REQUESTS,
      REMOTE_REQUESTS,
    ];
    const all: string[] = lists.flat();
    expect(new Set(all).size).toBe(all.length);
    expect(isComposerRequest({ type: "ListRules" } as Request)).toBe(false);
  });
});

describe("feature slots in the app shell", () => {
  let restoreSizes: () => void;
  beforeEach(() => {
    restoreSizes = stubElementSizes();
    setWindowWidth(1200);
  });
  afterEach(() => {
    restoreSizes();
    setWindowWidth(1024);
    for (const name of ["events", "effects"] as const) {
      (appRegistry[name] as unknown[]).length = 0;
    }
    delete (appRegistry.dialogs as Record<string, unknown>)["test-dialog"];
  });

  it("opens a feature dialog, runs effects and passes events", async () => {
    const seen: string[] = [];
    let mounted = 0;
    function Effect() {
      useEffect(() => {
        mounted++;
      }, []);
      return null;
    }
    function TestDialog({ name, close }: DialogProps) {
      return (
        <button type="button" data-part="test-dialog" onClick={close}>
          {name}
        </button>
      );
    }
    (appRegistry.events as unknown[]).push((event: { type: string }) => seen.push(event.type));
    (appRegistry.effects as unknown[]).push(Effect);
    (appRegistry.dialogs as Record<string, unknown>)["test-dialog"] = TestDialog;

    const env = createEnv();
    render(<App client={env.client} queryClient={env.queryClient} store={env.store} />);
    await screen.findByRole("treeitem", { name: /Inbox/ });
    expect(mounted).toBe(1);

    act(() => env.transport.emit({ type: "RulesChanged" }));
    expect(seen).toContain("RulesChanged");

    act(() => env.store.getState().openWindow("test-dialog"));
    const button = await screen.findByRole("button", { name: "test-dialog" });
    act(() => button.click());
    await waitFor(() => expect(env.store.getState().windows).toEqual([]));
  });
});
