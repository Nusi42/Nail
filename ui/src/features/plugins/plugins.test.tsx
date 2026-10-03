import { act, fireEvent, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { PluginDialog, PluginUiNode, Request } from "../../api/generated";
import { ClientContext } from "../../state/queries";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { appRegistry } from "../registry";
import {
  INSTALL_DIALOG,
  MANAGER_DIALOG,
  RUN_COMMAND,
  handlePluginsEvent,
  pluginKeys,
  usePluginLayout,
} from ".";
import { clientIdOf } from "./context";
import { InstallDialog } from "./InstallDialog";
import { permissionMessageId } from "./messages";
import { crashPlugin } from "./mockHost";
import { PluginDialogs } from "./PluginDialogs";
import { PluginNotifications, TOAST_MS } from "./PluginNotifications";
import { pluginsStoreFor } from "./store";
import {
  ALL_GRANTS,
  REMIND,
  installFollowUp,
  must,
  pluginLayout,
  tick,
  wireEvents,
} from "./testUtil";
import { PluginTree } from "./tree/PluginTree";

function sent<T extends Request["type"]>(env: Env, type: T): Extract<Request, { type: T }>[] {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

function openDialog(env: Env, dialog: Partial<PluginDialog> & { spec: PluginDialog["spec"] }) {
  act(() =>
    env.transport.emit({
      type: "PluginDialogOpened",
      dialog: {
        dialog_id: "d1",
        plugin_id: "follow-up",
        client_id: clientIdOf(env.client),
        ...dialog,
      },
    }),
  );
}

describe("plugins feature slot", () => {
  it("registers widgets, dialogs and actions", () => {
    expect(appRegistry.widgets["plugin-panel"]).toBeDefined();
    expect(appRegistry.widgets["plugin-manager"]).toBeDefined();
    expect(appRegistry.dialogs[INSTALL_DIALOG]).toBeDefined();
    expect(appRegistry.dialogs[MANAGER_DIALOG]).toBeDefined();
    for (const id of [RUN_COMMAND, "plugins.install", "plugins.load-dev", "plugins.manage"]) {
      expect(appRegistry.actions[id]).toBeDefined();
    }
    expect(appRegistry.effects).toContain(PluginDialogs);
    expect(appRegistry.effects).toContain(PluginNotifications);
  });

  it("names the permission labels like the core", () => {
    expect(permissionMessageId("mail.read.body")).toBe("plugin-permission-mail-read-body");
    expect(permissionMessageId("network:api.example.com")).toBe("plugin-permission-network");
  });

  it("refetches plug-ins when they change", () => {
    const env = createEnv();
    const queryClient = new QueryClient();
    const spy = vi.spyOn(queryClient, "invalidateQueries");
    handlePluginsEvent({ type: "PluginsChanged" }, { ...env, queryClient });
    expect(spy).toHaveBeenCalledWith({ queryKey: pluginKeys.all });
    handlePluginsEvent({ type: "SnoozeChanged" }, { ...env, queryClient });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("opens the install dialog and the manager from actions", () => {
    const env = createEnv();
    const ctx = { store: env.store } as never;
    void must(appRegistry.actions["plugins.load-dev"]).run(ctx, []);
    expect(env.store.getState().windows).toEqual([INSTALL_DIALOG]);
    expect(pluginsStoreFor(env.store).getState().installMode).toBe("dev");
    void must(appRegistry.actions["plugins.manage"]).run(ctx, []);
    expect(env.store.getState().windows.at(-1)).toBe(MANAGER_DIALOG);
  });
});

describe("plug-in UI trees", () => {
  const all: PluginUiNode[] = [
    { type: "text", value: "Heading", style: "heading" },
    { type: "text", value: '<img src="x" onerror="alert(1)">', style: "error" },
    { type: "markdown", value: "Some **bold** <script>alert(1)</script>" },
    { type: "input", id: "name", label: "Name", required: true, placeholder: "Your name" },
    { type: "textarea", id: "note", label: "Note", rows: 3 },
    {
      type: "select",
      id: "list",
      label: "List",
      options: [
        { value: "a", label: "Alpha" },
        { value: "b", label: "Beta" },
      ],
    },
    { type: "checkbox", id: "done", label: "Done" },
    { type: "date", id: "due", label: "Due", time: true },
    { type: "contact-picker", id: "who", label: "Who", multiple: true },
    { type: "folder-picker", id: "where", label: "Where", accountId: "acc-personal" },
    {
      type: "table",
      id: "rows",
      columns: [{ label: "Col" }],
      rows: [["r1"], ["r2"]],
      selectable: true,
      multiple: true,
    },
    { type: "button", id: "go", label: "Go", variant: "primary" },
    { type: "progress", value: 40, label: "Loading" },
    { type: "progress" },
    { type: "image", src: "data:image/png;base64,iVBORw0KGgo=", alt: "Logo", width: 10 },
    { type: "image", src: "https://evil.example/x.png", alt: "Remote" },
    { type: "image", src: "data:image/svg+xml;base64,PHN2Zz4=", alt: "Svg" },
    { type: "stack", direction: "horizontal", children: [{ type: "text", value: "Inside" }] },
  ];

  function renderTree(onChange = vi.fn(), onAction = vi.fn()) {
    const env = createEnv();
    let values: Record<string, unknown> = {};
    const view = renderWithProviders(
      <PluginTree
        plugin="follow-up"
        nodes={all}
        values={{
          name: "",
          note: "",
          list: "",
          done: false,
          due: null,
          who: [],
          where: null,
          rows: [],
        }}
        issues={{ name: "required" }}
        onChange={(id, value) => {
          values = { ...values, [id]: value };
          onChange(id, value);
        }}
        onAction={onAction}
      />,
      env,
    );
    return { env, view, onChange, onAction };
  }

  it("renders every node with its part and plug-in, never as HTML", () => {
    const { view } = renderTree();
    const root = view.container;
    expect(root.querySelector('[data-part="plugin.tree"]')).toHaveAttribute(
      "data-plugin",
      "follow-up",
    );
    expect(screen.getByRole("heading", { name: "Heading" })).toHaveAttribute(
      "data-part",
      "plugin.text",
    );
    expect(screen.getByText('<img src="x" onerror="alert(1)">')).toHaveAttribute(
      "data-appearance",
      "error",
    );
    expect(root.querySelector("script")).toBeNull();
    expect(root.querySelector('img[src="x"]')).toBeNull();
    expect(screen.getByText("bold").closest("strong")).not.toBeNull();
    expect(screen.getByText(/<script>alert\(1\)<\/script>/)).toBeInTheDocument();
    for (const part of [
      "plugin.input",
      "plugin.textarea",
      "plugin.select",
      "plugin.checkbox",
      "plugin.date",
      "plugin.contact-picker",
      "plugin.folder-picker",
      "plugin.table",
      "plugin.button",
      "plugin.progress",
      "plugin.image",
      "plugin.stack",
      "plugin.markdown",
    ]) {
      const el = root.querySelector(`[data-part="${part}"]`);
      expect(el, part).not.toBeNull();
      expect(el).toHaveAttribute("data-plugin", "follow-up");
    }
    expect(root.querySelector('[data-part="plugin.input"]')).toHaveAttribute("data-id", "name");
    expect(root.querySelectorAll("img")).toHaveLength(1);
    expect(screen.getByRole("img", { name: "Logo" })).toHaveAttribute("width", "10");
    expect(root.querySelector('[data-part="plugin.date"]')).toHaveAttribute(
      "type",
      "datetime-local",
    );
  });

  it("labels controls and describes invalid ones", () => {
    renderTree();
    const name = screen.getByRole("textbox", { name: "Name" });
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(name).toHaveAttribute("aria-required", "true");
    expect(name).toHaveAccessibleDescription("This field is required.");
    expect(name.closest('[data-part="plugin.field"]')).toHaveAttribute("data-state", "invalid");
    expect(screen.getByRole("combobox", { name: "List" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Done" })).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Loading" })).toHaveAttribute("value", "40");
    expect(screen.getByRole("progressbar", { name: "Progress" })).not.toHaveAttribute("value");
  });

  it("reports values of the documented kinds", async () => {
    const user = userEvent.setup();
    const { onChange, onAction } = renderTree();
    await user.type(screen.getByRole("textbox", { name: "Name" }), "A");
    expect(onChange).toHaveBeenLastCalledWith("name", "A");
    await user.selectOptions(screen.getByRole("combobox", { name: "List" }), "b");
    expect(onChange).toHaveBeenLastCalledWith("list", "b");
    await user.click(screen.getByRole("checkbox", { name: "Done" }));
    expect(onChange).toHaveBeenLastCalledWith("done", true);
    fireEvent.change(screen.getByLabelText("Due"), { target: { value: "2026-10-01T09:30" } });
    expect(onChange).toHaveBeenLastCalledWith("due", "2026-10-01T09:30");
    await user.click(screen.getByRole("checkbox", { name: "Select row 2" }));
    expect(onChange).toHaveBeenLastCalledWith("rows", [1]);
    await user.type(screen.getByRole("textbox", { name: "Who" }), "Ada <ada@example.org>{Enter}");
    expect(onChange).toHaveBeenLastCalledWith("who", [{ name: "Ada", email: "ada@example.org" }]);
    await user.click(screen.getByRole("button", { name: "Go" }));
    expect(onAction).toHaveBeenCalledWith("go");
  });

  it("refuses invalid contact addresses", async () => {
    const user = userEvent.setup();
    const { onChange } = renderTree();
    await user.type(screen.getByRole("textbox", { name: "Who" }), "nobody{Enter}");
    expect(onChange).not.toHaveBeenCalledWith("who", expect.anything());
    expect(screen.getByRole("textbox", { name: "Who" })).toHaveAccessibleDescription(
      "Enter a mail address like name@example.org.",
    );
  });

  it("lists the folders of the chosen account", async () => {
    renderTree();
    const select = screen.getByRole("combobox", { name: "Where" });
    await waitFor(() => expect(within(select).getAllByRole("option").length).toBeGreaterThan(1));
    expect(within(select).getAllByRole("option")[0]).toHaveTextContent("(no folder)");
  });

  it("asks before a Markdown link opens outside Nail", async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    renderWithProviders(
      <PluginTree
        plugin="p"
        nodes={[
          { type: "markdown", value: "See [docs](https://docs.example/a) or [bad](javascript:x)" },
        ]}
        values={{}}
        issues={{}}
        onChange={vi.fn()}
        onAction={vi.fn()}
      />,
      createEnv(),
    );
    expect(screen.queryByRole("link", { name: "bad" })).toBeNull();
    await user.click(screen.getByRole("link", { name: "docs" }));
    expect(open).not.toHaveBeenCalled();
    expect(screen.getByText("Open https://docs.example/a in your browser?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText(/in your browser/)).toBeNull();
    await user.click(screen.getByRole("link", { name: "docs" }));
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(open).toHaveBeenCalledWith("https://docs.example/a", "_blank", "noopener,noreferrer");
    open.mockRestore();
  });
});

describe("plug-in commands, dialogs and notifications", () => {
  async function setup() {
    const env = createEnv();
    const first = must(env.transport.data.messages[0]);
    env.store.getState().selectMessage(first.id);
    const plugin = await installFollowUp(env);
    const unsubscribe = wireEvents(env);
    const view = renderWithProviders(
      <>
        {renderToolbar()}
        <PluginDialogs />
        <PluginNotifications />
      </>,
      env,
      { layout: pluginLayout(env, [plugin]) },
    );
    return { env, first, view, unsubscribe };
  }

  function renderToolbar(): ReactNode {
    const View = must(appRegistry.widgets["toolbar"]);
    return <View node={widget("toolbar", { options: { toolbar: "reader" } })} nodeKey="t" />;
  }

  it("runs a toolbar command, validates the dialog and returns its values", async () => {
    const user = userEvent.setup();
    const { env, first, unsubscribe } = await setup();
    const button = screen.getByRole("button", { name: "Follow up" });
    expect(button).toBeEnabled();
    await user.click(button);
    expect(sent(env, "RunPluginCommand")[0]).toMatchObject({
      command: REMIND,
      context: { message_id: first.id },
      client_id: clientIdOf(env.client),
    });
    const dialog = await screen.findByRole("dialog", { name: "Follow up" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(dialog.closest('[data-part="plugin.dialog.backdrop"]')).not.toBeNull();
    expect(within(dialog).getByText("from Follow-up")).toBeInTheDocument();
    expect(within(dialog).getByText(new RegExp(first.subject))).toBeInTheDocument();
    const due = within(dialog).getByLabelText("Due");
    expect((due as HTMLInputElement).value).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(document.activeElement).toBe(due);

    fireEvent.change(due, { target: { value: "" } });
    await user.click(within(dialog).getByRole("button", { name: "Save reminder" }));
    expect(sent(env, "RespondPluginDialog")).toHaveLength(0);
    expect(dialog).toHaveAttribute("data-state", "invalid");
    expect(within(dialog).getByText("Please correct the marked field.")).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(due));
    expect(due).toHaveAccessibleDescription("This field is required.");

    fireEvent.change(due, { target: { value: "2026-10-01" } });
    expect(due).not.toHaveAttribute("aria-invalid");
    await user.type(within(dialog).getByLabelText("Note"), "call back");
    await user.click(within(dialog).getByRole("button", { name: "Save reminder" }));
    expect(sent(env, "RespondPluginDialog")[0]).toMatchObject({
      button: "ok",
      values: { due: "2026-10-01", note: "call back" },
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(await screen.findByText("Reminder saved")).toBeInTheDocument();
    expect(env.transport.data.features.plugins.host?.storage["follow-up"]?.[first.id]).toEqual({
      due: "2026-10-01",
      note: "call back",
    });
    unsubscribe();
  });

  it("runs a command from its key binding", async () => {
    const { env, unsubscribe } = await setup();
    fireEvent.keyDown(document.body, { key: "R", shiftKey: true });
    await waitFor(() => expect(sent(env, "RunPluginCommand")).toHaveLength(1));
    unsubscribe();
  });

  it("dismisses a dialog with Escape and keeps Tab inside", async () => {
    const user = userEvent.setup();
    const { env, unsubscribe } = await setup();
    await user.click(screen.getByRole("button", { name: "Follow up" }));
    const dialog = await screen.findByRole("dialog");
    const buttons = within(dialog).getAllByRole("button");
    must(buttons.at(-1)).focus();
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(sent(env, "RespondPluginDialog")[0]).toMatchObject({ button: null });
    unsubscribe();
  });

  it("disables the command when the plug-in is off or nothing is selected", async () => {
    const { env, unsubscribe } = await setup();
    act(() => env.store.getState().selectMessage(null));
    // The reader bar hides actions that cannot run (M5b `hide-disabled`).
    expect(screen.queryByRole("button", { name: "Follow up" })).toBeNull();
    unsubscribe();
  });

  it("shows only this UI's dialogs; non-modal ones do not block", async () => {
    const env = createEnv();
    const unsubscribe = wireEvents(env);
    renderWithProviders(<PluginDialogs />, env);
    openDialog(env, {
      dialog_id: "other",
      client_id: "someone-else",
      spec: { title: "Other", body: [] },
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    openDialog(env, {
      dialog_id: "float",
      client_id: null,
      spec: { title: "Floating", body: [{ type: "text", value: "Hi" }], modal: false },
    });
    const dialog = screen.getByRole("dialog", { name: "Floating" });
    expect(dialog).toHaveAttribute("aria-modal", "false");
    expect(dialog).not.toHaveAttribute("data-key-modal");
    expect(dialog.closest('[data-part="plugin.dialog.backdrop"]')).toBeNull();
    act(() =>
      env.transport.emit({
        type: "PluginDialogClosed",
        dialog_id: "float",
        plugin_id: "follow-up",
      }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    unsubscribe();
  });

  it("sends dialog button actions with the values and updates the dialog", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const unsubscribe = wireEvents(env);
    renderWithProviders(<PluginDialogs />, env);
    const spec = {
      title: "Search",
      body: [
        { type: "input" as const, id: "q", label: "Query" },
        { type: "button" as const, id: "find", label: "Find" },
      ],
    };
    openDialog(env, { spec });
    await user.type(screen.getByRole("textbox", { name: "Query" }), "abc");
    await user.click(screen.getByRole("button", { name: "Find" }));
    expect(sent(env, "PluginUiAction")[0]).toMatchObject({
      target: { kind: "dialog", dialog_id: "d1" },
      component_id: "find",
      values: { q: "abc" },
    });
    // The plug-in answers with a new tree for the same dialog: the query stays.
    openDialog(env, {
      spec: { ...spec, body: [...spec.body, { type: "text", value: "3 results" }] },
    });
    expect(screen.getByText("3 results")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Query" })).toHaveValue("abc");
    // The mock host has no dialog "d1": the core's error is shown in the dialog.
    expect(screen.getByRole("alert")).toHaveTextContent("The item no longer exists.");
    unsubscribe();
  });

  it("shows notifications for this UI and notices about stopped plug-ins", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await installFollowUp(env);
    const unsubscribe = wireEvents(env);
    renderWithProviders(<PluginNotifications />, env);
    act(() => {
      env.transport.emit({
        type: "PluginNotification",
        notification: {
          id: "n1",
          plugin_id: "follow-up",
          client_id: null,
          spec: { title: "Done", body: "All good", level: "success" },
        },
      });
      env.transport.emit({
        type: "PluginNotification",
        notification: {
          id: "n2",
          plugin_id: "follow-up",
          client_id: "another-ui",
          spec: { title: "Not mine" },
        },
      });
    });
    const toast = must(screen.getByText("Done").closest("li"));
    expect(toast).toHaveAttribute("data-level", "success");
    expect(within(toast).getByText("Follow-up")).toBeInTheDocument();
    expect(within(toast).getByText("All good")).toBeInTheDocument();
    expect(screen.queryByText("Not mine")).toBeNull();
    await user.click(within(toast).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText("Done")).toBeNull();

    act(() => crashPlugin(env.transport, "follow-up", "cpu-budget"));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Follow-up was stopped because it took too long.",
    );
    unsubscribe();
  });

  it("lets non-error toasts time out", () => {
    vi.useFakeTimers();
    try {
      const env = createEnv();
      const unsubscribe = wireEvents(env);
      renderWithProviders(<PluginNotifications />, env);
      act(() =>
        env.transport.emit({
          type: "PluginNotification",
          notification: { id: "n", plugin_id: "p", client_id: null, spec: { title: "Bye" } },
        }),
      );
      expect(screen.getByText("Bye")).toBeInTheDocument();
      act(() => vi.advanceTimersByTime(TOAST_MS + 1));
      expect(screen.queryByText("Bye")).toBeNull();
      unsubscribe();
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes a crashed plug-in's dialogs", async () => {
    const user = userEvent.setup();
    const { env, unsubscribe } = await setup();
    await user.click(screen.getByRole("button", { name: "Follow up" }));
    await screen.findByRole("dialog");
    act(() => crashPlugin(env.transport, "follow-up", "error"));
    expect(screen.queryByRole("dialog")).toBeNull();
    unsubscribe();
  });
});

describe("usePluginLayout", () => {
  it("adds the installed plug-ins' commands to the layout", async () => {
    const env = createEnv();
    await installFollowUp(env);
    env.queryClient.removeQueries({ queryKey: pluginKeys.list });
    const base = env.transport.data.layout;
    const { result } = renderHook(() => usePluginLayout(base), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={env.queryClient}>
          <ClientContext.Provider value={env.client}>{children}</ClientContext.Provider>
        </QueryClientProvider>
      ),
    });
    expect(result.current).toBe(base);
    await waitFor(() => expect(result.current.actions[REMIND]?.label).toBe("Follow up"));
    expect(result.current.actions[REMIND]?.alias_of).toBe(RUN_COMMAND);
  });
});

describe("plug-in panels", () => {
  it("renders the panel tree, follows changes and sends actions", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await installFollowUp(env);
    const unsubscribe = wireEvents(env);
    must(env.transport.data.features.plugins.host).storage["follow-up"] = {
      m1: { due: "2026-10-02" },
    };
    renderNode(widget("plugin-panel", { options: { panel: "follow-up.reminders" } }), env);
    const panel = screen.getByRole("region", { name: "Reminders" });
    expect(panel).toHaveAttribute("data-plugin", "follow-up");
    expect(await within(panel).findByRole("cell", { name: "2026-10-02" })).toBeInTheDocument();
    expect(sent(env, "GetPluginPanel")[0]).toMatchObject({
      panel: "follow-up.reminders",
      client_id: clientIdOf(env.client),
    });
    await user.click(within(panel).getByRole("button", { name: "Clear reminders" }));
    expect(sent(env, "PluginUiAction")[0]).toMatchObject({
      target: { kind: "panel", panel: "follow-up.reminders" },
      component_id: "clear",
    });
    expect(await within(panel).findByText("No reminders.")).toBeInTheDocument();
    unsubscribe();
  });

  it("says when the plug-in is missing or off", async () => {
    const env = createEnv();
    renderNode(widget("plugin-panel", { options: { panel: "follow-up.due" } }), env);
    const panel = screen.getByRole("region", { name: "Plug-in panel" });
    expect(panel.getAttribute("data-part")).toBe("plugin-panel");
    expect(panel.getAttribute("data-panel")).toBe("follow-up.due");
    await tick();
    expect(panel).toHaveAttribute("data-state", "missing");
    expect(panel.textContent).toContain("not installed or not enabled");
    expect(sent(env, "GetPluginPanel")).toHaveLength(0);
  });
});

describe("install dialog", () => {
  function renderInstall(env: Env) {
    const close = vi.fn();
    renderWithProviders(<InstallDialog name={INSTALL_DIALOG} close={close} />, env);
    return close;
  }

  it("shows the permissions, installs with the chosen grants and proposes placements", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderInstall(env);
    await user.type(
      screen.getByRole("textbox", { name: "Path of the .nailplugin file" }),
      "C:/plugins/follow-up.nailplugin",
    );
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    expect(await screen.findByRole("heading", { name: "Follow-up 1.0.0" })).toBeInTheDocument();
    expect(screen.getByText("By Nail examples")).toBeInTheDocument();
    expect(screen.getByText("Command “Follow up”")).toBeInTheDocument();
    const permissions = screen.getByRole("list", { name: "Permissions" });
    expect(within(permissions).getAllByRole("listitem")).toHaveLength(4);
    const storage = within(permissions).getByRole("group", {
      name: "Store its own data on this device",
    });
    await user.click(within(storage).getByRole("radio", { name: "Deny" }));
    const read = within(permissions).getByRole("group", { name: /^Read your mail/ });
    expect(within(read).getByRole("radio", { name: "Allow" })).toBeChecked();
    const scope = screen.getByRole("group", { name: /Accounts for/ });
    await user.click(within(scope).getByRole("radio", { name: "Only these accounts" }));
    const boxes = within(scope).getAllByRole("checkbox");
    await user.click(must(boxes[0]));
    expect(screen.getByText("Choose at least one account.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
    await user.click(must(boxes[1]));
    await user.click(screen.getByRole("button", { name: "Install" }));
    const install = must(sent(env, "InstallPlugin")[0]);
    expect(install.package_sha256).toHaveLength(64);
    expect(install.enable).toBe(true);
    expect(install.grants.map((g) => g.permission)).toEqual([
      "mail.read",
      "ui.dialogs",
      "notifications",
    ]);
    expect(must(install.grants[0]).accounts).toHaveLength(1);

    expect(await screen.findByText("Follow-up is installed.")).toBeInTheDocument();
    const items = await screen.findAllByRole("checkbox");
    expect(items.length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: /Add 1 item to the layout/ }));
    expect(sent(env, "ApplyConfigMerge")[0]).toMatchObject({
      file: "layout",
      choices: [{ take: "theirs" }],
      default: "mine",
    });
    expect(await screen.findByText(/Added to your layout/)).toBeInTheDocument();
    expect(env.transport.data.layout.keys).toContainEqual(
      expect.objectContaining({ action: REMIND }),
    );
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() =>
      expect(env.transport.data.layout.keys).not.toContainEqual(
        expect.objectContaining({ action: REMIND }),
      ),
    );
  });

  it("lets the user skip the placements", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const close = renderInstall(env);
    await user.type(screen.getByRole("textbox"), "C:/plugins/follow-up.nailplugin");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    await user.click(await screen.findByRole("button", { name: "Install" }));
    await user.click(await screen.findByRole("button", { name: "Not now" }));
    expect(sent(env, "ApplyConfigMerge")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(close).toHaveBeenCalled();
  });

  it("refuses a broken package and names its problems", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderInstall(env);
    await user.type(screen.getByRole("textbox"), "C:/plugins/broken.nailplugin");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    expect(
      await screen.findByText("This plug-in cannot be installed because of a problem:"),
    ).toBeInTheDocument();
    expect(screen.getByText(/The manifest is not valid: expected value/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("button", { name: "Check the plug-in" })).toBeInTheDocument();
  });

  it("explains a package that changed after the check", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderInstall(env);
    await user.type(screen.getByRole("textbox"), "C:/plugins/follow-up.nailplugin");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    await screen.findByRole("button", { name: "Install" });
    const host = must(env.transport.data.features.plugins.host);
    host.packages["file:C:/plugins/follow-up.nailplugin"] = {
      ...must(host.packages["file:C:/plugins/follow-up.nailplugin"]),
      sha256: "f".repeat(64),
    };
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The package changed");
  });

  it("shows errors of the preview", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderInstall(env);
    await user.click(screen.getByRole("radio", { name: "A web address" }));
    await user.type(screen.getByRole("textbox", { name: "Web address (https)" }), "https://nope");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(must(sent(env, "PreviewPluginInstall")[0]).source).toEqual({
      kind: "url",
      url: "https://nope",
    });
  });

  it("loads a development folder", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    pluginsStoreFor(env.store).getState().setInstallMode("dev");
    renderInstall(env);
    expect(screen.getByRole("radio", { name: "A development folder" })).toBeChecked();
    await user.type(
      screen.getByRole("textbox", { name: "Folder with manifest.json" }),
      "C:/dev/follow-up",
    );
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    expect(await screen.findByText(/Development folder/)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Turn it on right away" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Install" }));
    await screen.findByText("Follow-up is installed.");
    expect(sent(env, "LoadDevPlugin")[0]).toMatchObject({
      folder: "C:/dev/follow-up",
      grants: ALL_GRANTS,
    });
  });
});

describe("plug-in manager", () => {
  async function renderManager() {
    const env = createEnv();
    await installFollowUp(env);
    const unsubscribe = wireEvents(env);
    renderNode(widget("plugin-manager"), env);
    const row = must(
      (await screen.findByRole("heading", { name: "Follow-up 1.0.0" })).closest("li"),
    );
    return { env, row, unsubscribe };
  }

  it("lists plug-ins and turns them off and on", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    expect(within(row).getByText("Running")).toBeInTheDocument();
    expect(
      within(row).getByText("Installed from C:/plugins/follow-up.nailplugin"),
    ).toBeInTheDocument();
    const toggle = within(row).getByRole("switch", { name: "Follow-up is on" });
    expect(toggle).toBeChecked();
    await user.click(toggle);
    expect(sent(env, "DisablePlugin")).toHaveLength(1);
    await waitFor(() => expect(within(row).getByText("Off")).toBeInTheDocument());
    unsubscribe();
  });

  it("edits grants", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    const show = within(row).getByRole("button", { name: "Permissions" });
    await user.click(show);
    expect(show).toHaveAttribute("aria-expanded", "true");
    const save = within(row).getByRole("button", { name: "Save permissions" });
    expect(save).toBeDisabled();
    const notifications = within(row).getByRole("group", { name: "Show notifications" });
    await user.click(within(notifications).getByRole("radio", { name: "Deny" }));
    await user.click(save);
    expect(must(sent(env, "UpdatePluginGrants")[0]).grants.map((g) => g.permission)).toEqual([
      "mail.read",
      "ui.dialogs",
      "storage",
    ]);
    expect(await within(row).findByText("Permissions saved.")).toBeInTheDocument();
    unsubscribe();
  });

  it("shows, follows and clears the log", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    await user.click(within(row).getByRole("button", { name: "Log" }));
    const log = within(row).getByRole("region", { name: "Plug-in log" });
    expect(await within(log).findByText("installed 1.0.0")).toBeInTheDocument();
    act(() => {
      env.transport.data.features.plugins.logs.push({
        seq: 99,
        plugin_id: "follow-up",
        at: new Date().toISOString(),
        level: "warn",
        source: "plugin",
        message: "careful",
      });
      env.transport.emit({
        type: "PluginLogLine",
        entry: must(env.transport.data.features.plugins.logs.at(-1)),
      });
    });
    expect(await within(log).findByText("careful")).toBeInTheDocument();
    await user.selectOptions(
      within(log).getByRole("combobox", { name: "Show from level" }),
      "warn",
    );
    expect(within(log).queryByText("installed 1.0.0")).toBeNull();
    await user.click(within(log).getByRole("button", { name: "Clear log" }));
    expect(await within(log).findByText("The log is empty.")).toBeInTheDocument();
    expect(sent(env, "ClearPluginLogs")).toHaveLength(1);
    unsubscribe();
  });

  it("edits settings from the schema", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    await user.click(within(row).getByRole("button", { name: "Settings" }));
    const days = await within(row).findByRole("spinbutton", { name: "Default days" });
    expect(days).toHaveValue(1);
    await user.clear(days);
    await user.type(days, "99");
    await user.click(within(row).getByRole("button", { name: "Save settings" }));
    expect(await within(row).findByRole("alert")).toHaveTextContent(
      "These settings have invalid values: days",
    );
    await user.clear(days);
    await user.type(days, "3");
    await user.click(within(row).getByRole("button", { name: "Save settings" }));
    expect(await within(row).findByText("Settings saved.")).toBeInTheDocument();
    expect(sent(env, "UpdatePluginSettings").at(-1)?.values).toEqual({ days: 3 });
    unsubscribe();
  });

  it("proposes placements again from the manager", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    await user.click(within(row).getByRole("button", { name: "Place in layout" }));
    expect(await within(row).findByRole("button", { name: /Add 1 item/ })).toBeInTheDocument();
    expect(sent(env, "PreviewPluginPlacements")).toHaveLength(1);
    unsubscribe();
  });

  it("uninstalls after a confirmation", async () => {
    const user = userEvent.setup();
    const { env, row, unsubscribe } = await renderManager();
    await user.click(within(row).getByRole("button", { name: "Remove" }));
    const confirm = within(row).getByRole("group", { name: "Remove" });
    await user.click(within(confirm).getByRole("button", { name: "Keep" }));
    expect(sent(env, "UninstallPlugin")).toHaveLength(0);
    await user.click(within(row).getByRole("button", { name: "Remove" }));
    await user.click(
      within(within(row).getByRole("group", { name: "Remove" })).getByRole("button", {
        name: "Remove",
      }),
    );
    expect(sent(env, "UninstallPlugin")).toHaveLength(1);
    expect(await screen.findByText("No plug-ins are installed.")).toBeInTheDocument();
    unsubscribe();
  });

  it("shows why a plug-in stopped", async () => {
    const { env, row, unsubscribe } = await renderManager();
    act(() => crashPlugin(env.transport, "follow-up", "memory-limit"));
    expect(
      await within(row).findByText("Follow-up was stopped because it used too much memory."),
    ).toBeInTheDocument();
    expect(within(row).getByText("Stopped after a problem")).toBeInTheDocument();
    unsubscribe();
  });

  it("opens the install dialog in both modes", async () => {
    const user = userEvent.setup();
    const { env, unsubscribe } = await renderManager();
    await user.click(screen.getByRole("button", { name: "Load a development folder…" }));
    expect(env.store.getState().windows).toEqual([INSTALL_DIALOG]);
    expect(pluginsStoreFor(env.store).getState().installMode).toBe("dev");
    act(() => env.store.getState().closeWindow());
    await user.click(screen.getByRole("button", { name: "Install a plug-in…" }));
    expect(pluginsStoreFor(env.store).getState().installMode).toBe("package");
    unsubscribe();
  });

  it("says when nothing is installed", async () => {
    const env = createEnv();
    renderNode(widget("plugin-manager"), env);
    expect(await screen.findByText("No plug-ins are installed.")).toBeInTheDocument();
  });
});
