import { QueryClient } from "@tanstack/react-query";
import { createDispatcher, type ActionContext } from "../../actions/registry";
import { TransportClient } from "../../api/client";
import type { PluginInfo, ResolvedLayout } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { createUiStore } from "../../state/uiStore";
import { resolveMenu } from "../menus/resolve";
import { appRegistry } from "../registry";
import { RUN_COMMAND, findCommand, isRunnable, surfaceFor, withPluginActions } from "./commands";
import { clientIdOf, isForThisUi, surfaceOfKeyContext, uiContextOf } from "./context";
import { allGranted, grantsValid, orderedGrants, setAccounts, setGranted } from "./grants";
import { isPluginName, parsePluginCommandId, parsePluginPanelId, pluginCommandId } from "./ids";
import { permissionArgs } from "./messages";
import { followUpManifest } from "./mockHost";
import { installFollowUp, must } from "./testUtil";
import { PLUGIN_BASE_CSS, PLUGIN_PARTS, pluginPartSelector } from "./parts";
import { insertItem, placementText } from "./placementText";
import { appendLogLine, pluginKeys } from "./queries";
import { MAX_TOASTS, createPluginsStore } from "./store";
import { parseInline, parseMarkdown, safeHref } from "./tree/markdown";
import {
  checkNode,
  dateDefault,
  initialValues,
  mergeValues,
  parseAddress,
  validate,
} from "./tree/values";
import { objectContaining } from "../../test/matchers";

function plugin(extra: Partial<PluginInfo> = {}): PluginInfo {
  return {
    id: "follow-up",
    manifest: followUpManifest(),
    source: { kind: "file", path: "C:/p.nailplugin" },
    enabled: true,
    state: "running",
    grants: [],
    storage: { keys: 0, used_bytes: 0, quota_bytes: 1 },
    package_sha256: "a".repeat(64),
    installed_at: "2026-09-30T00:00:00Z",
    updated_at: "2026-09-30T00:00:00Z",
    error: null,
    ...extra,
  };
}

describe("plug-in ids", () => {
  it("follows the contract's naming rules", () => {
    expect(isPluginName("follow-up")).toBe(true);
    for (const bad of ["", "Follow", "a--b", "a-", "1a", "a.b", "x".repeat(65)]) {
      expect(isPluginName(bad)).toBe(false);
    }
    expect(pluginCommandId("follow-up", "remind")).toBe("plugin.follow-up.remind");
    expect(parsePluginCommandId("plugin.follow-up.remind")).toEqual({
      plugin: "follow-up",
      command: "remind",
    });
    expect(parsePluginCommandId("mail.reply")).toBeNull();
    expect(parsePluginCommandId("plugin.follow-up")).toBeNull();
    expect(parsePluginCommandId("plugin.a.b.c")).toBeNull();
    expect(parsePluginPanelId("follow-up.reminders")).toEqual({
      plugin: "follow-up",
      panel: "reminders",
    });
    expect(parsePluginPanelId("follow-up")).toBeNull();
  });
});

describe("plug-in parts", () => {
  it("maps namespaced parts to the generic part and the plug-in", () => {
    expect(pluginPartSelector("plugin.follow-up.button")).toBe(
      '[data-part="plugin.button"][data-plugin="follow-up"]',
    );
    expect(pluginPartSelector("plugin.follow-up.dialog.body")).toBe(
      '[data-part="plugin.dialog.body"][data-plugin="follow-up"]',
    );
    expect(pluginPartSelector("plugin.button")).toBeNull();
    expect(pluginPartSelector("plugin.follow-up.nope")).toBeNull();
    expect(pluginPartSelector("message-list.row")).toBeNull();
    expect(new Set(PLUGIN_PARTS).size).toBe(PLUGIN_PARTS.length);
  });

  it("gives the base CSS only for known parts", () => {
    const parts = [...PLUGIN_BASE_CSS.matchAll(/data-part="([^"]+)"/g)].map((m) => m[1] ?? "");
    expect(parts.length).toBeGreaterThan(10);
    for (const part of parts) {
      expect(
        (PLUGIN_PARTS as readonly string[]).includes(part) || /^plugin-[a-z]+/.test(part),
        part,
      ).toBe(true);
    }
    expect(PLUGIN_BASE_CSS).not.toMatch(/url\(|@import|expression/);
  });
});

describe("context", () => {
  it("gives each client one id and routes by it", () => {
    const a = new TransportClient(new MockTransport());
    const b = new TransportClient(new MockTransport());
    expect(clientIdOf(a)).toBe(clientIdOf(a));
    expect(clientIdOf(a)).not.toBe(clientIdOf(b));
    expect(isForThisUi(a, null)).toBe(true);
    expect(isForThisUi(a, clientIdOf(a))).toBe(true);
    expect(isForThisUi(a, clientIdOf(b))).toBe(false);
  });

  it("builds the UI context and surface", () => {
    const store = createUiStore({ activeAccountId: "acc", selectedMessageId: "m1" });
    expect(uiContextOf(store.getState())).toMatchObject({
      account_id: "acc",
      message_id: "m1",
      selected_message_ids: ["m1"],
      selected_text: null,
      draft_id: null,
    });
    expect(surfaceOfKeyContext("reader")).toBe("reader");
    expect(surfaceOfKeyContext("window")).toBe("global");
  });
});

describe("plug-in commands", () => {
  const layout = (): ResolvedLayout => ({
    name: "default",
    windows: {},
    actions: {
      "plugin.follow-up.remind": { label: "Later", icon: null, alias_of: null, args: [] },
    },
    keys: [],
    toolbars: {},
    menus: {},
    toolbar_settings: {},
    views: {},
    default_view: null,
    rail: [],
    settings: [],
    window_frames: {},
    presets: [],
  });

  it("adds an action definition per command, keeping the user's label", () => {
    const base = layout();
    expect(withPluginActions(base, [])).toBe(base);
    expect(withPluginActions(base, undefined)).toBe(base);
    const next = withPluginActions(base, [plugin()]);
    expect(next.actions["plugin.follow-up.remind"]).toEqual({
      label: "Later",
      icon: "flag",
      alias_of: RUN_COMMAND,
      args: ["plugin.follow-up.remind"],
    });
    delete base.actions["plugin.follow-up.remind"];
    expect(withPluginActions(base, [plugin()]).actions["plugin.follow-up.remind"]?.label).toBe(
      "Follow up",
    );
    base.actions["plugin.follow-up.remind"] = {
      label: null,
      icon: null,
      alias_of: "mail.reply",
      args: [],
    };
    expect(withPluginActions(base, [plugin()])).toBe(base);
  });

  it("labels plug-in items of context menus and shows their keys", () => {
    const base = layout();
    delete base.actions["plugin.follow-up.remind"];
    base.menus["context:message-list.row"] = [
      { type: "action", action: "plugin.follow-up.remind", args: [], label: null, icon: null },
    ];
    base.keys = [{ context: "list", key: "shift+r", action: "plugin.follow-up.remind", args: [] }];
    const menu = resolveMenu(
      withPluginActions(base, [plugin()]),
      "context:message-list.row",
      "list",
    );
    expect(menu?.items[0]).toMatchObject({
      action: "plugin.follow-up.remind",
      label: "Follow up",
      icon: "flag",
      shortcut: "shift+r",
    });
  });

  it("knows when a command can run", () => {
    expect(isRunnable(plugin())).toBe(true);
    expect(isRunnable(plugin({ enabled: false, state: "disabled" }))).toBe(false);
    expect(isRunnable(plugin({ state: "crashed" }))).toBe(false);
    const found = findCommand([plugin()], "plugin.follow-up.remind");
    expect(found?.command.id).toBe("remind");
    expect(findCommand([plugin()], "plugin.follow-up.nope")).toBeNull();
    const command = must(found).command;
    const none = { keyContext: "list", selectedMessageId: null, composer: null };
    expect(surfaceFor(command, none)).toBeNull();
    expect(surfaceFor(command, { ...none, selectedMessageId: "m" })).toBe("list");
    expect(surfaceFor(command, { ...none, keyContext: "global", selectedMessageId: "m" })).toBe(
      "list",
    );
    expect(surfaceFor({ id: "x", title: "X" }, none)).toBe("list");
    expect(surfaceFor({ id: "x", title: "X", contexts: ["composer"] }, none)).toBeNull();
  });

  it("dispatches plug-in actions through the alias to RunPluginCommand", async () => {
    const transport = new MockTransport();
    const client = new TransportClient(transport);
    const store = createUiStore({ selectedMessageId: "m1", keyContext: "reader" });
    const queryClient = new QueryClient();
    // The mock core runs the command once the plug-in is installed there (its
    // own query cache: this one starts without the plug-in list).
    await installFollowUp({ transport, client, store, queryClient: new QueryClient() });
    const ctx: ActionContext = {
      client,
      store,
      queryClient,
      layout: withPluginActions(layout(), [plugin()]),
      t: (id) => id,
      locale: "en",
    };
    const dispatcher = createDispatcher(ctx, appRegistry.actions);
    // Not known yet: the plug-in list is not loaded.
    expect(dispatcher.isEnabled("plugin.follow-up.remind")).toBe(false);
    queryClient.setQueryData(pluginKeys.list, [plugin()]);
    expect(dispatcher.isEnabled("plugin.follow-up.remind")).toBe(true);
    await dispatcher.dispatch("plugin.follow-up.remind");
    expect(transport.requests.at(-1)).toEqual({
      type: "RunPluginCommand",
      command: "plugin.follow-up.remind",
      surface: "reader",
      context: objectContaining({ message_id: "m1" }),
      client_id: clientIdOf(client),
    });
    queryClient.setQueryData(pluginKeys.list, [plugin({ enabled: false, state: "disabled" })]);
    expect(dispatcher.isEnabled("plugin.follow-up.remind")).toBe(false);
    store.getState().selectMessage(null);
    queryClient.setQueryData(pluginKeys.list, [plugin()]);
    expect(dispatcher.isEnabled("plugin.follow-up.remind")).toBe(false);
  });
});

describe("grants", () => {
  it("grants, denies and limits permissions", () => {
    const all = allGranted(["mail.read", "mail.read.body", "storage"]);
    expect(all).toHaveLength(3);
    const noRead = setGranted(all, "mail.read", false);
    expect(noRead.map((g) => g.permission)).toEqual(["storage"]);
    expect(setGranted(noRead, "mail.read.body", true)).toEqual(noRead);
    const limited = setAccounts(all, "mail.read", []);
    expect(grantsValid(limited)).toBe(false);
    expect(grantsValid(setAccounts(all, "mail.read", ["acc"]))).toBe(true);
    expect(
      orderedGrants(
        ["storage", "mail.read"],
        [
          { permission: "mail.read", accounts: ["a"] },
          { permission: "storage", accounts: ["x"] },
        ],
      ),
    ).toEqual([
      { permission: "storage", accounts: null },
      { permission: "mail.read", accounts: ["a"] },
    ]);
    expect(permissionArgs("network:api.example.com")).toEqual({ domain: "api.example.com" });
    expect(permissionArgs("storage")).toEqual({});
  });
});

describe("tree values", () => {
  const now = new Date(2026, 8, 30, 15, 0);

  it("resolves date defaults", () => {
    expect(dateDefault("today", false, now)).toBe("2026-09-30");
    expect(dateDefault("tomorrow", false, now)).toBe("2026-10-01");
    expect(dateDefault("tomorrow", true, now)).toBe("2026-10-01T09:00");
    expect(dateDefault("2026-01-02", true, now)).toBe("2026-01-02T09:00");
    expect(dateDefault("2026-01-02T10:30", false, now)).toBe("2026-01-02");
    expect(dateDefault(undefined, false, now)).toBeNull();
  });

  it("starts every input with its value kind", () => {
    const values = initialValues(
      [
        { type: "input", id: "a", label: "A" },
        {
          type: "stack",
          children: [
            {
              type: "select",
              id: "b",
              label: "B",
              options: [{ value: "x", label: "X" }],
              multiple: true,
            },
            { type: "select", id: "c", label: "C", options: [], default: ["y"] },
          ],
        },
        { type: "checkbox", id: "d", label: "D" },
        { type: "contact-picker", id: "e", label: "E" },
        { type: "folder-picker", id: "f", label: "F" },
        { type: "message-list", id: "g", messageIds: [], selectable: true },
        { type: "message-list", messageIds: [] },
        { type: "table", id: "h", columns: [], rows: [], selectable: true },
        { type: "button", id: "i", label: "I" },
        { type: "date", id: "j", label: "J", default: "today" },
      ],
      now,
    );
    expect(values).toEqual({
      a: "",
      b: [],
      c: "y",
      d: false,
      e: [],
      f: null,
      g: [],
      h: [],
      j: "2026-09-30",
    });
  });

  it("keeps entered values when a tree changes", () => {
    const tree = [
      { type: "input" as const, id: "a", label: "A", default: "new" },
      { type: "checkbox" as const, id: "b", label: "B" },
    ];
    expect(mergeValues(tree, { a: "typed", b: "wrong kind", gone: 1 }, now)).toEqual({
      a: "typed",
      b: false,
    });
  });

  it("validates like a primary button", () => {
    expect(checkNode({ type: "input", id: "a", label: "A", required: true }, " ")).toBe("required");
    expect(checkNode({ type: "input", id: "a", label: "A", kind: "email" }, "nope")).toBe("email");
    expect(checkNode({ type: "input", id: "a", label: "A", kind: "email" }, "a@b.org")).toBeNull();
    expect(checkNode({ type: "input", id: "a", label: "A", kind: "url" }, "ftp://x")).toBe("url");
    expect(checkNode({ type: "input", id: "a", label: "A", kind: "number" }, "1e")).toBe("number");
    expect(checkNode({ type: "input", id: "a", label: "A", maxLength: 2 }, "abc")).toBe("too-long");
    expect(checkNode({ type: "textarea", id: "a", label: "A", required: true }, "")).toBe(
      "required",
    );
    expect(
      checkNode({ type: "date", id: "d", label: "D", min: "2026-10-01" }, "2026-09-30T10:00"),
    ).toBe("date-range");
    expect(checkNode({ type: "date", id: "d", label: "D", required: true }, null)).toBe("required");
    expect(
      checkNode({ type: "select", id: "s", label: "S", options: [], required: true }, ""),
    ).toBe("required");
    expect(
      validate(
        [
          { type: "input", id: "a", label: "A", required: true },
          { type: "stack", children: [{ type: "input", id: "b", label: "B", kind: "email" }] },
        ],
        { a: "", b: "x" },
      ),
    ).toEqual({ a: "required", b: "email" });
  });

  it("parses typed addresses", () => {
    expect(parseAddress("Ada <ada@example.org>")).toEqual({
      name: "Ada",
      email: "ada@example.org",
    });
    expect(parseAddress('"Ada L" <ada@example.org>')?.name).toBe("Ada L");
    expect(parseAddress(" ada@example.org ")).toEqual({ name: null, email: "ada@example.org" });
    expect(parseAddress("ada")).toBeNull();
  });
});

describe("markdown", () => {
  it("parses blocks", () => {
    expect(
      parseMarkdown(
        "# Title\n\nSome *text*\ncontinued\n\n- a\n- b\n\n1. one\n\n> quote\n\n---\n\n```\n<b>x</b>\n```",
      ),
    ).toEqual([
      { kind: "heading", level: 1, inline: [{ kind: "text", text: "Title" }] },
      {
        kind: "paragraph",
        inline: [
          { kind: "text", text: "Some " },
          { kind: "em", children: [{ kind: "text", text: "text" }] },
          { kind: "text", text: " continued" },
        ],
      },
      {
        kind: "list",
        ordered: false,
        items: [[{ kind: "text", text: "a" }], [{ kind: "text", text: "b" }]],
      },
      { kind: "list", ordered: true, items: [[{ kind: "text", text: "one" }]] },
      { kind: "quote", blocks: [{ kind: "paragraph", inline: [{ kind: "text", text: "quote" }] }] },
      { kind: "rule" },
      { kind: "code", text: "<b>x</b>" },
    ]);
  });

  it("parses inline text and never makes unsafe links", () => {
    expect(parseInline("**b** `c` [l](https://x.example/) ![i](data:x) \\*n")).toEqual([
      { kind: "strong", children: [{ kind: "text", text: "b" }] },
      { kind: "text", text: " " },
      { kind: "code", text: "c" },
      { kind: "text", text: " " },
      { kind: "link", href: "https://x.example/", children: [{ kind: "text", text: "l" }] },
      { kind: "text", text: " i *n" },
    ]);
    expect(parseInline("[x](javascript:alert(1))")[0]).toMatchObject({ kind: "link", href: null });
    expect(parseInline("<https://a.example>")[0]).toMatchObject({ href: "https://a.example/" });
    expect(safeHref("mailto:a@b")).toBeNull();
    expect(parseInline("<b>raw</b>")).toEqual([{ kind: "text", text: "<b>raw</b>" }]);
  });
});

describe("placement text", () => {
  const base = (): ResolvedLayout => ({
    name: "mine",
    windows: {},
    actions: {},
    keys: [{ context: "list", key: "shift+r", action: "mail.reply", args: [] }],
    toolbars: {
      reader: [
        { type: "action", action: "mail.reply", args: [], label: null, icon: null },
        { type: "action", action: "mail.forward", args: [], label: null, icon: null },
        { type: "spacer" },
        { type: "action", action: "mail.archive", args: [], label: null, icon: null },
      ],
    },
    menus: {},
    toolbar_settings: {},
    views: {},
    default_view: null,
    rail: [],
    settings: [],
    window_frames: {},
    presets: [],
  });

  it("inserts after / before an anchor or at the end, once", () => {
    const items = [
      { type: "action" as const, action: "a", args: [] },
      { type: "action" as const, action: "b", args: [] },
    ];
    const names = (list: ReturnType<typeof insertItem>) =>
      list.map((i) => (i.type === "action" ? i.action : i.text));
    expect(names(insertItem(items, "x", { after: "a" }))).toEqual(["a", "x", "b"]);
    expect(names(insertItem(items, "x", { before: "a" }))).toEqual(["x", "a", "b"]);
    expect(names(insertItem(items, "x", { after: "zzz" }))).toEqual(["a", "b", "x"]);
    expect(insertItem(items, "a", {})).toBe(items);
  });

  it("writes the changed toolbars, menus, keys and panels", () => {
    const text = placementText(
      base(),
      plugin({
        manifest: {
          ...followUpManifest(),
          contributes: {
            ...followUpManifest().contributes,
            placements: [
              { toolbar: "reader", command: "remind", after: "mail.forward" },
              { menu: "context:message-list.row", command: "remind" },
              { panel: "reminders", after: "message-view" },
            ],
            keys: [
              { key: "shift+r", command: "remind", context: "list" },
              { key: "ctrl+shift+f", command: "remind" },
            ],
          },
        },
      }),
    );
    expect(text).toBe(
      [
        'layout "mine" {',
        '    toolbar "reader" {',
        '        item "mail.reply"',
        '        item "mail.forward"',
        '        item "plugin.follow-up.remind"',
        "        spacer",
        '        item "mail.archive"',
        "    }",
        '    menu "context:message-list.row" {',
        '        item "plugin.follow-up.remind"',
        "    }",
        "    keys {",
        '        bind "ctrl+shift+f" "plugin.follow-up.remind"',
        "    }",
        '    // plugin-panel "follow-up.reminders" // after message-view',
        "}",
        "",
      ].join("\n"),
    );
  });

  it("proposes nothing when everything is in place", () => {
    const layout = base();
    must(layout.toolbars.reader).push({
      type: "action",
      action: "plugin.follow-up.remind",
      args: [],
      label: null,
      icon: null,
    });
    const p = plugin();
    p.manifest.contributes = {
      ...p.manifest.contributes,
      placements: [{ toolbar: "reader", command: "remind" }],
      keys: [{ key: "shift+r", command: "remind", context: "list" }],
    };
    expect(placementText(layout, p)).toBeNull();
  });
});

describe("feature state", () => {
  it("keeps dialogs and toasts", () => {
    const store = createPluginsStore();
    const dialog = {
      dialog_id: "d1",
      plugin_id: "p",
      client_id: null,
      spec: { title: "T", body: [] },
    };
    store.getState().openDialog(dialog);
    store.getState().openDialog({ ...dialog, spec: { title: "U", body: [] } });
    expect(store.getState().dialogs.map((d) => d.spec.title)).toEqual(["U"]);
    store.getState().openDialog({ ...dialog, dialog_id: "d2", plugin_id: "q" });
    store.getState().closePluginDialogs("p");
    expect(store.getState().dialogs.map((d) => d.dialog_id)).toEqual(["d2"]);
    store.getState().closeDialog("d2");
    expect(store.getState().dialogs).toEqual([]);
    for (let i = 0; i < MAX_TOASTS + 2; i++) {
      store.getState().pushToast({
        kind: "stopped",
        id: `t${String(i)}`,
        pluginId: "p",
        level: "error",
        message: { id: "x" },
      });
    }
    expect(store.getState().toasts).toHaveLength(MAX_TOASTS);
    store.getState().dismissToast(`t${String(MAX_TOASTS + 1)}`);
    expect(store.getState().toasts).toHaveLength(MAX_TOASTS - 1);
  });

  it("appends live log lines once", () => {
    const entry = {
      seq: 2,
      plugin_id: "p",
      at: "2026-09-30T00:00:00.000Z",
      level: "info" as const,
      source: "plugin" as const,
      message: "hi",
    };
    expect(appendLogLine(undefined, entry)).toBeUndefined();
    expect(appendLogLine([], entry)).toEqual([entry]);
    expect(appendLogLine([entry], entry)).toEqual([entry]);
  });
});
