/**
 * The command palette: the fuzzy search, the descriptions of every
 * registered action, the contexts, and the palette itself against the
 * mock core (keyboard use, running actions, unavailable actions, recents,
 * plug-in commands, key bindings shown) plus axe.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { beforeAll, describe, expect, it } from "vitest";
import type { PaletteEntry } from "../../actions/palette";
import type { ResolvedLayout } from "../../api/generated";
import { WindowLayer } from "../../layout/WindowLayer";
import { useActions } from "../../actions/ActionsProvider";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { installFollowUp } from "../plugins/testUtil";
import { appRegistry } from "../registry";
import { BUILTIN_INFO, HIDDEN_ACTIONS } from "./builtinInfo";
import { contextsOf } from "./contexts";
import { fold, matchToken, mergeRanges, searchPalette } from "./fuzzy";
import { PALETTE_DIALOG, PALETTE_OPEN_ACTION, loadPalette } from ".";
import { paletteStoreFor } from "./store";
import { must } from "../../test/must";

beforeAll(() => loadPalette());

function entry(id: string, title: string, extra: Partial<PaletteEntry> = {}): PaletteEntry {
  return {
    id,
    args: [],
    title,
    keywords: title.toLowerCase().split(" "),
    shortcuts: [],
    available: true,
    recentRank: null,
    source: "app",
    ...extra,
  };
}

describe("fuzzy search", () => {
  it("folds case and accents one character per character", () => {
    expect(fold("Éxport Réglages")).toBe("export reglages");
    expect(fold("Straße").length).toBe("Straße".length);
  });

  it("prefers word starts, then substrings, then letters in order", () => {
    const start = matchToken("rep", "reply to all");
    const inside = matchToken("ply", "reply to all");
    const letters = matchToken("rta", "reply to all");
    expect(start?.ranges).toEqual([[0, 3]]);
    expect(start && inside && start.score > inside.score).toBe(true);
    expect(letters?.ranges).toEqual([
      [0, 1],
      [6, 7],
      [9, 10],
    ]);
    expect(inside && letters && inside.score > letters.score).toBe(true);
    expect(matchToken("xyz", "reply to all")).toBeNull();
    expect(
      mergeRanges([
        [3, 5],
        [0, 2],
        [2, 3],
      ]),
    ).toEqual([[0, 5]]);
  });

  it("needs every word, searches keywords, puts unavailable ones last", () => {
    const entries = [
      entry("mail.reply", "Reply", { keywords: ["reply", "answer", "respond"] }),
      entry("mail.reply-all", "Reply all"),
      entry("mail.forward", "Forward", { available: false }),
      entry("mail.archive", "Archive", { recentRank: 0 }),
    ];
    expect(searchPalette(entries, "reply all").map((s) => s.entry.id)).toEqual(["mail.reply-all"]);
    expect(searchPalette(entries, "answer").map((s) => s.entry.id)).toEqual(["mail.reply"]);
    expect(searchPalette(entries, "").map((s) => s.entry.id)).toEqual([
      "mail.reply",
      "mail.reply-all",
      "mail.archive",
      "mail.forward",
    ]);
    expect(searchPalette(entries, "r").at(-1)?.entry.id).toBe("mail.forward");
  });
});

describe("action descriptions", () => {
  it("describe every registered action (or hide it)", () => {
    const layout = createEnv().transport.data.layout;
    const missing = Object.keys(appRegistry.actions).filter(
      (id) => !BUILTIN_INFO[id] && !appRegistry.actionInfo[id] && layout.actions[id]?.label == null,
    );
    expect(missing).toEqual([]);
    for (const id of HIDDEN_ACTIONS) expect(BUILTIN_INFO[id]?.hidden).toBe(true);
  });

  it("let features describe their own actions", () => {
    expect(appRegistry.actionInfo["import.open"]?.title).toBe("portability-action-import");
    expect(appRegistry.actionInfo["print.message"]?.availability).toEqual(["list", "reader"]);
  });
});

describe("contexts", () => {
  it("come from focus, selection, composer, windows and the desktop", () => {
    const base = {
      keyContext: "calendar",
      selectedMessageId: null,
      messageOrder: [],
      composer: null,
      windows: [],
    };
    expect([...contextsOf(base, false)].sort()).toEqual(["calendar", "global"]);
    expect(
      [
        ...contextsOf(
          {
            ...base,
            keyContext: "window",
            selectedMessageId: "m",
            messageOrder: ["m"],
            windows: ["contacts"],
          },
          true,
        ),
      ].sort(),
    ).toEqual(["contacts", "desktop", "global", "list", "reader"]);
  });
});

function OpenButton() {
  const actions = useActions();
  return (
    <button type="button" onClick={() => void actions.dispatch(PALETTE_OPEN_ACTION)}>
      open palette
    </button>
  );
}

function render(env: Env, layout?: ResolvedLayout) {
  const resolved = layout ?? env.transport.data.layout;
  return renderWithProviders(
    <>
      <OpenButton />
      <WindowLayer layout={resolved} />
    </>,
    env,
    { layout: resolved },
  );
}

async function openPalette(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "open palette" }));
  return screen.findByRole("combobox", { name: "Search actions" });
}

function withSelection(): Env {
  const env = createEnv({ state: { activeAccountId: "acc-personal" } });
  const first = env.transport.data.messages.find((m) => m.folder_id === "acc-personal/Inbox");
  act(() => {
    env.store.getState().setMessageOrder(first ? [first.id] : []);
    env.store.getState().selectMessage(first?.id ?? null);
    env.store.getState().setKeyContext("list");
  });
  return env;
}

describe("command palette", () => {
  it("finds an action by fuzzy words, shows its keys and runs it with Enter", async () => {
    const env = withSelection();
    const user = userEvent.setup();
    render(env);
    const input = await openPalette(user);
    expect(paletteStoreFor(env.store).getState().contexts).toEqual(
      expect.arrayContaining(["list", "reader", "global"]),
    );
    await user.type(input, "rep al");
    const list = await screen.findByRole("listbox", { name: "Actions" });
    const first = within(list).getAllByRole("option")[0];
    expect(first).toHaveAttribute("data-action", "mail.reply-all");
    expect(first).toHaveAttribute("aria-selected", "true");
    expect(input).toHaveAttribute("aria-activedescendant", first?.id);
    expect(within(must(first)).getByText("A")).toBeVisible();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(env.store.getState().windows).not.toContain(PALETTE_DIALOG);
    });
    await waitFor(() => {
      expect(env.store.getState().composer).not.toBeNull();
    });
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "RecordActionUsed",
        action_id: "mail.reply-all",
      });
    });
  });

  it("moves with the arrow keys and lists recent actions first", async () => {
    const env = withSelection();
    await env.transport.send({ type: "RecordActionUsed", action_id: "sync.now" });
    const user = userEvent.setup();
    render(env);
    const input = await openPalette(user);
    const list = await screen.findByRole("listbox");
    const options = await within(list).findAllByRole("option");
    await waitFor(() =>
      expect(within(list).getAllByRole("option")[0]).toHaveAttribute("data-action", "sync.now"),
    );
    expect(within(list).getAllByRole("option")[0]).toHaveTextContent("Recent");
    await user.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute(
      "aria-activedescendant",
      within(list).getAllByRole("option")[1]?.id,
    );
    await user.keyboard("{ArrowUp}{ArrowUp}");
    const all = within(list).getAllByRole("option");
    expect(input).toHaveAttribute("aria-activedescendant", all[all.length - 1]?.id);
    expect(options.length).toBeGreaterThan(20);
    await user.click(screen.getByRole("button", { name: "Forget recent actions" }));
    await waitFor(() =>
      expect(within(list).getAllByRole("option")[0]).not.toHaveTextContent("Recent"),
    );
  });

  it("lists unavailable actions last and explains instead of running them", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    render(env);
    const input = await openPalette(user);
    await user.type(input, "forward");
    const option = await screen.findByRole("option", { name: /Forward/ });
    expect(option).toHaveAttribute("aria-disabled", "true");
    expect(option).toHaveTextContent("Not available here");
    await user.keyboard("{Enter}");
    expect(await screen.findByText("This action works in: Message list, Reader.")).toBeVisible();
    expect(env.store.getState().windows).toContain(PALETTE_DIALOG);
    expect(env.transport.requests.some((r) => r.type === "RecordActionUsed")).toBe(false);
  });

  it("lists plug-in commands and layout aliases and says when nothing matches", async () => {
    const env = withSelection();
    await installFollowUp(env);
    const user = userEvent.setup();
    render(env);
    const input = await openPalette(user);
    await user.type(input, "follow");
    const plugin = await screen.findByRole("option", { name: /Follow up/ });
    expect(plugin).toHaveAttribute("data-source", "plugin");
    expect(plugin).toHaveTextContent("Plug-in");
    await user.clear(input);
    await user.type(input, "outbox");
    expect(await screen.findByRole("option", { name: /outbox/i })).toHaveAttribute(
      "data-source",
      "layout",
    );
    await user.clear(input);
    await user.type(input, "qqqqzz");
    expect(await screen.findByText("No action matches “qqqqzz”.")).toBeVisible();
  });

  it("closes again with the same action and with Escape", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    render(env);
    await openPalette(user);
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(env.store.getState().windows).not.toContain(PALETTE_DIALOG);
    });
    await user.click(screen.getByRole("button", { name: "open palette" }));
    await user.click(screen.getByRole("button", { name: "open palette" }));
    expect(env.store.getState().windows).not.toContain(PALETTE_DIALOG);
  });

  it("has no axe violations", async () => {
    const env = withSelection();
    await env.transport.send({ type: "RecordActionUsed", action_id: "mail.compose" });
    const user = userEvent.setup();
    const { container } = render(env);
    const input = await openPalette(user);
    await user.type(input, "re");
    await screen.findAllByRole("option");
    const result = await axe.run(container, {
      rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
    });
    expect(result.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
