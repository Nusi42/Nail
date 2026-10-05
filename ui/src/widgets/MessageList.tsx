import { useQueries } from "@tanstack/react-query";
import { measureElement, useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { comboFromEvent, normalizeCombo } from "../actions/keys";
import { errorMessageIdOf } from "../api/client";
import type {
  Address,
  ConversationMessage,
  KeyBinding,
  MessageSummary,
  ThreadParticipant,
  ThreadSummary,
} from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { LayoutContext } from "../layout/LayoutContext";
import {
  optionBool,
  optionNumber,
  optionString,
  type WidgetNode,
  type WidgetProps,
} from "../layout/widgetRegistry";
import { useNow } from "../state/clock";
import {
  isUnsupported,
  queryKeys,
  useAccounts,
  useClient,
  useMessages,
  useSearch,
  useThreads,
} from "../state/queries";
import { useUi, useUiStoreApi } from "../state/uiStore";
import {
  compileTemplate,
  fieldPart,
  renderTemplate,
  type CompiledTemplate,
  type Segment,
} from "../template/template";
import { useGeometry } from "../theme/ThemeProvider";
import { ALL_ACCOUNTS } from "./AccountSwitcher";
import { accountColorSlot } from "./folderPane";
import { filteredScope } from "./listFilter";
import { Icon } from "./Icon";
import { ChildRowView } from "./MessageListChild";
import { registerListControl } from "./messageListControl";
import {
  childKey,
  collapseMove,
  expandMove,
  messageOrderOf,
  selectedIndexOf,
  siblingPosition,
  stepIndex,
  toggled,
  visibleRows,
  type RowRef,
  type TreeMove,
} from "./messageListModel";
import { Avatar, Tag } from "./primitives";

/**
 * Rows rendered beyond the viewport on each side. M5a rows are taller
 * (avatar, tag line), so about a screen's worth stays rendered for keyboard
 * moves and find-in-page; paging ahead ignores these.
 */
const LIST_OVERSCAN = 12;

/**
 * A rendered row's exact height. TanStack Virtual's default rounds it
 * (`Math.round` of the border box, or `offsetHeight`), but rows are
 * fractional (e.g. 111.22 px from the font's line heights): the virtualizer's
 * offsets then drift from the real layout by a fraction of a pixel per row, so
 * "scroll the selected row into view" stopped up to a pixel or two short of
 * where the browser puts it — and which of the two won depended on timing
 * (the visual screen main-1024 landed at either of two positions).
 */
export function exactRowSize(
  element: Element,
  entry: ResizeObserverEntry | undefined,
  instance: Virtualizer<HTMLDivElement, Element>,
): number {
  const box = entry?.borderBoxSize[0];
  if (box) return box.blockSize;
  const height = element.getBoundingClientRect().height;
  // No layout (jsdom, a detached row): TanStack's own measuring (its cache,
  // else `offsetHeight`).
  return height > 0 ? height : measureElement(element, entry, instance);
}

/** Row templates used when the layout gives none (or only invalid ones). */
export const DEFAULT_ROWS = ["{from.name | bold-if-unread}  {date | relative}", "{subject}"];

/**
 * Columns a list can show instead of row templates (`columns="from subject
 * date"`, edited in Settings → Customize), each rendered by a template.
 */
export const COLUMN_TEMPLATES = {
  from: "{from.name | bold-if-unread}",
  subject: "{subject | bold-if-unread}",
  date: "{date | short-date}",
  preview: "{preview}",
  tags: "{tags | chips}",
  account: "{account}",
} as const;

export type ColumnId = keyof typeof COLUMN_TEMPLATES;
export const COLUMN_IDS = Object.keys(COLUMN_TEMPLATES) as ColumnId[];

/** The known columns of a `columns` option, in order (unknown ones are skipped). */
export function columnsOf(node: WidgetNode): ColumnId[] {
  const value = node.options.columns;
  if (typeof value !== "string") return [];
  return value.split(/\s+/).filter((c): c is ColumnId => Object.hasOwn(COLUMN_TEMPLATES, c));
}

/** Compiles row templates, dropping invalid ones with a warning. */
export function compileRows(rows: readonly string[]): CompiledTemplate[] {
  const compiled: CompiledTemplate[] = [];
  for (const row of rows) {
    try {
      compiled.push(compileTemplate(row));
    } catch (error) {
      console.warn(`nail: invalid row template "${row}": ${(error as Error).message}`);
    }
  }
  return compiled.length > 0 ? compiled : DEFAULT_ROWS.map(compileTemplate);
}

/** Space-separated `data-state` tokens of a row. */
export function rowStates(message: MessageSummary, selected: boolean): string | undefined {
  const states: string[] = [];
  if (selected) states.push("selected");
  if (!message.flags.seen) states.push("unread");
  if (message.flags.flagged) states.push("flagged");
  if (message.flags.answered) states.push("answered");
  if (message.has_attachments) states.push("has-attachments");
  // M2c: only the headers are stored (the body downloads on open).
  if (message.headers_only) states.push("headers-only");
  return states.length > 0 ? states.join(" ") : undefined;
}

/** Space-separated `data-state` tokens of a thread row. */
export function threadStates(thread: ThreadSummary, selected: boolean): string | undefined {
  const states: string[] = [];
  if (selected) states.push("selected");
  if (thread.unread_count > 0) states.push("unread");
  if (thread.flagged) states.push("flagged");
  if (thread.has_attachments) states.push("has-attachments");
  if (thread.message_count > 1) states.push("thread");
  return states.length > 0 ? states.join(" ") : undefined;
}

/** Template data of a message: the summary plus derived fields. */
function templateData(message: MessageSummary) {
  return {
    ...message,
    unread: !message.flags.seen,
    flagged: message.flags.flagged,
    account: message.account_id,
    count: 1 + message.duplicates.length,
    // P2: a message row's people are its sender (`{participants | names(N)}`),
    // and it is its own newest message ("Sender: preview", WP202).
    participants: message.from ? [{ ...message.from, me: false, unread: !message.flags.seen }] : [],
    latest_from: message.from ? { ...message.from, me: false, unread: !message.flags.seen } : null,
  };
}

/** The sender a thread row stands for: the newest message's, else the last participant. */
export function threadSender(thread: ThreadSummary): ThreadParticipant | null {
  return thread.latest_from ?? thread.participants[thread.participants.length - 1] ?? null;
}

/**
 * Template data of a thread row: the fields a message row offers (`from` =
 * the newest message's sender), plus `count` and `participants` (P2).
 */
export function threadTemplateData(thread: ThreadSummary) {
  return {
    ...thread,
    from: threadSender(thread),
    unread: thread.unread_count > 0,
    account: thread.account_ids.join(" "),
    count: thread.message_count,
  };
}

/**
 * The list's conversation mode (`threads` option; P2, ADR 0148): `off`
 * lists messages, `collapsed` one row per conversation, `expandable`
 * rows that open into their messages (Design B; `expanded` is its pre-P2
 * spelling).
 */
export type ThreadMode = "off" | "collapsed" | "expandable";

export function threadModeOf(node: WidgetNode): ThreadMode {
  switch (optionString(node, "threads", "off")) {
    case "collapsed":
      return "collapsed";
    case "expandable":
    case "expanded":
      return "expandable";
    default:
      return "off";
  }
}

/**
 * Summary fields the M5a rows show when the core sends them (proposed to the
 * contract by WP153: the number of attachments, an invitation inside, a
 * security / link warning). Absent fields simply hide their tag.
 */
export interface SummaryExtras {
  attachment_count?: number;
  invitation?: boolean;
  warning?: boolean;
}

/** What the row's tag line shows (M5a; hidden in Compact by the skin). */
export interface RowTags {
  accountIds: readonly string[];
  attachments: boolean;
  attachmentCount: number | null;
  invitation: boolean;
  warning: boolean;
  flagged: boolean;
  labels: readonly string[];
}

/**
 * One row of the list: a message, a conversation represented by its newest
 * message, or (Design B) a message of an expanded conversation.
 */
export interface Row extends RowRef {
  accountId: string;
  states: (selected: boolean) => string | undefined;
  data: object;
  unread: boolean;
  /** The sender the avatar stands for. */
  from: Address | null;
  tags: RowTags;
  /** A child row's message (Design B). */
  child?: ConversationMessage;
}

/** The proposed extra fields of a summary, when present and well typed. */
export function extrasOf(value: object): SummaryExtras {
  const v = value as Record<string, unknown>;
  const out: SummaryExtras = {};
  if (typeof v.attachment_count === "number") out.attachment_count = v.attachment_count;
  if (v.invitation === true) out.invitation = true;
  if (v.warning === true) out.warning = true;
  return out;
}

/** The tag line of a message row. */
export function messageTags(m: MessageSummary): RowTags {
  const extras = extrasOf(m);
  return {
    accountIds: [m.account_id],
    attachments: m.has_attachments || (extras.attachment_count ?? 0) > 0,
    attachmentCount: extras.attachment_count ?? null,
    invitation: extras.invitation ?? false,
    warning: extras.warning ?? false,
    flagged: m.flags.flagged,
    labels: m.tags,
  };
}

function messageRow(m: MessageSummary): Row {
  return {
    key: m.id,
    kind: "message",
    messageId: m.id,
    threadId: m.thread_id,
    parentKey: null,
    expandable: false,
    accountId: m.account_id,
    states: (selected) => rowStates(m, selected),
    data: templateData(m),
    unread: !m.flags.seen,
    from: m.from,
    tags: messageTags(m),
  };
}

/** A conversation row (Design A); the tag line adds the newest message's labels. */
export function threadRow(thread: ThreadSummary): Row {
  // Older cores (and some tests' fixtures) may lack the newest summary.
  const latest = (thread as Partial<ThreadSummary>).latest_message;
  const extras = latest ? extrasOf(latest) : {};
  return {
    key: thread.id,
    kind: "conversation",
    messageId: thread.latest_message_id,
    threadId: thread.id,
    parentKey: null,
    expandable: thread.message_count > 1,
    accountId: thread.account_ids.join(" "),
    states: (selected) => threadStates(thread, selected),
    data: threadTemplateData(thread),
    unread: thread.unread_count > 0,
    from: threadSender(thread),
    tags: {
      accountIds: thread.account_ids,
      attachments: thread.has_attachments,
      attachmentCount: null,
      invitation: extras.invitation ?? false,
      warning: extras.warning ?? false,
      flagged: thread.flagged,
      labels: latest?.tags ?? [],
    },
  };
}

/** Space-separated `data-state` tokens of a child row (Design B). */
export function childStates(m: ConversationMessage, selected: boolean): string | undefined {
  const states: string[] = [];
  if (selected) states.push("selected");
  if (!m.flags.seen) states.push("unread");
  if (m.flags.flagged) states.push("flagged");
  if (m.from_me) states.push("from-me");
  return states.length > 0 ? states.join(" ") : undefined;
}

/** A message of an expanded conversation (Design B): the reader shows it alone. */
export function childRow(parent: RowRef, m: ConversationMessage): Row {
  return {
    key: childKey(parent.key, m.id),
    kind: "child",
    messageId: m.id,
    threadId: null,
    parentKey: parent.key,
    expandable: false,
    accountId: m.account_id,
    states: (selected) => childStates(m, selected),
    data: templateData(m),
    unread: !m.flags.seen,
    from: m.from,
    tags: messageTags(m),
    child: m,
  };
}

/**
 * The list's step for a key typed in it (`mail.next` / `mail.previous`
 * bound in the `list` context; P2 WP202): +1, -1, or `null` for any other
 * key. An expandable list steps through its own rows with it, because a
 * conversation row and its newest child select the same message.
 */
export function listStep(keys: readonly KeyBinding[], combo: string): number | null {
  for (const binding of keys) {
    if (binding.context !== "list" || /\s/.test(binding.key.trim())) continue;
    if (normalizeCombo(binding.key.trim()) !== combo) continue;
    if (binding.action === "mail.next") return 1;
    if (binding.action === "mail.previous") return -1;
    return null;
  }
  return null;
}

/** An account's name and colour on the tag line. */
export interface AccountTag {
  name: string;
  /** Palette slot, 0-based (`data-color`): the folder pane's account colour. */
  slot: number;
}

/**
 * Account names and colours by id: the colour follows the account order
 * like the folder pane's sections (`account_color_slot`), so a row's dot
 * matches its account's section.
 */
export function accountTags(
  accounts: readonly { id: string; name: string; email: string }[],
): Map<string, AccountTag> {
  const out = new Map<string, AccountTag>();
  accounts
    .filter((a) => a.id !== ALL_ACCOUNTS)
    .forEach((a, index) => {
      out.set(a.id, { name: a.name || a.email || a.id, slot: accountColorSlot(index) - 1 });
    });
  return out;
}

/** Labels shown on the tag line at most (the rest stay in the reader). */
const MAX_LABELS = 3;

/**
 * The tag line: account colour (with the name when several accounts are
 * set up), attachments, invitation, warning, flag and labels. Nothing when
 * there is nothing to say.
 */
function TagLine({ tags, accounts }: { tags: RowTags; accounts: ReadonlyMap<string, AccountTag> }) {
  const t = useT();
  const items: ReactNode[] = [];
  if (accounts.size > 1) {
    for (const id of tags.accountIds) {
      const account = accounts.get(id);
      items.push(
        <Tag
          key={`account-${id}`}
          kind="account"
          {...(account ? { dot: account.slot } : {})}
          text={account?.name ?? id}
        />,
      );
    }
  }
  if (tags.attachments) {
    items.push(
      <Tag
        key="attachments"
        kind="attachments"
        icon="paperclip"
        text={
          tags.attachmentCount === null
            ? t("message-list-tag-attachment")
            : t("message-list-tag-attachments", { count: tags.attachmentCount })
        }
      />,
    );
  }
  if (tags.invitation) {
    items.push(
      <Tag
        key="invitation"
        kind="invitation"
        tone="accent"
        icon="calendar"
        text={t("message-list-tag-invitation")}
      />,
    );
  }
  if (tags.warning) {
    items.push(
      <Tag
        key="warning"
        kind="warning"
        tone="warning"
        icon="triangle-alert"
        text={t("message-list-tag-warning")}
      />,
    );
  }
  if (tags.flagged) {
    items.push(
      <Tag
        key="flagged"
        kind="flagged"
        tone="danger"
        icon="flag"
        text={t("message-list-tag-flagged")}
      />,
    );
  }
  for (const label of tags.labels.slice(0, MAX_LABELS)) {
    items.push(<Tag key={`label-${label}`} kind="label" icon="tag" text={label} />);
  }
  if (items.length === 0) return null;
  return <div data-part="message-list.row.tag-line">{items}</div>;
}

/**
 * The rows of the list: search hits while a search is active, threads when
 * the `threads` option asks for them (and the core supports it), else the
 * folder's messages.
 */
function useRows(node: WidgetNode, pageSize: number) {
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const listFilter = useUi((s) => s.listFilter);
  const followSearch = optionBool(node, "follow-search", true);
  const activeSearch = followSearch && search !== null ? search : null;
  // A filter chip (M5b) is a search: its term within the folder, or added
  // to the user's search.
  const filterSearch = filteredScope(listFilter, activeSearch, accountId, folderId);
  const searching = filterSearch ?? activeSearch;
  const mode = threadModeOf(node);
  const threadMode = mode !== "off";
  const threads = useThreads(folderId, pageSize, threadMode && searching === null);
  const threadRows = threadMode && searching === null && !isUnsupported(threads.error);
  const messages = useMessages(searching === null && !threadRows ? folderId : null, pageSize);
  const hits = useSearch(searching, pageSize);
  const query = searching ? hits : threadRows ? threads : messages;

  const rows = useMemo((): Row[] => {
    if (searching) {
      return (hits.data?.pages ?? []).flatMap((p) => p.hits.map((h) => messageRow(h.message)));
    }
    if (threadRows) {
      return (threads.data?.pages ?? []).flatMap((p) => p.threads.map(threadRow));
    }
    return (messages.data?.pages ?? []).flatMap((p) => p.messages.map(messageRow));
  }, [searching, threadRows, hits.data, threads.data, messages.data]);

  const searchError = hits.data?.pages.find((p) => p.error !== null)?.error ?? null;
  const total = searching ? (hits.data?.pages[0]?.total ?? 0) : null;
  const filtered = filterSearch !== null;
  // Design B: conversation rows open into their messages (not while
  // searching: hits are messages).
  const expandable = mode === "expandable" && threadRows;
  return { rows, query, activeSearch, searchError, total, filtered, listFilter, expandable };
}

const NO_KEYS: ReadonlySet<string> = new Set();

interface Expansion {
  scope: string | null;
  keys: ReadonlySet<string>;
}

/**
 * The open conversations per UI state and list (`node.id`), so a list that
 * remounts — the layout switching breakpoints, a view coming back — keeps
 * them open.
 */
const remembered = new WeakMap<object, Map<string, Expansion>>();

/**
 * Design B: the expanded conversations of the active folder and their
 * messages (`GetConversation`, the conversation as the folder lists it).
 * Opening another folder starts with every conversation closed.
 */
function useExpansion(listKey: string, folderId: string | null, enabled: boolean) {
  const client = useClient();
  const store = useUiStoreApi();
  const memory = useMemo(() => {
    let map = remembered.get(store);
    if (!map) {
      map = new Map();
      remembered.set(store, map);
    }
    return map;
  }, [store]);
  const [state, setState] = useState<Expansion>(
    () => memory.get(listKey) ?? { scope: folderId, keys: NO_KEYS },
  );
  useEffect(() => {
    memory.set(listKey, state);
  }, [memory, listKey, state]);
  const expanded = enabled && state.scope === folderId ? state.keys : NO_KEYS;
  const setExpanded = useCallback(
    (update: (keys: ReadonlySet<string>) => ReadonlySet<string>) => {
      setState((old) => ({
        scope: folderId,
        keys: update(old.scope === folderId ? old.keys : NO_KEYS),
      }));
    },
    [folderId],
  );
  const ids = [...expanded];
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: queryKeys.conversation(id, folderId),
      queryFn: () =>
        client.request(
          { type: "GetConversation", thread_id: id, folder_id: folderId },
          "Conversation",
        ),
    })),
  });
  const children = new Map<string, readonly ConversationMessage[]>();
  ids.forEach((id, index) => {
    const messages = results[index]?.data?.messages;
    if (messages) children.set(id, messages);
  });
  const loading = results.some((r) => r.isPending);
  return { expanded, setExpanded, children, loading };
}

function SegmentView({ segment }: { segment: Segment }) {
  const t = useT();
  switch (segment.kind) {
    case "text":
      return <span data-part="message-list.row.text">{segment.text}</span>;
    case "field":
      return (
        <span
          data-part={`message-list.row.${segment.field}`}
          data-emphasis={segment.strong ? "strong" : undefined}
        >
          {segment.text}
          {segment.suffix !== undefined && (
            <span data-part="message-list.row.suffix">{segment.suffix}</span>
          )}
        </span>
      );
    case "chips":
      return (
        <span data-part={`message-list.row.${segment.field}`}>
          {segment.items.map((item, index) => (
            <span key={index} data-part="message-list.row.chip">
              {item}
            </span>
          ))}
        </span>
      );
    case "names":
      // P2: `{participants | names(N)}`; the skipped people sit between the
      // first names and the newest one.
      return (
        <span data-part={`message-list.row.${segment.field}`}>
          {segment.items.map((item, index) => {
            const states = [item.me ? "me" : null, item.unread ? "unread" : null].filter(
              (state) => state !== null,
            );
            const gap = segment.skipped > 0 && index === segment.items.length - 1;
            return (
              <span key={index} data-part="message-list.row.participant-group">
                {index > 0 && (
                  <span data-part="message-list.row.separator">
                    {t(gap ? "conversations-names-gap" : "conversations-names-separator")}
                  </span>
                )}
                <span
                  data-part="message-list.row.participant"
                  data-state={states.length > 0 ? states.join(" ") : undefined}
                >
                  {item.text}
                </span>
              </span>
            );
          })}
          {segment.suffix !== undefined && (
            <span data-part="message-list.row.suffix">{segment.suffix}</span>
          )}
        </span>
      );
  }
}

/** The field parts a row template shows (`data-fields` of its line). */
export function templateFields(template: CompiledTemplate): string | undefined {
  const fields = template.nodes.flatMap((n) => (n.kind === "field" ? [fieldPart(n.path)] : []));
  return fields.length > 0 ? [...new Set(fields)].join(" ") : undefined;
}

/** The virtualized message list of the active folder, or of the active search. */
export function MessageList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const now = useNow();
  const geom = useId();
  const folderId = useUi((s) => s.activeFolderId);
  const selectedId = useUi((s) => s.selectedMessageId);
  const selectMessage = useUi((s) => s.selectMessage);
  const selectThread = useUi((s) => s.selectThread);
  const setMessageOrder = useUi((s) => s.setMessageOrder);
  const setSearch = useUi((s) => s.setSearch);
  const pageSize = Math.max(1, Math.min(1000, optionNumber(node, "page-size", 100)));
  const {
    rows: baseRows,
    query,
    activeSearch,
    searchError,
    total,
    filtered,
    listFilter,
    expandable,
  } = useRows(node, pageSize);
  const { expanded, setExpanded, children, loading } = useExpansion(
    node.id ?? "message-list",
    folderId,
    expandable,
  );
  const rows = useMemo(
    () =>
      expandable
        ? visibleRows(baseRows, expanded, (row) =>
            children.get(row.key)?.map((m) => childRow(row, m)),
          )
        : baseRows,
    [expandable, baseRows, expanded, children],
  );
  // The row the user chose (a conversation and its newest child select the
  // same message; Design B).
  const [chosenKey, setChosenKey] = useState<string | null>(null);
  const layout = useContext(LayoutContext);
  const templates = useMemo(() => compileRows(node.rows), [node.rows]);
  // The fields of each line (`data-fields`: Compact hides the preview line).
  const lineFields = useMemo(() => templates.map(templateFields), [templates]);
  const columnKey = columnsOf(node).join(" ");
  const columns = useMemo(
    () =>
      columnKey === ""
        ? []
        : columnKey.split(" ").map((c) => ({
            id: c as ColumnId,
            template: compileTemplate(COLUMN_TEMPLATES[c as ColumnId]),
          })),
    [columnKey],
  );
  const scrollRef = useRef<HTMLDivElement>(null);
  const accountList = useAccounts().data;
  const accountNames = useMemo(() => accountTags(accountList ?? []), [accountList]);

  const order = useMemo(() => messageOrderOf(rows), [rows]);
  useEffect(() => {
    setMessageOrder(order);
  }, [order, setMessageOrder]);

  // Report a query the core rejected to the search bar (its positions refer
  // to the typed text only without a filter term).
  useEffect(() => {
    if (activeSearch && !filtered && searchError && activeSearch.error !== searchError) {
      setSearch({ ...activeSearch, error: searchError });
    }
  }, [activeSearch, filtered, searchError, setSearch]);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns non-memoizable functions by design.
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    // Row lines, the tag line and the row padding (the virtualizer measures
    // the real height afterwards).
    estimateSize: (index) =>
      rows[index]?.kind === "child"
        ? 20 + 12
        : 20 * (columns.length > 0 ? 1 : templates.length + 1) + 24,
    overscan: LIST_OVERSCAN,
    measureElement: exactRowSize,
    getItemKey: (index) => rows[index]?.key ?? index,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0];
  const last = items[items.length - 1];
  useGeometry(`${geom}-before`, { height: `${String(first?.start ?? 0)}px` });
  useGeometry(`${geom}-after`, {
    height: `${String(Math.max(0, virtualizer.getTotalSize() - (last?.end ?? 0)))}px`,
  });

  // Load the next page one rendered window before the end comes into view
  // (M4c: with only a few rows left, fast scrolling stalled at every page
  // end).
  const lastIndex = last?.index ?? -1;
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query;
  useEffect(() => {
    if (
      hasNextPage &&
      !isFetchingNextPage &&
      lastIndex >= rows.length - Math.max(5, items.length - LIST_OVERSCAN)
    ) {
      void fetchNextPage();
    }
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, lastIndex, rows.length, items.length]);

  // Keep the selection visible (keyboard navigation).
  const selectedIndex = selectedIndexOf(rows, chosenKey, selectedId);
  useEffect(() => {
    if (selectedIndex >= 0) virtualizer.scrollToIndex(selectedIndex, { align: "auto" });
  }, [selectedIndex, virtualizer]);

  // Keep the selected message's thread in the UI state (thread view).
  const selectedThread = selectedIndex >= 0 ? (rows[selectedIndex]?.threadId ?? null) : undefined;
  useEffect(() => {
    if (selectedThread !== undefined) selectThread(selectedThread);
  }, [selectedThread, selectThread]);

  const choose = useCallback(
    (row: Row | undefined) => {
      if (!row) return;
      setChosenKey(row.key);
      selectMessage(row.messageId);
      selectThread(row.threadId);
    },
    [selectMessage, selectThread],
  );

  // Design B: → / ← (the actions `list.expand` / `list.collapse`).
  const move = useCallback(
    (step: TreeMove) => {
      switch (step.kind) {
        case "expand":
          setExpanded((keys) => toggled(keys, step.key, true));
          break;
        case "collapse":
          setExpanded((keys) => toggled(keys, step.key, false));
          // A child row disappears with its conversation: keep the selection
          // on the conversation.
          if (rows[selectedIndex]?.parentKey === step.key) {
            choose(rows.find((r) => r.key === step.key));
          }
          break;
        case "select":
          choose(rows[step.index]);
          break;
        case "none":
          break;
      }
    },
    [setExpanded, rows, selectedIndex, choose],
  );
  const toggle = (row: Row) => {
    move(
      expanded.has(row.key) ? { kind: "collapse", key: row.key } : { kind: "expand", key: row.key },
    );
  };
  const controlState = useRef({ rows, selectedIndex, expanded, move });
  useEffect(() => {
    controlState.current = { rows, selectedIndex, expanded, move };
  });
  useEffect(() => {
    if (!expandable) return undefined;
    return registerListControl({
      element: () => scrollRef.current,
      expand: () => {
        const s = controlState.current;
        s.move(expandMove(s.rows, s.selectedIndex, s.expanded));
      },
      collapse: () => {
        const s = controlState.current;
        s.move(collapseMove(s.rows, s.selectedIndex, s.expanded));
      },
    });
  }, [expandable]);

  // Design B: j / k (or what the list context binds to mail.next /
  // mail.previous) step through the rows, children included.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!expandable || event.defaultPrevented || event.target !== event.currentTarget) return;
    const combo = comboFromEvent(event.nativeEvent);
    const delta = combo === null ? null : listStep(layout?.keys ?? [], combo);
    if (delta === null) return;
    event.preventDefault();
    choose(rows[stepIndex(rows.length, selectedIndex, delta)]);
  };

  const rowId = (index: number) => `${geom}-row-${String(index)}`;
  let status: ReactNode = null;
  if (activeSearch && searchError) {
    status = <div data-part="message-list.empty">{t("message-list-search-invalid")}</div>;
  } else if (!activeSearch && folderId === null) {
    status = <div data-part="message-list.empty">{t("message-list-empty")}</div>;
  } else if (!activeSearch && filtered && query.isSuccess && rows.length === 0) {
    status = (
      <div data-part="message-list.empty" data-filter={listFilter}>
        {t("message-list-filter-empty")}
      </div>
    );
  } else if (!activeSearch && query.isSuccess && rows.length === 0) {
    status = <div data-part="message-list.empty">{t("message-list-empty")}</div>;
  } else if (activeSearch && query.isSuccess && rows.length === 0) {
    status = <div data-part="message-list.empty">{t("message-list-search-empty")}</div>;
  } else if (query.isError) {
    status = <div data-part="message-list.error">{t(errorMessageIdOf(query.error))}</div>;
  } else if (query.isPending) {
    status = <div data-part="message-list.loading">{t("message-list-loading")}</div>;
  }

  return (
    <div
      ref={scrollRef}
      data-part="message-list"
      data-key-context="list"
      data-node-id={node.id ?? undefined}
      data-state={activeSearch ? "search" : undefined}
      data-filter={filtered ? listFilter : undefined}
      role={expandable ? "tree" : "listbox"}
      tabIndex={0}
      onKeyDown={onKeyDown}
      aria-label={node.label ? label(node.label) : t("message-list-label")}
      aria-activedescendant={
        selectedIndex >= 0 && items.some((i) => i.index === selectedIndex)
          ? rowId(selectedIndex)
          : undefined
      }
      aria-busy={query.isFetching || loading}
    >
      {activeSearch && total !== null && !searchError && query.isSuccess && (
        <div data-part="message-list.search-summary">
          {t("message-list-search-results", { count: total, query: activeSearch.text })}
        </div>
      )}
      {status}
      {columns.length > 0 && (
        <div data-part="message-list.header" aria-hidden="true">
          {columns.map((c) => (
            <span key={c.id} data-part="message-list.header.cell" data-column={c.id}>
              {t(`message-list-column-${c.id}`)}
            </span>
          ))}
        </div>
      )}
      <div data-part="message-list.spacer" data-geom={`${geom}-before`} aria-hidden="true" />
      {items.map((item) => {
        const row = rows[item.index];
        if (!row) return null;
        const selected = item.index === selectedIndex;
        const ctx = { locale, now, unread: row.unread, me: t("conversations-me") };
        if (row.child) {
          const position = siblingPosition(rows, item.index);
          return (
            <ChildRowView
              key={item.key}
              id={rowId(item.index)}
              index={item.index}
              measure={virtualizer.measureElement}
              message={row.child}
              parentKey={row.parentKey}
              selected={selected}
              states={row.states(selected)}
              posinset={position?.posinset ?? 1}
              setsize={position?.setsize ?? 1}
              locale={locale}
              now={now}
              onChoose={() => {
                choose(row);
                scrollRef.current?.focus({ preventScroll: true });
              }}
            />
          );
        }
        const isOpen = expandable && row.expandable && expanded.has(row.key);
        return (
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the list box handles the keyboard and the focus for its rows (aria-activedescendant).
          <div
            key={item.key}
            id={rowId(item.index)}
            ref={virtualizer.measureElement}
            data-index={item.index}
            role={expandable ? "treeitem" : "option"}
            aria-selected={selected}
            {...(expandable
              ? { "aria-level": 1, ...(row.expandable ? { "aria-expanded": isOpen } : {}) }
              : {})}
            data-part="message-list.row"
            data-view={columns.length > 0 ? "columns" : "rows"}
            data-state={
              [row.states(selected), isOpen ? "expanded" : null].filter(Boolean).join(" ") ||
              undefined
            }
            data-account={row.accountId}
            data-message={row.messageId}
            data-thread={row.threadId ?? undefined}
            onClick={() => {
              choose(row);
              scrollRef.current?.focus({ preventScroll: true });
            }}
          >
            {columns.length > 0 ? (
              <div data-part="message-list.row.columns">
                {columns.map((c) => (
                  <span key={c.id} data-part="message-list.row.cell" data-column={c.id}>
                    {renderTemplate(c.template, row.data, ctx).map((segment, index) => (
                      <SegmentView key={index} segment={segment} />
                    ))}
                  </span>
                ))}
              </div>
            ) : (
              <>
                {row.unread && <span data-part="message-list.row.unread-dot" aria-hidden="true" />}
                <Avatar
                  part="message-list.row.avatar"
                  name={row.from?.name ?? null}
                  email={row.from?.email ?? ""}
                />
                <div data-part="message-list.row.body">
                  {templates.map((template, line) => (
                    <div
                      key={line}
                      data-part="message-list.row.line"
                      data-fields={lineFields[line]}
                    >
                      {renderTemplate(template, row.data, ctx).map((segment, index) => (
                        <SegmentView key={index} segment={segment} />
                      ))}
                    </div>
                  ))}
                  <TagLine tags={row.tags} accounts={accountNames} />
                </div>
                {expandable && (
                  // The list's keyboard (→ / ←) opens and closes the
                  // conversation; the chevron is its pointer twin, so it is
                  // no focus stop of its own inside the tree item (hidden
                  // from assistive technology: the item says aria-expanded).
                  <span
                    data-part="message-list.row.expander"
                    data-state={row.expandable ? (isOpen ? "expanded" : undefined) : "single"}
                    title={
                      row.expandable
                        ? t(isOpen ? "conversations-collapse" : "conversations-expand")
                        : undefined
                    }
                    aria-hidden="true"
                    onClick={(event) => {
                      if (!row.expandable) return;
                      event.stopPropagation();
                      toggle(row);
                      scrollRef.current?.focus({ preventScroll: true });
                    }}
                  >
                    {row.expandable && (
                      <Icon name="chevron-right" part="message-list.row.expander.icon" />
                    )}
                  </span>
                )}
              </>
            )}
          </div>
        );
      })}
      <div data-part="message-list.spacer" data-geom={`${geom}-after`} aria-hidden="true" />
    </div>
  );
}
