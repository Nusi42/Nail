/**
 * Accessibility of the plug-in UI (axe-core in jsdom, like `src/a11y.test.tsx`):
 * every component of a tree, a modal and a non-modal dialog with invalid
 * fields, notifications, a panel, every stage of the install dialog and
 * the manager with each section open. Contrast needs a real layout engine
 * (Playwright).
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import type { PluginUiNode } from "../../api/generated";
import { clientIdOf } from "./context";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { INSTALL_DIALOG } from ".";
import { InstallDialog } from "./InstallDialog";
import { crashPlugin } from "./mockHost";
import { PluginDialogs } from "./PluginDialogs";
import { PluginNotifications } from "./PluginNotifications";
import { installFollowUp, must, tick, wireEvents } from "./testUtil";

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(() => tick());
}

const EVERY_NODE: PluginUiNode[] = [
  { type: "text", value: "Title", style: "heading" },
  { type: "text", value: "Body" },
  { type: "text", value: "Muted", style: "muted" },
  {
    type: "markdown",
    value: "# Head\n\nText with [a link](https://x.example) and `code`.\n\n- one\n- two\n\n> quote",
  },
  { type: "input", id: "name", label: "Name", required: true },
  { type: "input", id: "mail", label: "Mail", kind: "email" },
  { type: "textarea", id: "note", label: "Note" },
  { type: "select", id: "one", label: "One", options: [{ value: "a", label: "A" }] },
  {
    type: "select",
    id: "many",
    label: "Many",
    multiple: true,
    options: [
      { value: "a", label: "A" },
      { value: "b", label: "B" },
    ],
  },
  { type: "checkbox", id: "ok", label: "OK" },
  { type: "date", id: "day", label: "Day", default: "today" },
  { type: "date", id: "when", label: "When", time: true },
  {
    type: "contact-picker",
    id: "who",
    label: "Who",
    default: [{ name: "Ada", email: "ada@example.org" }],
  },
  { type: "folder-picker", id: "where", label: "Where" },
  {
    type: "message-list",
    id: "msgs",
    messageIds: ["m1"],
    selectable: true,
    messages: [
      {
        id: "m1",
        uri: "nail://messages/m1",
        accountId: "acc-personal",
        folderId: "acc-personal/Inbox",
        subject: "Hello",
        to: [],
        cc: [],
        date: "2026-09-30T10:00:00Z",
        flags: { seen: false, flagged: false, answered: false, draft: false },
        tags: [],
        hasAttachments: false,
        encrypted: false,
      },
    ],
  },
  { type: "table", id: "t", columns: [{ label: "A" }], rows: [["1"]], selectable: true },
  { type: "table", columns: [{ label: "B" }], rows: [["2"]] },
  { type: "button", id: "go", label: "Go" },
  { type: "progress", value: 10, label: "Busy" },
  { type: "progress" },
  { type: "image", src: "data:image/png;base64,iVBORw0KGgo=", alt: "Logo" },
  { type: "image", src: "data:image/png;base64,iVBORw0KGgo=", alt: "" },
  {
    type: "stack",
    direction: "horizontal",
    children: [{ type: "button", id: "b2", label: "Two" }],
  },
];

describe("plug-in UI accessibility", () => {
  it("a modal dialog with every component and invalid fields", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const unsubscribe = wireEvents(env);
    const view = renderWithProviders(<PluginDialogs />, env);
    act(() =>
      env.transport.emit({
        type: "PluginDialogOpened",
        dialog: {
          dialog_id: "d1",
          plugin_id: "follow-up",
          client_id: clientIdOf(env.client),
          spec: {
            title: "Everything",
            body: EVERY_NODE,
            buttons: [
              { id: "ok", label: "Save", primary: true },
              { id: "no", label: "Cancel" },
            ],
          },
        },
      }),
    );
    await settle(env);
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("dialog")).toHaveAttribute("data-state", "invalid");
    expect(await violations(view.container)).toEqual([]);
    unsubscribe();
  });

  it("a non-modal dialog and notifications", async () => {
    const env = createEnv();
    await installFollowUp(env);
    const unsubscribe = wireEvents(env);
    const view = renderWithProviders(
      <>
        <PluginDialogs />
        <PluginNotifications />
      </>,
      env,
    );
    act(() => {
      env.transport.emit({
        type: "PluginDialogOpened",
        dialog: {
          dialog_id: "d2",
          plugin_id: "follow-up",
          client_id: null,
          spec: { title: "Floating", body: [{ type: "text", value: "Hi" }], modal: false },
        },
      });
      env.transport.emit({
        type: "PluginNotification",
        notification: {
          id: "n",
          plugin_id: "follow-up",
          client_id: null,
          spec: { title: "Saved", body: "Done" },
        },
      });
    });
    act(() => crashPlugin(env.transport, "follow-up", "cpu-budget"));
    await settle(env);
    expect(await violations(view.container)).toEqual([]);
    unsubscribe();
  });

  it("a panel", async () => {
    const env = createEnv();
    await installFollowUp(env);
    must(env.transport.data.features.plugins.host).storage["follow-up"] = {
      m1: { due: "2026-10-02" },
    };
    const view = renderNode(
      widget("plugin-panel", { options: { panel: "follow-up.reminders" } }),
      env,
    );
    await screen.findByRole("table");
    await settle(env);
    expect(await violations(view.container)).toEqual([]);
  });

  it("every stage of the install dialog", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const view = renderWithProviders(<InstallDialog name={INSTALL_DIALOG} close={vi.fn()} />, env);
    expect(await violations(view.container)).toEqual([]);
    await user.type(screen.getByRole("textbox"), "C:/plugins/follow-up.nailplugin");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    await screen.findByRole("button", { name: "Install" });
    const scope = screen.getByRole("group", { name: /Accounts for/ });
    await user.click(within(scope).getByRole("radio", { name: "Only these accounts" }));
    await settle(env);
    expect(await violations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Install" }));
    await screen.findByRole("button", { name: /Add 1 item/ });
    await settle(env);
    expect(await violations(view.container)).toEqual([]);
  });

  it("a broken package", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const view = renderWithProviders(<InstallDialog name={INSTALL_DIALOG} close={vi.fn()} />, env);
    await user.type(screen.getByRole("textbox"), "C:/plugins/broken.nailplugin");
    await user.click(screen.getByRole("button", { name: "Check the plug-in" }));
    await screen.findByText(/cannot be installed/);
    expect(await violations(view.container)).toEqual([]);
  });

  it.each(["Permissions", "Settings", "Log", "Place in layout"])(
    "the manager with %s open",
    async (section) => {
      const user = userEvent.setup();
      const env = createEnv();
      await installFollowUp(env);
      const view = renderNode(widget("plugin-manager"), env);
      await user.click(await screen.findByRole("button", { name: section }));
      await user.click(screen.getByRole("button", { name: "Remove" }));
      await settle(env);
      expect(await violations(view.container)).toEqual([]);
    },
  );
});
