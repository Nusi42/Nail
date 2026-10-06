/**
 * Gmail label UI against the mock core: the label list (tree, colours,
 * visibility, label views), the label editor (palette, nesting,
 * validation), deleting, applying labels to a message, the category tabs,
 * the message label chips and the feature's actions.
 */
import type { InfiniteData } from "@tanstack/react-query";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentType } from "react";
import type { MessageSummary, Request, Response } from "../../api/generated";
import { listSelectionFor } from "../../actions/mailTargets";
import type { ActionContext } from "../../actions/registry";
import type { DialogProps } from "../../layout/dialogRegistry";
import { NodeView } from "../../layout/LayoutRenderer";
import { queryKeys } from "../../state/queries";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { actions, dialogs, MessageLabels } from ".";
import { GMAIL_ACCOUNT, GMAIL_FOLDERS, seedGmailDemo } from "./mockSeed";
import { APPLY_LABELS_DIALOG, LABEL_DELETE_DIALOG, LABEL_EDITOR_DIALOG } from "./names";
import { gmailStoreFor } from "./store";

function setup(account = GMAIL_ACCOUNT): Env {
  const env = createEnv();
  seedGmailDemo(env.transport.data);
  env.transport.data.behaviour.workMs = 0;
  env.store.getState().selectAccount(account);
  return env;
}

function requests<T extends Request["type"]>(env: Env, type: T): Extract<Request, { type: T }>[] {
  return env.transport.requests.filter((r) => r.type === type) as Extract<Request, { type: T }>[];
}

function dialog(name: string): ComponentType<DialogProps> {
  const found = dialogs[name];
  if (!found) throw new Error(`no dialog ${name}`);
  return found;
}

function message(env: Env, subject: string): MessageSummary {
  const found = env.transport.data.messages.find((m) => m.subject === subject);
  if (!found) throw new Error(`no message ${subject}`);
  return found;
}

/** Puts messages (of one folder) into the list cache, as the message list would. */
function cacheList(env: Env, summary: MessageSummary, ...more: MessageSummary[]): void {
  env.queryClient.setQueryData<InfiniteData<Extract<Response, { type: "Messages" }>>>(
    queryKeys.messages(summary.folder_id, 50),
    {
      pages: [
        {
          type: "Messages",
          folder_id: summary.folder_id,
          offset: 0,
          total: 1 + more.length,
          messages: [summary, ...more],
        },
      ],
      pageParams: [0],
    },
  );
}

function ctx(env: Env): ActionContext {
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    layout: env.transport.data.layout,
    t: (id) => id,
    locale: "en",
  };
}

describe("label list", () => {
  it("renders nothing for accounts that are not Gmail accounts", async () => {
    const env = setup("acc-work");
    const { container } = renderWithProviders(
      <NodeView node={widget("label-list")} path="t/0" />,
      env,
    );
    await waitFor(() => {
      expect(env.transport.requests.some((r) => r.type === "ListAccounts")).toBe(true);
    });
    await act(async () => new Promise((resolve) => setTimeout(resolve, 10)));
    expect(container.querySelector('[data-part="label-list"]')).toBeNull();
    expect(requests(env, "ListGmailLabels")).toEqual([]);
  });

  it("shows the label tree with counts, hiding hidden labels", async () => {
    const env = setup();
    const { container } = renderWithProviders(
      <NodeView node={widget("label-list")} path="t/0" />,
      env,
    );
    const nav = await screen.findByRole("navigation", { name: "Labels of Gmail" });
    await within(nav).findByRole("button", { name: /^Travel/ });
    expect(within(nav).getByRole("button", { name: "Travel, 1 unread" })).toBeInTheDocument();
    expect(within(nav).getByRole("button", { name: /^Projects/ })).toBeInTheDocument();
    expect(within(nav).getByRole("button", { name: /^Follow up/ })).toBeInTheDocument();
    expect(within(nav).queryByRole("button", { name: /^Old newsletters/ })).toBeNull();
    expect(within(nav).queryByText("INBOX")).toBeNull();
    // Colours are theme geometry, not inline styles.
    const swatch = container.querySelector('[data-part="gmail.swatch"][data-color="custom"]');
    expect(swatch?.getAttribute("style")).toBeNull();
    expect(swatch?.getAttribute("data-geom")).toBeTruthy();
  });

  it("shows hidden labels with the option", async () => {
    const env = setup();
    renderWithProviders(
      <NodeView node={widget("label-list", { options: { "show-hidden": true } })} path="t/0" />,
      env,
    );
    expect(await screen.findByRole("button", { name: /^Old newsletters/ })).toBeInTheDocument();
  });

  it("opens a label view across folders", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(<NodeView node={widget("label-list")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: /^Follow up/ }));
    expect(env.store.getState().search).toMatchObject({
      text: 'tag:"Follow up"',
      accountId: GMAIL_ACCOUNT,
      folderId: null,
    });
    expect(screen.getByRole("button", { name: /^Follow up/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
  });

  it("opens the editor and the delete confirmation for a label", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(<NodeView node={widget("label-list")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Edit label Travel" }));
    expect(env.store.getState().windows).toContain(LABEL_EDITOR_DIALOG);
    expect(gmailStoreFor(env.store).getState().editor).toEqual({
      accountId: GMAIL_ACCOUNT,
      labelId: "Label_2",
      parent: null,
    });
    await user.click(screen.getByRole("button", { name: "Delete label Travel" }));
    expect(gmailStoreFor(env.store).getState().deleting?.name).toBe("Travel");
    await user.click(screen.getByRole("button", { name: "New label" }));
    expect(gmailStoreFor(env.store).getState().editor?.labelId).toBeNull();
  });
});

describe("label dialogs", () => {
  it("creates a nested label with a palette colour", async () => {
    const user = userEvent.setup();
    const env = setup();
    gmailStoreFor(env.store)
      .getState()
      .editLabel({ accountId: GMAIL_ACCOUNT, labelId: null, parent: null });
    const Editor = dialog(LABEL_EDITOR_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Editor name={LABEL_EDITOR_DIALOG} close={close} />, env);
    await user.type(await screen.findByLabelText("Name"), "Family");
    await user.selectOptions(screen.getByLabelText("Nest under"), "Work");
    await user.click(screen.getByRole("radio", { name: /Light blue/ }));
    await user.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "CreateGmailLabel")[0]?.settings).toEqual({
      name: "Work/Family",
      color: { background: "#c9daf8", text: "#1c4587" },
      list_visibility: "show",
      message_visibility: "show",
    });
    expect(gmailStoreFor(env.store).getState().editor).toBeNull();
  });

  it("refuses a duplicate name before asking the core", async () => {
    const user = userEvent.setup();
    const env = setup();
    gmailStoreFor(env.store)
      .getState()
      .editLabel({ accountId: GMAIL_ACCOUNT, labelId: null, parent: null });
    const Editor = dialog(LABEL_EDITOR_DIALOG);
    renderWithProviders(<Editor name={LABEL_EDITOR_DIALOG} close={() => undefined} />, env);
    await user.type(await screen.findByLabelText("Name"), "travel");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(screen.getByRole("alert")).toHaveTextContent("A label with this name already exists.");
    expect(requests(env, "CreateGmailLabel")).toEqual([]);
  });

  it("edits a nested label: renames, changes visibility and removes the colour", async () => {
    const user = userEvent.setup();
    const env = setup();
    gmailStoreFor(env.store)
      .getState()
      .editLabel({ accountId: GMAIL_ACCOUNT, labelId: "Label_4", parent: null });
    const Editor = dialog(LABEL_EDITOR_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Editor name={LABEL_EDITOR_DIALOG} close={close} />, env);
    const name = await screen.findByLabelText("Name");
    expect(name).toHaveValue("Projects");
    expect(screen.getByLabelText("Nest under")).toHaveValue("Work");
    expect(screen.getByRole("radio", { name: /Light purple/ })).toBeChecked();
    await user.clear(name);
    await user.type(name, "Research");
    await user.click(screen.getByRole("radio", { name: /No colour/ }));
    await user.selectOptions(screen.getByLabelText("In the label list"), "show-if-unread");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "UpdateGmailLabel")[0]).toMatchObject({
      label_id: "Label_4",
      settings: { name: "Work/Research", color: null, list_visibility: "show-if-unread" },
    });
  });

  it("shows the core's refusal", async () => {
    const user = userEvent.setup();
    const env = setup();
    env.transport.data.features.gmail.unsupported = true;
    gmailStoreFor(env.store)
      .getState()
      .editLabel({ accountId: GMAIL_ACCOUNT, labelId: null, parent: null });
    env.queryClient.setQueryData(["gmail", "labels", GMAIL_ACCOUNT], []);
    const Editor = dialog(LABEL_EDITOR_DIALOG);
    renderWithProviders(<Editor name={LABEL_EDITOR_DIALOG} close={() => undefined} />, env);
    await user.type(await screen.findByLabelText("Name"), "New");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });

  it("deletes a label after confirmation", async () => {
    const user = userEvent.setup();
    const env = setup();
    const labels = await env.client.request(
      { type: "ListGmailLabels", account_id: GMAIL_ACCOUNT },
      "GmailLabels",
    );
    const travel = labels.labels.find((l) => l.name === "Travel") ?? null;
    gmailStoreFor(env.store).getState().deleteLabel(travel);
    const Delete = dialog(LABEL_DELETE_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Delete name={LABEL_DELETE_DIALOG} close={close} />, env);
    expect(screen.getByText(/Delete the label “Travel”\? Its 2 messages/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "DeleteGmailLabel")[0]?.label_id).toBe("Label_2");
  });

  it("applies and removes labels on the selected message as one mail action", async () => {
    const user = userEvent.setup();
    const env = setup();
    const summary = message(env, "Your train tickets to Lisbon");
    cacheList(env, summary);
    env.store.getState().selectMessage(summary.id);
    const Apply = dialog(APPLY_LABELS_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Apply name={APPLY_LABELS_DIALOG} close={close} />, env);
    const travel = await screen.findByRole("checkbox", { name: "Travel" });
    await waitFor(() => {
      expect(travel).toBeChecked();
    });
    await user.type(screen.getByLabelText("Find a label"), "rec");
    expect(screen.queryByRole("checkbox", { name: "Travel" })).toBeNull();
    await user.click(screen.getByRole("checkbox", { name: "Receipts" }));
    await user.clear(screen.getByLabelText("Find a label"));
    await user.click(screen.getByRole("checkbox", { name: "Travel" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "ApplyMailAction")[0]).toMatchObject({
      targets: [{ kind: "message", message_id: summary.id }],
      action: { type: "tags", add: ["Receipts"], remove: ["Travel"] },
    });
    expect(requests(env, "UpdateFlags")).toEqual([]);
    expect(env.store.getState().pendingOps.length).toBeGreaterThan(0);
  });

  it("labels the whole selected conversation when the list shows conversations", async () => {
    const user = userEvent.setup();
    const env = setup();
    const summary = message(env, "Your train tickets to Lisbon");
    cacheList(env, summary);
    listSelectionFor(env.store).setState({
      scope: { conversations: true, folderId: summary.folder_id },
    });
    act(() => {
      env.store.getState().selectMessage(summary.id);
      env.store.getState().selectThread(summary.thread_id);
    });
    await actions["gmail.apply-labels"]?.run(ctx(env), []);
    const Apply = dialog(APPLY_LABELS_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Apply name={APPLY_LABELS_DIALOG} close={close} />, env);
    await user.click(await screen.findByRole("checkbox", { name: "Receipts" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "ApplyMailAction")[0]).toMatchObject({
      targets: [
        { kind: "conversation", thread_id: summary.thread_id, folder_id: summary.folder_id },
      ],
      action: { type: "tags", add: ["Receipts"], remove: [] },
    });
  });

  it("shows labels some picked messages have as mixed and applies the user's choice to all", async () => {
    const user = userEvent.setup();
    const env = setup();
    const lisbon = message(env, "Your train tickets to Lisbon");
    const receipt = message(env, "Receipt for your order #4711");
    expect(receipt.folder_id).toBe(lisbon.folder_id);
    cacheList(env, lisbon, receipt);
    act(() => {
      env.store.getState().selectMessage(lisbon.id);
    });
    listSelectionFor(env.store).setState({
      scope: { conversations: false, folderId: lisbon.folder_id },
      picked: [lisbon, receipt].map((m) => ({
        key: m.id,
        messageId: m.id,
        threadId: null,
        child: false,
      })),
    });
    await actions["gmail.apply-labels"]?.run(ctx(env), []);
    const Apply = dialog(APPLY_LABELS_DIALOG);
    const close = vi.fn();
    renderWithProviders(<Apply name={APPLY_LABELS_DIALOG} close={close} />, env);
    const travel = await screen.findByRole("checkbox", { name: "Travel" });
    const receipts = screen.getByRole("checkbox", { name: "Receipts" });
    await waitFor(() => {
      expect(travel).toHaveAttribute("data-mixed", "true");
    });
    expect(receipts).toHaveAttribute("data-mixed", "true");
    expect((travel as HTMLInputElement).indeterminate).toBe(true);
    expect(travel).not.toBeChecked();
    // A selection change behind the window changes nothing in it.
    act(() => {
      env.store.getState().selectMessage(message(env, "Hotel confirmation").id);
    });
    expect(travel).toHaveAttribute("data-mixed", "true");
    await user.click(travel);
    await user.click(receipts);
    await user.click(receipts);
    expect(travel).toBeChecked();
    expect(receipts).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "ApplyMailAction")[0]).toMatchObject({
      targets: [
        { kind: "message", message_id: lisbon.id },
        { kind: "message", message_id: receipt.id },
      ],
      action: { type: "tags", add: ["Travel"], remove: ["Receipts"] },
    });
  });

  it("apply labels without a Gmail message explains itself", () => {
    const env = setup();
    const Apply = dialog(APPLY_LABELS_DIALOG);
    renderWithProviders(<Apply name={APPLY_LABELS_DIALOG} close={() => undefined} />, env);
    expect(screen.getByText("Select a Gmail message first.")).toBeInTheDocument();
  });
});

describe("categories and chips", () => {
  it("filters the inbox by category", async () => {
    const user = userEvent.setup();
    const env = setup();
    env.store.getState().selectFolder(GMAIL_FOLDERS.inbox);
    renderWithProviders(<NodeView node={widget("category-tabs")} path="t/0" />, env);
    const nav = await screen.findByRole("navigation", { name: "Inbox categories" });
    expect(within(nav).getByRole("button", { name: "All" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await within(nav).findByRole("button", { name: "Social, 1 unread" });
    await user.click(within(nav).getByRole("button", { name: "Social, 1 unread" }));
    expect(env.store.getState().search).toMatchObject({
      text: "category:social",
      accountId: GMAIL_ACCOUNT,
      folderId: GMAIL_FOLDERS.inbox,
    });
    expect(within(nav).getByRole("button", { name: "Social, 1 unread" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(within(nav).getByRole("button", { name: "All" }));
    expect(env.store.getState().search).toBeNull();
    expect(env.store.getState().activeFolderId).toBe(GMAIL_FOLDERS.inbox);
  });

  it("offers only the configured categories", async () => {
    const env = setup();
    renderWithProviders(
      <NodeView
        node={widget("category-tabs", {
          options: { categories: "primary social", "show-all": false },
        })}
        path="t/0"
      />,
      env,
    );
    const nav = await screen.findByRole("navigation", { name: "Inbox categories" });
    expect(
      within(nav)
        .getAllByRole("button")
        .map((b) => b.dataset.category),
    ).toEqual(["primary", "social"]);
  });

  it("shows a message's labels and category", async () => {
    const env = setup();
    renderWithProviders(<MessageLabels message={message(env, "Hotel confirmation")} />, env);
    const list = await screen.findByRole("list", { name: "Labels" });
    await within(list).findByText("Travel");
    expect(within(list).getByText("Receipts")).toBeInTheDocument();
    expect(within(list).getByText("Updates")).toBeInTheDocument();
  });

  it("shows nothing for mail of other accounts", () => {
    const env = setup();
    const other = env.transport.data.messages.find((m) => m.account_id === "acc-work");
    if (!other) throw new Error("no message");
    const { container } = renderWithProviders(<MessageLabels message={other} />, env);
    expect(container.querySelector('[data-part="gmail.labels"]')).toBeNull();
  });
});

describe("gmail actions", () => {
  it("are enabled for Gmail accounts and open views and dialogs", async () => {
    const env = setup();
    const c = ctx(env);
    expect(actions["gmail.new-label"]?.enabled?.(c, [])).toBe(false);
    await env.queryClient.query({
      queryKey: queryKeys.accounts,
      queryFn: async () =>
        (await env.client.request({ type: "ListAccounts" }, "Accounts")).accounts,
    });
    await env.queryClient.query({
      queryKey: queryKeys.folders(GMAIL_ACCOUNT),
      queryFn: async () =>
        (await env.client.request({ type: "ListFolders", account_id: GMAIL_ACCOUNT }, "Folders"))
          .folders,
    });
    expect(actions["gmail.new-label"]?.enabled?.(c, [])).toBe(true);
    await actions["gmail.new-label"]?.run(c, []);
    expect(env.store.getState().windows).toContain(LABEL_EDITOR_DIALOG);
    await actions["gmail.label-view"]?.run(c, ["Receipts"]);
    expect(env.store.getState().search?.text).toBe("tag:Receipts");
    await actions["gmail.category"]?.run(c, ["promotions"]);
    expect(env.store.getState().search?.text).toBe("category:promotions");
    expect(actions["gmail.apply-labels"]?.enabled?.(c, [])).toBe(false);
    const summary = message(env, "Lunch on Friday?");
    cacheList(env, summary);
    env.store.getState().selectMessage(summary.id);
    expect(actions["gmail.apply-labels"]?.enabled?.(c, [])).toBe(true);
  });
});
