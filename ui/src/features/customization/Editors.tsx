/**
 * The Customize tab's editors (M2c WP54). Every change is written back into
 * `layout.kdl` through the existing layout patches (`writeBack.ts`); the
 * new layout comes back as `ConfigChanged`, so the editors always show the
 * file's state:
 *
 * - `toolbar-editor`: reorder, add and remove the items of a toolbar
 *   (`toolbar-order`), or go back to the default (`reset-toolbar`);
 * - `key-binding-editor`: every key binding by context, record a new one
 *   (`add-key-binding`), remove one (`remove-key-binding`) or restore the
 *   default (`reset-key-binding`);
 * - `column-editor`: the column order of a message list
 *   (`column-order`).
 */
import { useId, useMemo, useState, type KeyboardEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  JsonValue,
  KeyBinding,
  LayoutNode,
  LayoutNodeRef,
  LocalizedMessage,
  ResolvedLayout,
  ToolbarItem,
} from "../../api/generated";
import { comboFromEvent } from "../../actions/keys";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { optionList, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { COLUMN_IDS } from "../../widgets/MessageList";
import { keyLabel } from "../menus";
import { ReorderList, moved, type ReorderEntry } from "./ReorderList";
import {
  updateLayout,
  writeColumnOrder,
  writeKeyBinding,
  writeToolbar,
  removeKeyBinding,
} from "./writeBack";

import { KEY_CONTEXTS, isKnownKeyContext } from "./editorModel";

export { KEY_CONTEXTS };

interface Status {
  ok: boolean;
  message: LocalizedMessage;
}

function StatusLine({ status, part }: { status: Status | null; part: string }) {
  const t = useT();
  if (status === null) return null;
  return (
    <p
      data-part={`${part}.status`}
      data-state={status.ok ? "ok" : "error"}
      role={status.ok ? "status" : "alert"}
    >
      {t(status.message.id, status.message.args)}
    </p>
  );
}

type Write = (run: () => Promise<void>, done: string) => Promise<void>;

/** Runs a write and reports its outcome. */
function useWrite(): [Status | null, Write] {
  const [status, setStatus] = useState<Status | null>(null);
  const write = async (run: () => Promise<void>, done: string) => {
    setStatus(null);
    try {
      await run();
      setStatus({ ok: true, message: { id: done } });
    } catch (error) {
      setStatus({ ok: false, message: errorMessageOf(error) });
    }
  };
  return [status, write];
}

/** Every action of the layout's `actions` section, sorted by label. */
function useActionChoices(layout: ResolvedLayout): { value: string; label: string }[] {
  const label = useLabel();
  return useMemo(() => {
    return Object.keys(layout.actions)
      .map((id) => ({ value: id, label: label(layout.actions[id]?.label ?? id) }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [layout, label]);
}

function actionText(layout: ResolvedLayout, id: string, label: (l: string) => string): string {
  const def = Object.hasOwn(layout.actions, id) ? layout.actions[id] : undefined;
  return label(def?.label ?? id);
}

// ---- Toolbars ---------------------------------------------------------------

interface DraftItem {
  key: string;
  item: ToolbarItem;
}

function keyed(items: readonly ToolbarItem[]): DraftItem[] {
  return items.map((item, index) => ({ key: `i${String(index)}`, item }));
}

function ToolbarDraft({
  name,
  items,
  write,
}: {
  name: string;
  items: readonly ToolbarItem[];
  /** The editor's: a written toolbar re-creates the draft. */
  write: Write;
}) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const layout = useLayoutConfig();
  const ids = useId();
  const choices = useActionChoices(layout);
  const [draft, setDraft] = useState(() => keyed(items));
  const [counter, setCounter] = useState(items.length);
  const [adding, setAdding] = useState(choices[0]?.value ?? "");

  const text = (item: ToolbarItem) =>
    item.type === "action"
      ? label(item.label ?? layout.actions[item.action]?.label ?? item.action)
      : t(`customize-toolbar-${item.type}`);
  const entries: ReorderEntry[] = draft.map((d) => ({
    key: d.key,
    label: text(d.item),
    kind: d.item.type,
  }));
  const add = (item: ToolbarItem) => {
    setDraft([...draft, { key: `n${String(counter)}`, item }]);
    setCounter(counter + 1);
  };
  const changed = JSON.stringify(draft.map((d) => d.item)) !== JSON.stringify(items);

  return (
    <div data-part="toolbar-editor.draft">
      <ReorderList
        entries={entries}
        label={t("customize-toolbar-items", { toolbar: name })}
        part="toolbar-editor"
        onMove={(from, to) => {
          setDraft(moved(draft, from, to));
        }}
        onRemove={(index) => {
          setDraft(draft.filter((_, i) => i !== index));
        }}
      />
      <div data-part="toolbar-editor.add" role="group" aria-label={t("customize-toolbar-add")}>
        <label data-part="form.label" htmlFor={`${ids}-action`}>
          {t("common-action")}
        </label>
        <select
          id={`${ids}-action`}
          data-part="form.select"
          value={adding}
          onChange={(event) => {
            setAdding(event.target.value);
          }}
        >
          {choices.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
        <button
          type="button"
          data-part="form.button"
          disabled={adding === ""}
          onClick={() => {
            add({ type: "action", action: adding, args: [], label: null, icon: null });
          }}
        >
          {t("common-add-action")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            add({ type: "separator" });
          }}
        >
          {t("customize-toolbar-add-separator")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            add({ type: "spacer" });
          }}
        >
          {t("customize-toolbar-add-spacer")}
        </button>
      </div>
      <div data-part="toolbar-editor.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={!changed}
          onClick={() =>
            void write(
              () =>
                writeToolbar(
                  client,
                  name,
                  draft.map((d) => d.item),
                ),
              "customize-saved",
            )
          }
        >
          {t("customize-save")}
        </button>
        <button
          type="button"
          data-part="form.button"
          disabled={!changed}
          onClick={() => {
            setDraft(keyed(items));
          }}
        >
          {t("common-undo-changes")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() =>
            void write(
              () => updateLayout(client, [{ op: "reset-toolbar", toolbar: name }]),
              "customize-reset-done",
            )
          }
        >
          {t("customize-toolbar-reset")}
        </button>
      </div>
    </div>
  );
}

/** Settings → Customize → Toolbars (widget `toolbar-editor`). */
export function ToolbarEditor({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const names = Object.keys(layout.toolbars).sort();
  const [chosen, setChosen] = useState<string | null>(null);
  const ids = useId();
  const name = chosen !== null && names.includes(chosen) ? chosen : (names[0] ?? null);
  const items = name === null ? [] : (layout.toolbars[name] ?? []);
  const [status, write] = useWrite();
  return (
    <section
      data-part="toolbar-editor"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("customize-toolbar-label")}
    >
      <p data-part="toolbar-editor.intro">{t("customize-toolbar-intro")}</p>
      <div data-part="form.field" data-field="toolbar">
        <label data-part="form.label" htmlFor={`${ids}-toolbar`}>
          {t("customize-toolbar-choose")}
        </label>
        <select
          id={`${ids}-toolbar`}
          data-part="form.select"
          value={name ?? ""}
          onChange={(event) => {
            setChosen(event.target.value);
          }}
        >
          {names.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </div>
      {name !== null && (
        <ToolbarDraft
          key={`${name}:${JSON.stringify(items)}`}
          name={name}
          items={items}
          write={write}
        />
      )}
      <StatusLine status={status} part="toolbar-editor" />
    </section>
  );
}

// ---- Key bindings -----------------------------------------------------------

/** Records key presses as a binding (`ctrl+s`, `g r`). */
export function recordKey(
  previous: string,
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">,
): string | null {
  if (event.key === "Backspace" && !event.ctrlKey && !event.altKey && !event.metaKey) {
    const steps = previous.split(" ").filter(Boolean);
    steps.pop();
    return steps.join(" ");
  }
  const combo = comboFromEvent(event);
  if (combo === null) return null;
  const steps = previous.split(" ").filter(Boolean);
  // A sequence has at most three steps; a new key after that starts over.
  return steps.length >= 3 ? combo : [...steps, combo].join(" ");
}

function KeyRecorder({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const t = useT();
  const ids = useId();
  return (
    <div data-part="form.field" data-field="key">
      <label data-part="form.label" htmlFor={`${ids}-key`}>
        {t("customize-keys-key")}
      </label>
      <input
        id={`${ids}-key`}
        data-part="key-binding-editor.capture"
        type="text"
        readOnly
        value={value === "" ? "" : keyLabel(value, t)}
        placeholder={t("customize-keys-press")}
        aria-describedby={`${ids}-hint`}
        onKeyDown={(event) => {
          // Tab leaves the field, Escape closes the window; everything else is recorded.
          if (event.key === "Tab" || event.key === "Escape") return;
          event.preventDefault();
          event.stopPropagation();
          const next = recordKey(value, event);
          if (next !== null) onChange(next);
        }}
      />
      <span data-part="form.hint" id={`${ids}-hint`}>
        {t("customize-keys-press-hint")}
      </span>
    </div>
  );
}

function argsText(args: readonly JsonValue[]): string {
  return args.map((a) => (typeof a === "string" ? a : JSON.stringify(a))).join(" ");
}

/**
 * The label of a key context: Nail's own contexts are translated, one a
 * user's layout invents is the user's own text.
 */
function contextLabel(context: string, t: (id: string) => string): string {
  return isKnownKeyContext(context) ? t(`customize-keys-context-${context}`) : context;
}

/** Settings → Customize → Keys (widget `key-binding-editor`). */
export function KeyBindingEditor({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const layout = useLayoutConfig();
  const ids = useId();
  const choices = useActionChoices(layout);
  const contexts = [...new Set([...KEY_CONTEXTS, ...layout.keys.map((k) => k.context)])];
  const [filter, setFilter] = useState("");
  const [context, setContext] = useState("global");
  const [action, setAction] = useState(choices[0]?.value ?? "");
  const [arg, setArg] = useState("");
  const [key, setKey] = useState("");
  const [status, write] = useWrite();

  const shown = layout.keys.filter((k) => filter === "" || k.context === filter);
  const conflict = layout.keys.find((k) => k.context === context && k.key === key);

  const add = () =>
    void write(async () => {
      await writeKeyBinding(client, context, key, action, arg.trim() === "" ? [] : [arg.trim()]);
      setKey("");
      setArg("");
    }, "customize-keys-added");

  const row = (binding: KeyBinding, index: number) => {
    const name = keyLabel(binding.key, t);
    const what = actionText(layout, binding.action, label);
    return (
      <tr
        key={`${binding.context}:${binding.key}:${String(index)}`}
        data-part="key-binding-editor.row"
      >
        <td data-part="key-binding-editor.key">
          <kbd data-part="key-binding-editor.kbd">{name}</kbd>
        </td>
        <td data-part="key-binding-editor.action">
          {what}
          {binding.args.length > 0 && (
            <span data-part="key-binding-editor.args">{argsText(binding.args)}</span>
          )}
        </td>
        <td data-part="key-binding-editor.context">{contextLabel(binding.context, t)}</td>
        <td data-part="key-binding-editor.row-actions">
          <button
            type="button"
            data-part="form.button"
            data-action="remove"
            aria-label={t("customize-keys-remove", { key: name, action: what })}
            onClick={() =>
              void write(
                () => removeKeyBinding(client, binding.context, binding.key),
                "customize-keys-removed",
              )
            }
          >
            {t("common-remove")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="reset"
            aria-label={t("customize-keys-reset", { key: name })}
            onClick={() =>
              void write(
                () =>
                  updateLayout(client, [
                    { op: "reset-key-binding", context: binding.context, key: binding.key },
                  ]),
                "customize-reset-done",
              )
            }
          >
            {t("customize-keys-reset-short")}
          </button>
        </td>
      </tr>
    );
  };

  return (
    <section
      data-part="key-binding-editor"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("customize-keys-label")}
    >
      <p data-part="key-binding-editor.intro">{t("customize-keys-intro")}</p>
      <fieldset data-part="form.section" data-section="add">
        <legend data-part="form.section.title">{t("customize-keys-add-title")}</legend>
        <div data-part="form.field" data-field="context">
          <label data-part="form.label" htmlFor={`${ids}-context`}>
            {t("customize-keys-context")}
          </label>
          <select
            id={`${ids}-context`}
            data-part="form.select"
            value={context}
            onChange={(event) => {
              setContext(event.target.value);
            }}
          >
            {contexts.map((c) => (
              <option key={c} value={c}>
                {contextLabel(c, t)}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="action">
          <label data-part="form.label" htmlFor={`${ids}-action`}>
            {t("common-action")}
          </label>
          <select
            id={`${ids}-action`}
            data-part="form.select"
            value={action}
            onChange={(event) => {
              setAction(event.target.value);
            }}
          >
            {choices.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="argument">
          <label data-part="form.label" htmlFor={`${ids}-arg`}>
            {t("customize-keys-argument")}
          </label>
          <input
            id={`${ids}-arg`}
            data-part="form.input"
            type="text"
            value={arg}
            onChange={(event) => {
              setArg(event.target.value);
            }}
          />
        </div>
        <KeyRecorder value={key} onChange={setKey} />
        {conflict && key !== "" && (
          <p data-part="key-binding-editor.conflict" role="status">
            {t("customize-keys-conflict", { action: actionText(layout, conflict.action, label) })}
          </p>
        )}
        <div data-part="key-binding-editor.add-actions">
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            disabled={key === "" || action === ""}
            onClick={add}
          >
            {t("customize-keys-add")}
          </button>
          <button
            type="button"
            data-part="form.button"
            disabled={key === ""}
            onClick={() => {
              setKey("");
            }}
          >
            {t("customize-keys-clear")}
          </button>
        </div>
      </fieldset>
      <StatusLine status={status} part="key-binding-editor" />
      <div data-part="form.field" data-field="filter">
        <label data-part="form.label" htmlFor={`${ids}-filter`}>
          {t("common-show")}
        </label>
        <select
          id={`${ids}-filter`}
          data-part="form.select"
          value={filter}
          onChange={(event) => {
            setFilter(event.target.value);
          }}
        >
          <option value="">{t("customize-keys-filter-all")}</option>
          {contexts.map((c) => (
            <option key={c} value={c}>
              {contextLabel(c, t)}
            </option>
          ))}
        </select>
      </div>
      <table data-part="key-binding-editor.table">
        <caption data-part="key-binding-editor.caption">{t("customize-keys-table")}</caption>
        <thead>
          <tr data-part="key-binding-editor.head">
            <th scope="col">{t("customize-keys-key")}</th>
            <th scope="col">{t("common-action")}</th>
            <th scope="col">{t("customize-keys-context")}</th>
            <th scope="col">{t("customize-keys-actions")}</th>
          </tr>
        </thead>
        <tbody>{shown.map(row)}</tbody>
      </table>
    </section>
  );
}

// ---- Columns ----------------------------------------------------------------

interface ListNode {
  ref: LayoutNodeRef;
  node: Extract<LayoutNode, { type: "widget" }>;
}

/** Every `message-list` of the layout with its node reference. */
export function messageLists(layout: ResolvedLayout): ListNode[] {
  const out: ListNode[] = [];
  const walk = (node: LayoutNode, window: string, breakpoint: number, path: number[]) => {
    if (node.type === "widget") {
      if (node.kind === "message-list") out.push({ ref: { window, breakpoint, path }, node });
      return;
    }
    node.children.forEach((child, index) => {
      walk(child, window, breakpoint, [...path, index]);
    });
  };
  for (const [window, breakpoints] of Object.entries(layout.windows)) {
    breakpoints.forEach((bp, index) => {
      walk(bp.root, window, index, []);
    });
  }
  return out;
}

function ColumnDraft({ list, write }: { list: ListNode; write: Write }) {
  const t = useT();
  const client = useClient();
  const ids = useId();
  const initial = optionList(list.node, "columns", []).filter((c) =>
    (COLUMN_IDS as readonly string[]).includes(c),
  );
  const [columns, setColumns] = useState(initial);
  const available = COLUMN_IDS.filter((c) => !columns.includes(c));
  const [adding, setAdding] = useState<string>(available[0] ?? "");
  const changed = JSON.stringify(columns) !== JSON.stringify(initial);
  const entries = columns.map((c) => ({ key: c, label: t(`message-list-column-${c}`) }));
  const addable = available.includes(adding as (typeof COLUMN_IDS)[number])
    ? adding
    : (available[0] ?? "");

  return (
    <div data-part="column-editor.draft">
      {initial.length === 0 && <p data-part="form.hint">{t("customize-columns-rows")}</p>}
      <ReorderList
        entries={entries}
        label={t("customize-columns-items")}
        part="column-editor"
        onMove={(from, to) => {
          setColumns(moved(columns, from, to));
        }}
        onRemove={(index) => {
          setColumns(columns.filter((_, i) => i !== index));
        }}
      />
      {available.length > 0 && (
        <div data-part="column-editor.add" role="group" aria-label={t("customize-columns-add")}>
          <label data-part="form.label" htmlFor={`${ids}-column`}>
            {t("customize-columns-column")}
          </label>
          <select
            id={`${ids}-column`}
            data-part="form.select"
            value={addable}
            onChange={(event) => {
              setAdding(event.target.value);
            }}
          >
            {available.map((c) => (
              <option key={c} value={c}>
                {t(`message-list-column-${c}`)}
              </option>
            ))}
          </select>
          <button
            type="button"
            data-part="form.button"
            onClick={() => {
              setColumns([...columns, addable]);
            }}
          >
            {t("common-add-column")}
          </button>
        </div>
      )}
      <div data-part="column-editor.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={!changed || columns.length === 0}
          onClick={() =>
            void write(() => writeColumnOrder(client, list.ref, columns), "customize-saved")
          }
        >
          {t("customize-save")}
        </button>
        <button
          type="button"
          data-part="form.button"
          disabled={!changed}
          onClick={() => {
            setColumns(initial);
          }}
        >
          {t("common-undo-changes")}
        </button>
      </div>
    </div>
  );
}

/** Settings → Customize → Columns (widget `column-editor`). */
export function ColumnEditor({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const lists = messageLists(layout);
  const ids = useId();
  const [chosen, setChosen] = useState(0);
  const [status, write] = useWrite();
  const list = lists[chosen] ?? lists[0];
  const name = (l: ListNode, index: number) =>
    t("customize-columns-list", {
      window: l.ref.window,
      breakpoint: l.ref.breakpoint + 1,
      id: l.node.id ?? String(index + 1),
    });
  return (
    <section
      data-part="column-editor"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("customize-columns-label")}
    >
      <p data-part="column-editor.intro">{t("customize-columns-intro")}</p>
      {lists.length === 0 ? (
        <p data-part="column-editor.empty">{t("customize-columns-none")}</p>
      ) : (
        <>
          <div data-part="form.field" data-field="list">
            <label data-part="form.label" htmlFor={`${ids}-list`}>
              {t("common-list")}
            </label>
            <select
              id={`${ids}-list`}
              data-part="form.select"
              value={String(chosen)}
              onChange={(event) => {
                setChosen(Number(event.target.value));
              }}
            >
              {lists.map((l, index) => (
                <option key={index} value={String(index)}>
                  {name(l, index)}
                </option>
              ))}
            </select>
          </div>
          {list && (
            <ColumnDraft
              key={`${JSON.stringify(list.ref)}:${JSON.stringify(list.node.options.columns ?? null)}`}
              list={list}
              write={write}
            />
          )}
        </>
      )}
      <StatusLine status={status} part="column-editor" />
    </section>
  );
}
