import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useMemo, useRef, type ReactNode } from "react";
import { errorMessageIdOf } from "../api/client";
import type { Address, MessageSummary, ThreadSummary } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import {
  optionBool,
  optionNumber,
  optionString,
  type WidgetNode,
  type WidgetProps,
} from "../layout/widgetRegistry";
import { useNow } from "../state/clock";
import { isUnsupported, useAccounts, useMessages, useSearch, useThreads } from "../state/queries";
import { useUi } from "../state/uiStore";
import {
  compileTemplate,
  renderTemplate,
  type CompiledTemplate,
  type Segment,
} from "../template/template";
import { useGeometry } from "../theme/ThemeProvider";
import { colorSlot } from "../theme/skin";
import { Avatar, Tag } from "./primitives";

/**
 * Rows rendered beyond the viewport on each side. M5a rows are taller
 * (avatar, tag line), so about a screen's worth stays rendered for keyboard
 * moves and find-in-page; paging ahead ignores these.
 */
const LIST_OVERSCAN = 12;

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
  };
}

/** Template data of a thread row: the fields a message row offers, plus `count`. */
export function threadTemplateData(thread: ThreadSummary) {
  return {
    ...thread,
    from: thread.participants[thread.participants.length - 1] ?? null,
    unread: thread.unread_count > 0,
    account: thread.account_ids.join(" "),
    count: thread.message_count,
  };
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

/** One row of the list: a message, or a thread represented by its newest message. */
interface Row {
  key: string;
  /** The message selected when the row is chosen. */
  messageId: string;
  threadId: string | null;
  accountId: string;
  states: (selected: boolean) => string | undefined;
  data: object;
  unread: boolean;
  /** The sender the avatar stands for. */
  from: Address | null;
  tags: RowTags;
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
    messageId: m.id,
    threadId: m.thread_id,
    accountId: m.account_id,
    states: (selected) => rowStates(m, selected),
    data: templateData(m),
    unread: !m.flags.seen,
    from: m.from,
    tags: messageTags(m),
  };
}

function threadRow(thread: ThreadSummary): Row {
  return {
    key: thread.id,
    messageId: thread.latest_message_id,
    threadId: thread.id,
    accountId: thread.account_ids.join(" "),
    states: (selected) => threadStates(thread, selected),
    data: threadTemplateData(thread),
    unread: thread.unread_count > 0,
    from: thread.participants[thread.participants.length - 1] ?? null,
    tags: {
      accountIds: thread.account_ids,
      attachments: thread.has_attachments,
      attachmentCount: null,
      invitation: false,
      warning: false,
      flagged: thread.flagged,
      labels: [],
    },
  };
}

/** Labels shown on the tag line at most (the rest stay in the reader). */
const MAX_LABELS = 3;

/**
 * The tag line: account colour (with the name when several accounts are
 * set up), attachments, invitation, warning, flag and labels. Nothing when
 * there is nothing to say.
 */
function TagLine({ tags, accounts }: { tags: RowTags; accounts: ReadonlyMap<string, string> }) {
  const t = useT();
  const items: ReactNode[] = [];
  if (accounts.size > 1) {
    for (const id of tags.accountIds) {
      items.push(
        <Tag
          key={`account-${id}`}
          kind="account"
          dot={colorSlot(id)}
          text={accounts.get(id) ?? id}
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
  const folderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const followSearch = optionBool(node, "follow-search", true);
  const activeSearch = followSearch && search !== null ? search : null;
  const threadMode = optionString(node, "threads", "off") === "collapsed";
  const threads = useThreads(folderId, pageSize, threadMode && activeSearch === null);
  const threadRows = threadMode && activeSearch === null && !isUnsupported(threads.error);
  const messages = useMessages(activeSearch === null && !threadRows ? folderId : null, pageSize);
  const hits = useSearch(activeSearch, pageSize);
  const query = activeSearch ? hits : threadRows ? threads : messages;

  const rows = useMemo((): Row[] => {
    if (activeSearch) {
      return (hits.data?.pages ?? []).flatMap((p) => p.hits.map((h) => messageRow(h.message)));
    }
    if (threadRows) {
      return (threads.data?.pages ?? []).flatMap((p) => p.threads.map(threadRow));
    }
    return (messages.data?.pages ?? []).flatMap((p) => p.messages.map(messageRow));
  }, [activeSearch, threadRows, hits.data, threads.data, messages.data]);

  const searchError = hits.data?.pages.find((p) => p.error !== null)?.error ?? null;
  const total = activeSearch ? (hits.data?.pages[0]?.total ?? 0) : null;
  return { rows, query, activeSearch, searchError, total };
}

function SegmentView({ segment }: { segment: Segment }) {
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
  }
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
  const { rows, query, activeSearch, searchError, total } = useRows(node, pageSize);
  const templates = useMemo(() => compileRows(node.rows), [node.rows]);
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
  const accountNames = useMemo(
    () => new Map((accountList ?? []).map((a) => [a.id, a.name || a.email])),
    [accountList],
  );

  useEffect(() => {
    setMessageOrder(rows.map((r) => r.messageId));
  }, [rows, setMessageOrder]);

  // Report a query the core rejected to the search bar.
  useEffect(() => {
    if (activeSearch && searchError && activeSearch.error !== searchError) {
      setSearch({ ...activeSearch, error: searchError });
    }
  }, [activeSearch, searchError, setSearch]);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns non-memoizable functions by design.
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    // Row lines, the tag line and the row padding (the virtualizer measures
    // the real height afterwards).
    estimateSize: () => 20 * (columns.length > 0 ? 1 : templates.length + 1) + 24,
    overscan: LIST_OVERSCAN,
    getItemKey: (index) => rows[index]?.key ?? index,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0];
  const last = items[items.length - 1];
  useGeometry(`${geom}-before`, { height: `${first?.start ?? 0}px` });
  useGeometry(`${geom}-after`, {
    height: `${Math.max(0, virtualizer.getTotalSize() - (last?.end ?? 0))}px`,
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
  const selectedIndex =
    selectedId === null ? -1 : rows.findIndex((r) => r.messageId === selectedId);
  useEffect(() => {
    if (selectedIndex >= 0) virtualizer.scrollToIndex(selectedIndex, { align: "auto" });
  }, [selectedIndex, virtualizer]);

  // Keep the selected message's thread in the UI state (thread view).
  const selectedThread = selectedIndex >= 0 ? (rows[selectedIndex]?.threadId ?? null) : undefined;
  useEffect(() => {
    if (selectedThread !== undefined) selectThread(selectedThread);
  }, [selectedThread, selectThread]);

  const rowId = (index: number) => `${geom}-row-${index}`;
  let status: ReactNode = null;
  if (activeSearch && searchError) {
    status = <div data-part="message-list.empty">{t("message-list-search-invalid")}</div>;
  } else if (!activeSearch && (folderId === null || (query.isSuccess && rows.length === 0))) {
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
      role="listbox"
      tabIndex={0}
      aria-label={node.label ? label(node.label) : t("message-list-label")}
      aria-activedescendant={
        selectedIndex >= 0 && items.some((i) => i.index === selectedIndex)
          ? rowId(selectedIndex)
          : undefined
      }
      aria-busy={query.isFetching}
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
        const selected = row.messageId === selectedId;
        const ctx = { locale, now, unread: row.unread };
        return (
          <div
            key={item.key}
            id={rowId(item.index)}
            ref={virtualizer.measureElement}
            data-index={item.index}
            role="option"
            aria-selected={selected}
            data-part="message-list.row"
            data-view={columns.length > 0 ? "columns" : "rows"}
            data-state={row.states(selected)}
            data-account={row.accountId}
            data-message={row.messageId}
            data-thread={row.threadId ?? undefined}
            onClick={() => {
              selectMessage(row.messageId);
              selectThread(row.threadId);
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
                    <div key={line} data-part="message-list.row.line">
                      {renderTemplate(template, row.data, ctx).map((segment, index) => (
                        <SegmentView key={index} segment={segment} />
                      ))}
                    </div>
                  ))}
                  <TagLine tags={row.tags} accounts={accountNames} />
                </div>
              </>
            )}
          </div>
        );
      })}
      <div data-part="message-list.spacer" data-geom={`${geom}-after`} aria-hidden="true" />
    </div>
  );
}
