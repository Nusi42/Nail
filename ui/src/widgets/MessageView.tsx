import { useEffect, useLayoutEffect, useMemo, useRef, type ReactNode } from "react";
import { errorMessageIdOf } from "../api/client";
import { AttachmentWarningMark } from "../features/security";
import { MessageLabels } from "../features/gmail";
import { InvitationCard } from "../features/scheduling";
import type { Address, MessageBody } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { optionBool, optionList, type WidgetProps } from "../layout/widgetRegistry";
import { useComposeContext } from "../state/hooks";
import { findSummary, setFlags } from "../state/mailOps";
import { useMessage } from "../state/queries";
import { useUi } from "../state/uiStore";
import { useReaderCss } from "../theme/ThemeProvider";
import { proxyBase, readerDocument } from "../theme/reader";
import { setSrcdoc } from "../theme/trustedHtml";
import { FULL_DATE_TIME, formatDateTime } from "../i18n/format";

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

function formatSize(bytes: number, locale: string): string {
  const kb = bytes / 1024;
  return kb < 1024
    ? new Intl.NumberFormat(locale, {
        style: "unit",
        unit: "kilobyte",
        maximumFractionDigits: 0,
      }).format(Math.max(1, kb))
    : new Intl.NumberFormat(locale, {
        style: "unit",
        unit: "megabyte",
        maximumFractionDigits: 1,
      }).format(kb / 1024);
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
 * `srcdoc` (set through the Trusted Types policy).
 */
function Body({ message }: { message: MessageBody }) {
  const t = useT();
  const css = useReaderCss();
  const html = message.sanitized_html;
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
  if (html !== null && renderUrl !== null) {
    return (
      <iframe
        data-part="message-view.body"
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
        sandbox=""
        referrerPolicy="no-referrer"
        title={title}
      />
    );
  }
  return <pre data-part="message-view.text">{message.text}</pre>;
}

/** Label chips / category of a Gmail message (from the cached summary). */
function summaryOf(ctx: ReturnType<typeof useComposeContext>, id: MessageBody["id"]): ReactNode {
  const summary = findSummary(ctx.queryClient, id);
  return summary?.gmail ? <MessageLabels message={summary} /> : null;
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
  const message = query.data;

  // Reading a message marks it as read (a queued write, like any other).
  const loadedId = message?.id ?? null;
  useEffect(() => {
    if (!markRead || loadedId === null) return;
    const summary = findSummary(ctx.queryClient, loadedId);
    if (summary && !summary.flags.seen) {
      setFlags(ctx, [loadedId], { seen: true }, { quiet: true }).catch(() => {});
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
    content = (
      <>
        <header data-part="message-view.header">
          <h1 data-part="message-view.header.subject">
            {message.headers.subject || t("message-no-subject")}
          </h1>
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
        </header>
        {summaryOf(ctx, message.id)}
        <InvitationCard messageId={message.id} invitation={message.invitation ?? null} />
        <Body message={message} />
        {message.attachments.length > 0 && (
          <ul data-part="message-view.attachments" aria-label={t("message-view-attachments")}>
            {message.attachments.map((a) => (
              <li
                key={a.index}
                data-part="message-view.attachment"
                data-mime={a.mime}
                data-inline={a.inline ? "" : undefined}
              >
                <span data-part="message-view.attachment.name">
                  {a.filename ?? t("message-view-attachment-unnamed")}
                </span>
                <span data-part="message-view.attachment.size">{formatSize(a.size, locale)}</span>
                <AttachmentWarningMark warnings={message.warnings} index={a.index} />
              </li>
            ))}
          </ul>
        )}
      </>
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
