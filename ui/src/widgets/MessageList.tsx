import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useId, useMemo, useRef, type ReactNode } from "react";
import { errorMessageIdOf } from "../api/client";
import type { MessageSummary } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { optionNumber, type WidgetProps } from "../layout/widgetRegistry";
import { useNow } from "../state/clock";
import { useMessages } from "../state/queries";
import { useUi } from "../state/uiStore";
import {
  compileTemplate,
  renderTemplate,
  type CompiledTemplate,
  type Segment,
} from "../template/template";
import { useGeometry } from "../theme/ThemeProvider";

/** Row templates used when the layout gives none (or only invalid ones). */
export const DEFAULT_ROWS = ["{from.name | bold-if-unread}  {date | relative}", "{subject}"];

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
  return states.length > 0 ? states.join(" ") : undefined;
}

/** Template data of a message: the summary plus derived fields. */
function templateData(message: MessageSummary) {
  return {
    ...message,
    unread: !message.flags.seen,
    flagged: message.flags.flagged,
    account: message.account_id,
  };
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

/** The virtualized message list of the active folder. */
export function MessageList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const now = useNow();
  const geom = useId();
  const folderId = useUi((s) => s.activeFolderId);
  const selectedId = useUi((s) => s.selectedMessageId);
  const selectMessage = useUi((s) => s.selectMessage);
  const setMessageOrder = useUi((s) => s.setMessageOrder);
  const pageSize = Math.max(1, Math.min(1000, optionNumber(node, "page-size", 100)));
  const query = useMessages(folderId, pageSize);
  const templates = useMemo(() => compileRows(node.rows), [node.rows]);
  const scrollRef = useRef<HTMLDivElement>(null);

  const messages = useMemo(() => query.data?.pages.flatMap((p) => p.messages) ?? [], [query.data]);

  useEffect(() => {
    setMessageOrder(messages.map((m) => m.id));
  }, [messages, setMessageOrder]);

  // eslint-disable-next-line react-hooks/incompatible-library -- TanStack Virtual returns non-memoizable functions by design.
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 20 * templates.length + 14,
    overscan: 8,
    getItemKey: (index) => messages[index]?.id ?? index,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0];
  const last = items[items.length - 1];
  useGeometry(`${geom}-before`, { height: `${first?.start ?? 0}px` });
  useGeometry(`${geom}-after`, {
    height: `${Math.max(0, virtualizer.getTotalSize() - (last?.end ?? 0))}px`,
  });

  // Load the next page when the end comes into view.
  const lastIndex = last?.index ?? -1;
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = query;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage && lastIndex >= messages.length - 5) {
      void fetchNextPage();
    }
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, lastIndex, messages.length]);

  // Keep the selection visible (keyboard navigation).
  const selectedIndex = selectedId === null ? -1 : messages.findIndex((m) => m.id === selectedId);
  useEffect(() => {
    if (selectedIndex >= 0) virtualizer.scrollToIndex(selectedIndex, { align: "auto" });
  }, [selectedIndex, virtualizer]);

  const rowId = (index: number) => `${geom}-row-${index}`;
  let status: ReactNode = null;
  if (folderId === null || (query.isSuccess && messages.length === 0)) {
    status = <div data-part="message-list.empty">{t("message-list-empty")}</div>;
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
      {status}
      <div data-part="message-list.spacer" data-geom={`${geom}-before`} aria-hidden="true" />
      {items.map((item) => {
        const message = messages[item.index];
        if (!message) return null;
        const selected = message.id === selectedId;
        const ctx = { locale, now, unread: !message.flags.seen };
        const data = templateData(message);
        return (
          <div
            key={item.key}
            id={rowId(item.index)}
            ref={virtualizer.measureElement}
            data-index={item.index}
            role="option"
            aria-selected={selected}
            data-part="message-list.row"
            data-state={rowStates(message, selected)}
            data-account={message.account_id}
            data-message={message.id}
            onClick={() => {
              selectMessage(message.id);
              scrollRef.current?.focus({ preventScroll: true });
            }}
          >
            {templates.map((template, line) => (
              <div key={line} data-part="message-list.row.line">
                {renderTemplate(template, data, ctx).map((segment, index) => (
                  <SegmentView key={index} segment={segment} />
                ))}
              </div>
            ))}
          </div>
        );
      })}
      <div data-part="message-list.spacer" data-geom={`${geom}-after`} aria-hidden="true" />
    </div>
  );
}
