import { useEffect, useId, useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { errorMessageIdOf } from "../api/client";
import { FocusMoveButton, MessageCategories } from "../features/exchange";
import { MessageLabels } from "../features/gmail";
import { AttachmentPreviewPane, PluginBadges, PluginBanners } from "../features/plugins";
import { InvitationCard } from "../features/scheduling";
import type { Address, MessageBody } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { optionBool, optionList, type WidgetProps } from "../layout/widgetRegistry";
import { useComposeContext } from "../state/hooks";
import { findSummary, setFlags } from "../state/mailOps";
import { useMessage } from "../state/queries";
import { useUi } from "../state/uiStore";
import { useReaderCss } from "../theme/ThemeProvider";
import { isDesignedMail, proxyBase, readerDocument } from "../theme/reader";
import { setSrcdoc } from "../theme/trustedHtml";
import { FULL_DATE_TIME, formatDateTime } from "../i18n/format";
import { AttachmentCard } from "./AttachmentCard";
import { splitQuotes } from "./ThreadViewModel";
import { cardShowsSubject, useReaderCard } from "./ThreadViewScope";

/** Header fields the widget can show (`headers` option). */
export const HEADER_FIELDS = ["from", "to", "cc", "reply-to", "date"] as const;
type HeaderField = (typeof HEADER_FIELDS)[number];

function isHeaderField(value: string): value is HeaderField {
  return (HEADER_FIELDS as readonly string[]).includes(value);
}

function formatAddresses(addresses: readonly Address[]): string {
  return addresses.map((a) => (a.name ? `${a.name} <${a.email}>` : a.email)).join(", ");
}

function headerValue(message: MessageBody, field: HeaderField, locale: string): string {
  const h = message.headers;
  switch (field) {
    case "from":
      return formatAddresses(h.from);
    case "to":
      return formatAddresses(h.to);
    case "cc":
      return formatAddresses(h.cc);
    case "reply-to":
      return formatAddresses(h.reply_to);
    case "date": {
      if (!h.date) return "";
      return formatDateTime(h.date, locale, FULL_DATE_TIME);
    }
  }
}

/** The address of the message document on the remote UI's sandbox origin. */
function renderSrc(renderUrl: string, css: string): string {
  const url = new URL(renderUrl);
  url.searchParams.set("css", css);
  return url.toString();
}

/**
 * The HTML body in a sandboxed frame without scripts: from the remote UI's
 * separate sandbox origin when the core names one (`render_url`), else as
 * `srcdoc` (set through the Trusted Types policy). The frame fills the rest
 * of the reader and scrolls the mail itself (it cannot report its content
 * height without scripts); mail with its own design (`data-state="designed"`)
 * gets a wider column than the reading width.
 */
function Body({ message, quotesFold }: { message: MessageBody; quotesFold: boolean }) {
  const t = useT();
  // The reader's form (inline images embedded as data: URLs) when the core sends one.
  const html = message.reader_html ?? message.sanitized_html;
  const designed = html !== null && isDesignedMail(html);
  const css = useReaderCss(designed);
  const renderUrl = message.render_url ?? null;
  const srcDoc = useMemo(
    () => (html === null || renderUrl !== null ? null : readerDocument(html, css, proxyBase())),
    [html, css, renderUrl],
  );
  const frame = useRef<HTMLIFrameElement>(null);
  useLayoutEffect(() => {
    if (frame.current && srcDoc !== null) setSrcdoc(frame.current, srcDoc);
  }, [srcDoc]);
  const title = t("message-view-body-title", { subject: message.headers.subject });
  const state = designed ? "designed" : undefined;
  if (html !== null && renderUrl !== null) {
    return (
      <iframe
        data-part="message-view.body"
        data-state={state}
        sandbox=""
        src={renderSrc(renderUrl, css)}
        referrerPolicy="no-referrer"
        title={title}
      />
    );
  }
  if (srcDoc !== null) {
    return (
      <iframe
        ref={frame}
        data-part="message-view.body"
        data-state={state}
        sandbox=""
        referrerPolicy="no-referrer"
        title={title}
      />
    );
  }
  if (!quotesFold) return <pre data-part="message-view.text">{message.text}</pre>;
  // In a card of the conversation feed (P2): quoted lines fold away — the
  // feed shows the earlier messages themselves.
  return (
    <div data-part="message-view.text">
      {splitQuotes(message.text).map((block, index) =>
        block.quoted ? (
          <details key={index} data-part="message-view.quote-block">
            <summary data-part="message-view.quote-toggle">{t("thread-view-show-quote")}</summary>
            <pre data-part="message-view.quote">{block.text}</pre>
          </details>
        ) : (
          <pre key={index} data-part="message-view.text-block">
            {block.text}
          </pre>
        ),
      )}
    </div>
  );
}

/**
 * Label chips / category of a Gmail message, or the category chips and the
 * Focused / Other move of an Exchange message (from the cached summary).
 */
function summaryOf(ctx: ReturnType<typeof useComposeContext>, id: MessageBody["id"]): ReactNode {
  const summary = findSummary(ctx.queryClient, id);
  if (summary?.gmail) return <MessageLabels message={summary} />;
  if (summary?.exchange) {
    return (
      <>
        <MessageCategories message={summary} />
        <FocusMoveButton message={summary} />
      </>
    );
  }
  return null;
}

/** Headers and body of the selected message. */
export function MessageView({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const selectedId = useUi((s) => s.selectedMessageId);
  const query = useMessage(selectedId);
  const fields = optionList(node, "headers", ["from", "to", "cc", "date"]).filter(isHeaderField);
  const markRead = optionBool(node, "mark-read", true);
  const ctx = useComposeContext();
  const ids = useId();
  const message = query.data;
  // Inside a card of the conversation feed (P2): the feed's header shows the
  // subject and the card's header the sender, so the card shows neither —
  // unless the message's subject says something the heading does not (the
  // real subject of an encrypted message; P2 integration).
  const card = useReaderCard();
  const inCard = card !== null;
  const accountId = message ? (findSummary(ctx.queryClient, message.id)?.account_id ?? null) : null;

  // Reading a message marks it as read (a queued write, like any other).
  const loadedId = message?.id ?? null;
  useEffect(() => {
    if (!markRead || loadedId === null) return;
    const summary = findSummary(ctx.queryClient, loadedId);
    if (summary && !summary.flags.seen) {
      // Quiet: a failed mark-as-read is retried the next time the message is read.
      setFlags(ctx, [loadedId], { seen: true }, { quiet: true }).catch(() => null);
    }
  }, [markRead, loadedId, ctx]);

  let content: ReactNode;
  if (selectedId === null) {
    content = <div data-part="message-view.empty">{t("message-view-empty")}</div>;
  } else if (query.isError) {
    content = <div data-part="message-view.error">{t(errorMessageIdOf(query.error))}</div>;
  } else if (!message) {
    content = <div data-part="message-view.loading">{t("message-view-loading")}</div>;
  } else {
    const previewId = (index: number) => `${ids}-preview-${String(index)}`;
    content = (
      // One centred column (M5a): header, attachments and body share their
      // left edge; text lines stay at a readable length.
      <div data-part="message-view.column">
        <header data-part="message-view.header">
          {(card === null || cardShowsSubject(message.headers.subject, card)) && (
            <h1 data-part="message-view.header.subject">
              {message.headers.subject || t("message-no-subject")}
            </h1>
          )}
          {fields.map((field) => {
            const value = headerValue(message, field, locale);
            if (!value) return null;
            return (
              <div key={field} data-part="message-view.header.field" data-field={field}>
                <span data-part="message-view.header.label">{t(`message-header-${field}`)}</span>
                <span data-part="message-view.header.value">{value}</span>
              </div>
            );
          })}
          <PluginBadges messageId={message.id} />
        </header>
        {summaryOf(ctx, message.id)}
        <InvitationCard messageId={message.id} invitation={message.invitation ?? null} />
        <PluginBanners messageId={message.id} />
        {message.attachments.length > 0 && (
          <ul data-part="message-view.attachments" aria-label={t("message-view-attachments")}>
            {message.attachments.map((a) => (
              <AttachmentCard
                key={a.index}
                messageId={message.id}
                attachment={a}
                accountId={accountId}
                warnings={message.warnings ?? []}
                previewId={previewId(a.index)}
              />
            ))}
          </ul>
        )}
        <Body message={message} quotesFold={inCard} />
        {message.attachments
          .filter((a) => !a.inline)
          .map((a) => (
            <AttachmentPreviewPane
              key={a.index}
              id={previewId(a.index)}
              messageId={message.id}
              attachment={a}
              accountId={accountId}
            />
          ))}
      </div>
    );
  }

  if (inCard) {
    // The card is the message's article: no landmark or label of its own.
    return (
      <div
        data-part="message-view"
        data-state="card"
        data-key-context="reader"
        data-message={message?.id}
        aria-busy={query.isFetching}
      >
        {content}
      </div>
    );
  }

  return (
    <article
      data-part="message-view"
      data-key-context="reader"
      data-node-id={node.id ?? undefined}
      data-message={message?.id}
      tabIndex={-1}
      aria-label={node.label ? label(node.label) : t("message-view-label")}
      aria-busy={query.isFetching}
    >
      {content}
    </article>
  );
}
