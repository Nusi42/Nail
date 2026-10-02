import type { RecentAction, ResolvedLayout } from "../api/generated";
import { defaultLayout } from "../api/mockData";
import { buildPalette, shortcutsFor, type ActionInfo } from "./palette";

const t = (id: string) => (id === "reply-words" ? "Answer, respond" : `T(${id})`);

function layout(): ResolvedLayout {
  const base = defaultLayout();
  return {
    ...base,
    actions: {
      ...base.actions,
      "my.archive": { label: "Archive to 2026", icon: null, alias_of: "mail.move-to", args: ["a"] },
    },
    keys: [
      { context: "list", key: "r", action: "mail.reply", args: [] },
      { context: "global", key: "ctrl+r", action: "mail.reply", args: [] },
      { context: "list", key: "r", action: "mail.reply", args: [] },
      { context: "list", key: "g a", action: "mail.move-to", args: ["archive"] },
    ],
  };
}

const infos: Record<string, ActionInfo> = {
  "mail.reply": { title: "reply-title", keywords: "reply-words", availability: ["list", "reader"] },
  "mail.compose": { title: "compose-title" },
  "mail.move-to": { title: "move-title", hidden: true },
};

describe("command palette model", () => {
  it("collects shortcuts without arguments, once each", () => {
    expect(shortcutsFor(layout(), "mail.reply")).toEqual(["r", "ctrl+r"]);
    expect(shortcutsFor(layout(), "mail.move-to")).toEqual([]);
  });

  it("builds titled, available entries with recent ones first", () => {
    const recent: RecentAction[] = [
      { action_id: "mail.compose", last_used_at: "2026-10-02T10:00:00Z", count: 3 },
    ];
    const plain = { ...layout(), actions: {} };
    const entries = buildPalette({
      actionIds: ["mail.reply", "mail.compose", "mail.move-to", "plugin.x.run", "no.title"],
      infos,
      pluginTitles: { "plugin.x.run": "Run X" },
      layout: plain,
      contexts: new Set(["composer"]),
      recent,
      t,
    });
    expect(entries.map((e) => e.id)).toEqual(["mail.compose", "plugin.x.run", "mail.reply"]);
    const reply = entries.find((e) => e.id === "mail.reply");
    expect(reply?.available).toBe(false);
    expect(reply?.keywords).toEqual(expect.arrayContaining(["answer", "respond"]));
    expect(reply?.shortcuts).toEqual(["r", "ctrl+r"]);
    expect(entries[0]?.recentRank).toBe(0);
    expect(entries.find((e) => e.id === "plugin.x.run")?.source).toBe("plugin");
  });

  it("uses layout labels and lists aliases", () => {
    const l = layout();
    l.actions["mail.reply"] = { label: "@mail.action.reply", icon: null, alias_of: null, args: [] };
    l.actions["settings.open"] = {
      label: "Settings",
      icon: null,
      alias_of: "window.open",
      args: ["settings"],
    };
    // Like the dispatcher: an alias is asked by its own id (it adds the
    // alias's arguments); `window.open` alone, without a window, is not
    // enabled, nor is `mail.move-to` without a folder.
    const entries = buildPalette({
      actionIds: ["mail.reply"],
      infos,
      layout: l,
      contexts: new Set(["list"]),
      recent: [],
      t,
      enabled: (id) => !["mail.move-to", "my.archive", "window.open"].includes(id),
    });
    expect(entries.find((e) => e.id === "settings.open")?.available).toBe(true);
    const reply = entries.find((e) => e.id === "mail.reply");
    expect(reply?.title).toBe("T(mail-action-reply)");
    expect(reply?.available).toBe(true);
    const alias = entries.find((e) => e.id === "my.archive");
    expect(alias?.source).toBe("layout");
    expect(alias?.title).toBe("Archive to 2026");
    expect(alias?.available).toBe(false);
  });
});
