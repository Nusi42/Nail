/**
 * The shared pickers (R2, ADR 0222): the option sources per kind, the tree
 * model, the combobox (ARIA pattern, keyboard, closing) and the search
 * field's value helpers.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Account, Address, Folder } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { compileTheme } from "../../theme/compile";
import { rankValues } from "../../api/mockValues";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { requestsOf } from "../../test/env";
import { chipsOf } from "./testing";
import {
  AccountChoiceField,
  NullablePickerField,
  accountOptions,
  addressOptions,
  identityAddresses,
  ownAddressOptions,
  valueChanges,
  applySearchValue,
  chipLabel,
  folderAccounts,
  offeredAccounts,
  splitTyped,
  toggledValue,
  withValues,
  withoutValue,
  PickerListField,
  COMMON_HEADER_NAMES,
  folderTreeOptions,
  initialActive,
  moveActive,
  optionKey,
  optionNamed,
  Picker,
  PICKER_KINDS,
  popupGeometry,
  popupPlacement,
  popupRoom,
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
    expect(popupPlacement({ top: 100, bottom: 130 }, 700, 320)).toBe("below");
    expect(popupPlacement({ top: 520, bottom: 550 }, 700, 320)).toBe("above");
    // Little room either way: below, where the list starts at its top.
    expect(popupPlacement({ top: 100, bottom: 130 }, 130 + 320 - 1, 320)).toBe("below");
    // A short list fits below.
    expect(popupPlacement({ top: 520, bottom: 550 }, 700, 100)).toBe("below");
  });

  it("fixes the popup at its field, inside the viewport", () => {
    const viewport = { width: 1024, height: 700 };
    const field = { top: 461, bottom: 491, left: 300, width: 200 };
    expect(popupGeometry(field, { width: 260, room: 320 }, viewport)).toEqual({
      placement: "above",
      declarations: { left: "300px", "min-width": "200px", top: "auto", bottom: "239px" },
    });
    expect(
      popupGeometry(
        { ...field, top: 100, bottom: 130, left: 900 },
        { width: 260, room: 320 },
        viewport,
      ),
    ).toEqual({
      placement: "below",
      declarations: { left: "764px", "min-width": "200px", top: "130px", bottom: "auto" },
    });
  });

  it("takes the room the popup needs from the theme's max-height", () => {
    // The default theme's `picker.popup` rule is the one source of the size.
    const rule = defaultTheme().parts.find(
      (r) => r.selector === "picker.popup" && r.states.length === 0 && r.conditions.length === 0,
    );
    expect(rule?.declarations["max-height"]).toBe("min(320px, 50vh)");
    const popup = document.createElement("span");
    document.body.append(popup);
    popup.setAttribute("data-max", "");
    const style = document.createElement("style");
    style.textContent = "[data-max] { max-height: 320px; }";
    document.head.append(style);
    expect(popupRoom(popup)).toBe(320);
    style.remove();
    // Not in px (or not styled): the height it has.
    expect(popupRoom(popup)).toBe(0);
    popup.remove();
  });

  it("outlines the highlighted option, which forced colours (contrast themes) keep", () => {
    const rule = defaultTheme().parts.find(
      (r) =>
        r.selector === "picker.option" &&
        r.states.join() === "highlighted" &&
        r.conditions.length === 0,
    );
    expect(rule?.declarations).toMatchObject({
      outline: "2px solid transparent",
      "outline-offset": "-2px",
    });
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

  it("lists the most-used addresses while empty and finds contacts by name or address", async () => {
    const user = userEvent.setup();
    const { env, input } = renderPicker("address");
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    // Opened empty: the core's most-used correspondents (an empty prefix).
    const list = await screen.findByRole("listbox");
    expect((await within(list).findAllByRole("option")).length).toBeGreaterThan(0);
    expect(requestsOf(env, "SuggestAddresses")).toContainEqual(
      expect.objectContaining({ prefix: "" }),
    );
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

describe("picker scope and multi-value rules", () => {
  it("offers only the scope's accounts, real ones unless virtual ones are asked for", () => {
    const V = account("v", "All mail", "virtual");
    const scope = { accountId: null, folderValue: "path" as const };
    expect(offeredAccounts([A, B, V], scope).map((a) => a.id)).toEqual(["a", "b"]);
    expect(
      offeredAccounts([A, B, V], { ...scope, virtualAccounts: true }).map((a) => a.id),
    ).toEqual(["a", "b", "v"]);
    expect(offeredAccounts([A, B, V], { ...scope, accounts: ["b"] }).map((a) => a.id)).toEqual([
      "b",
    ]);
    expect(folderAccounts([A, B, V], { ...scope, accounts: ["b", "v"] }).map((a) => a.id)).toEqual([
      "b",
    ]);
    expect(folderAccounts([A, B, V], { ...scope, accountId: "a" }).map((a) => a.id)).toEqual(["a"]);
  });

  it("adds, removes and toggles values case-insensitively, keeping the first spelling", () => {
    expect(withValues(["a@x.org"], [" b@x.org ", "A@X.org", "", "b@x.org"])).toEqual([
      "a@x.org",
      "b@x.org",
    ]);
    expect(withoutValue(["a@x.org", "b@x.org"], "B@x.org")).toEqual(["a@x.org"]);
    expect(toggledValue(["a"], "b")).toEqual(["a", "b"]);
    expect(toggledValue(["a", "b"], "a")).toEqual(["b"]);
  });

  it("splits typed text at commas and semicolons", () => {
    expect(splitTyped("a@x.org, b@x.org; c")).toEqual({ done: ["a@x.org", "b@x.org"], rest: " c" });
    expect(splitTyped("abc")).toEqual({ done: [], rest: "abc" });
    expect(splitTyped(" , ;")).toEqual({ done: [], rest: "" });
  });

  it("names a chip by its folder's path or its option's name", () => {
    const options = tree("", true, "id");
    expect(chipLabel(options, "a/Lists/Rust")).toBe("Lists/Rust");
    expect(chipLabel(accountOptions([A], ""), "a")).toBe("Home");
    expect(chipLabel(options, "unknown")).toBe("unknown");
  });
});

/** A multi-value picker that keeps its values, recording them. */
function MultiControlled(props: {
  kind: PickerKind;
  initial: string[];
  folderValue?: "id" | undefined;
  ownAddresses?: readonly Address[] | undefined;
}) {
  const [values, setValues] = useState<string[]>(props.initial);
  return (
    <PickerListField
      kind={props.kind}
      folderValue={props.folderValue}
      ownAddresses={props.ownAddresses}
      labelId="vacation-addresses"
      name="multi"
      values={values}
      onChange={(next) => {
        chosen.push(next);
        setValues(next);
      }}
    />
  );
}

let chosen: string[][] = [];
const MULTI = "Also my addresses";

function renderMulti(
  kind: PickerKind,
  initial: string[] = [],
  folderValue?: "id",
  ownAddresses?: readonly Address[],
) {
  chosen = [];
  renderWithProviders(
    <MultiControlled
      kind={kind}
      initial={initial}
      folderValue={folderValue}
      ownAddresses={ownAddresses}
    />,
    createEnv(),
  );
  return screen.getByRole("combobox", { name: MULTI });
}

describe("Picker: multi-value mode", () => {
  it("turns typed addresses into chips on comma, Enter and leaving the field", async () => {
    const user = userEvent.setup();
    const input = renderMulti("address");
    await user.type(input, "a@example.org, b@example.org;");
    expect(chipsOf(document.body, MULTI)).toEqual(["a@example.org", "b@example.org"]);
    expect(input).toHaveValue("");
    await user.type(input, "zz@example.net{Enter}");
    // The typed value is taken; the list closes until it is opened again.
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(chipsOf(document.body, MULTI)).toEqual([
      "a@example.org",
      "b@example.org",
      "zz@example.net",
    ]);
    await user.type(input, "last@example.com");
    await user.tab();
    expect(chosen.at(-1)).toEqual([
      "a@example.org",
      "b@example.org",
      "zz@example.net",
      "last@example.com",
    ]);
    expect(input.closest('[data-part="picker"]')).toHaveAttribute("data-multiple", "true");
  });

  it("removes a chip by its button and the last one by Backspace in the empty field", async () => {
    const user = userEvent.setup();
    const input = renderMulti("address", ["a@example.org", "b@example.org", "c@example.org"]);
    await user.click(screen.getByRole("button", { name: "Remove b@example.org" }));
    expect(chipsOf(document.body, MULTI)).toEqual(["a@example.org", "c@example.org"]);
    expect(input).toHaveFocus();
    await user.keyboard("{Backspace}");
    expect(chipsOf(document.body, MULTI)).toEqual(["a@example.org"]);
    // Backspace with text only edits the text.
    await user.type(input, "x{Backspace}");
    expect(chipsOf(document.body, MULTI)).toEqual(["a@example.org"]);
  });

  it("picks several options from a multi-selectable list that stays open", async () => {
    const user = userEvent.setup();
    const input = renderMulti("account");
    input.focus();
    await user.keyboard("{ArrowDown}");
    const list = await screen.findByRole("listbox");
    expect(list).toHaveAttribute("aria-multiselectable", "true");
    await within(list).findByRole("option", { name: /Work/ });
    await user.keyboard("{Enter}");
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    await user.keyboard("{ArrowDown}{Enter}");
    expect(chosen.at(-1)).toHaveLength(2);
    const picked = within(list).getAllByRole("option", { selected: true });
    expect(picked).toHaveLength(2);
    // Picking a chosen option again removes it.
    await user.click(must(picked[0]));
    expect(chosen.at(-1)).toHaveLength(1);
    // An id kind shows names on its chips, and only takes typed text that names an option.
    await user.keyboard("{Escape}");
    await user.type(input, "nobody{Enter}");
    expect(chosen.at(-1)).toHaveLength(1);
    expect(input).toHaveValue("nobody");
    const names = chipsOf(document.body, MULTI);
    expect(names).toHaveLength(1);
    expect(names[0]).toMatch(/^(Work|Personal)$/);
  });

  it("shows folder chips by path", async () => {
    renderMulti("folder", ["acc-work/Archive"], "id");
    await waitFor(() => {
      expect(chipsOf(document.body, MULTI)).toEqual(["Archive"]);
    });
  });
});

describe("Picker: strict and scoped", () => {
  it("changes a strict field's value only to an option or an empty text", async () => {
    const user = userEvent.setup();
    const { input } = renderPicker("account", { initial: "acc-work", strict: true });
    await waitFor(() => {
      expect(input).toHaveValue("Work");
    });
    await user.type(input, "zz");
    expect(changes).toEqual([]);
    await user.tab();
    expect(input).toHaveValue("Work");
    await user.clear(input);
    expect(changes.at(-1)).toEqual(["", null]);
  });

  it("offers only the given accounts", async () => {
    const user = userEvent.setup();
    renderPicker("account", { accounts: ["acc-personal"] });
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    const list = await screen.findByRole("listbox");
    await within(list).findByRole("option", { name: /Personal/ });
    expect(within(list).queryByRole("option", { name: /Work/ })).toBeNull();
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
      return (
        found && (
          <div role="listbox">
            <SearchValues
              text={text}
              found={found}
              onApply={(t) => applied.push(t)}
              active={0}
              optionId={(i) => `v-${String(i)}`}
            />
          </div>
        )
      );
    }
    const view = renderWithProviders(<Values text="subject:x" />, env);
    expect(screen.queryByRole("group")).toBeNull();
    view.unmount();
    renderWithProviders(<Values text="is:unread folder:Rec" />, env);
    await typingPause();
    const list = await screen.findByRole("group", { name: "Values for this search field" });
    // One row per path, though both accounts have one.
    expect(within(list).getAllByRole("option")).toHaveLength(1);
    const row = within(list).getByRole("option", { name: /Inbox\/Receipts/ });
    // The one the arrow keys picked (`aria-activedescendant` names its id).
    expect(row).toHaveAttribute("id", "v-0");
    expect(row).toHaveAttribute("aria-selected", "true");
    await user.click(row);
    expect(applied).toEqual(["is:unread folder:Inbox/Receipts "]);
  });
});

const OWN: readonly Address[] = [
  { name: "Ada Lovelace", email: "ada@example.org" },
  { name: null, email: "ada.l@example.net" },
];

describe("Picker: the user's own addresses", () => {
  it("filters own addresses by name or address, each once", () => {
    const own = [...OWN, { name: "Ada", email: "ADA@example.org" }];
    expect(ownAddressOptions(own, "").map((o) => o.value)).toEqual([
      "ada@example.org",
      "ada.l@example.net",
    ]);
    expect(ownAddressOptions(own, "LOVE").map((o) => o.value)).toEqual(["ada@example.org"]);
    expect(ownAddressOptions(own, ".net").map((o) => o.value)).toEqual(["ada.l@example.net"]);
    expect(identityAddresses([{ name: "Ada", email: "ada@example.org" }])).toEqual([
      { name: "Ada", email: "ada@example.org" },
    ]);
  });

  it("offers only the own addresses, never the most-used correspondents", async () => {
    const user = userEvent.setup();
    const { env, input } = renderPicker("address", { ownAddresses: OWN });
    await user.click(screen.getByRole("button", { name: `Show choices for ${LABEL}` }));
    const list = await screen.findByRole("listbox");
    expect(
      within(list)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Ada Lovelaceada@example.org", "ada.l@example.net"]);
    await user.type(input, "grace");
    await typingPause();
    expect(screen.queryByRole("option", { name: /Grace Hopper/ })).toBeNull();
    expect(requestsOf(env, "SuggestAddresses")).toEqual([]);
  });

  it("lists only own addresses after a comma in a multi-value own-address field", async () => {
    const user = userEvent.setup();
    const input = renderMulti("address", [], undefined, OWN);
    await user.type(input, "alias@example.com,");
    expect(chipsOf(document.body, MULTI)).toEqual(["alias@example.com"]);
    const list = await screen.findByRole("listbox");
    expect(within(list).getAllByRole("option")).toHaveLength(2);
    expect(within(list).queryByRole("option", { name: /Grace/ })).toBeNull();
  });
});

describe("Picker: multi-value accessibility", () => {
  it("describes the field by what is chosen and announces additions and removals", async () => {
    const user = userEvent.setup();
    const input = renderMulti("address", ["a@example.org"]);
    expect(input).toHaveAccessibleDescription(/Chosen: a@example.org/);
    const status = must(document.querySelector('[data-part="picker.status"]'));
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveTextContent("");
    await user.type(input, "b@example.org,");
    expect(status).toHaveTextContent("Added b@example.org");
    expect(input).toHaveAccessibleDescription(/2 chosen: a@example.org, b@example.org/);
    await user.keyboard("{Escape}");
    await user.keyboard("{Backspace}");
    expect(status).toHaveTextContent("Removed b@example.org");
    await user.click(screen.getByRole("button", { name: "Remove a@example.org" }));
    expect(status).toHaveTextContent("Removed a@example.org");
    expect(input).toHaveAccessibleDescription(/Nothing chosen yet/);
  });

  it("announces an id value by its name and marks chosen options with a check", async () => {
    const user = userEvent.setup();
    const input = renderMulti("account");
    input.focus();
    await user.keyboard("{ArrowDown}");
    const list = await screen.findByRole("listbox");
    const work = await within(list).findByRole("option", { name: /Work/ });
    expect(work.querySelector('[data-part="picker.option.check.icon"]')).toBeNull();
    await user.click(work);
    expect(document.querySelector('[data-part="picker.status"]')).toHaveTextContent("Added Work");
    const chosenWork = within(list).getByRole("option", { name: /Work/, selected: true });
    expect(
      chosenWork.querySelector(
        '[data-part="picker.option.check"] [data-part="picker.option.check.icon"]',
      ),
    ).not.toBeNull();
  });

  it("tells what a change added and removed", () => {
    expect(valueChanges(["a", "b"], ["B", "c"])).toEqual({ added: ["c"], removed: ["a"] });
  });

  it("styles a chosen option with more than its weight", () => {
    const { css } = compileTheme(defaultTheme());
    const at = css.indexOf('[data-part="picker.option"][data-selected~="true"] {');
    expect(at).toBeGreaterThanOrEqual(0);
    expect(css.slice(at, css.indexOf("}", at))).toContain("background-color");
  });
});

let nullable: (string | null)[] = [];

/** A nullable picker field (or the account choice) that records its values. */
function NullableControlled({ account = false }: { account?: boolean }) {
  const [value, setValue] = useState<string | null>(account ? "acc-work" : null);
  const record = (next: string | null) => {
    nullable.push(next);
    setValue(next);
  };
  return account ? (
    <AccountChoiceField
      labelId="common-account"
      name="account"
      accounts={[{ id: "acc-work" }]}
      value={value}
      onChange={record}
    />
  ) : (
    <NullablePickerField
      kind="address"
      labelId="vacation-from"
      name="from"
      value={value}
      onChange={record}
    />
  );
}

describe("nullable picker fields", () => {
  it("maps an empty text to null and back", async () => {
    nullable = [];
    const user = userEvent.setup();
    renderWithProviders(<NullableControlled />, createEnv());
    const input = screen.getByRole("combobox", { name: "Reply from" });
    expect(input).toHaveValue("");
    await user.type(input, "x");
    expect(nullable.at(-1)).toBe("x");
    await user.clear(input);
    expect(nullable.at(-1)).toBeNull();
  });

  it("offers only the given accounts, strictly, and empties to null", async () => {
    nullable = [];
    const user = userEvent.setup();
    renderWithProviders(<NullableControlled account />, createEnv());
    const input = screen.getByRole("combobox", { name: "Account" });
    await waitFor(() => {
      expect(input).toHaveValue("Work");
    });
    await user.click(screen.getByRole("button", { name: "Show choices for Account" }));
    const list = await screen.findByRole("listbox");
    await within(list).findByRole("option", { name: /Work/ });
    expect(within(list).queryByRole("option", { name: /Personal/ })).toBeNull();
    await user.keyboard("{Escape}");
    await user.type(input, "zz");
    expect(nullable).toEqual([]);
    await user.clear(input);
    expect(nullable.at(-1)).toBeNull();
  });
});
