/**
 * Following links in mail (ADR 0206; the frames' side is `mailLinks.ts`):
 *
 * - {@link useMailLinks}: the reader's link state. While a link is hovered or
 *   focused, a status overlay at the reader's bottom edge shows its target
 *   (shortened in the middle, the full value in the accessible text) over
 *   the content, so the layout never moves; a link the core flagged
 *   (`MessageWarning` with its number) shows the warning there too.
 * - A click asks the core (`OpenLink`): the desktop app opens web links with
 *   the system, web mode and remote sessions open them in a new tab
 *   (`noopener`, `noreferrer`), `mailto:` links open Nail's composer.
 * - A flagged link first asks in a modal confirmation
 *   ({@link MAIL_OPEN_LINK_DIALOG}): "Open anyway" or "Cancel".
 */
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { errorMessageOf } from "../api/client";
import type { MessageWarning } from "../api/generated";
import { startMailtoCompose } from "../features/composer";
import { mailActionsStoreFor } from "../features/mail-actions/store";
import { linkWarningText, warningsForLink } from "../features/security";
import { useT } from "../i18n/I18nProvider";
import type { DialogProps } from "../layout/dialogRegistry";
import type { ComposeContext } from "../state/compose";
import { useComposeContext } from "../state/hooks";
import type { UiStore } from "../state/uiStore";
import { shortenMiddle, type MailLink, type MailLinkReport } from "./mailLinks";

/** The confirmation's window kind (dialog name). */
export const MAIL_OPEN_LINK_DIALOG = "mail-open-link";

/** The new tab's window features (web mode, remote sessions). */
const NEW_TAB_FEATURES = "noopener,noreferrer";

/** A flagged link waiting for the user's answer. */
interface PendingLink {
  link: MailLink;
  warnings: readonly MessageWarning[];
}

/**
 * Pending links by the app's store. A card of the reader's feed has a scoped
 * view of that store (`scopedUiStore`) that shares its `setState`, so that
 * is the key: the card asks, the window (outside the card) answers.
 */
const pending = new WeakMap<UiStore["setState"], PendingLink>();

/** The link the open confirmation is about (`null` = none). */
export function pendingMailLink(store: UiStore): PendingLink | null {
  return pending.get(store.setState) ?? null;
}

/** Asks the core to open `url` and does what it answers; failures show in the toast. */
export async function openMailLink(ctx: ComposeContext, url: string): Promise<void> {
  try {
    const { outcome } = await ctx.client.request({ type: "OpenLink", url }, "LinkOpened");
    switch (outcome.type) {
      case "Opened":
        return;
      case "OpenInBrowser":
        window.open(outcome.url, "_blank", NEW_TAB_FEATURES);
        return;
      case "Compose":
        await startMailtoCompose(ctx, outcome.mailto);
        return;
    }
  } catch (error) {
    mailActionsStoreFor(ctx.store)
      .getState()
      .show({ kind: "error", message: errorMessageOf(error) });
  }
}

/**
 * Follows a clicked link: at once, or after the confirmation when the core
 * flagged it (`warnings` of its message).
 */
export async function followMailLink(
  ctx: ComposeContext,
  link: MailLink,
  warnings: readonly MessageWarning[] | undefined,
): Promise<void> {
  const flagged = warningsForLink(warnings, link.index);
  if (flagged.length === 0) {
    await openMailLink(ctx, link.href);
    return;
  }
  pending.set(ctx.store.setState, { link, warnings: flagged });
  ctx.store.getState().openWindow(MAIL_OPEN_LINK_DIALOG);
}

/** The warnings' sentences (each flagged kind once). */
function useWarningTexts(warnings: readonly MessageWarning[]): string[] {
  const t = useT();
  const texts = warnings.flatMap((w) =>
    w.kind === "dangerous-attachment" || w.kind === "lookalike-sender"
      ? []
      : [linkWarningText(t, w)],
  );
  return [...new Set(texts)];
}

/** The status overlay of the hovered or focused link. */
export function MailLinkStatus({
  link,
  warnings,
}: {
  link: MailLink;
  warnings: readonly MessageWarning[] | undefined;
}) {
  const t = useT();
  const texts = useWarningTexts(warningsForLink(warnings, link.index));
  const warned = texts.length > 0;
  return (
    <div data-part="message-view.link-dock">
      <div
        data-part="message-view.link-status"
        data-state={warned ? "warning" : undefined}
        role="status"
      >
        <span data-part="message-view.link-status.url" aria-hidden="true">
          {shortenMiddle(link.href)}
        </span>
        <span data-part="message-view.link-status.full">
          {t("mail-link-status", { href: link.href })}
        </span>
        {texts.map((text) => (
          <span key={text} data-part="message-view.link-status.warning">
            {text}
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * The reader's link handling for one message: `onLink` for its frame and
 * the status overlay to mount at the reader's end (`null` while no link is
 * hovered or focused).
 */
export function useMailLinks(
  messageId: string | null,
  warnings: readonly MessageWarning[] | undefined,
): { onLink: (report: MailLinkReport) => void; status: ReactNode } {
  const ctx = useComposeContext();
  // The hovered link with its message: another message shows nothing of it.
  const [hovered, setHovered] = useState<{ messageId: string | null; link: MailLink } | null>(null);
  const latest = useRef({ ctx, warnings, messageId });
  useEffect(() => {
    latest.current = { ctx, warnings, messageId };
  }, [ctx, warnings, messageId]);
  const onLink = useCallback((report: MailLinkReport) => {
    const now = latest.current;
    if (!report.open) {
      setHovered(report.link === null ? null : { messageId: now.messageId, link: report.link });
      return;
    }
    if (report.link !== null) void followMailLink(now.ctx, report.link, now.warnings);
  }, []);
  const link = hovered !== null && hovered.messageId === messageId ? hovered.link : null;
  return {
    onLink,
    status: link === null ? null : <MailLinkStatus link={link} warnings={warnings} />,
  };
}

/** The confirmation before a flagged link opens. */
export function OpenLinkDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useComposeContext();
  const [what] = useState(() => pendingMailLink(ctx.store));
  const texts = useWarningTexts(what?.warnings ?? []);
  const cancel = useRef<HTMLButtonElement>(null);
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => {
    queueMicrotask(() => cancel.current?.focus());
  }, []);
  const done = () => {
    pending.delete(ctx.store.setState);
    close();
  };
  return (
    <div data-part="mail-confirm" data-confirm="open-link">
      <p data-part="mail-confirm.question">{t("mail-open-link-question")}</p>
      {texts.map((text) => (
        <p key={text} data-part="mail-confirm.warning">
          {text}
        </p>
      ))}
      {what !== null && (
        <p data-part="mail-confirm.target">
          {t("security-warning-real-target", { href: what.link.href })}
        </p>
      )}
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          data-action="open-link"
          disabled={what === null}
          onClick={() => {
            done();
            if (what !== null) void openMailLink(ctx, what.link.href);
          }}
        >
          {t("mail-open-link-open")}
        </button>
        <button ref={cancel} type="button" data-part="form.button" onClick={done}>
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}
