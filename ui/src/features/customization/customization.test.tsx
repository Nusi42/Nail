/**
 * Customization slot: the mock core's answers, the config error list, smart
 * folders, the merge dialog with live preview, the inspector and the split
 * write-back.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionContext } from "../../actions/registry";
import type { Request, Response } from "../../api/generated";
import { queryKeys, type AppConfig } from "../../state/queries";
import {
  Providers,
  createEnv,
  renderNode,
  renderWithProviders,
  widget,
  type Env,
} from "../../test/harness";
import { Inspector } from "./Inspector";
import { MergeDialog, PREVIEW_DELAY_MS } from "./MergeDialog";
import { SplitWriteBack } from "./SplitWriteBack";
import { actions, dialogs, effects, handleCustomizationEvent, widgets } from "./index";
import { runtimeIds } from "./messages";
import { answerCustomization, isCustomizationRequest } from "./mock";
import { customizationStore } from "./state";
import { WRITE_DELAY_MS } from "./writeBack";
import { requestsOf, wireEvents } from "../../test/env";
import { must } from "../../test/must";

async function call(env: Env, request: Request): Promise<Response> {
  return env.client.call(request);
}

function seedConfig(env: Env): AppConfig {
  const config: AppConfig = {
    theme: structuredClone(env.transport.data.theme),
    layout: structuredClone(env.transport.data.layout),
    locale: null,
    platform: structuredClone(env.transport.data.platform),
  };
  env.queryClient.setQueryData(queryKeys.config, config);
  return config;
}

const THEME = `theme "Nord" extends="default" {
    tokens {
        color "accent" "#88c0d0"
        color "frost" "#8fbcbb"
    }
}
`;

beforeEach(() => {
  customizationStore.getState().reset();
});
afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

/** Waits until the configuration the UI shows has the theme `name` (the live preview). */
async function previewing(env: Env, name: string): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.getQueryData<AppConfig>(queryKeys.config)?.theme.name).toBe(name);
  });
}

describe("slot contributions", () => {
  it("registers widgets, actions, dialogs, events and effects", () => {
    expect(Object.keys(widgets).sort()).toEqual([
      "column-editor",
      "config-errors",
      "key-binding-editor",
      "smart-folders",
      "toolbar-editor",
    ]);
    expect(Object.keys(actions).sort()).toEqual([
      "customize.errors",
      "customize.merge",
      "inspector.toggle",
    ]);
    expect(Object.keys(dialogs).sort()).toEqual(["config-errors", "config-merge"]);
    expect(effects).toHaveLength(3);
    expect(runtimeIds()).toContain("window-config-merge");
    expect(runtimeIds()).toContain("merge-conflict-duplicate-key");
    for (const type of ["UpdateLayout", "OpenConfigFile", "ListSavedSearches"] as const) {
      expect(isCustomizationRequest({ type } as Request)).toBe(true);
    }
  });

  it("actions open the dialogs and toggle the inspector", () => {
    const env = createEnv();
    const ctx = { store: env.store } as unknown as ActionContext;
    void actions["customize.merge"]?.run(ctx, []);
    void actions["customize.errors"]?.run(ctx, []);
    expect(env.store.getState().windows).toEqual(["config-merge", "config-errors"]);
    void actions["inspector.toggle"]?.run(ctx, []);
    expect(customizationStore.getState().inspecting).toBe(true);
    void actions["inspector.toggle"]?.run(ctx, []);
    expect(customizationStore.getState().inspecting).toBe(false);
  });
});

describe("mock core", () => {
  it("applies layout patches and announces the new layout", async () => {
    const env = createEnv();
    const events: string[] = [];
    env.client.subscribe((e) => events.push(e.type));
    const patches = [
      {
        op: "split-sizes" as const,
        node: { window: "mail", breakpoint: 0, path: [] },
        sizes: ["300px", "1fr", "2fr"],
      },
      {
        op: "add-key-binding" as const,
        context: "list",
        key: "E",
        action: "mail.archive",
        args: [],
      },
      { op: "remove-key-binding" as const, context: "list", key: "j" },
      { op: "toolbar-order" as const, toolbar: "reader", items: [{ type: "spacer" as const }] },
      {
        op: "column-order" as const,
        node: { window: "mail", breakpoint: 0, path: [1, 4] },
        columns: ["subject", "from"],
      },
    ];
    expect(await call(env, { type: "UpdateLayout", patches })).toEqual({ type: "Done" });
    const layout = env.transport.data.layout;
    // The M5b mail window's root is the panes split.
    expect(layout.windows.mail?.[0]?.root).toMatchObject({
      type: "split",
      sizes: ["300px", "1fr", "2fr"],
    });
    expect(layout.keys).toContainEqual({
      context: "list",
      key: "e",
      action: "mail.archive",
      args: [],
    });
    expect(layout.keys.some((k) => k.context === "list" && k.key === "j")).toBe(false);
    expect(layout.toolbars.reader).toEqual([{ type: "spacer" }]);
    // The new layout, then its undo entry (M5d).
    expect(events).toEqual(["ConfigChanged", "EditHistoryChanged"]);
    expect(env.transport.data.features.customization.layoutPatches).toHaveLength(5);

    const bad = await call(env, {
      type: "UpdateLayout",
      patches: [
        { op: "split-sizes", node: { window: "main", breakpoint: 0, path: [1] }, sizes: [] },
      ],
    });
    expect(bad).toMatchObject({ type: "Error", code: "invalid_request" });
  });

  it("previews, resolves, applies and reverts a merge", async () => {
    const env = createEnv();
    const original = structuredClone(env.transport.data.theme);
    const preview = await call(env, { type: "PreviewConfigMerge", file: "theme", incoming: THEME });
    expect(preview.type === "ConfigMergePreview" && preview.preview.sections).toEqual([
      {
        section: "header",
        items: [expect.objectContaining({ id: "header:name", change: "changed" })],
      },
      {
        section: "tokens",
        items: [
          expect.objectContaining({ id: "tokens:color.accent", change: "changed" }),
          expect.objectContaining({ id: "tokens:color.frost", change: "added" }),
        ],
      },
    ]);
    const resolved = await call(env, {
      type: "ResolveConfigMerge",
      file: "theme",
      incoming: THEME,
      choices: [{ item_id: "tokens:color.accent", take: "theirs" }],
      default: "mine",
    });
    expect(resolved.type === "ConfigMergeResolved" && resolved.theme?.tokens.color?.accent).toBe(
      "#88c0d0",
    );
    expect(resolved.type === "ConfigMergeResolved" && resolved.theme?.name).toBe(original.name);
    expect(env.transport.data.theme).toEqual(original);

    const applied = await call(env, {
      type: "ApplyConfigMerge",
      file: "theme",
      incoming: THEME,
      choices: [],
      default: "theirs",
    });
    expect(applied.type).toBe("ConfigMergeApplied");
    expect(env.transport.data.theme.name).toBe("Nord");
    const backups = await call(env, { type: "ListConfigBackups", file: "theme" });
    expect(backups.type === "ConfigBackups" && backups.backups).toHaveLength(1);
    const backupId = applied.type === "ConfigMergeApplied" ? applied.backup.id : "";
    expect(await call(env, { type: "RevertConfig", file: "theme", backup_id: backupId })).toEqual({
      type: "Done",
    });
    expect(env.transport.data.theme).toEqual(original);
    expect(
      await call(env, { type: "RevertConfig", file: "layout", backup_id: null }),
    ).toMatchObject({ code: "not_found" });

    const broken = await call(env, {
      type: "PreviewConfigMerge",
      file: "layout",
      incoming: "nonsense",
    });
    expect(broken.type === "ConfigMergePreview" && broken.preview.errors).toHaveLength(1);
    expect(
      await call(env, {
        type: "ResolveConfigMerge",
        file: "theme",
        incoming: THEME,
        choices: [{ item_id: "tokens:nope", take: "theirs" }],
        default: "mine",
      }),
    ).toMatchObject({ code: "invalid_request" });
  });

  it("flags two actions on one key in a layout merge", async () => {
    const env = createEnv();
    const text =
      'layout "x" extends="default" {\n    keys context="list" {\n        bind "j" "mail.archive"\n    }\n}\n';
    const r = await call(env, { type: "PreviewConfigMerge", file: "layout", incoming: text });
    const item = r.type === "ConfigMergePreview" ? r.preview.sections[1]?.items[0] : undefined;
    expect(item).toMatchObject({ key: "list: j", change: "changed" });
    expect(item?.conflicts[0]?.kind).toBe("duplicate-key");
  });

  it("records opened files and answers unsupported in web mode", async () => {
    const env = createEnv();
    expect(
      answerCustomization(env.transport, {
        type: "OpenConfigFile",
        file: "layout",
        line: 3,
        column: 1,
      }),
    ).toEqual({ type: "Done" });
    expect(env.transport.data.features.customization.opened).toEqual([
      { file: "layout", line: 3, column: 1 },
    ]);
    env.transport.data.features.customization.openUnsupported = true;
    expect(
      await call(env, { type: "OpenConfigFile", file: "theme", line: 1, column: 1 }),
    ).toMatchObject({ code: "unsupported" });
  });
});

describe("config-errors", () => {
  it("lists every error of every file and opens a file at the line", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env, handleCustomizationEvent);
    renderNode(widget("config-errors"), env);
    expect(screen.getByText("All configuration files are valid.")).toBeInTheDocument();

    act(() => {
      env.transport.emit({
        type: "ConfigErrors",
        file: "theme.kdl",
        errors: [
          { line: 2, column: 5, message: "unknown theme section `bogus`" },
          { line: 7, column: 9, message: "unknown token `$color.nope`" },
        ],
      });
      env.transport.emit({
        type: "ConfigError",
        file: "accounts.kdl",
        line: 1,
        column: 1,
        message: "syntax error",
      });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("3 errors in 2 files");
    const theme = must(
      screen.getByRole("heading", { name: "theme.kdl — 2 errors" }).closest("section"),
    );
    expect(within(theme).getByText("unknown token `$color.nope`")).toBeInTheDocument();
    expect(within(theme).getByText("theme.kdl:7:9")).toBeInTheDocument();

    await user.click(must(within(theme).getAllByRole("button", { name: "Open in editor" })[1]));
    await waitFor(() => {
      expect(env.transport.data.features.customization.opened).toEqual([
        { file: "theme", line: 7, column: 9 },
      ]);
    });

    env.transport.data.features.customization.openUnsupported = true;
    await user.click(must(within(theme).getAllByRole("button", { name: "Open in editor" })[0]));
    expect(await screen.findByText(/Opening files needs the desktop app/)).toBeInTheDocument();

    act(() => {
      env.transport.emit({ type: "ConfigErrors", file: "theme.kdl", errors: [] });
      env.transport.emit({ type: "AccountsChanged" });
    });
    expect(screen.getByText("All configuration files are valid.")).toBeInTheDocument();
  });

  it("can hide itself while everything is valid", () => {
    const env = createEnv();
    renderNode(widget("config-errors", { options: { "hide-when-valid": true } }), env);
    expect(document.querySelector('[data-part="config-errors"]')).toBeNull();
  });
});

describe("smart-folders", () => {
  it("saves the current search, runs a saved one and removes it", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env, handleCustomizationEvent);
    renderNode(widget("smart-folders"), env);
    expect(await screen.findByText(/No smart folders yet/)).toBeInTheDocument();
    const add = screen.getByRole("button", { name: "Save current search" });
    expect(add).toBeDisabled();

    act(() => {
      env.store.getState().setSearch({
        text: "is:unread",
        accountId: null,
        folderId: null,
        error: null,
      });
    });
    await user.click(add);
    await user.type(screen.getByRole("textbox", { name: "Name of the smart folder" }), "Unread");
    await user.click(screen.getByRole("button", { name: "Save" }));

    const item = await screen.findByRole("button", { name: "Unread" });
    expect(requestsOf(env, "SaveSavedSearch")[0]?.search).toMatchObject({
      id: null,
      name: "Unread",
      query: "is:unread",
    });
    expect(item).toHaveAttribute("data-state", "selected");

    act(() => {
      env.store.getState().setSearch(null);
    });
    expect(item).not.toHaveAttribute("data-state");
    await user.click(item);
    expect(env.store.getState().search?.text).toBe("is:unread");

    await user.click(screen.getByRole("button", { name: "Remove “Unread”" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Unread" })).toBeNull();
    });
    expect(requestsOf(env, "DeleteSavedSearch")).toHaveLength(1);
  });

  it("shows the core's refusal", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("smart-folders"), env);
    act(() => {
      env.store
        .getState()
        .setSearch({ text: "from:(", accountId: null, folderId: null, error: null });
    });
    await user.click(await screen.findByRole("button", { name: "Save current search" }));
    await user.type(screen.getByRole("textbox", { name: "Name of the smart folder" }), "Broken");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("hides editing when not editable", async () => {
    const env = createEnv();
    renderNode(widget("smart-folders", { options: { editable: false } }), env);
    await screen.findByText(/No smart folders yet/);
    expect(screen.queryByRole("button", { name: "Save current search" })).toBeNull();
  });
});

describe("merge dialog", () => {
  it("compares, previews live, applies and reverts", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const config = seedConfig(env);
    const close = vi.fn();
    renderWithProviders(<MergeDialog name="config-merge" close={close} />, env);

    await user.click(screen.getByRole("textbox"));
    await user.paste(THEME);
    await user.click(screen.getByRole("button", { name: "Compare with mine" }));

    const tokens = await screen.findByRole("heading", { name: "Tokens" });
    const section = must(tokens.closest<HTMLElement>('[data-part="merge-dialog.section"]'));
    expect(within(section).getByText("color.accent")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("3 differences, 0 taken, no conflicts");
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();

    // Take the accent colour only: the live preview shows it.
    const accent = must(
      within(section)
        .getByText("color.accent")
        .closest<HTMLElement>('[data-part="merge-dialog.item"]'),
    );
    await user.click(within(accent).getByRole("radio", { name: "Take theirs" }));
    await waitFor(
      () => {
        expect(
          env.queryClient.getQueryData<AppConfig>(queryKeys.config)?.theme.tokens.color?.accent,
        ).toBe("#88c0d0");
      },
      { timeout: PREVIEW_DELAY_MS * 20 },
    );
    expect(env.transport.data.theme.tokens.color?.accent).toBe(config.theme.tokens.color?.accent);
    expect(requestsOf(env, "ResolveConfigMerge").at(-1)?.choices).toEqual([
      { item_id: "tokens:color.accent", take: "theirs" },
    ]);

    // Section shortcut.
    await user.click(within(section).getByRole("button", { name: "Take theirs: Tokens" }));
    expect(screen.getByRole("status")).toHaveTextContent("3 differences, 2 taken");

    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(
      await screen.findByText(/Applied\. Your previous file was backed up/),
    ).toBeInTheDocument();
    expect(env.transport.data.theme.tokens.color?.frost).toBe("#8fbcbb");
    expect(env.transport.data.theme.name).toBe(config.theme.name);

    await user.click(screen.getByRole("button", { name: "Revert" }));
    expect(await screen.findByText("The previous version is back.")).toBeInTheDocument();
    expect(env.transport.data.theme.tokens.color?.accent).toBe(config.theme.tokens.color?.accent);

    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(close).toHaveBeenCalled();
  });

  it("restores the real configuration when closed without applying", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const config = seedConfig(env);
    const view = renderWithProviders(<MergeDialog name="config-merge" close={vi.fn()} />, env);
    await user.click(screen.getByRole("textbox"));
    await user.paste(THEME);
    await user.click(screen.getByRole("button", { name: "Compare with mine" }));
    await user.click(await screen.findByRole("button", { name: "Take all theirs" }));
    await previewing(env, "Nord");
    // Turning the live preview off restores the real configuration, too.
    await user.click(screen.getByRole("checkbox", { name: "Live preview" }));
    await previewing(env, config.theme.name);
    await user.click(screen.getByRole("checkbox", { name: "Live preview" }));
    await previewing(env, "Nord");
    view.unmount();
    expect(env.queryClient.getQueryData<AppConfig>(queryKeys.config)?.theme.name).toBe(
      config.theme.name,
    );
    expect(env.transport.data.theme.name).toBe(config.theme.name);
  });

  it("lists the errors of an invalid incoming file", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    seedConfig(env);
    renderWithProviders(<MergeDialog name="config-merge" close={vi.fn()} />, env);
    await user.click(screen.getByRole("radio", { name: "Layout (layout.kdl)" }));
    await user.click(screen.getByRole("textbox"));
    await user.paste('theme "x" {}');
    await user.click(screen.getByRole("button", { name: "Compare with mine" }));
    expect(
      await screen.findByText("The file you loaded has an error. Fix it and compare again."),
    ).toBeInTheDocument();
    expect(screen.getByText(/Line 1, column 1: expected `layout/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("button", { name: "Compare with mine" })).toBeInTheDocument();
    expect(requestsOf(env, "PreviewConfigMerge")[0]?.file).toBe("layout");
  });

  it("loads a file and lists earlier versions to revert to", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    seedConfig(env);
    await call(env, {
      type: "ApplyConfigMerge",
      file: "layout",
      incoming: 'layout "Shared" extends="default" {\n}\n',
      choices: [],
      default: "theirs",
    });
    expect(env.transport.data.layout.name).toBe("Shared");
    renderWithProviders(<MergeDialog name="config-merge" close={vi.fn()} />, env);
    const upload = must(
      document.querySelector<HTMLElement>('[data-part="merge-dialog.upload.input"]'),
    );
    await user.upload(upload, new File([THEME.replace("Nord", "Mine")], "my-layout.kdl"));
    await waitFor(() =>
      expect(screen.getByRole("textbox")).toHaveValue(THEME.replace("Nord", "Mine")),
    );
    expect(screen.getByRole("radio", { name: "Layout (layout.kdl)" })).toBeChecked();

    const backups = await screen.findByRole("heading", { name: "Earlier versions" });
    const list = must(backups.closest<HTMLElement>("section"));
    await user.click(await within(list).findByRole("button", { name: "Revert" }));
    expect(await within(list).findByText("The previous version is back.")).toBeInTheDocument();
    expect(env.transport.data.layout.name).not.toBe("Shared");
  });
});

describe("inspector", () => {
  function renderInspected(env: Env) {
    const clicked = vi.fn();
    render(
      <Providers env={env}>
        <main data-part="app">
          <section data-window="mail">
            <div data-part="message-list" data-node-id="list">
              <button
                type="button"
                data-part="message-list.row"
                data-state="selected"
                onClick={clicked}
              >
                <span data-part="message-list.row.subject">Hello</span>
              </button>
            </div>
          </section>
        </main>
        <Inspector />
      </Providers>,
    );
    return clicked;
  }

  it("shows part path, states, rules and the layout node; a click pins", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const clicked = renderInspected(env);
    expect(document.querySelector('[data-part="inspector"]')).toBeNull();

    act(() => {
      customizationStore.getState().setInspecting(true);
    });
    const panel = screen.getByRole("complementary", { name: "Inspector" });
    expect(within(panel).getByText("Nothing picked yet.")).toBeInTheDocument();

    fireEvent.mouseOver(screen.getByText("Hello"));
    expect(within(panel).getByText("message-list.row.subject")).toHaveAttribute(
      "data-state",
      "current",
    );
    expect(document.querySelector('[data-part="inspector.highlight"]')).not.toBeNull();
    expect(within(panel).getByText(/Theme rules/)).toBeInTheDocument();
    expect(within(panel).getByText("message-list #list")).toBeInTheDocument();
    expect(within(panel).getByText(/window “mail”, breakpoint 0, path 1\/4/)).toBeInTheDocument();

    // The click picks instead of pressing the row.
    await user.click(screen.getByRole("button", { name: "Hello" }));
    expect(clicked).not.toHaveBeenCalled();
    expect(within(panel).getByText("Showing the element you clicked.")).toBeInTheDocument();
    expect(within(panel).getByText(":selected")).toBeInTheDocument();
    // Hovering elsewhere does not move a pinned panel.
    fireEvent.mouseOver(must(document.querySelector<HTMLElement>('[data-part="message-list"]')));
    expect(within(panel).getByText(":selected")).toBeInTheDocument();

    await user.click(within(panel).getByRole("button", { name: "Pick another element" }));
    expect(within(panel).getByText("Nothing picked yet.")).toBeInTheDocument();

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(document.querySelector('[data-part="inspector"]')).toBeNull();
    await user.click(screen.getByRole("button", { name: "Hello" }));
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("lists matching theme rules with their source", () => {
    const env = createEnv();
    env.transport.data.theme.parts.push({
      selector: "message-list.row",
      states: ["selected"],
      conditions: [],
      ancestors: [],
      declarations: { background: "var(--nail-color-accent)" },
      source: { file: "theme.kdl", line: 12, column: 5 },
    });
    renderInspected(env);
    act(() => {
      customizationStore.getState().setInspecting(true);
      customizationStore.getState().inspect(screen.getByRole("button", { name: "Hello" }), true);
    });
    const panel = screen.getByRole("complementary", { name: "Inspector" });
    expect(within(panel).getAllByText("message-list.row:selected").length).toBeGreaterThan(0);
    expect(within(panel).getByText("background: var(--nail-color-accent)")).toBeInTheDocument();
    expect(within(panel).getByText("theme.kdl:12:5")).toBeInTheDocument();
    expect(within(panel).getAllByRole("button", { name: "Open in editor" }).length).toBeGreaterThan(
      0,
    );
  });
});

describe("split write-back", () => {
  it("writes dragged sizes into layout.kdl once dragging settles", async () => {
    vi.useFakeTimers();
    const env = createEnv();
    render(
      <Providers env={env}>
        <SplitWriteBack />
      </Providers>,
    );
    act(() => {
      env.store.getState().setSplitSizes("panes", ["250px", "1fr", "1fr"]);
      env.store.getState().setSplitSizes("panes", ["260px", "1fr", "1fr"]);
    });
    expect(requestsOf(env, "UpdateLayout")).toHaveLength(0);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS + 10);
    });
    expect(requestsOf(env, "UpdateLayout")).toEqual([
      {
        type: "UpdateLayout",
        patches: [
          {
            op: "split-sizes",
            node: { window: "mail", breakpoint: 0, path: [] },
            sizes: ["260px", "1fr", "1fr"],
          },
        ],
      },
    ]);
    // Unchanged sizes are not written again.
    act(() => {
      env.store.getState().setActiveTab("x", 1);
    });
    act(() => {
      env.store.getState().setSplitSizes("panes", ["260px", "1fr", "1fr"]);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(WRITE_DELAY_MS + 10);
    });
    expect(requestsOf(env, "UpdateLayout")).toHaveLength(1);
  });
});
