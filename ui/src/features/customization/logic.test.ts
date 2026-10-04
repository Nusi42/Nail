/**
 * Pure logic of the customization slot: error bookkeeping, inspector
 * queries, merge choices, split write-back addressing.
 */
import type { ConfigMergePreview, PartRule, ResolvedTheme } from "../../api/generated";
import { createMockData } from "../../api/mockData";
import {
  configFileOf,
  conditionsOf,
  findNodeById,
  layoutNodeOf,
  matchingRules,
  partElement,
  partPath,
  ruleText,
  statesOf,
} from "./inspect";
import { chooseAll, choiceList, summarize, takeOf } from "./merge";
import { allConfigErrors, nextConfigErrors } from "./state";
import { splitPatches, splitRefOf } from "./writeBack";

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error("missing value");
  return value;
}

const issue = (line: number) => ({ line, column: 1, message: `problem ${String(line)}` });

describe("config error bookkeeping", () => {
  it("keeps every error per file and drops a file that became valid", () => {
    let state = nextConfigErrors({}, [], {
      type: "ConfigErrors",
      file: "theme.kdl",
      errors: [issue(2), issue(5)],
    });
    expect(state?.configErrors["theme.kdl"]?.issues).toHaveLength(2);
    expect(state?.modernFiles).toEqual(["theme.kdl"]);
    state = nextConfigErrors(must(state).configErrors, must(state).modernFiles, {
      type: "ConfigErrors",
      file: "layout.kdl",
      errors: [issue(1)],
    });
    expect(allConfigErrors(must(state).configErrors).map((f) => f.file)).toEqual([
      "layout.kdl",
      "theme.kdl",
    ]);
    state = nextConfigErrors(must(state).configErrors, must(state).modernFiles, {
      type: "ConfigErrors",
      file: "theme.kdl",
      errors: [],
    });
    expect(Object.keys(must(state).configErrors)).toEqual(["layout.kdl"]);
  });

  it("honours the M1a first-error event until the file sends all errors", () => {
    const legacy = nextConfigErrors({}, [], {
      type: "ConfigError",
      file: "accounts.kdl",
      line: 3,
      column: 4,
      message: "bad",
    });
    expect(legacy?.configErrors["accounts.kdl"]).toEqual({
      issues: [{ line: 3, column: 4, message: "bad" }],
      legacy: true,
    });
    // A valid accounts file clears it.
    const cleared = nextConfigErrors(must(legacy).configErrors, [], { type: "AccountsChanged" });
    expect(cleared?.configErrors).toEqual({});
    // Once a file sent ConfigErrors, the legacy event is ignored for it.
    expect(
      nextConfigErrors({}, ["theme.kdl"], {
        type: "ConfigError",
        file: "theme.kdl",
        line: 1,
        column: 1,
        message: "x",
      }),
    ).toBeNull();
    // ConfigChanged does not clear full lists.
    const full = { "theme.kdl": { issues: [issue(1)], legacy: false } };
    expect(nextConfigErrors(full, ["theme.kdl"], { type: "RulesChanged" })).toBeNull();
    expect(nextConfigErrors({}, [], { type: "SavedSearchesChanged" })).toBeNull();
  });
});

function dom(html: string): HTMLElement {
  const root = document.createElement("div");
  root.innerHTML = html;
  document.body.appendChild(root);
  return root;
}

const rule = (partial: Partial<PartRule> & { selector: string }): PartRule => ({
  states: [],
  conditions: [],
  ancestors: [],
  declarations: { color: "red" },
  source: { file: "theme.kdl", line: 3, column: 5 },
  ...partial,
});

describe("inspector queries", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("reads part path, states and conditions", () => {
    const root = dom(`
      <main data-part="app">
        <div data-part="message-list" data-node-id="list">
          <div data-part="message-list.row" data-state="selected unread" data-tag="work" data-encrypted="">
            <span id="t"><b>x</b></span>
          </div>
        </div>
        <aside data-part="inspector"><span id="own">y</span></aside>
      </main>`);
    const inner = root.querySelector("#t b");
    const row = partElement(inner);
    expect(row?.getAttribute("data-part")).toBe("message-list.row");
    expect(partPath(must(row))).toEqual(["app", "message-list", "message-list.row"]);
    expect(statesOf(must(row))).toEqual(["selected", "unread"]);
    expect(conditionsOf(must(row))).toEqual([
      { key: "tag", value: "work" },
      { key: "encrypted", value: null },
    ]);
    // The inspector's own parts are never picked.
    expect(partElement(root.querySelector("#own"))).toBeNull();
    expect(partElement(null)).toBeNull();
  });

  it("finds the theme rules that apply, pseudo states included", () => {
    const root = dom(`
      <div data-part="message-list.row" data-state="unread" data-tag="work">
        <span data-part="message-list.row.subject" id="s"></span>
      </div>`);
    const subject = must(root.querySelector("#s"));
    const theme: ResolvedTheme = {
      ...createMockData(5).theme,
      parts: [
        rule({ selector: "message-list.row.subject" }),
        rule({
          selector: "message-list.row.subject",
          ancestors: [{ selector: "message-list.row", states: ["unread"], conditions: [] }],
          declarations: { "font-weight": "700" },
        }),
        rule({ selector: "message-list.row.subject", states: ["hover"] }),
        rule({
          selector: "message-list.row.subject",
          ancestors: [
            {
              selector: "message-list.row",
              states: [],
              conditions: [{ key: "tag", value: "home" }],
            },
          ],
        }),
        rule({ selector: "folder-tree.item" }),
      ],
    };
    const rules = matchingRules(subject, theme);
    expect(rules.map((r) => r.text)).toEqual([
      "message-list.row.subject",
      "message-list.row:unread message-list.row.subject",
      "message-list.row.subject:hover",
    ]);
    expect(rules[1]?.declarations).toEqual({ "font-weight": "700" });
    expect(rules[2]?.when).toEqual(["hover"]);
    expect(rules[0]?.source?.line).toBe(3);
    expect(
      ruleText(
        rule({
          selector: "a.b",
          conditions: [
            { key: "tag", value: "work" },
            { key: "encrypted", value: null },
          ],
        }),
      ),
    ).toBe('a.b[tag="work"][encrypted]');
  });

  it("maps an element to the layout node that placed it", () => {
    const layout = createMockData(5).layout;
    const folders = must(findNodeById(must(layout.windows.mail?.[0]).root, "folders")).node;
    if (folders.type === "widget") folders.source = { file: "layout.kdl", line: 21, column: 25 };
    const root = dom(`
      <section data-window="mail"><div data-part="stack" data-node-id="sidebar">
        <nav data-part="folder-tree" data-node-id="folders"><span id="f" data-part="folder-tree.item"></span></nav>
      </div></section>
      <div data-window="outbox"><section data-part="outbox" data-node-id="outbox"><i id="o"></i></section></div>
      <p data-part="x" id="none"></p>`);
    const wide = layoutNodeOf(must(root.querySelector("#f")), layout, 1200);
    expect(wide).toMatchObject({ window: "mail", breakpoint: 0, kind: "folder-tree" });
    expect(wide?.path).toEqual([0, 0]);
    expect(wide?.source).toEqual({ file: "layout.kdl", line: 21, column: 25 });
    const narrow = layoutNodeOf(must(root.querySelector("#f")), layout, 600);
    expect(narrow).toMatchObject({ breakpoint: 1, path: [0, 0] });
    expect(layoutNodeOf(must(root.querySelector("#o")), layout, 1200)).toMatchObject({
      window: "outbox",
      kind: "outbox",
      path: [],
    });
    const container = layoutNodeOf(
      must(root.querySelector("[data-node-id=sidebar]")),
      layout,
      1200,
    );
    expect(container).toMatchObject({ kind: "stack", source: null });
    expect(layoutNodeOf(must(root.querySelector("#none")), layout, 1200)).toBeNull();
    const main = layout.windows.main?.[0]?.root;
    expect(main && findNodeById(main, "nope")).toBeNull();
  });

  it("knows which files the user can open", () => {
    expect(configFileOf("layout.kdl")).toBe("layout");
    expect(configFileOf("C:\\cfg\\theme.kdl")).toBe("theme");
    expect(configFileOf("rules.kdl")).toBe("rules");
    expect(configFileOf("defaults/layout.kdl")).toBeNull();
    expect(configFileOf("notes.txt")).toBeNull();
  });
});

const preview: ConfigMergePreview = {
  file: "theme",
  errors: [],
  sections: [
    {
      section: "tokens",
      items: [
        { id: "tokens:a", key: "a", change: "changed", mine: "x", theirs: "y", conflicts: [] },
        { id: "tokens:b", key: "b", change: "unchanged", mine: "x", theirs: "x", conflicts: [] },
        {
          id: "tokens:c",
          key: "c",
          change: "added",
          mine: null,
          theirs: "z",
          conflicts: [{ kind: "invalid", message: "m" }],
        },
      ],
    },
    {
      section: "icons",
      items: [
        { id: "icons:i", key: "i", change: "removed", mine: "p", theirs: null, conflicts: [] },
      ],
    },
  ],
};

describe("merge choices", () => {
  it("chooses per item, per section and for everything", () => {
    const item = must(preview.sections[0]?.items[0]);
    expect(takeOf({}, item, "mine")).toBe("mine");
    expect(takeOf({ "tokens:a": "theirs" }, item, "mine")).toBe("theirs");

    const tokens = chooseAll({}, preview, "theirs", "tokens");
    expect(tokens).toEqual({ "tokens:a": "theirs", "tokens:c": "theirs" });
    const all = chooseAll(tokens, preview, "mine");
    expect(Object.values(all)).toEqual(["mine", "mine", "mine"]);
    expect(choiceList({ ...tokens, "tokens:b": "theirs", ghost: "theirs" }, preview)).toEqual([
      { item_id: "tokens:a", take: "theirs" },
      { item_id: "tokens:c", take: "theirs" },
    ]);
    expect(summarize(preview, tokens, "mine")).toEqual({ differing: 3, taken: 2, conflicts: 1 });
    expect(summarize(preview, {}, "theirs").taken).toBe(3);
  });
});

describe("split write-back addressing", () => {
  const layout = createMockData(5).layout;

  it("maps tree paths and node ids to layout node refs", () => {
    // M5b: the panes live in the mail view's window.
    expect(splitRefOf("mail/0", layout, 1200)).toEqual({
      window: "mail",
      breakpoint: 0,
      path: [],
    });
    expect(splitRefOf("mail/0/0", layout, 1200)).toEqual({
      window: "mail",
      breakpoint: 0,
      path: [0],
    });
    // `panes` is a split when wide and tabs when narrow: only the split fits.
    expect(splitRefOf("panes", layout, 1200)).toEqual({ window: "mail", breakpoint: 0, path: [] });
    expect(splitRefOf("panes", layout, 600)).toEqual({ window: "mail", breakpoint: 0, path: [] });
    expect(splitRefOf("root", layout, 600)).toEqual({ window: "main", breakpoint: 0, path: [] });
    expect(splitRefOf("main/0/0", layout, 1200)).toBeNull(); // the config errors
    expect(splitRefOf("nope", layout, 1200)).toBeNull();
    expect(splitRefOf("test", layout, 1200)).toBeNull();
  });

  it("patches each changed split once", () => {
    const written = new Map<string, string>();
    const sizes = { panes: ["200px", "1fr", "1fr"], unknown: ["1fr"] };
    expect(splitPatches(sizes, written, layout, 1200)).toEqual([
      {
        op: "split-sizes",
        node: { window: "mail", breakpoint: 0, path: [] },
        sizes: ["200px", "1fr", "1fr"],
      },
    ]);
    expect(splitPatches(sizes, written, layout, 1200)).toEqual([]);
    expect(
      splitPatches({ ...sizes, panes: ["300px", "1fr", "1fr"] }, written, layout, 1200),
    ).toHaveLength(1);
  });
});
