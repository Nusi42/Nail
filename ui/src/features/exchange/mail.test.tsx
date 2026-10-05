/**
 * Exchange mail UI against the mock core: categories (list, colours, tag
 * views, editor, delete), message category chips, other mailboxes (list,
 * add with validation and sync, hide / remove, folder badges), Focused
 * Inbox (tabs, switch, classify dialog, move button) and the actions.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NodeView } from "../../layout/LayoutRenderer";
import { renderWithProviders, widget } from "../../test/harness";
import {
  actions,
  FocusMoveButton,
  handleExchangeEvent,
  MailboxFolderBadge,
  MessageCategories,
} from ".";
import {
  ADDABLE_MAILBOX,
  DENIED_MAILBOX,
  EXCHANGE_ACCOUNT,
  EXCHANGE_FOLDERS,
  SHARED_MAILBOX,
  SHARED_ROOT,
} from "./mockSeed";
import {
  APPLY_CATEGORIES_DIALOG,
  CATEGORY_DELETE_DIALOG,
  CATEGORY_EDITOR_DIALOG,
  CLASSIFY_DIALOG,
  MAILBOX_EDITOR_DIALOG,
  MAILBOX_REMOVE_DIALOG,
} from "./names";
import { exchangeStoreFor } from "./store";
import { actionContext, cacheList, dialog, message, requests, setup } from "./testing";
import { must } from "../../test/must";

describe("categories", () => {
  it("renders nothing for accounts that are not Exchange accounts", async () => {
    const env = setup("acc-work");
    const { container } = renderWithProviders(
      <NodeView node={widget("exchange-categories")} path="t/0" />,
      env,
    );
    await act(() => Promise.resolve());
    expect(container.querySelector('[data-part="exchange-categories"]')).toBeNull();
  });

  it("lists the master list with colours and shows a category's messages", async () => {
    const env = setup();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("exchange-categories")} path="t/0" />, env);
    const urgent = await screen.findByRole("button", { name: "Urgent, Red, 1 unread" });
    expect(screen.getByRole("button", { name: "Personal, No colour" })).toBeInTheDocument();
    const item = urgent.closest('[data-part="exchange-categories.item"]');
    expect(item?.getAttribute("data-color")).toBe("preset0");
    // The swatch's colour is theme geometry, not an inline style.
    const swatch = item?.querySelector('[data-part="exchange.swatch"]');
    expect(swatch?.getAttribute("style")).toBeNull();
    expect(swatch?.getAttribute("data-geom")).toBeTruthy();
    await user.click(urgent);
    expect(env.store.getState().search).toMatchObject({
      text: 'tag:"Urgent"',
      accountId: EXCHANGE_ACCOUNT,
      folderId: null,
    });
    expect(urgent.getAttribute("aria-current")).toBe("true");
  });

  it("creates a category with a colour and validates the name", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    exchangeStoreFor(env.store).getState().editCategory({
      accountId: EXCHANGE_ACCOUNT,
      categoryId: null,
    });
    const Editor = dialog(CATEGORY_EDITOR_DIALOG);
    renderWithProviders(<Editor name={CATEGORY_EDITOR_DIALOG} close={close} />, env);
    const name = await screen.findByLabelText("Name");
    await user.type(name, "urgent");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A category with this name already exists.",
    );
    expect(requests(env, "CreateExchangeCategory")).toHaveLength(0);
    await user.clear(name);
    await user.type(name, "Travel");
    await user.click(screen.getByRole("radio", { name: "Teal" }));
    await user.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "CreateExchangeCategory")[0]?.settings).toEqual({
      name: "Travel",
      color: "preset5",
    });
  });

  it("warns that a rename retags messages and sends it", async () => {
    const env = setup();
    const user = userEvent.setup();
    exchangeStoreFor(env.store).getState().editCategory({
      accountId: EXCHANGE_ACCOUNT,
      categoryId: "{0004-cat}",
    });
    const Editor = dialog(CATEGORY_EDITOR_DIALOG);
    renderWithProviders(<Editor name={CATEGORY_EDITOR_DIALOG} close={vi.fn()} />, env);
    const name = await screen.findByLabelText("Name");
    expect(screen.getByRole("radio", { name: "Red" })).toBeChecked();
    await user.clear(name);
    await user.type(name, "Important");
    expect(screen.getByText("The category is renamed on 2 messages too.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(requests(env, "UpdateExchangeCategory")).toHaveLength(1);
    });
    expect(message(env, "Phoenix launch checklist").tags).toContain("Important");
  });

  it("deletes a category but keeps the tags", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    renderWithProviders(<NodeView node={widget("exchange-categories")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Delete category Urgent" }));
    expect(env.store.getState().windows).toContain(CATEGORY_DELETE_DIALOG);
    const Delete = dialog(CATEGORY_DELETE_DIALOG);
    renderWithProviders(<Delete name={CATEGORY_DELETE_DIALOG} close={close} />, env);
    expect(screen.getByText(/The 2 messages that have it keep it/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "DeleteExchangeCategory")[0]?.category_id).toBe("{0004-cat}");
    expect(message(env, "Phoenix launch checklist").tags).toContain("Urgent");
  });

  it("applies categories to the selected message as tags", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    const checklist = message(env, "Phoenix launch checklist");
    cacheList(env, [checklist]);
    const handler = actions["exchange.apply-categories"];
    expect(handler?.enabled?.(actionContext(env), [])).toBe(false);
    env.store.getState().selectMessage(checklist.id);
    expect(handler?.enabled?.(actionContext(env), [])).toBe(true);
    await handler?.run(actionContext(env), []);
    expect(env.store.getState().windows).toContain(APPLY_CATEGORIES_DIALOG);
    const Apply = dialog(APPLY_CATEGORIES_DIALOG);
    renderWithProviders(<Apply name={APPLY_CATEGORIES_DIALOG} close={close} />, env);
    const urgent = await screen.findByRole("checkbox", { name: "Urgent" });
    expect(urgent).toBeChecked();
    await user.click(urgent);
    await user.click(screen.getByRole("checkbox", { name: "Customers" }));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "ApplyMailAction")[0]).toMatchObject({
      targets: [{ kind: "message", message_id: checklist.id }],
      action: { type: "tags", add: ["Customers"], remove: ["Urgent"] },
    });
  });

  it("shows a message's categories as coloured chips, nothing for other mail", async () => {
    const env = setup();
    renderWithProviders(
      <MessageCategories message={message(env, "Phoenix launch checklist")} />,
      env,
    );
    const list = await screen.findByRole("list", { name: "Categories" });
    await waitFor(() => {
      expect(
        [...list.querySelectorAll('[data-part="exchange.chip"]')].map((c) =>
          c.getAttribute("data-color"),
        ),
      ).toEqual(["preset7", "preset0"]);
    });
    // A tag outside the master list: its own spelling, no colour.
    expect(within(list).getByText("Project Phoenix")).toBeInTheDocument();
    // The tag `customers` is shown with the category's spelling.
    renderWithProviders(
      <MessageCategories message={message(env, "Contract renewal for Fabrikam")} />,
      env,
    );
    await waitFor(() => expect(screen.getByText("Customers")).toBeInTheDocument());
    const work = env.transport.data.messages.find((m) => m.account_id === "acc-work");
    if (!work) throw new Error("no work message");
    const { container } = renderWithProviders(
      <MessageCategories message={{ ...work, tags: ["x"] }} />,
      env,
    );
    expect(container.querySelector('[data-part="exchange.categories"]')).toBeNull();
  });
});

describe("other mailboxes", () => {
  it("lists shared and delegate mailboxes with their state and opens a folder tree", async () => {
    const env = setup();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("exchange-mailboxes")} path="t/0" />, env);
    const support = await screen.findByRole("button", { name: "Support" });
    const item = support.closest('[data-part="exchange-mailboxes.item"]');
    expect(item).toHaveTextContent("Shared mailbox · added by your organization");
    expect(screen.getByText("Charles Babbage")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("No access to this mailbox");
    expect(screen.getByText(DENIED_MAILBOX)).toBeInTheDocument();
    await user.click(support);
    expect(env.store.getState().activeFolderId).toBe(SHARED_ROOT);
  });

  it("adds a shared mailbox after validating it; the sync opens it", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    // The app refetches on the core's events.
    env.transport.onEvent((event) => {
      handleExchangeEvent(event, {
        client: env.client,
        queryClient: env.queryClient,
        store: env.store,
      });
    });
    renderWithProviders(<NodeView node={widget("exchange-mailboxes")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Add shared mailbox…" }));
    expect(env.store.getState().windows).toContain(MAILBOX_EDITOR_DIALOG);
    const Editor = dialog(MAILBOX_EDITOR_DIALOG);
    renderWithProviders(<Editor name={MAILBOX_EDITOR_DIALOG} close={close} />, env);
    const address = await screen.findByLabelText("Mailbox address");
    await user.type(address, "ada@contoso.example");
    await user.click(screen.getByRole("button", { name: "Open mailbox" }));
    expect(await screen.findByText("This is the account's own mailbox.")).toBeInTheDocument();
    await user.clear(address);
    await user.type(address, ADDABLE_MAILBOX);
    await user.click(screen.getByRole("checkbox", { name: "Calendar" }));
    await user.click(screen.getByRole("button", { name: "Open mailbox" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "AddExchangeMailbox")[0]?.settings).toEqual({
      address: ADDABLE_MAILBOX,
      name: null,
      kind: "shared",
      mail: true,
      calendar: true,
      contacts: false,
      tasks: false,
    });
    expect(await screen.findByRole("button", { name: "Sales" })).toBeInTheDocument();
    expect(env.transport.data.folders.some((f) => f.path === `@${ADDABLE_MAILBOX}/Leads`)).toBe(
      true,
    );
  });

  it("hides an automapped mailbox", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    env.store.getState().selectFolder(SHARED_ROOT);
    renderWithProviders(<NodeView node={widget("exchange-mailboxes")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Remove mailbox Support" }));
    expect(env.store.getState().windows).toContain(MAILBOX_REMOVE_DIALOG);
    const Remove = dialog(MAILBOX_REMOVE_DIALOG);
    renderWithProviders(<Remove name={MAILBOX_REMOVE_DIALOG} close={close} />, env);
    expect(screen.getByText("Hide the mailbox “Support”?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Hide" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "RemoveExchangeMailbox")[0]?.address).toBe(SHARED_MAILBOX);
    expect(env.store.getState().activeFolderId).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Support" })).toBeNull();
    });
  });

  it("badges another mailbox's root folder, not other folders", async () => {
    const env = setup();
    const root = env.transport.data.folders.find((f) => f.id === SHARED_ROOT);
    const inbox = env.transport.data.folders.find((f) => f.id === EXCHANGE_FOLDERS.inbox);
    if (!root || !inbox) throw new Error("demo folders missing");
    const { container } = renderWithProviders(
      <>
        <MailboxFolderBadge folder={root} />
        <MailboxFolderBadge folder={inbox} />
        <MailboxFolderBadge folder={{ account_id: EXCHANGE_ACCOUNT, path: `@${DENIED_MAILBOX}` }} />
      </>,
      env,
    );
    await waitFor(() => {
      expect(container.querySelectorAll('[data-part="exchange.mailbox-badge"]')).toHaveLength(2);
    });
    expect(screen.getByText("Shared mailbox")).toBeInTheDocument();
    expect(screen.getByText("Shared mailbox · No access to this mailbox")).toBeInTheDocument();
  });

  it("opens the editor from the action, only for Exchange accounts", async () => {
    const env = setup();
    await env.queryClient.query({
      queryKey: ["accounts"],
      queryFn: async () =>
        (await env.client.request({ type: "ListAccounts" }, "Accounts")).accounts,
    });
    const handler = actions["exchange.add-mailbox"];
    expect(handler?.enabled?.(actionContext(env), [])).toBe(true);
    await handler?.run(actionContext(env), []);
    expect(exchangeStoreFor(env.store).getState().mailboxEditor).toEqual({
      accountId: EXCHANGE_ACCOUNT,
      address: null,
    });
    env.store.getState().selectAccount("acc-work");
    expect(handler?.enabled?.(actionContext(env), [])).toBe(false);
  });
});

describe("Focused Inbox", () => {
  it("filters the inbox by tab and switches the tabs off", async () => {
    const env = setup();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("focused-tabs")} path="t/0" />, env);
    const other = await screen.findByRole("button", { name: "Other" });
    await user.click(other);
    expect(env.store.getState().activeFolderId).toBe(EXCHANGE_FOLDERS.inbox);
    expect(env.store.getState().search).toMatchObject({
      text: "focus:other",
      folderId: EXCHANGE_FOLDERS.inbox,
    });
    expect(other).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "All" }));
    expect(env.store.getState().search).toBeNull();
    const toggle = screen.getByRole("switch", { name: "Focused Inbox" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("button", { name: "Focused" }));
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("button", { name: "Other" })).toBeNull();
    // Switching off leaves the Focused view.
    expect(env.store.getState().search).toBeNull();
  });

  it("explains when the server does not classify, and is absent elsewhere", async () => {
    const env = setup();
    const status = env.transport.data.features.exchange.status[EXCHANGE_ACCOUNT];
    status?.warnings.push("focused-inbox-unavailable");
    renderWithProviders(<NodeView node={widget("focused-tabs")} path="t/0" />, env);
    expect(
      await screen.findByText("The server does not sort mail into Focused and Other."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("switch")).toBeNull();
    const work = setup("acc-work");
    const { container } = renderWithProviders(
      <NodeView node={widget("focused-tabs")} path="t/1" />,
      work,
    );
    await act(() => Promise.resolve());
    expect(container.querySelector('[data-part="focused-tabs"]')).toBeNull();
  });

  it("moves a message to Other, always for its sender", async () => {
    const env = setup();
    const user = userEvent.setup();
    const close = vi.fn();
    const digest = message(env, "Phoenix launch checklist");
    cacheList(env, [digest]);
    renderWithProviders(<FocusMoveButton message={digest} />, env);
    await user.click(screen.getByRole("button", { name: "Move to Other" }));
    expect(env.store.getState().windows).toContain(CLASSIFY_DIALOG);
    const Classify = dialog(CLASSIFY_DIALOG);
    renderWithProviders(<Classify name={CLASSIFY_DIALOG} close={close} />, env);
    expect(screen.getByText("Move this message to Other?")).toBeInTheDocument();
    await user.click(
      screen.getByRole("checkbox", { name: "Always move messages from grace@contoso.example" }),
    );
    await user.click(must(screen.getAllByRole("button", { name: "Move to Other" }).at(-1)));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requests(env, "ClassifyFocused")[0]).toEqual({
      type: "ClassifyFocused",
      message_ids: [digest.id],
      classification: "other",
      always: true,
    });
    // The status bar counts the queued move.
    expect(env.store.getState().pendingOps).toHaveLength(1);
  });

  it("offers the classify action for classified Exchange mail only", async () => {
    const env = setup();
    const digest = message(env, "Your weekly digest");
    cacheList(env, [digest]);
    const handler = actions["exchange.classify"];
    expect(handler?.enabled?.(actionContext(env), [])).toBe(false);
    env.store.getState().selectMessage(digest.id);
    expect(handler?.enabled?.(actionContext(env), [])).toBe(true);
    await handler?.run(actionContext(env), []);
    expect(exchangeStoreFor(env.store).getState().classify).toEqual({
      messageIds: [digest.id],
      to: "focused",
    });
    const archived = message(env, "Dentist appointment reminder");
    cacheList(env, [archived]);
    env.store.getState().selectMessage(archived.id);
    expect(handler?.enabled?.(actionContext(env), [])).toBe(false);
  });
});
