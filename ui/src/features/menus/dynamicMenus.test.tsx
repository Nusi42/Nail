/**
 * R2 (ADR 0222): dynamic submenus — a layout item whose action has a menu
 * source opens as a hover / arrow-key submenu of the source's entries
 * ("Run rule" lists the saved rules), and labels that are user data show
 * as written even when they start with `@`.
 */
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Rule } from "../../api/generated";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  createEnv,
  renderWithProviders,
  stubElementSizesEachTest,
  widget,
  type Env,
} from "../../test/harness";
import { requestsOf } from "../../test/env";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { expectConsole } from "../../test/console";
import { appRegistry } from "../registry";
import { menuPosition, submenuPlace } from "./ContextMenu";
import { ContextMenuHost } from "./ContextMenuHost";
import {
  literalMenuLabel,
  menuLabelText,
  MenuSourcesContext,
  type MenuSourceRegistry,
} from "./sources";

stubElementSizesEachTest();

const RUN_RULE = "Run rule";

function rule(id: string, name: string, enabled = true): Rule {
  return {
    id,
    name,
    enabled,
    triggers: [],
    scope: [],
    conditions: { type: "all", conditions: [] },
    actions: [{ type: "flag", flagged: true }],
    source: null,
  };
}

/** The folder tree and the menu host with `sources` (default: the app's). */
function renderTree(env: Env, sources: MenuSourceRegistry = appRegistry.menuSources) {
  return renderWithProviders(
    <MenuSourcesContext.Provider value={sources}>
      <NodeView node={widget("folder-tree")} path="t/0" />
      <ContextMenuHost />
    </MenuSourcesContext.Provider>,
    env,
  );
}

async function openFolderMenu(env: Env, sources?: MenuSourceRegistry) {
  renderTree(env, sources);
  const folder = await screen.findByRole("treeitem", { name: /Inbox/ });
  fireEvent.contextMenu(folder, { clientX: 5, clientY: 5, button: 2 });
  return screen.findByRole("menu", { name: "Folder actions" });
}

function setup(): Env {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  env.transport.data.behaviour.workMs = 0;
  return env;
}

describe("menu placement (R8)", () => {
  const viewport = { width: 800, height: 600 };
  const size = { width: 200, height: 100 };
  it("starts at the point, or ends there for a button at a bar's end, inside the window", () => {
    expect(menuPosition({ x: 100, y: 50 }, size, viewport)).toEqual({ x: 100, y: 50 });
    expect(menuPosition({ x: 700, y: 50 }, size, viewport, true)).toEqual({ x: 500, y: 50 });
    expect(menuPosition({ x: 750, y: 560 }, size, viewport)).toEqual({ x: 600, y: 500 });
    expect(menuPosition({ x: 120, y: 50 }, size, viewport, true)).toEqual({ x: 0, y: 50 });
  });
});

describe("submenu placement", () => {
  const viewport = { width: 800, height: 600 };
  it("stays beside its item, flips at the inline end and moves up at the bottom", () => {
    expect(submenuPlace({ left: 500, right: 700, bottom: 400 }, viewport, false)).toEqual({
      flip: false,
      shift: 0,
    });
    expect(submenuPlace({ left: 700, right: 900, bottom: 650 }, viewport, false)).toEqual({
      flip: true,
      shift: -50,
    });
    // Right to left: the inline end is the left edge.
    expect(submenuPlace({ left: -20, right: 180, bottom: 100 }, viewport, true).flip).toBe(true);
    expect(submenuPlace({ left: 700, right: 900, bottom: 100 }, viewport, true).flip).toBe(false);
  });
});

describe("literal menu labels", () => {
  it("marks user labels starting with @ (or the mark) and shows them as written", () => {
    const resolve = (label: string) => (label.startsWith("@") ? `resolved ${label}` : label);
    expect(literalMenuLabel("Home")).toBe("Home");
    for (const text of ["@home", "\\x", "Home", "@rules.menu.all"]) {
      expect(menuLabelText(literalMenuLabel(text), resolve)).toBe(text);
    }
    expect(menuLabelText("@rules.menu.all", resolve)).toBe("resolved @rules.menu.all");
  });
});

describe("a menu source as a submenu", () => {
  it("opens Run rule on hover, lists the rules and runs one on the folder", async () => {
    const env = setup();
    env.transport.data.features.rules.rules.push(rule("at-home", "@home"));
    const user = userEvent.setup();
    const menu = await openFolderMenu(env);
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    expect(item).toHaveAttribute("aria-haspopup", "menu");
    await waitFor(() => {
      expect(item).not.toHaveAttribute("aria-busy");
    });

    await user.hover(item);
    const menus = await screen.findAllByRole("menu");
    expect(menus).toHaveLength(2);
    expect(item).toHaveAttribute("aria-expanded", "true");
    // Hovering keeps the focus on the item.
    expect(item).toHaveFocus();
    const rules = must(menus[1]);
    expect(rules).toHaveAccessibleName(RUN_RULE);
    // A rule named "@home" is not a Fluent id.
    expect(within(rules).getByRole("menuitem", { name: "@home" })).toBeInTheDocument();
    expect(within(rules).getByRole("menuitem", { name: "All rules that are on" })).toBeVisible();

    await user.click(within(rules).getByRole("menuitem", { name: "@home" }));
    await waitFor(() => {
      expect(requestsOf(env, "RunRules")).toEqual([
        { type: "RunRules", rule_ids: ["at-home"], message_ids: [], folder_id: INBOX, list: null },
      ]);
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens with ArrowRight and closes with ArrowLeft", async () => {
    const env = setup();
    const user = userEvent.setup();
    const menu = await openFolderMenu(env);
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    item.focus();
    await user.keyboard("{ArrowRight}");
    const rules = must((await screen.findAllByRole("menu"))[1]);
    expect(within(rules).getAllByRole("menuitem")[0]).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    expect(item).toHaveFocus();
  });

  it("shows an empty source disabled and a plain item without the sources", async () => {
    const env = setup();
    const empty: MenuSourceRegistry = { "rules.run-menu": { load: () => Promise.resolve([]) } };
    const user = userEvent.setup();
    const menu = await openFolderMenu(env, empty);
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    await waitFor(() => {
      expect(item).toHaveAttribute("aria-disabled", "true");
    });
    await user.hover(item);
    await user.keyboard("{ArrowRight}");
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    expect(item).toHaveAttribute("aria-expanded", "false");
  });

  it("treats a failing source as empty", async () => {
    const env = setup();
    expectConsole('nail: the menu source of "rules.run-menu" failed');
    const failing: MenuSourceRegistry = {
      "rules.run-menu": { load: () => Promise.reject(new Error("boom")) },
    };
    const menu = await openFolderMenu(env, failing);
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    await waitFor(() => {
      expect(item).toHaveAttribute("aria-disabled", "true");
    });
  });

  it("shows an empty source that runs when empty as its plain, enabled item", async () => {
    const env = setup();
    const runs: MenuSourceRegistry = {
      "rules.run-menu": { load: () => Promise.resolve([]), runsWhenEmpty: true },
    };
    const menu = await openFolderMenu(env, runs);
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    await waitFor(() => {
      expect(item).not.toHaveAttribute("aria-haspopup");
    });
    expect(item).not.toHaveAttribute("aria-disabled");
  });

  it("is a plain item where the app provides no sources", async () => {
    const env = setup();
    const menu = await openFolderMenu(env, {});
    const item = within(menu).getByRole("menuitem", { name: RUN_RULE });
    expect(item).not.toHaveAttribute("aria-haspopup");
  });
});
