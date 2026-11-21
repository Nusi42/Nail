/**
 * The shared pickers (R2, ADR 0222): the option sources per kind, the tree
 * model, the combobox (ARIA pattern, keyboard, closing) and the search
 * field's value helpers.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Account, Folder } from "../../api/generated";
import { rankValues } from "../../api/mockValues";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import {
  accountOptions,
  addressOptions,
  applySearchValue,
  COMMON_HEADER_NAMES,
  folderTreeOptions,
  initialActive,
  moveActive,
  optionKey,
  optionNamed,
  Picker,
  PICKER_KINDS,
  POPUP_ROOM,
  popupPlacement,
  PickerField,
  searchValueContext,
  searchValueOf,
  searchValueText,
  SearchValues,
  suggestKindOf,
  useSearchValues,
  suggestValues,
  toggled,
  valueOptions,
  visibleRows,
  type PickerKind,
  type PickerOption,
  type PickerProps,
} from ".";

const LABEL = "Folder";

function account(id: string, name: string, kind: Account["kind"] = "imap"): Account {
  return { id, name, email: `${id}@example.org`, kind };
}

function folder(
  accountId: string,
  path: string,
  role: Folder["role"] = "other",
  parent: string | null = null,
): Folder {
  return {
    id: `${accountId}/${path}`,
    account_id: accountId,
    parent_id: parent === null ? null : `${accountId}/${parent}`,
    name: path.split("/").pop() ?? path,
    path,
    role,
    unread: 0,
    total: 0,
  };
}

const A = account("a", "Home");
const B = account("b", "Office");
const FOLDERS = new Map([
  [
    "a",
    [
      folder("a", "Lists"),
      folder("a", "Lists/Rust", "other", "Lists"),
      folder("a", "INBOX", "inbox"),
      folder("a", "Archive", "archive"),
    ],
  ],
  ["b", [folder("b", "Inbox", "inbox"), folder("b", "Projects")]],
]);

function tree(query = "", grouped = true, folderValue: "path" | "id" = "path") {
  return folderTreeOptions({
    accounts: [A, B],
    folders: FOLDERS,
    query,
    folderValue,
    grouped,
    roleLabel: (f) => (f.role === "other" ? null : f.role === "inbox" ? "Inbox" : "Archive"),
  });
}

/** A picker that keeps its own value, recording every change. */
function Controlled(props: Omit<PickerProps, "value" | "onChange"> & { initial?: string }) {
  const { initial = "", ...rest } = props;
  const [value, setValue] = useState(initial);
  return (
    <Picker
      {...rest}
      value={value}
      onChange={(next, option) => {
        changes.push([next, option?.value ?? null]);
        setValue(next);
      }}
    />
  );
}

let changes: [string, string | null][] = [];

function renderPicker(kind: PickerKind, extra: Partial<PickerProps> & { initial?: string } = {}) {
  changes = [];
  const env = createEnv();
  renderWithProviders(<Controlled kind={kind} label={LABEL} {...extra} />, env);
  return { env, input: screen.getByRole("combobox", { name: LABEL }) };
}

const typingPause = () => act(() => new Promise((resolve) => setTimeout(resolve, 200)));

describe("picker sources", () => {
  it("maps each kind to its source", () => {
    expect(PICKER_KINDS.map(suggestKindOf)).toEqual([
      null,
      null,
      "tag",
      null,
      "mailing-list",
      "header-name",
    ]);
  });

  it("builds the folder tree per account, roles first, with the account rows as groups", () => {
    const rows = tree();
    expect(rows.map((r) => [optionKey(r), r.depth, r.parent])).toEqual([
      ["account:a", 0, null],
      ["folder:a/INBOX", 1, "account:a"],
      ["folder:a/Archive", 1, "account:a"],
      ["folder:a/Lists", 1, "account:a"],
      ["folder:a/Lists/Rust", 2, "folder:a/Lists"],
      ["account:b", 0, null],
      ["folder:b/Inbox", 1, "account:b"],
      ["folder:b/Projects", 1, "account:b"],
    ]);
    expect(rows[0]).toMatchObject({ variant: "group", label: "Home", value: "a" });
    // The role is shown when the server's name differs from it.
    expect(rows[1]).toMatchObject({ label: "INBOX", detail: "Inbox", value: "INBOX" });
    expect(rows[6]).toMatchObject({ label: "Inbox", detail: null });
  });

  it("filters folders by name or path and keeps their parents", () => {
    expect(tree("rust").map(optionKey)).toEqual([
      "account:a",
      "folder:a/Lists",
      "folder:a/Lists/Rust",
    ]);
    expect(tree("proj", false).map((r) => [optionKey(r), r.depth, r.parent])).toEqual([
      ["folder:b/Projects", 0, null],
    ]);
    expect(tree("", false, "id").find((r) => r.label === "Rust")?.value).toBe("a/Lists/Rust");
    expect(tree("nothing")).toEqual([]);
  });

  it("lists accounts and addresses", () => {
    expect(accountOptions([A, B], "OFF").map((o) => o.value)).toEqual(["b"]);
    expect(accountOptions([A, B], "a@example").map((o) => o.label)).toEqual(["Home"]);
    expect(
      addressOptions([
        { name: "Grace Hopper", email: "grace@navy.example" },
        { name: null, email: "noreply@shop.example" },
      ]).map((o) => [o.value, o.label, o.detail]),
    ).toEqual([
      ["grace@navy.example", "Grace Hopper", "grace@navy.example"],
      ["noreply@shop.example", "noreply@shop.example", null],
    ]);
  });

  it("offers a new tag and the common header names", () => {
    const create = (tag: string) => `Create ${tag}`;
    const tags = valueOptions("tag", [{ value: "project", label: null, count: 3 }], "pro", create);
    expect(tags.map((o) => [o.value, o.label, o.variant])).toEqual([
      ["project", "project", undefined],
      ["pro", "Create pro", "create"],
    ]);
    expect(
      valueOptions("tag", [{ value: "Pro", label: null, count: 1 }], "pro", create),
    ).toHaveLength(1);
    expect(valueOptions("tag", [], " ", create)).toEqual([]);
    const headers = valueOptions(
      "header-name",
      [{ value: "X-Seen", label: null, count: 4 }],
      "",
      create,
    );
    expect(headers.map((o) => o.value)).toEqual(["X-Seen", ...COMMON_HEADER_NAMES]);
    const seenSubject = valueOptions(
      "header-name",
      [{ value: "subject", label: null, count: 1 }],
      "s",
      create,
    );
    expect(seenSubject.map((o) => o.value)).toEqual(["subject", "Sender"]);
    const lists = valueOptions(
      "mailing-list",
      [{ value: "x.lists.example", label: "X", count: 1 }],
      "",
      create,
    );
    expect(lists[0]).toMatchObject({
      value: "x.lists.example",
      label: "X",
      detail: "x.lists.example",
    });
  });

  it("asks the core for tags, mailing lists and header names", async () => {
    const env = createEnv();
    const [message] = env.transport.data.messages;
    if (message) message.tags = ["project"];
    const tags = await suggestValues(env.client, "tag", "PRO");
    expect(tags.map((v) => v.value)).toContain("project");
    const headers = await suggestValues(env.client, "header-name", "subj");
    expect(headers.map((v) => v.value)).toEqual(["Subject"]);
    expect(await suggestValues(env.client, "mailing-list", "")).toEqual([
      { value: "rust-announce.lists.example", label: "Rust Announce", count: 2 },
    ]);
    expect(await suggestValues(env.client, "mailing-list", "x")).toEqual([]);
  });

  it("ranks values by use, case-insensitively", () => {
    expect(rankValues(["Work", "home", "work", "x"], "", 2)).toEqual([
      { value: "Work", label: null, count: 2 },
      { value: "home", label: null, count: 1 },
    ]);
    expect(rankValues(["Work", "home"], "WO", 10).map((v) => v.value)).toEqual(["Work"]);
    expect(rankValues(["L"], "", 1, new Map([["l", "List"]]))[0]?.label).toBe("List");
  });
});

describe("picker tree model", () => {
  const rows = tree();

  it("hides the rows under closed rows unless filtering", () => {
    const closed = toggled(new Set(), "folder:a/Lists");
    expect(visibleRows(rows, closed, false).map(optionKey)).not.toContain("folder:a/Lists/Rust");
    expect(visibleRows(rows, closed, true)).toHaveLength(rows.length);
    const account = toggled(new Set(), "account:a");
    expect(visibleRows(rows, account, false).map(optionKey)).toEqual([
      "account:a",
      "account:b",
      "folder:b/Inbox",
      "folder:b/Projects",
    ]);
    expect(toggled(account, "account:a").size).toBe(0);
  });

  it("starts on the value, on what was typed, or on the first pickable row", () => {
    expect(initialActive(rows, "Archive")).toBe("folder:a/Archive");
    expect(initialActive(rows, "missing")).toBe("folder:a/INBOX");
    expect(initialActive(rows, null, "proj")).toBe("folder:b/Projects");
    expect(initialActive([], "x")).toBeNull();
  });

  it("moves through the tree", () => {
    expect(moveActive(rows, null, "next")).toBe("account:a");
    expect(moveActive(rows, null, "previous")).toBe("folder:b/Projects");
    expect(moveActive(rows, "account:a", "previous")).toBe("account:a");
    expect(moveActive(rows, "folder:b/Projects", "next")).toBe("folder:b/Projects");
    expect(moveActive(rows, "folder:a/Lists", "child")).toBe("folder:a/Lists/Rust");
    expect(moveActive(rows, "folder:a/Archive", "child")).toBe("folder:a/Archive");
    expect(moveActive(rows, "folder:a/Lists/Rust", "parent")).toBe("folder:a/Lists");
    expect(moveActive(rows, "account:a", "parent")).toBe("account:a");
    expect(moveActive(rows, "folder:a/Lists", "last")).toBe("folder:b/Projects");
    expect(moveActive(rows, "folder:a/Lists", "first")).toBe("account:a");
    expect(moveActive([], "x", "next")).toBeNull();
  });

  it("opens the popup above a field near the bottom", () => {
    expect(popupPlacement({ top: 100, bottom: 130 }, 700)).toBe("below");
    expect(popupPlacement({ top: 520, bottom: 550 }, 700)).toBe("above");
    // Little room either way: below, where the list starts at its top.
    expect(popupPlacement({ top: 100, bottom: 130 }, 130 + POPUP_ROOM - 1)).toBe("below");
  });

  it("finds the one option a typed name means", () => {
    const options: PickerOption[] = accountOptions([A, B, account("c", "home")], "");
    expect(optionNamed(options, "office")?.value).toBe("b");
    // Two accounts named "Home": no guess.
    expect(optionNamed(options, "Home")).toBeNull();
    expect(optionNamed(options, " ")).toBeNull();
  });
});

describe("Picker", () => {
  it("is a combobox that takes free text", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("folder", { invalid: true, describedBy: "problems" });
    expect(input).toHaveAttribute("data-part", "picker.input");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAttribute("aria-describedby", "problems");
    expect(input).toHaveAttribute("aria-expanded", "false");
    expect(input).toHaveAttribute("aria-haspopup", "tree");
    expect(input.closest('[data-part="picker"]')).toHaveAttribute("data-kind", "folder");
    await user.type(input, "Arc");
    expect(changes.at(-1)).toEqual(["Arc", null]);
    expect(input).toHaveAttribute("aria-expanded", "true");
    expect(input.closest('[data-part="picker"]')).toHaveAttribute("data-state", "open");
  });

  it("browses the folder tree across accounts by keyboard and picks a path", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("folder");
    input.focus();
    await user.keyboard("{ArrowDown}");
    const list = await screen.findByRole("tree", { name: `Choices for ${LABEL}` });
    expect(input).toHaveAttribute("aria-controls", list.id);
    await within(list).findByRole("treeitem", { name: /Personal/ });
    const active = () => document.getElementById(input.getAttribute("aria-activedescendant") ?? "");
    // The first pickable row: the first account's Inbox.
    expect(active()).toHaveTextContent("Inbox");
    expect(active()).toHaveAttribute("aria-level", "2");
    await user.keyboard("{ArrowRight}");
    // Inbox has a child (Receipts): Right opens it… it is open, so Right enters it.
    expect(active()).toHaveTextContent("Receipts");
    await user.keyboard("{ArrowLeft}");
    expect(active()).toHaveTextContent("Inbox");
    await user.keyboard("{ArrowLeft}");
    expect(active()).toHaveAttribute("aria-expanded", "false");
    // Only the other account's Receipts is left.
    expect(within(list).getAllByRole("treeitem", { name: /Receipts/ })).toHaveLength(1);
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(active()).toHaveTextContent("Receipts");
    await user.keyboard("{Enter}");
    expect(changes).toEqual([["Inbox/Receipts", "Inbox/Receipts"]]);
    expect(input).toHaveValue("Inbox/Receipts");
    expect(screen.queryByRole("tree")).toBeNull();
  });

  it("opens and closes an account row instead of picking it", async () => {
    const user = userEvent.setup();
    renderPicker("folder");
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    const work = await screen.findByRole("treeitem", { name: /Work/ });
    expect(work).toHaveAttribute("aria-expanded", "true");
    await user.click(work);
    expect(work).toHaveAttribute("aria-expanded", "false");
    expect(changes).toEqual([]);
    // The chevron of a folder row opens / closes it too; clicking the name picks.
    const inbox = must(screen.getAllByRole("treeitem", { name: /^Inbox/ })[0]);
    const toggle = must(inbox.querySelector('[data-part="picker.option.toggle"]'));
    await user.click(toggle);
    expect(inbox).toHaveAttribute("aria-expanded", "false");
    await user.click(within(inbox).getByText("Inbox"));
    expect(changes).toEqual([["Inbox", "Inbox"]]);
  });

  it("closes with Escape without closing the window, and on a click outside", async () => {
    const user = userEvent.setup();
    const outer = vi.fn();
    const env = createEnv();
    changes = [];
    renderWithProviders(
      <div role="presentation" onKeyDown={outer}>
        <Controlled kind="account" label={LABEL} />
        <p>outside</p>
      </div>,
      env,
    );
    const input = screen.getByRole("combobox", { name: LABEL });
    input.focus();
    await user.keyboard("{ArrowDown}");
    expect(await screen.findByRole("listbox")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(outer).not.toHaveBeenCalledWith(expect.objectContaining({ key: "Escape" }));
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    expect(await screen.findByRole("listbox")).toBeInTheDocument();
    fireEvent.mouseDown(screen.getByText("outside"));
    expect(screen.queryByRole("listbox")).toBeNull();
    await user.keyboard("{ArrowDown}");
    await screen.findByRole("listbox");
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(changes).toEqual([]);
  });

  it("shows an account by its name and takes a typed name as that account", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("account", { initial: "acc-work" });
    await waitFor(() => {
      expect(input).toHaveValue("Work");
    });
    await user.clear(input);
    await user.type(input, "personal");
    await user.tab();
    expect(changes.at(-1)).toEqual(["acc-personal", "acc-personal"]);
    expect(input).toHaveValue("Personal");
  });

  it("offers tags in use and a new one", async () => {
    const user = userEvent.setup();
    const { env, input } = renderPicker("tag");
    expect(env.transport.data.messages.some((m) => m.tags.includes("work"))).toBe(true);
    await user.type(input, "wo");
    await typingPause();
    const list = await screen.findByRole("listbox");
    await within(list).findByRole("option", { name: "work" });
    await user.type(input, "x");
    await typingPause();
    await user.click(await within(list).findByRole("option", { name: /Create tag “wox”/ }));
    expect(changes.at(-1)).toEqual(["wox", "wox"]);
  });

  it("finds contacts and correspondents by name or address", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("address");
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    expect(await screen.findByText("Type a name or an address")).toBeInTheDocument();
    await user.type(input, "grace");
    await typingPause();
    const grace = await screen.findByRole("option", { name: /Grace Hopper/ });
    expect(grace).toHaveTextContent("grace@navy.example");
    await user.keyboard("{ArrowDown}{ArrowUp}{Enter}");
    expect(changes.at(-1)).toEqual(["grace@navy.example", "grace@navy.example"]);
  });

  it("offers mailing lists and header names", async () => {
    const user = userEvent.setup();
    renderPicker("mailing-list");
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    await user.click(await screen.findByRole("option", { name: /Rust Announce/ }));
    expect(changes.at(-1)).toEqual(["rust-announce.lists.example", "rust-announce.lists.example"]);
  });

  it("says when nothing matches and keeps the typed text", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("header-name");
    await user.type(input, "Zz-");
    await typingPause();
    expect(await screen.findByText(/Nothing matches/)).toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(input).toHaveValue("Zz-");
    expect(changes.at(-1)).toEqual(["Zz-", null]);
  });

  it("is labelled as a form field", async () => {
    const env: Env = createEnv();
    renderWithProviders(
      <PickerField
        kind="folder"
        labelId="virtual-source-folder"
        hintId="vacation-from-hint"
        name="f"
        accountId="acc-work"
        folderValue="id"
        value="acc-work/Archive"
        onChange={() => undefined}
      />,
      env,
    );
    const input = screen.getByRole("combobox", { name: "Folder" });
    expect(input.closest('[data-part="form.field"]')).toHaveAttribute("data-field", "f");
    expect(input).toHaveAccessibleDescription(/.+/);
    await waitFor(() => {
      expect(input).toHaveValue("Archive");
    });
  });
});

describe("search field values", () => {
  it("knows the field value being typed", () => {
    expect(searchValueContext("is:unread folder:Arc")).toEqual({
      field: "folder:",
      kind: "folder",
      prefix: "Arc",
      start: 17,
    });
    expect(searchValueContext("FROM:")).toMatchObject({
      field: "from:",
      kind: "address",
      prefix: "",
    });
    expect(searchValueContext('tag:"my pro')).toMatchObject({
      kind: "tag",
      prefix: "my pro",
      start: 4,
    });
    expect(searchValueContext("(-account:wo")).toMatchObject({ kind: "account", prefix: "wo" });
    expect(searchValueContext("subject:x")).toBeNull();
    expect(searchValueContext("folder:Archive ")).toBeNull();
    expect(searchValueContext("hello")).toBeNull();
  });

  it("completes the value, quoted when it needs it", () => {
    expect(searchValueText("Archive")).toBe("Archive");
    expect(searchValueText('My "old" mail')).toBe('"My \\"old\\" mail"');
    const text = 'is:unread folder:"Ol';
    expect(applySearchValue(text, must(searchValueContext(text)), "Old Projects")).toBe(
      'is:unread folder:"Old Projects" ',
    );
    expect(searchValueOf(must(accountOptions([A], "")[0]))).toBe("a@example.org");
    expect(searchValueOf(must(tree("rust", false)[1]))).toBe("Lists/Rust");
  });

  it("lists the values for the field being typed and applies one", async () => {
    const user = userEvent.setup();
    const applied: string[] = [];
    const env = createEnv();
    function Values({ text }: { text: string }) {
      const found = useSearchValues(text);
      return found && <SearchValues text={text} found={found} onApply={(t) => applied.push(t)} />;
    }
    const view = renderWithProviders(<Values text="subject:x" />, env);
    expect(screen.queryByRole("list")).toBeNull();
    view.unmount();
    renderWithProviders(<Values text="is:unread folder:Rec" />, env);
    await typingPause();
    const list = await screen.findByRole("list", { name: "Values for this search field" });
    // One row per path, though both accounts have one.
    expect(within(list).getAllByRole("button")).toHaveLength(1);
    await user.click(within(list).getByRole("button", { name: /Inbox\/Receipts/ }));
    expect(applied).toEqual(["is:unread folder:Inbox/Receipts "]);
  });
});
