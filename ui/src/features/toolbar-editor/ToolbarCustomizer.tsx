/**
 * The toolbar customizer (M5d WP182, ADR 0131): a window of the window
 * manager (`toolbar-customizer:<toolbar>`) opened from a toolbar's
 * right-click menu, the command palette or Settings.
 *
 * Three lists: the bar, its "More" menu and the available actions (built-in
 * actions, layout aliases and plug-in commands, plus Separator and Flexible
 * space). Buttons are dragged between them — onto the bar or into "More" to
 * add or move, back onto the available actions to remove. Every drag has a
 * keyboard alternative: the arrow keys choose an item, Ctrl+arrow moves it,
 * M moves it between the bar and "More", Delete removes it, Enter adds an
 * available action to the bar (Shift+Enter to "More"); the buttons under
 * the lists do the same for the chosen item.
 *
 * Every change is one `UpdateLayout` patch with the layout history's
 * revision as `base_revision` (a file changed on disk answers `conflict`);
 * Undo / Redo step through the shared `layout` history, "Reset to default"
 * sends `reset-toolbar`. The lists show the layout the core announced
 * (`ConfigChanged`), never a local copy.
 */
import { useQueryClient } from "@tanstack/react-query";
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import { NailError, errorMessageOf } from "../../api/client";
import type {
  LayoutPatch,
  LocalizedMessage,
  ToolbarArea,
  ToolbarItem,
  ToolbarSlot,
} from "../../api/generated";
import { buildPalette } from "../../actions/palette";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { MessageArgs } from "../../i18n/localizer";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useLayoutConfig } from "../../layout/LayoutContext";
import {
  applyEditHistory,
  nextRedo,
  nextUndo,
  useEditHistory,
  useEditHistoryStep,
} from "../../state/editHistory";
import { useClient } from "../../state/queries";
import { Icon } from "../../widgets/Icon";
import { paletteInfos, pluginTitles } from "../palette/Palette";
import { appRegistry } from "../registry";
import { CUSTOMIZER_PART, toolbarOfWindow } from "./ids";
import {
  addPatch,
  areaItems,
  canPlace,
  isNoOpMove,
  moveDestination,
  movePatch,
  removePatch,
  resetPatch,
  usedActions,
  type CustomizerItem,
} from "./model";
import { toolbarTitleId } from "./names";

/** The drag payload's media type (the source travels in a ref as well). */
export const DRAG_TYPE = "application/x-nail-toolbar-item";

/**
 * How many available actions the list shows at once; the search finds the
 * rest (a list of every action would be hundreds of rows).
 */
export const PALETTE_LIMIT = 30;

/** One available action (or Separator / Flexible space). */
export interface AvailableEntry {
  /** Stable key: the action id, or `separator` / `spacer`. */
  key: string;
  /** What it adds. */
  item: ToolbarItem;
  title: string;
  icon: string | null;
  plugin: boolean;
}

/** Where a drag started. */
type DragSource = { kind: "slot"; slot: ToolbarSlot } | { kind: "palette"; entry: AvailableEntry };

/** The list an item is chosen in. */
type ListName = ToolbarArea | "palette";

interface Selection {
  list: ListName;
  index: number;
}

/** What the status line says. */
interface Status {
  tone: "done" | "error";
  message: LocalizedMessage;
  /** The undone / redone change (`{ $change }` of the message). */
  change?: LocalizedMessage;
  /** Offer "Undo anyway" / "Redo anyway" (the file changed on disk). */
  retry?: "undo" | "redo";
}

/** The text and icon of an item. */
function useItemText() {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const infos = useMemo(() => paletteInfos(), []);
  const plugins = useMemo(() => pluginTitles(layout), [layout]);
  return (item: CustomizerItem): { text: string; icon: string | null } => {
    switch (item.type) {
      case "action": {
        const def = Object.hasOwn(layout.actions, item.action)
          ? layout.actions[item.action]
          : undefined;
        const info = Object.hasOwn(infos, item.action) ? infos[item.action] : undefined;
        const own = item.label ?? def?.label ?? null;
        const text =
          own !== null
            ? label(own)
            : info
              ? t(info.title)
              : ((Object.hasOwn(plugins, item.action) ? plugins[item.action] : undefined) ??
                item.action);
        return { text, icon: item.icon ?? def?.icon ?? info?.icon ?? null };
      }
      case "separator":
        return { text: t("toolbar-editor-item-separator"), icon: "minus" };
      case "spacer":
        return { text: t("toolbar-editor-item-spacer"), icon: "arrow-left-right" };
      case "header":
        return { text: t("toolbar-editor-item-header", { label: label(item.label) }), icon: null };
      case "submenu":
        return {
          text: t("toolbar-editor-item-submenu", { label: label(item.label) }),
          icon: "chevron-right",
        };
      case "more":
        return { text: label(item.label ?? "@toolbar.more"), icon: item.icon ?? "ellipsis" };
    }
  };
}

/** The available actions: Separator, Flexible space, then every action by title. */
export function useAvailable(): AvailableEntry[] {
  const t = useT();
  const layout = useLayoutConfig();
  return useMemo(() => {
    const infos = paletteInfos();
    const plugins = pluginTitles(layout);
    const entries = buildPalette({
      actionIds: [...Object.keys(appRegistry.actions), ...Object.keys(plugins)],
      infos,
      pluginTitles: plugins,
      layout,
      contexts: new Set(),
      recent: [],
      t,
    });
    const actions = entries.map((entry): AvailableEntry => ({
      key: entry.id,
      item: { type: "action", action: entry.id, args: [], label: null, icon: null },
      title: entry.title,
      icon:
        (Object.hasOwn(layout.actions, entry.id) ? layout.actions[entry.id]?.icon : null) ??
        infos[entry.id]?.icon ??
        null,
      plugin: entry.source === "plugin",
    }));
    return [
      {
        key: "separator",
        item: { type: "separator" },
        title: t("toolbar-editor-item-separator"),
        icon: "minus",
        plugin: false,
      },
      {
        key: "spacer",
        item: { type: "spacer" },
        title: t("toolbar-editor-item-spacer"),
        icon: "arrow-left-right",
        plugin: false,
      },
      ...actions,
    ];
  }, [layout, t]);
}

/** Entries whose title or id contains every word of `query`. */
export function filterAvailable(
  entries: readonly AvailableEntry[],
  query: string,
): AvailableEntry[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...entries];
  return entries.filter((entry) => {
    const text = `${entry.title} ${entry.key}`.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** Sends the customizer's patches and steps through the history. */
function useToolbarWriter() {
  const client = useClient();
  const queryClient = useQueryClient();
  const history = useEditHistory("layout");
  const step = useEditHistoryStep("layout");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status | null>(null);

  const refreshHistory = async () => {
    const answer = await client.call({ type: "GetEditHistory", scope: "layout" });
    if (answer.type === "EditHistory") applyEditHistory(queryClient, answer.history);
  };

  /** Sends `patches`; resolves `true` once written. */
  const send = async (patches: LayoutPatch[], done: LocalizedMessage): Promise<boolean> => {
    setBusy(true);
    try {
      const base = history.data?.revision;
      await client.request(
        { type: "UpdateLayout", patches, ...(base === undefined ? {} : { base_revision: base }) },
        "Done",
      );
      setStatus({ tone: "done", message: done });
      await refreshHistory();
      return true;
    } catch (error) {
      setStatus({ tone: "error", message: errorMessageOf(error) });
      return false;
    } finally {
      setBusy(false);
    }
  };

  const undoRedo = async (direction: "undo" | "redo", force = false) => {
    const entry = direction === "undo" ? nextUndo(history.data) : nextRedo(history.data);
    setBusy(true);
    try {
      await step.mutateAsync({ direction, force });
      if (entry) {
        setStatus({
          tone: "done",
          message: { id: direction === "undo" ? "toolbar-editor-undone" : "toolbar-editor-redone" },
          change: entry.label,
        });
      }
    } catch (error) {
      const conflict = error instanceof NailError && error.code === "conflict";
      setStatus({
        tone: "error",
        message: errorMessageOf(error),
        ...(conflict ? { retry: direction } : {}),
      });
    } finally {
      setBusy(false);
    }
  };

  return { history: history.data, busy, status, send, undoRedo };
}

/** Before or after an item, by the pointer's position over it. */
function dropsBefore(event: DragEvent<HTMLElement>, area: ToolbarArea): boolean {
  const rect = event.currentTarget.getBoundingClientRect();
  return area === "bar"
    ? event.clientX < rect.left + rect.width / 2
    : event.clientY < rect.top + rect.height / 2;
}

/** The customizer window's content. */
export function ToolbarCustomizer({ name, close }: DialogProps) {
  const t = useT();
  const layout = useLayoutConfig();
  const text = useItemText();
  const available = useAvailable();
  const writer = useToolbarWriter();
  const base = useId();
  const names = Object.keys(layout.toolbars).sort();
  const requested = toolbarOfWindow(name);
  const [chosen, setChosen] = useState<string | null>(requested);
  const toolbar =
    chosen !== null && names.includes(chosen)
      ? chosen
      : requested !== null && names.includes(requested)
        ? requested
        : (names[0] ?? null);
  const items: readonly ToolbarItem[] =
    toolbar !== null && Object.hasOwn(layout.toolbars, toolbar)
      ? (layout.toolbars[toolbar] ?? [])
      : [];
  const bar = areaItems(items, "bar");
  const more = areaItems(items, "more");
  const used = usedActions(items);
  const [query, setQuery] = useState("");
  const matching = filterAvailable(available, query);
  const shownAvailable = matching.slice(0, PALETTE_LIMIT);
  const lists: Record<ListName, number> = {
    bar: bar.length,
    more: more.length,
    palette: shownAvailable.length,
  };

  const [selection, setSelection] = useState<Selection | null>(null);
  const [active, setActive] = useState<Record<ListName, number>>({ bar: 0, more: 0, palette: 0 });
  const [focusRequest, setFocusRequest] = useState<Selection | null>(null);
  const drag = useRef<DragSource | null>(null);
  const [drop, setDrop] = useState<{ list: ListName; index: number } | null>(null);
  const root = useRef<HTMLDivElement>(null);

  const clampIndex = (list: ListName, index: number) =>
    Math.max(0, Math.min(index, lists[list] - 1));
  const current: Selection | null =
    selection !== null && lists[selection.list] > 0
      ? { list: selection.list, index: clampIndex(selection.list, selection.index) }
      : null;
  const selectedItem: CustomizerItem | undefined =
    current === null
      ? undefined
      : current.list === "palette"
        ? shownAvailable[current.index]?.item
        : areaItems(items, current.list)[current.index];

  // Focus the item a keyboard edit moved (once the list shows it).
  useEffect(() => {
    if (focusRequest === null) return;
    const el = root.current?.querySelector<HTMLElement>(
      `[data-list="${focusRequest.list}"] [data-index="${String(focusRequest.index)}"]`,
    );
    if (el) {
      el.focus();
      setFocusRequest(null);
    }
  }, [focusRequest, layout]);

  const choose = (next: Selection, focus: boolean) => {
    setSelection(next);
    setActive((a) => ({ ...a, [next.list]: next.index }));
    if (focus) setFocusRequest(next);
  };

  const describe = (item: CustomizerItem | undefined) => (item ? text(item).text : "");

  // --- Edits ---------------------------------------------------------------

  const move = async (from: ToolbarSlot, to: ToolbarSlot, focus: boolean) => {
    if (toolbar === null || isNoOpMove(from, to)) return;
    const item = areaItems(items, from.area)[from.index];
    if (item === undefined || !canPlace(item, to.area)) return;
    const done: LocalizedMessage = {
      id:
        from.area === to.area
          ? "toolbar-editor-moved"
          : to.area === "more"
            ? "toolbar-editor-moved-to-more"
            : "toolbar-editor-moved-to-bar",
      args: { item: describe(item) },
    };
    if (await writer.send([movePatch(toolbar, from, to)], done)) {
      const length = areaItems(items, to.area).length + (from.area === to.area ? 0 : 1);
      choose({ list: to.area, index: Math.min(to.index, length - 1) }, focus);
    }
  };

  const add = async (entry: AvailableEntry, to: ToolbarSlot) => {
    if (toolbar === null || !canPlace(entry.item, to.area)) return;
    const done: LocalizedMessage = {
      id: to.area === "more" ? "toolbar-editor-added-to-more" : "toolbar-editor-added",
      args: { item: entry.title },
    };
    await writer.send([addPatch(toolbar, entry.item, to)], done);
  };

  const remove = async (from: ToolbarSlot, focus: boolean) => {
    if (toolbar === null) return;
    const item = areaItems(items, from.area)[from.index];
    if (item === undefined) return;
    const done: LocalizedMessage = { id: "toolbar-editor-removed", args: { item: describe(item) } };
    if (await writer.send([removePatch(toolbar, from)], done)) {
      const left = areaItems(items, from.area).length - 1;
      if (left > 0) choose({ list: from.area, index: Math.min(from.index, left - 1) }, focus);
      else setSelection(null);
    }
  };

  const reset = async () => {
    if (toolbar === null) return;
    if (await writer.send([resetPatch(toolbar)], { id: "toolbar-editor-reset-done" })) {
      setSelection(null);
    }
  };

  const otherArea = (area: ToolbarArea): ToolbarArea => (area === "bar" ? "more" : "bar");
  const endOf = (area: ToolbarArea): ToolbarSlot => ({
    area,
    index: areaItems(items, area).length,
  });

  // --- Keyboard --------------------------------------------------------------

  const onItemKeyDown = (event: KeyboardEvent<HTMLElement>, list: ListName, index: number) => {
    if (event.altKey || event.metaKey || writer.busy) return;
    const prev = ["ArrowLeft", "ArrowUp"].includes(event.key);
    const next = ["ArrowRight", "ArrowDown"].includes(event.key);
    const count = lists[list];
    if (list !== "palette" && event.ctrlKey && (prev || next)) {
      event.preventDefault();
      const to = index + (next ? 1 : -1);
      if (to >= 0 && to < count) void move({ area: list, index }, { area: list, index: to }, true);
      return;
    }
    if (event.ctrlKey) return;
    if (prev || next || event.key === "Home" || event.key === "End") {
      event.preventDefault();
      let target = index + (next ? 1 : prev ? -1 : 0);
      if (event.key === "Home") target = 0;
      if (event.key === "End") target = count - 1;
      choose({ list, index: Math.max(0, Math.min(target, count - 1)) }, true);
      return;
    }
    if (list === "palette") {
      const entry = shownAvailable[index];
      if (event.key === "Enter" && entry) {
        event.preventDefault();
        const area: ToolbarArea = event.shiftKey ? "more" : "bar";
        void add(entry, endOf(area));
      }
      return;
    }
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      void remove({ area: list, index }, true);
    } else if (event.key.toLowerCase() === "m" && !event.shiftKey) {
      event.preventDefault();
      void move({ area: list, index }, endOf(otherArea(list)), true);
    }
  };

  // --- Drag and drop ---------------------------------------------------------

  const startDrag = (event: DragEvent<HTMLElement>, source: DragSource) => {
    drag.current = source;
    event.dataTransfer.effectAllowed = source.kind === "palette" ? "copy" : "move";
    event.dataTransfer.setData(
      DRAG_TYPE,
      JSON.stringify(source.kind === "slot" ? source.slot : { add: source.entry.key }),
    );
  };
  const endDrag = () => {
    drag.current = null;
    setDrop(null);
  };
  const dragged = (): CustomizerItem | undefined => {
    const source = drag.current;
    if (source === null) return undefined;
    return source.kind === "palette"
      ? source.entry.item
      : areaItems(items, source.slot.area)[source.slot.index];
  };

  /** Over a bar / More list (`index` = the slot dropped before). */
  const overArea = (event: DragEvent<HTMLElement>, area: ToolbarArea, index: number) => {
    const item = dragged();
    if (item === undefined || !canPlace(item, area) || writer.busy) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = drag.current?.kind === "palette" ? "copy" : "move";
    if (drop?.list !== area || drop.index !== index) setDrop({ list: area, index });
  };
  const dropOnArea = (event: DragEvent<HTMLElement>, area: ToolbarArea, index: number) => {
    const source = drag.current;
    event.preventDefault();
    event.stopPropagation();
    endDrag();
    if (source === null) return;
    if (source.kind === "palette") void add(source.entry, { area, index });
    else void move(source.slot, moveDestination(source.slot, { area, index }), false);
  };
  const overPalette = (event: DragEvent<HTMLElement>) => {
    if (drag.current?.kind !== "slot" || writer.busy) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    if (drop?.list !== "palette") setDrop({ list: "palette", index: 0 });
  };
  const dropOnPalette = (event: DragEvent<HTMLElement>) => {
    const source = drag.current;
    event.preventDefault();
    endDrag();
    if (source?.kind === "slot") void remove(source.slot, false);
  };

  // --- Rendering -------------------------------------------------------------

  const renderArea = (area: ToolbarArea) => {
    const entries = areaItems(items, area);
    const headingId = `${base}-${area}`;
    const tabStop = Math.min(active[area], Math.max(0, entries.length - 1));
    return (
      <section data-part="toolbar-customizer.area" data-area={area} aria-labelledby={headingId}>
        <h3 id={headingId} data-part="toolbar-customizer.heading">
          {t(area === "bar" ? "toolbar-editor-area-bar" : "toolbar-editor-area-more")}
        </h3>
        <ul
          data-part="toolbar-customizer.list"
          data-list={area}
          data-orientation={area === "bar" ? "horizontal" : "vertical"}
          data-drop={drop?.list === area && drop.index >= entries.length ? "end" : undefined}
          role="listbox"
          aria-labelledby={headingId}
          aria-orientation={area === "bar" ? "horizontal" : "vertical"}
          aria-describedby={`${base}-keys`}
          onDragOver={(event) => {
            overArea(event, area, entries.length);
          }}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDrop(null);
          }}
          onDrop={(event) => {
            dropOnArea(event, area, entries.length);
          }}
        >
          {entries.length === 0 && (
            <li data-part="toolbar-customizer.empty" role="presentation">
              {t(area === "bar" ? "toolbar-editor-empty-bar" : "toolbar-editor-empty-more")}
            </li>
          )}
          {entries.map((item, index) => {
            const { text: itemText, icon } = text(item);
            const isSelected = current?.list === area && current.index === index;
            return (
              <li
                // Keyed by slot: focus stays on the slot a keyboard edit filled.
                key={`${area}-${String(index)}`}
                data-part="toolbar-customizer.item"
                data-kind={item.type}
                data-index={index}
                data-drop={drop?.list === area && drop.index === index ? "before" : undefined}
                data-state={isSelected ? "selected" : undefined}
                role="option"
                aria-selected={isSelected}
                tabIndex={index === tabStop ? 0 : -1}
                draggable={!writer.busy}
                onFocus={() => {
                  if (!isSelected) choose({ list: area, index }, false);
                }}
                onClick={() => {
                  choose({ list: area, index }, false);
                }}
                onKeyDown={(event) => {
                  onItemKeyDown(event, area, index);
                }}
                onDragStart={(event) => {
                  startDrag(event, { kind: "slot", slot: { area, index } });
                }}
                onDragEnd={endDrag}
                onDragOver={(event) => {
                  overArea(event, area, dropsBefore(event, area) ? index : index + 1);
                }}
                onDrop={(event) => {
                  dropOnArea(event, area, dropsBefore(event, area) ? index : index + 1);
                }}
              >
                <Icon name={icon} part="toolbar-customizer.item.icon" />
                <span data-part="toolbar-customizer.item.label">{itemText}</span>
              </li>
            );
          })}
        </ul>
      </section>
    );
  };

  const paletteHeading = `${base}-palette`;
  const paletteStop = Math.min(active.palette, Math.max(0, shownAvailable.length - 1));
  const selectedSlot: ToolbarSlot | null =
    current !== null && current.list !== "palette"
      ? { area: current.list, index: current.index }
      : null;
  const selectedEntry = current?.list === "palette" ? shownAvailable[current.index] : undefined;
  const undo = nextUndo(writer.history);
  const redo = nextRedo(writer.history);
  const busy = writer.busy;

  if (toolbar === null) {
    return (
      <div data-part={CUSTOMIZER_PART} data-state="empty">
        <p data-part="toolbar-customizer.intro">{t("toolbar-editor-no-toolbars")}</p>
        <div data-part="toolbar-customizer.footer">
          <button type="button" data-part="form.button" onClick={close}>
            {t("toolbar-editor-done")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={root}
      data-part={CUSTOMIZER_PART}
      data-toolbar={toolbar}
      data-dragging={drop !== null ? "" : undefined}
      aria-busy={busy}
    >
      <div data-part="toolbar-customizer.header">
        <div data-part="form.field" data-field="toolbar">
          <label data-part="form.label" htmlFor={`${base}-toolbar`}>
            {t("toolbar-editor-choose")}
          </label>
          <select
            id={`${base}-toolbar`}
            data-part="form.select"
            value={toolbar}
            onChange={(event) => {
              setChosen(event.target.value);
              setSelection(null);
            }}
          >
            {names.map((n) => {
              const id = toolbarTitleId(n);
              return (
                <option key={n} value={n}>
                  {id === null ? n : t(id)}
                </option>
              );
            })}
          </select>
        </div>
        <p id={`${base}-keys`} data-part="toolbar-customizer.intro">
          {t("toolbar-editor-intro")}
        </p>
      </div>

      <div data-part="toolbar-customizer.body">
        <div data-part="toolbar-customizer.areas">
          {renderArea("bar")}
          {renderArea("more")}
          <div
            data-part="toolbar-customizer.commands"
            role="group"
            aria-label={t("toolbar-editor-commands")}
          >
            <button
              type="button"
              data-part="form.button"
              data-command="earlier"
              disabled={busy || selectedSlot === null || selectedSlot.index === 0}
              onClick={() => {
                if (selectedSlot)
                  void move(
                    selectedSlot,
                    { ...selectedSlot, index: selectedSlot.index - 1 },
                    false,
                  );
              }}
            >
              {t("toolbar-editor-move-earlier")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-command="later"
              disabled={
                busy || selectedSlot === null || selectedSlot.index >= lists[selectedSlot.area] - 1
              }
              onClick={() => {
                if (selectedSlot)
                  void move(
                    selectedSlot,
                    { ...selectedSlot, index: selectedSlot.index + 1 },
                    false,
                  );
              }}
            >
              {t("toolbar-editor-move-later")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-command="other-area"
              disabled={
                busy ||
                selectedSlot === null ||
                selectedItem === undefined ||
                !canPlace(selectedItem, otherArea(selectedSlot.area))
              }
              onClick={() => {
                if (selectedSlot)
                  void move(selectedSlot, endOf(otherArea(selectedSlot.area)), false);
              }}
            >
              {t(
                selectedSlot?.area === "more"
                  ? "toolbar-editor-move-to-bar"
                  : "toolbar-editor-move-to-more",
              )}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-command="remove"
              data-variant="danger"
              disabled={busy || selectedSlot === null}
              onClick={() => {
                if (selectedSlot) void remove(selectedSlot, false);
              }}
            >
              {t("toolbar-editor-remove")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-command="add-to-bar"
              disabled={busy || selectedEntry === undefined || !canPlace(selectedEntry.item, "bar")}
              onClick={() => {
                if (selectedEntry) void add(selectedEntry, endOf("bar"));
              }}
            >
              {t("toolbar-editor-add-to-bar")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-command="add-to-more"
              disabled={
                busy || selectedEntry === undefined || !canPlace(selectedEntry.item, "more")
              }
              onClick={() => {
                if (selectedEntry) void add(selectedEntry, endOf("more"));
              }}
            >
              {t("toolbar-editor-add-to-more")}
            </button>
          </div>
        </div>

        <section
          data-part="toolbar-customizer.palette"
          data-drop={drop?.list === "palette" ? "remove" : undefined}
          aria-labelledby={paletteHeading}
          onDragOver={overPalette}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDrop(null);
          }}
          onDrop={dropOnPalette}
        >
          <h3 id={paletteHeading} data-part="toolbar-customizer.heading">
            {t("toolbar-editor-area-palette")}
          </h3>
          <div data-part="form.field" data-field="search">
            <label data-part="form.label" htmlFor={`${base}-search`}>
              {t("toolbar-editor-search")}
            </label>
            <input
              id={`${base}-search`}
              type="search"
              data-part="form.input"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive((a) => ({ ...a, palette: 0 }));
                if (selection?.list === "palette") setSelection(null);
              }}
            />
          </div>
          <p data-part="toolbar-customizer.drop-hint">{t("toolbar-editor-drop-to-remove")}</p>
          <ul
            data-part="toolbar-customizer.list"
            data-list="palette"
            data-orientation="vertical"
            role="listbox"
            aria-labelledby={paletteHeading}
            aria-describedby={`${base}-keys`}
          >
            {shownAvailable.length === 0 && (
              <li data-part="toolbar-customizer.empty" role="presentation">
                {t("toolbar-editor-no-match")}
              </li>
            )}
            {shownAvailable.map((entry, index) => {
              const isSelected = current?.list === "palette" && current.index === index;
              const inUse = entry.item.type === "action" && used.has(entry.item.action);
              return (
                <li
                  key={entry.key}
                  data-part="toolbar-customizer.item"
                  data-kind={entry.item.type}
                  data-action={entry.item.type === "action" ? entry.item.action : undefined}
                  data-index={index}
                  data-state={isSelected ? "selected" : undefined}
                  data-source={entry.plugin ? "plugin" : undefined}
                  role="option"
                  aria-selected={isSelected}
                  tabIndex={index === paletteStop ? 0 : -1}
                  draggable={!busy}
                  onFocus={() => {
                    if (!isSelected) choose({ list: "palette", index }, false);
                  }}
                  onClick={() => {
                    choose({ list: "palette", index }, false);
                  }}
                  onDoubleClick={() => {
                    void add(entry, endOf("bar"));
                  }}
                  onKeyDown={(event) => {
                    onItemKeyDown(event, "palette", index);
                  }}
                  onDragStart={(event) => {
                    startDrag(event, { kind: "palette", entry });
                  }}
                  onDragEnd={endDrag}
                >
                  <Icon name={entry.icon} part="toolbar-customizer.item.icon" />
                  <span data-part="toolbar-customizer.item.label">{entry.title}</span>
                  {entry.plugin && (
                    <span data-part="toolbar-customizer.item.badge" data-badge="plugin">
                      {t("toolbar-editor-badge-plugin")}
                    </span>
                  )}
                  {inUse && (
                    <span data-part="toolbar-customizer.item.badge" data-badge="used">
                      {t("toolbar-editor-badge-used")}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
          {matching.length > shownAvailable.length && (
            <p data-part="toolbar-customizer.more-matches">
              {t("toolbar-editor-more-matches", {
                shown: shownAvailable.length,
                total: matching.length,
              })}
            </p>
          )}
        </section>
      </div>

      <p data-part="toolbar-customizer.status" data-tone={writer.status?.tone} role="status">
        {writer.status ? statusText(writer.status, t) : null}
        {writer.status?.retry && (
          <button
            type="button"
            data-part="form.button"
            data-command="force"
            disabled={busy}
            onClick={() => {
              if (writer.status?.retry) void writer.undoRedo(writer.status.retry, true);
            }}
          >
            {t(
              writer.status.retry === "undo"
                ? "toolbar-editor-undo-anyway"
                : "toolbar-editor-redo-anyway",
            )}
          </button>
        )}
      </p>

      <div data-part="toolbar-customizer.footer">
        <button
          type="button"
          data-part="form.button"
          data-command="undo"
          disabled={busy || undo === null}
          title={
            undo
              ? t("toolbar-editor-undo-title", { change: t(undo.label.id, undo.label.args) })
              : undefined
          }
          onClick={() => {
            void writer.undoRedo("undo");
          }}
        >
          <Icon name="undo-2" part="form.button.icon" />
          {t("toolbar-editor-undo")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-command="redo"
          disabled={busy || redo === null}
          title={
            redo
              ? t("toolbar-editor-redo-title", { change: t(redo.label.id, redo.label.args) })
              : undefined
          }
          onClick={() => {
            void writer.undoRedo("redo");
          }}
        >
          <Icon name="redo-2" part="form.button.icon" />
          {t("toolbar-editor-redo")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-command="reset"
          disabled={busy}
          onClick={() => {
            void reset();
          }}
        >
          <Icon name="rotate-ccw" part="form.button.icon" />
          {t("toolbar-editor-reset")}
        </button>
        <span data-part="toolbar-customizer.footer.spacer" />
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-command="done"
          onClick={close}
        >
          {t("toolbar-editor-done")}
        </button>
      </div>
    </div>
  );
}

/** The status line's text (an undo names the change it undid). */
function statusText(status: Status, t: (id: string, args?: MessageArgs) => string): string {
  const { id, args } = status.message;
  if (status.change) return t(id, { change: t(status.change.id, status.change.args) });
  return t(id, args);
}
