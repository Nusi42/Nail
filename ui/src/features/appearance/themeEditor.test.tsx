/**
 * M5d WP183: the theme editor (token groups, modes, part overrides, live
 * preview, duplicate / rename / delete, undo, conflicts), the New theme /
 * Rename / Delete windows, the Appearance page's New theme / Edit theme,
 * and the inspector's "Edit style" — against the mock core, with axe in
 * every built-in theme.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { Request, ResolvedTheme } from "../../api/generated";
import { mockEditHistory, simulateExternalEdit } from "../../api/editHistoryMock";
import { defaultTheme } from "../../api/mockData";
import { appRegistry } from "../registry";
import { violations } from "../../test/axe";
import { applyEditHistory } from "../../state/editHistory";
import { features } from "../registry";
import {
  createEnv as createBareEnv,
  renderNode,
  renderWithProviders,
  settleQueries,
  stubElementSizes,
  widget,
  type Env,
} from "../../test/harness";
import { must } from "../../test/must";
import { EditStyleButton } from "./EditStyle";
import {
  THEME_DELETE_DIALOG,
  THEME_EDITOR_DIALOG,
  THEME_NEW_DIALOG,
  THEME_RENAME_DIALOG,
  themeEditorStoreFor,
} from "./editorStore";
import { BUILTIN_THEMES } from "./model";
import { DeleteThemeDialog, NewThemeDialog, RenameThemeDialog } from "./ThemeDialogs";
import { ThemeEditorDialog } from "./ThemeEditor";
import { resolveMockTheme } from "./themes";
import { setThemeEditDelay } from "./useThemeEdits";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  setThemeEditDelay(0);
});
afterEach(() => {
  restoreSizes();
  setThemeEditDelay(400);
});

/**
 * An env whose query cache follows the core's events like the app does
 * (`App.tsx`: `EditHistoryChanged`; this feature's handlers).
 */
function createEnv(): Env {
  const env = createBareEnv();
  env.transport.onEvent((event) => {
    if (event.type === "EditHistoryChanged") applyEditHistory(env.queryClient, event.history);
    for (const handler of features.appearance?.events ?? []) {
      handler(event, { client: env.client, queryClient: env.queryClient, store: env.store });
    }
  });
  return env;
}

/** Requests the mock received. */
function recordRequests(env: Env): Request[] {
  const seen: Request[] = [];
  const send = env.transport.send.bind(env.transport);
  env.transport.send = (request) => {
    seen.push(request);
    return send(request);
  };
  return seen;
}

/** A user theme copied from `base` in the mock (as "New theme" makes it). */
async function userTheme(env: Env, base = "frost", name = "Mine"): Promise<string> {
  const answer = await env.client.request(
    { type: "DuplicateTheme", theme_id: base, name },
    "ThemeSaved",
  );
  return answer.theme_id;
}

/** Opens the editor on `themeId` (`null` = the active theme), optionally at a part. */
function showEditor(env: Env, themeId: string | null, part: string | null = null) {
  act(() => {
    themeEditorStoreFor(env.store).getState().show(themeId, part);
  });
  return renderWithProviders(<ThemeEditorDialog />, env);
}

/** The token row of `color.accent` etc. */
async function tokenRow(id: string): Promise<HTMLElement> {
  const name = await screen.findByText(id, { selector: "code" });
  return must(name.closest<HTMLElement>('[data-part="theme-editor.token"]'));
}

const editRequests = (requests: Request[]) =>
  requests.filter((r): r is Extract<Request, { type: "EditTheme" }> => r.type === "EditTheme");

describe("theme editor registrations", () => {
  it("registers the editor, its windows and actions", () => {
    expect(appRegistry.widgets["theme-editor"]).toBeDefined();
    for (const dialog of [THEME_EDITOR_DIALOG, THEME_NEW_DIALOG, THEME_RENAME_DIALOG]) {
      expect(appRegistry.dialogs[dialog]).toBeDefined();
      expect(appRegistry.windows[dialog]?.confirmation).toBeUndefined();
    }
    expect(appRegistry.windows[THEME_DELETE_DIALOG]).toEqual({ confirmation: true });
    expect(appRegistry.windows[THEME_EDITOR_DIALOG]?.min_size).toEqual({
      width: 560,
      height: 420,
    });
    expect(appRegistry.actionInfo["theme.edit"]?.icon).toBe("palette");
    expect(appRegistry.actionInfo["theme.new"]?.icon).toBe("plus");
  });

  it("opens the editor and New theme through actions", () => {
    const env = createEnv();
    const ctx = { store: env.store } as unknown as Parameters<
      NonNullable<(typeof appRegistry.actions)["theme.edit"]>["run"]
    >[0];
    void must(appRegistry.actions["theme.edit"]).run(ctx, []);
    expect(env.store.getState().windows).toContain(THEME_EDITOR_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().themeId).toBeNull();
    void must(appRegistry.actions["theme.new"]).run(ctx, []);
    expect(env.store.getState().windows).toContain(THEME_NEW_DIALOG);
  });
});

describe("theme editor", () => {
  it("shows a built-in theme read-only and duplicates it as a new theme", async () => {
    const env = createEnv();
    const requests = recordRequests(env);
    showEditor(env, null);
    expect(await screen.findByText(/This is a built-in theme/)).toBeInTheDocument();
    expect(screen.getByLabelText("Theme")).toHaveValue("frost");
    expect(screen.getByText("In use")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
    const accent = await tokenRow("color.accent");
    expect(within(accent).getByRole("textbox", { name: "Value of color.accent" })).toHaveAttribute(
      "readonly",
    );
    fireEvent.click(screen.getByRole("button", { name: "Duplicate as new theme" }));
    // The copy is used in place of the original and opened in the editor.
    await waitFor(() => {
      expect(screen.getByLabelText("Theme")).toHaveValue("frost-copy");
    });
    expect(requests).toContainEqual({
      type: "DuplicateTheme",
      theme_id: "frost",
      name: "Frost copy",
    });
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.light).toBe("frost-copy");
    });
    expect(screen.queryByText(/This is a built-in theme/)).toBeNull();
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
    await settleQueries(env);
  });

  it("edits a colour live: the preview first, one write after the pause", async () => {
    const env = createEnv();
    const id = await userTheme(env);
    const requests = recordRequests(env);
    setThemeEditDelay(60_000);
    showEditor(env, id);
    const row = await tokenRow("color.accent");
    expect(within(row).getByText("Inherited")).toBeInTheDocument();
    fireEvent.change(within(row).getByLabelText("Pick a colour for color.accent"), {
      target: { value: "#ff0066" },
    });
    fireEvent.change(
      within(await tokenRow("color.bg")).getByLabelText("Pick a colour for color.bg"),
      {
        target: { value: "#fafafa" },
      },
    );
    // Shown at once (the row is marked, the preview uses it), not written yet.
    expect(row).toHaveAttribute("data-state", "changed");
    expect(screen.getByText("Saving…")).toBeInTheDocument();
    expect(editRequests(requests)).toEqual([]);
    // Typing a value and pressing Enter writes everything waiting, as one write.
    const bg = within(await tokenRow("color.border")).getByRole("textbox", {
      name: "Value of color.border",
    });
    fireEvent.change(bg, { target: { value: "#cccccc" } });
    fireEvent.keyDown(bg, { key: "Enter" });
    await waitFor(() => {
      expect(editRequests(requests)).toHaveLength(1);
    });
    const edit = must(editRequests(requests)[0]);
    expect(edit.target).toEqual({ kind: "theme", theme_id: id });
    expect(edit.base_revision).toMatch(/^mock-/);
    expect(edit.patches).toEqual([
      { op: "set-token", category: "color", name: "accent", value: "#ff0066", mode: null },
      { op: "set-token", category: "color", name: "bg", value: "#fafafa", mode: null },
      { op: "set-token", category: "color", name: "border", value: "#cccccc", mode: null },
    ]);
    await waitFor(async () => {
      expect(within(await tokenRow("color.accent")).getByText("This theme")).toBeInTheDocument();
    });
    expect(mockEditHistory(env.transport, "appearance").undo.at(-1)?.kind).toBe("edit-theme");
    // A theme's own value can go back to the inherited one.
    fireEvent.click(
      within(await tokenRow("color.accent")).getByRole("button", {
        name: "Use the inherited value of color.accent",
      }),
    );
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        { op: "set-token", category: "color", name: "accent", value: null, mode: null },
      ]);
    });
    await settleQueries(env);
  });

  it("edits mode values and other token groups", async () => {
    const env = createEnv();
    const id = await userTheme(env, "graphite", "Night");
    const requests = recordRequests(env);
    showEditor(env, id);
    await tokenRow("color.accent");
    fireEvent.change(screen.getByLabelText("Values for"), { target: { value: "high-contrast" } });
    const field = within(await tokenRow("color.accent")).getByRole("textbox", {
      name: "Value of color.accent",
    });
    fireEvent.change(field, { target: { value: "#ffff00" } });
    fireEvent.blur(field);
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        {
          op: "set-token",
          category: "color",
          name: "accent",
          value: "#ffff00",
          mode: "high-contrast",
        },
      ]);
    });
    // The groups are tabs; Corners lists the radius tokens.
    const tabs = screen.getByRole("tablist", { name: "Theme sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((t) => t.textContent),
    ).toEqual(["Colours", "Type", "Spacing", "Corners", "Motion", "More", "Parts"]);
    fireEvent.click(within(tabs).getByRole("tab", { name: "Corners" }));
    fireEvent.change(screen.getByLabelText("Values for"), { target: { value: "" } });
    const radius = within(await tokenRow("radius.md")).getByRole("textbox", {
      name: "Value of radius.md",
    });
    fireEvent.change(radius, { target: { value: "2" } });
    fireEvent.keyDown(radius, { key: "Enter" });
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        { op: "set-token", category: "radius", name: "md", value: "2", mode: null },
      ]);
    });
    // Keyboard: arrows move between the tabs.
    const corners = within(tabs).getByRole("tab", { name: "Corners" });
    fireEvent.keyDown(corners, { key: "ArrowDown" });
    expect(within(tabs).getByRole("tab", { name: "Motion" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await tokenRow("motion.fast")).toBeInTheDocument();
    await settleQueries(env);
  });

  it("overrides part styles: add, override inherited, remove, reset", async () => {
    const env = createEnv();
    const id = await userTheme(env);
    const requests = recordRequests(env);
    showEditor(env, id, "toolbar.spacer");
    // "Edit style" opens the Parts tab at the part.
    const part = await screen.findByRole("region", { name: "toolbar.spacer" });
    expect(screen.getByRole("tab", { name: "Parts" })).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByRole("button", { name: "toolbar.spacer", pressed: true }),
    ).toBeInTheDocument();
    // Inherited property of the base theme → this theme's own copy.
    fireEvent.click(within(part).getByRole("button", { name: "Override flex-grow in this theme" }));
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        { op: "set-part-property", selector: "toolbar.spacer", property: "flex-grow", value: "1" },
      ]);
    });
    // A new property.
    fireEvent.change(within(part).getByLabelText("Property"), {
      target: { value: "background-color" },
    });
    fireEvent.change(within(part).getByLabelText("Value"), { target: { value: "$color.accent" } });
    fireEvent.click(within(part).getByRole("button", { name: "Add" }));
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        {
          op: "set-part-property",
          selector: "toolbar.spacer",
          property: "background-color",
          value: "$color.accent",
        },
      ]);
    });
    await waitFor(() => {
      expect(
        within(screen.getByRole("region", { name: "toolbar.spacer" })).getByRole("textbox", {
          name: "Value of background-color",
        }),
      ).toHaveValue("$color.accent");
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove background-color" }));
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        {
          op: "set-part-property",
          selector: "toolbar.spacer",
          property: "background-color",
          value: null,
        },
      ]);
    });
    fireEvent.click(await screen.findByRole("button", { name: "Reset part" }));
    await waitFor(() => {
      expect(editRequests(requests).at(-1)?.patches).toEqual([
        { op: "reset-part", selector: "toolbar.spacer" },
      ]);
    });
    await waitFor(() => {
      expect(env.transport.data.features.appearance.files[id]?.parts).toEqual([]);
    });
    // An invalid property name is explained, not sent.
    const count = editRequests(requests).length;
    const region = screen.getByRole("region", { name: "toolbar.spacer" });
    fireEvent.change(within(region).getByLabelText("Property"), { target: { value: "--x" } });
    fireEvent.change(within(region).getByLabelText("Value"), { target: { value: "1" } });
    fireEvent.click(within(region).getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/property name/);
    expect(editRequests(requests)).toHaveLength(count);
    await settleQueries(env);
  });

  it("styles a part no rule names yet, and finds parts by search", async () => {
    const env = createEnv();
    const id = await userTheme(env);
    showEditor(env, id);
    fireEvent.click(await screen.findByRole("tab", { name: "Parts" }));
    expect(screen.getByText("Choose a part to see and change its style.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Find a part"), { target: { value: "split.divider" } });
    const list = screen.getByRole("list", { name: "Parts with styles" });
    expect(
      within(list)
        .getAllByRole("button")
        .every((b) => b.textContent.includes("split.divider")),
    ).toBe(true);
    fireEvent.change(screen.getByLabelText("Part"), { target: { value: "Not A Part" } });
    fireEvent.click(screen.getAllByRole("button", { name: "Add" })[0] ?? must(null));
    expect(await screen.findByRole("alert")).toHaveTextContent(/part name/);
    fireEvent.change(screen.getByLabelText("Part"), { target: { value: "my-widget.title" } });
    fireEvent.click(screen.getAllByRole("button", { name: "Add" })[0] ?? must(null));
    const part = await screen.findByRole("region", { name: "my-widget.title" });
    expect(within(part).getByText(/No theme styles this part yet/)).toBeInTheDocument();
    await settleQueries(env);
  });

  it("undoes and redoes edits through the appearance history", async () => {
    const env = createEnv();
    const id = await userTheme(env);
    showEditor(env, id);
    const field = within(await tokenRow("color.accent")).getByRole("textbox", {
      name: "Value of color.accent",
    });
    fireEvent.change(field, { target: { value: "#00aa00" } });
    fireEvent.keyDown(field, { key: "Enter" });
    const undo = await screen.findByRole("button", { name: "Undo Edit theme" });
    fireEvent.click(undo);
    await waitFor(async () => {
      expect(within(await tokenRow("color.accent")).getByText("Inherited")).toBeInTheDocument();
    });
    fireEvent.click(await screen.findByRole("button", { name: "Redo Edit theme" }));
    await waitFor(async () => {
      expect(
        within(await tokenRow("color.accent")).getByRole("textbox", {
          name: "Value of color.accent",
        }),
      ).toHaveValue("#00aa00");
    });
    await settleQueries(env);
  });

  it("reports a theme file changed on disk and reloads it", async () => {
    const env = createEnv();
    const id = await userTheme(env);
    showEditor(env, id);
    const field = within(await tokenRow("color.accent")).getByRole("textbox", {
      name: "Value of color.accent",
    });
    simulateExternalEdit(env.transport, "appearance");
    fireEvent.change(field, { target: { value: "#00aa00" } });
    fireEvent.keyDown(field, { key: "Enter" });
    const alert = await screen.findByText(/was changed outside Nail/);
    expect(env.transport.data.features.appearance.files[id]?.tokens).toEqual([]);
    fireEvent.click(
      within(must(alert.closest("p"))).getByRole("button", { name: "Reload the theme" }),
    );
    await waitFor(() => {
      expect(screen.queryByText(/was changed outside Nail/)).toBeNull();
    });
    await settleQueries(env);
  });

  it("switches themes, uses one, and opens Rename / Delete for user themes", async () => {
    const env = createEnv();
    const id = await userTheme(env, "sage", "Moss");
    showEditor(env, null);
    const chooser = await screen.findByLabelText("Theme");
    expect(within(chooser).getByRole("option", { name: "Moss (your theme)" })).toBeInTheDocument();
    fireEvent.change(chooser, { target: { value: id } });
    fireEvent.click(await screen.findByRole("button", { name: "Use this theme" }));
    await waitFor(() => {
      // Follow Windows stays on: the light theme of the pair.
      expect(env.transport.data.features.appearance.settings).toMatchObject({
        follow_system: true,
        light: id,
      });
    });
    expect(await screen.findByText("In use")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(env.store.getState().windows).toContain(THEME_RENAME_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().subject).toBe(id);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(env.store.getState().windows).toContain(THEME_DELETE_DIALOG);
    fireEvent.click(screen.getByRole("button", { name: "New theme" }));
    expect(env.store.getState().windows).toContain(THEME_NEW_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().newBase).toBe(id);
    await settleQueries(env);
  });

  it("is axe-clean in every built-in theme (tokens and parts)", async () => {
    const pristine = defaultTheme();
    for (const [themeId] of BUILTIN_THEMES) {
      const env = createEnv();
      const id = await userTheme(env, themeId, `Copy of ${themeId}`);
      act(() => {
        themeEditorStoreFor(env.store).getState().show(id, "toolbar.spacer");
      });
      const theme: ResolvedTheme = resolveMockTheme(
        pristine,
        themeId,
        { density: "comfortable", overrides: {} },
        pristine,
      );
      const { container, unmount } = renderWithProviders(<ThemeEditorDialog />, env, { theme });
      await screen.findByRole("region", { name: "toolbar.spacer" });
      await settleQueries(env);
      expect(await violations(container)).toEqual([]);
      fireEvent.click(screen.getByRole("tab", { name: "Colours" }));
      // Every row has the same structure: a few of them keep axe fast.
      fireEvent.change(screen.getByLabelText("Find a setting"), { target: { value: "accent" } });
      await tokenRow("color.accent");
      expect(await violations(container)).toEqual([]);
      unmount();
    }
  }, 180_000);
});

describe("theme windows", () => {
  it("creates a new theme from another one, uses it and opens it in the editor", async () => {
    const env = createEnv();
    const close = vi.fn();
    act(() => {
      themeEditorStoreFor(env.store).getState().setNewBase("nocturne");
    });
    const { container } = renderWithProviders(
      <NewThemeDialog name={THEME_NEW_DIALOG} close={close} />,
      env,
    );
    const base = await screen.findByLabelText("Start from");
    await waitFor(() => {
      expect(base).toHaveValue("nocturne");
    });
    expect(await violations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Enter a theme name");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Deep Sea" } });
    expect(screen.getByText("Saved as themes/deep-sea.kdl")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const state = env.transport.data.features.appearance;
    expect(state.themes.find((t) => t.id === "deep-sea")).toMatchObject({ extends: "nocturne" });
    await waitFor(() => {
      expect(state.settings.dark).toBe("deep-sea");
    });
    expect(env.store.getState().windows).toContain(THEME_EDITOR_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().themeId).toBe("deep-sea");
    await settleQueries(env);
  });

  it("renames a theme, optionally with its file", async () => {
    const env = createEnv();
    const id = await userTheme(env, "sage", "Moss");
    act(() => {
      const editor = themeEditorStoreFor(env.store).getState();
      editor.setSubject(id);
      editor.show(id);
    });
    const close = vi.fn();
    const { container } = renderWithProviders(
      <RenameThemeDialog name={THEME_RENAME_DIALOG} close={close} />,
      env,
    );
    const name = await screen.findByLabelText("Name");
    await waitFor(() => {
      expect(name).toHaveValue("Moss");
    });
    expect(await violations(container)).toEqual([]);
    fireEvent.change(name, { target: { value: "Forest" } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Also rename the file/ }));
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const state = env.transport.data.features.appearance;
    expect(state.themes.find((t) => t.id === "forest")?.name).toBe("Forest");
    // The editor follows the renamed file.
    expect(themeEditorStoreFor(env.store).getState().themeId).toBe("forest");
    await settleQueries(env);
  });

  it("asks before deleting a theme and deletes it", async () => {
    const env = createEnv();
    const id = await userTheme(env, "sage", "Moss");
    act(() => {
      const editor = themeEditorStoreFor(env.store).getState();
      editor.setSubject(id);
      editor.show(id);
    });
    const close = vi.fn();
    const { container } = renderWithProviders(
      <DeleteThemeDialog name={THEME_DELETE_DIALOG} close={close} />,
      env,
    );
    expect(await screen.findByText("Delete the theme “Moss”?")).toBeInTheDocument();
    expect(screen.getByText(/themes\/moss\.kdl is removed/)).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(env.transport.data.features.appearance.themes.map((t) => t.id)).not.toContain(id);
    expect(themeEditorStoreFor(env.store).getState().themeId).toBeNull();
    await settleQueries(env);
  });
});

describe("Appearance page and inspector", () => {
  it("opens New theme and the theme editor from the Appearance page", async () => {
    const env = createEnv();
    const { container } = renderNode(widget("appearance-settings"), env);
    fireEvent.click(await screen.findByRole("button", { name: "New theme" }));
    expect(env.store.getState().windows).toContain(THEME_NEW_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().newBase).toBe("frost");
    fireEvent.click(screen.getByRole("button", { name: "Edit theme" }));
    expect(env.store.getState().windows).toContain(THEME_EDITOR_DIALOG);
    expect(themeEditorStoreFor(env.store).getState().themeId).toBe("frost");
    expect(await violations(container)).toEqual([]);
    await settleQueries(env);
  });

  it("undoes an appearance change from the page", async () => {
    const env = createEnv();
    renderNode(widget("appearance-settings"), env);
    fireEvent.click(await screen.findByRole("radio", { name: /Nocturne/ }));
    const undo = await screen.findByRole("button", { name: "Undo Choose theme" });
    fireEvent.click(undo);
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.follow_system).toBe(true);
    });
    expect(await screen.findByRole("button", { name: "Redo Choose theme" })).toBeEnabled();
    await settleQueries(env);
  });

  it("jumps from the inspector's Edit style to the part in the theme editor", async () => {
    const env = createEnv();
    const target = document.createElement("div");
    target.setAttribute("data-part", "message-list.row");
    document.body.append(target);
    const onOpen = vi.fn();
    renderWithProviders(<EditStyleButton element={target} onOpen={onOpen} />, env);
    fireEvent.click(screen.getByRole("button", { name: "Edit style" }));
    expect(onOpen).toHaveBeenCalled();
    expect(env.store.getState().windows).toContain(THEME_EDITOR_DIALOG);
    expect(themeEditorStoreFor(env.store).getState()).toMatchObject({
      themeId: null,
      part: "message-list.row",
    });
    target.remove();
    await settleQueries(env);
  });

  it("offers no Edit style for an element without a part", () => {
    const env = createEnv();
    const plain = document.createElement("div");
    renderWithProviders(<EditStyleButton element={plain} />, env);
    expect(screen.queryByRole("button", { name: "Edit style" })).toBeNull();
  });
});
