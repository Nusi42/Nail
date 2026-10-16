/**
 * The central registry merges the feature slots, refuses duplicates, and
 * the app shell uses what the slots contribute (dialogs, events, effects).
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { App } from "../App";
import { builtinActions } from "../actions/registry";
import { COMPOSER_REQUESTS, isComposerRequest } from "./composer/mock";
import { CUSTOMIZATION_REQUESTS } from "./customization/mock";
import { MCP_REQUESTS } from "./mcp/mock";
import { REMOTE_REQUESTS } from "./remote/mock";
import { RULES_REQUESTS } from "./rules/mock";
import { SECURITY_REQUESTS } from "./security/mock";
import { VAULT_REQUESTS } from "./security/vaultMock";
import { PALETTE_REQUESTS } from "./palette/mock";
import { PORTABILITY_REQUESTS } from "./portability/mock";
import { PRINT_REQUESTS } from "./print/mock";
import { SYSTEM_REQUESTS } from "./system/mock";
import { builtinWidgets } from "../widgets";
import { appRegistry, features, mergeFeatures } from "./registry";
import type { Feature } from "./types";
import { createEnv, setWindowWidth, stubElementSizes } from "../test/harness";
import type { DialogProps } from "../layout/dialogRegistry";
import { shippedCatalogs } from "../i18n/catalogs";
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../api/normalize";
import { LUCIDE_ICONS } from "../widgets/lucideIcons";

const TEST_DIALOG_TITLE = "window-test-dialog = Test dialog";

const empty: Feature = { widgets: {}, actions: {}, dialogs: {}, events: [], effects: [] };

describe("feature registry", () => {
  it("has the M1b … M5d, P2, P3, connection log and source slots and contains every built-in", () => {
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
      "exchange",
      "agents",
      "system",
      "portability",
      "palette",
      "print",
      "appearance",
      "shell",
      "settings",
      "mailWidgets",
      "windows",
      "popout",
      "layoutEditor",
      "toolbarEditor",
      "conversationList",
      "conversationReader",
      "mailActions",
      "connlog",
      "messageSource",
      "folders",
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
    expect(merged.actionInfo).toEqual({});
    expect(merged.settings).toEqual({});

    // M5b: settings labels per widget kind merge and refuse duplicates.
    const labels = { labels: ["junk-settings-enabled"] };
    expect(
      mergeFeatures(
        { widgets: {}, actions: {} },
        { a: { ...empty, settings: { "junk-settings": labels } } },
      ).settings,
    ).toEqual({ "junk-settings": labels });
    expect(() =>
      mergeFeatures(
        { widgets: {}, actions: {} },
        {
          a: { ...empty, settings: { x: labels } },
          b: { ...empty, settings: { x: labels } },
        },
      ),
    ).toThrow(/settings info "x"/);

    // M5c: window registrations merge and refuse duplicates.
    expect(merged.windows).toEqual({});
    expect(
      mergeFeatures(
        { widgets: {}, actions: {} },
        { a: { ...empty, windows: { "x-confirm": { confirmation: true } } } },
      ).windows,
    ).toEqual({ "x-confirm": { confirmation: true } });
    expect(() =>
      mergeFeatures(
        { widgets: {}, actions: {} },
        {
          a: { ...empty, windows: { x: { modal: false } } },
          b: { ...empty, windows: { x: { modal: true } } },
        },
      ),
    ).toThrow(/window "x"/);

    // M4c: palette descriptions merge like actions and refuse duplicates.
    const info = { title: "rules-run-title" };
    expect(
      mergeFeatures(
        { widgets: {}, actions: {} },
        { a: { ...empty, actionInfo: { "rules.run": info } } },
      ).actionInfo,
    ).toEqual({ "rules.run": info });
    expect(() =>
      mergeFeatures(
        { widgets: {}, actions: {} },
        {
          a: { ...empty, actionInfo: { x: info } },
          b: { ...empty, actionInfo: { x: info } },
        },
      ),
    ).toThrow(/action info "x"/);

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
      SYSTEM_REQUESTS,
      PORTABILITY_REQUESTS,
      PALETTE_REQUESTS,
      PRINT_REQUESTS,
      VAULT_REQUESTS,
    ];
    const all: string[] = lists.flat();
    expect(new Set(all).size).toBe(all.length);
    expect(isComposerRequest({ type: "ListRules" })).toBe(false);
  });
});

describe("action icons (M5b)", () => {
  it("gives every registered action a bundled Lucide icon", () => {
    const layout = normalizeLayout(defaultLayoutJson);
    const missing = Object.keys(appRegistry.actions).filter((id) => {
      const icon = layout.actions[id]?.icon ?? appRegistry.actionInfo[id]?.icon ?? null;
      return icon === null || !Object.hasOwn(LUCIDE_ICONS, icon);
    });
    expect(missing).toEqual([]);
    // Icons named by feature descriptions are bundled too.
    for (const [id, info] of Object.entries(appRegistry.actionInfo)) {
      if (info.icon !== undefined) expect(Object.hasOwn(LUCIDE_ICONS, info.icon), id).toBe(true);
    }
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
    shippedCatalogs.en = (shippedCatalogs.en ?? []).filter(
      (source) => source !== TEST_DIALOG_TITLE,
    );
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
    // The window title of the fixture dialog (`window-<name>`).
    shippedCatalogs.en = [...(shippedCatalogs.en ?? []), TEST_DIALOG_TITLE];

    const env = createEnv();
    render(<App client={env.client} queryClient={env.queryClient} store={env.store} />);
    expect((await screen.findAllByRole("treeitem", { name: /Inbox/ })).length).toBeGreaterThan(0);
    expect(mounted).toBe(1);

    act(() => {
      env.transport.emit({ type: "RulesChanged" });
    });
    expect(seen).toContain("RulesChanged");

    act(() => {
      env.store.getState().openWindow("test-dialog");
    });
    const button = await screen.findByRole("button", { name: "test-dialog" });
    act(() => {
      button.click();
    });
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
  });
});
