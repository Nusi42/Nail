import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { Request, ResolvedLayout } from "../../api/generated";
import { LayoutContext } from "../../layout/LayoutContext";
import { NodeView } from "../../layout/LayoutRenderer";
import { queryKeys, useClient } from "../../state/queries";
import {
  createEnv,
  renderWithProviders,
  stubElementSizes,
  widget,
  type Env,
} from "../../test/harness";
import { columnsOf } from "../../widgets/MessageList";
import { messageLists, recordKey } from "./Editors";
import { moved } from "./ReorderList";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

function sent<T extends Request["type"]>(env: Env, type: T): Extract<Request, { type: T }>[] {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

/** Follows `ConfigChanged` like the app shell: the editors see the written layout. */
function LiveLayout({ children }: { children: ReactNode }) {
  const client = useClient();
  const config = useQuery({
    queryKey: queryKeys.config,
    queryFn: async () => (await client.request({ type: "GetConfig" }, "Config")).layout,
  });
  if (!config.data) return null;
  return <LayoutContext.Provider value={config.data}>{children}</LayoutContext.Provider>;
}

function renderEditor(kind: string, env = createEnv()) {
  env.transport.onEvent((event) => {
    if (event.type === "ConfigChanged") {
      env.queryClient.setQueryData<ResolvedLayout>(queryKeys.config, event.layout);
    }
  });
  const view = renderWithProviders(
    <LiveLayout>
      <NodeView node={widget(kind)} path="t" />
    </LiveLayout>,
    env,
  );
  return { env, ...view };
}

describe("editor helpers", () => {
  it("records keys, chords and sequences", () => {
    const key = (
      k: string,
      mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {},
    ) => ({
      key: k,
      ctrlKey: false,
      altKey: false,
      shiftKey: false,
      metaKey: false,
      ...mods,
    });
    expect(recordKey("", key("M", { ctrlKey: true, shiftKey: true }))).toBe("ctrl+shift+m");
    expect(recordKey("g", key("r"))).toBe("g r");
    expect(recordKey("g r", key("Backspace"))).toBe("g");
    expect(recordKey("a b c", key("d"))).toBe("d");
    expect(recordKey("", key("Shift"))).toBeNull();
    expect(moved(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
  });

  it("finds the message lists of the layout", () => {
    const layout = createEnv().transport.data.layout;
    const lists = messageLists(layout);
    expect(lists.length).toBeGreaterThanOrEqual(2);
    expect(lists[0]?.ref.window).toBe("main");
    expect(lists[0]?.node.kind).toBe("message-list");
  });
});

describe("toolbar-editor", () => {
  it("reorders with buttons and Alt+arrows, adds items and saves", async () => {
    const user = userEvent.setup();
    const { env } = renderEditor("toolbar-editor");
    const section = await screen.findByRole("region", { name: "Toolbar editor" });
    await user.selectOptions(within(section).getByLabelText("Toolbar"), "reader");
    const list = within(section).getByRole("list", { name: "Items of the toolbar reader" });
    const items = () => within(list).getAllByRole("listitem");
    expect(items()[0]).toHaveTextContent("Reply");
    await user.click(
      within(items()[0] as HTMLElement).getByRole("button", { name: /Move Reply down/ }),
    );
    expect(items()[1]).toHaveTextContent("Reply");
    // Focus follows the moved item.
    expect(
      within(items()[1] as HTMLElement).getByRole("button", { name: /Move Reply down/ }),
    ).toHaveFocus();
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(items()[0]).toHaveTextContent("Reply");
    await user.click(within(section).getByRole("button", { name: "Add separator" }));
    expect(items().at(-1)).toHaveTextContent("Separator");
    await user.click(within(items()[1] as HTMLElement).getByRole("button", { name: /Remove/ }));
    await user.click(within(section).getByRole("button", { name: "Save to layout.kdl" }));
    expect(await within(section).findByText("Saved to layout.kdl.")).toBeVisible();
    const [update] = sent(env, "UpdateLayout");
    expect(update?.patches[0]).toMatchObject({ op: "toolbar-order", toolbar: "reader" });
    const written = env.transport.data.layout.toolbars.reader ?? [];
    expect(written.at(-1)).toEqual({ type: "separator" });
    expect(written.some((i) => i.type === "action" && i.action === "mail.reply-all")).toBe(false);

    await user.click(within(section).getByRole("button", { name: "Back to the default toolbar" }));
    await waitFor(() =>
      expect(sent(env, "UpdateLayout")[1]?.patches).toEqual([
        { op: "reset-toolbar", toolbar: "reader" },
      ]),
    );
  });
});

describe("key-binding-editor", () => {
  it("records a shortcut, warns about conflicts, removes and resets", async () => {
    const user = userEvent.setup();
    const { env } = renderEditor("key-binding-editor");
    const section = await screen.findByRole("region", { name: "Key binding editor" });
    const table = within(section).getByRole("table", { name: "Shortcuts" });
    expect(within(table).getAllByRole("row").length).toBeGreaterThan(5);
    await user.selectOptions(within(section).getByLabelText("Where"), "global");
    await user.selectOptions(within(section).getByLabelText("Action"), "mail.archive");
    const capture = within(section).getByLabelText("Keys");
    capture.focus();
    fireEvent.keyDown(capture, { key: "r" });
    expect(capture).toHaveValue("R");
    expect(within(section).getByText(/Replaces the shortcut for Reply/)).toBeVisible();
    fireEvent.keyDown(capture, { key: "Backspace" });
    fireEvent.keyDown(capture, { key: "k", ctrlKey: true, shiftKey: true });
    expect(capture).toHaveValue("Ctrl+Shift+K");
    await user.click(within(section).getByRole("button", { name: "Add shortcut" }));
    expect(await within(section).findByText("Shortcut saved to layout.kdl.")).toBeVisible();
    expect(sent(env, "UpdateLayout")[0]?.patches).toEqual([
      {
        op: "add-key-binding",
        context: "global",
        key: "ctrl+shift+k",
        action: "mail.archive",
        args: [],
      },
    ]);
    await user.selectOptions(within(section).getByLabelText("Show"), "global");
    const row = await within(table).findByText("Ctrl+Shift+K");
    const tr = row.closest("tr") as HTMLElement;
    await user.click(within(tr).getByRole("button", { name: /Remove Ctrl\+Shift\+K/ }));
    await waitFor(() =>
      expect(sent(env, "UpdateLayout")[1]?.patches).toEqual([
        { op: "remove-key-binding", context: "global", key: "ctrl+shift+k" },
      ]),
    );
    const reply = within(table).getByRole("button", { name: "Restore the default for R" });
    await user.click(reply);
    await waitFor(() =>
      expect(sent(env, "UpdateLayout")[2]?.patches).toEqual([
        { op: "reset-key-binding", context: "global", key: "r" },
      ]),
    );
  });

  it("does not let recorded keys trigger the app's shortcuts", async () => {
    const { env } = renderEditor("key-binding-editor");
    const capture = await screen.findByLabelText("Keys");
    act(() => capture.focus());
    const event = new KeyboardEvent("keydown", { key: "c", bubbles: true, cancelable: true });
    act(() => {
      capture.dispatchEvent(event);
    });
    expect(event.defaultPrevented).toBe(true);
    expect(capture).toHaveValue("C");
    expect(env.store.getState().windows).toEqual([]);
  });
});

describe("column-editor", () => {
  it("switches a list to columns and the list renders them", async () => {
    const user = userEvent.setup();
    const { env } = renderEditor("column-editor");
    const section = await screen.findByRole("region", { name: "Column editor" });
    expect(within(section).getByText(/shows rows made from templates/)).toBeVisible();
    const save = within(section).getByRole("button", { name: "Save to layout.kdl" });
    expect(save).toBeDisabled();
    await user.selectOptions(within(section).getByLabelText("Column"), "subject");
    await user.click(within(section).getByRole("button", { name: "Add column" }));
    await user.selectOptions(within(section).getByLabelText("Column"), "from");
    await user.click(within(section).getByRole("button", { name: "Add column" }));
    await user.selectOptions(within(section).getByLabelText("Column"), "date");
    await user.click(within(section).getByRole("button", { name: "Add column" }));
    const list = within(section).getByRole("list", { name: "Columns in display order" });
    await user.click(within(list).getByRole("button", { name: "Move From up" }));
    await user.click(save);
    expect(await within(section).findByText("Saved to layout.kdl.")).toBeVisible();
    const [update] = sent(env, "UpdateLayout");
    expect(update?.patches[0]).toMatchObject({
      op: "column-order",
      node: { window: "main", breakpoint: 0 },
      columns: ["from", "subject", "date"],
    });
    const node = messageLists(env.transport.data.layout)[0]?.node;
    if (!node) throw new Error("no list");
    expect(columnsOf(node)).toEqual(["from", "subject", "date"]);

    // The list now renders one cell per column, in that order.
    env.store.setState({ activeAccountId: "acc-personal", activeFolderId: "acc-personal/Inbox" });
    const { container } = renderWithProviders(<NodeView node={node} path="t" />, env);
    await waitFor(() =>
      expect(container.querySelector('[data-part="message-list.row.columns"]')).not.toBeNull(),
    );
    const row = container.querySelector('[data-part="message-list.row"]');
    const cells = row?.querySelectorAll('[data-part="message-list.row.cell"]') ?? [];
    expect([...cells].map((c) => c.getAttribute("data-column"))).toEqual([
      "from",
      "subject",
      "date",
    ]);
    expect(container.querySelector('[data-part="message-list.header"]')).toHaveTextContent(
      "FromSubjectDate",
    );
  });
});
