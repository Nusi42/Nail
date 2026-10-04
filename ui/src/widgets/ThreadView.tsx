import { useState } from "react";
import { errorMessageIdOf } from "../api/client";
import type { MessageSummary } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { optionBool, optionString, type WidgetProps } from "../layout/widgetRegistry";
import { displayName } from "../state/addresses";
import { useNow } from "../state/clock";
import { isUnsupported, useMessage, useThread } from "../state/queries";
import { useUi } from "../state/uiStore";
import { formatRelative } from "../template/template";

/** Which messages of a conversation start expanded (`expand` option). */
export type ExpandMode = "latest" | "unread" | "all" | "none";

export function initiallyExpanded(
  mode: ExpandMode,
  message: MessageSummary,
  latestId: string,
): boolean {
  switch (mode) {
    case "all":
      return true;
    case "none":
      return false;
    case "unread":
      return !message.flags.seen || message.id === latestId;
    case "latest":
      return message.id === latestId;
  }
}

/** Splits text into runs of quoted (`>`) and own lines. */
export function splitQuotes(text: string): { quoted: boolean; text: string }[] {
  const blocks: { quoted: boolean; text: string }[] = [];
  for (const line of text.split("\n")) {
    const quoted = line.startsWith(">");
    const last = blocks[blocks.length - 1];
    if (last?.quoted === quoted) last.text += `\n${line}`;
    else blocks.push({ quoted, text: line });
  }
  return blocks;
}

function MessageText({ id }: { id: string }) {
  const t = useT();
  const query = useMessage(id);
  if (query.isError) {
    return <div data-part="thread-view.error">{t(errorMessageIdOf(query.error))}</div>;
  }
  if (!query.data) return <div data-part="thread-view.loading">{t("message-view-loading")}</div>;
  return (
    <div data-part="thread-view.text">
      {splitQuotes(query.data.text).map((block, index) =>
        block.quoted ? (
          <details key={index} data-part="thread-view.quote-block">
            <summary data-part="thread-view.quote-toggle">{t("thread-view-show-quote")}</summary>
            <pre data-part="thread-view.quote">{block.text}</pre>
          </details>
        ) : (
          <pre key={index} data-part="thread-view.body">
            {block.text}
          </pre>
        ),
      )}
    </div>
  );
}

/**
 * The conversation of the selected message: every message of its thread,
 * oldest first, each expandable in place with quoted text collapsed.
 * Choosing a message opens it in the message view.
 */
export function ThreadView({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const now = useNow();
  const threadId = useUi((s) => s.selectedThreadId);
  const selectedId = useUi((s) => s.selectedMessageId);
  const selectMessage = useUi((s) => s.selectMessage);
  const query = useThread(threadId);
  const hideSingle = optionBool(node, "hide-single", true);
  const mode = optionString(node, "expand", "latest") as ExpandMode;
  const [toggled, setToggled] = useState<Record<string, boolean>>({});

  const messages = query.data?.messages ?? [];
  // Nothing to show: no thread, the core cannot thread yet, or a lone message.
  if (
    threadId === null ||
    query.isPending ||
    isUnsupported(query.error) ||
    (query.isSuccess && hideSingle && messages.length < 2)
  ) {
    return null;
  }
  const latestId = messages[messages.length - 1]?.id ?? "";
  const isExpanded = (m: MessageSummary) => toggled[m.id] ?? initiallyExpanded(mode, m, latestId);

  return (
    <section
      data-part="thread-view"
      data-node-id={node.id ?? undefined}
      data-key-context="reader"
      aria-label={node.label ? label(node.label) : t("thread-view-label")}
      aria-busy={query.isFetching}
    >
      {query.isError && <div data-part="thread-view.error">{t(errorMessageIdOf(query.error))}</div>}
      {query.data && (
        <>
          <div data-part="thread-view.header">
            <span data-part="thread-view.subject">
              {query.data.thread.subject || t("message-no-subject")}
            </span>
            <span data-part="thread-view.count">
              {t("thread-view-count", { count: messages.length })}
            </span>
          </div>
          <ol data-part="thread-view.list">
            {messages.map((m) => {
              const expanded = isExpanded(m);
              const states = [
                m.id === selectedId ? "selected" : null,
                m.flags.seen ? null : "unread",
                m.flags.flagged ? "flagged" : null,
                expanded ? "expanded" : null,
              ].filter((s) => s !== null);
              const date = new Date(m.date);
              return (
                <li
                  key={m.id}
                  data-part="thread-view.item"
                  data-state={states.length > 0 ? states.join(" ") : undefined}
                  data-message={m.id}
                  data-account={m.account_id}
                  aria-current={m.id === selectedId ? "true" : undefined}
                >
                  <div data-part="thread-view.item.header">
                    <button
                      type="button"
                      data-part="thread-view.item.toggle"
                      aria-expanded={expanded}
                      onClick={() => {
                        setToggled((old) => ({ ...old, [m.id]: !expanded }));
                      }}
                    >
                      <span data-part="thread-view.item.from">
                        {displayName(m.from) || t("composer-unknown-sender")}
                      </span>
                      <span data-part="thread-view.item.date">
                        {Number.isNaN(date.getTime()) ? m.date : formatRelative(date, now, locale)}
                      </span>
                      {!expanded && <span data-part="thread-view.item.preview">{m.preview}</span>}
                    </button>
                    <button
                      type="button"
                      data-part="thread-view.item.open"
                      disabled={m.id === selectedId}
                      onClick={() => {
                        selectMessage(m.id);
                      }}
                    >
                      {t("thread-view-open")}
                    </button>
                  </div>
                  {expanded && <MessageText id={m.id} />}
                </li>
              );
            })}
          </ol>
        </>
      )}
    </section>
  );
}
