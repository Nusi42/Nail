/**
 * One attachment of the reader as a card (M5a WP153): a type badge, the
 * name, the size and the actions — open, preview (when a plug-in renders
 * this type), save, and the plug-ins' attachment actions. Replaces the chips
 * whose size ran into the name. The badge and the name open the attachment
 * too (ADR 0211): the desktop app hands it to the system's handler, anywhere
 * else it downloads; a flagged attachment asks first (`OpenAttachment.tsx`).
 */
import { useState } from "react";
import type { AttachmentInfo, MessageId, MessageWarning } from "../api/generated";
import {
  openAttachment,
  saveAttachment,
  type OpenOutcome,
  type SaveOutcome,
} from "./attachmentSave";
import { AttachmentWarningMark, attachmentWarning } from "../features/security";
import { AttachmentActionMenu, useHasAttachmentPreview } from "../features/plugins";
import { useLocale, useT } from "../i18n/I18nProvider";
import { useComposeContext } from "../state/hooks";
import { useClient } from "../state/queries";
import { Icon } from "./Icon";
import { askToOpenAttachment } from "./OpenAttachment";

/** Size in kB / MB for the locale (at least 1 kB). */
export function formatSize(bytes: number, locale: string): string {
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

export type { FileKind } from "./fileKinds";
import type { FileKind } from "./fileKinds";

const KIND_BY_EXTENSION: Readonly<Record<string, FileKind>> = {
  pdf: "pdf",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  webp: "image",
  svg: "image",
  heic: "image",
  doc: "document",
  docx: "document",
  odt: "document",
  rtf: "document",
  txt: "document",
  md: "document",
  xls: "spreadsheet",
  xlsx: "spreadsheet",
  ods: "spreadsheet",
  csv: "spreadsheet",
  zip: "archive",
  "7z": "archive",
  rar: "archive",
  gz: "archive",
  tar: "archive",
  ics: "calendar",
  vcf: "calendar",
};

/**
 * The badge text (the file extension, at most four characters, upper case;
 * else the MIME subtype) and its colour group. File-type names are data,
 * not translated text.
 */
export function fileType(filename: string | null, mime: string): { label: string; kind: FileKind } {
  const ext = /\.([a-z0-9]{1,8})$/i.exec(filename ?? "")?.[1]?.toLowerCase() ?? null;
  const subtype =
    mime
      .split("/")[1]
      ?.split(/[+.;-]/)[0]
      ?.toLowerCase() ?? "";
  const key = ext ?? subtype;
  let kind: FileKind = KIND_BY_EXTENSION[key] ?? "other";
  if (kind === "other") {
    if (mime.startsWith("image/")) kind = "image";
    else if (mime === "application/pdf") kind = "pdf";
    else if (mime === "text/calendar") kind = "calendar";
  }
  return { label: key.slice(0, 4).toUpperCase(), kind };
}

/**
 * What saving or opening an attachment has come to (the card's
 * `data-state`): `saving` / `opening` while it runs, then the outcome.
 */
export type SaveState =
  | "idle"
  | "saving"
  | "opening"
  | Exclude<SaveOutcome, "cancelled">
  | Exclude<OpenOutcome, "downloaded" | "unsupported">
  | "failed"
  | "open-failed"
  | "open-unsupported";

/**
 * The status line of a card: its Fluent id and icon, `null` while idle.
 * "Saving…" / "Opening…" show at once, the outcome replaces it.
 */
export function saveStatus(state: SaveState): { id: string; icon: string } | null {
  switch (state) {
    case "idle":
      return null;
    case "saving":
      return { id: "attachment-card-saving", icon: "download" };
    case "opening":
      return { id: "attachment-card-opening", icon: "external-link" };
    case "saved":
    case "downloaded":
      return { id: `message-view-attachment-save-${state}`, icon: "check" };
    case "opened":
      return { id: "message-view-attachment-opened", icon: "check" };
    case "unsupported":
    case "failed":
      return { id: `message-view-attachment-save-${state}`, icon: "circle-alert" };
    case "open-failed":
    case "open-unsupported":
      return { id: `message-view-attachment-${state}`, icon: "circle-alert" };
  }
}

export interface AttachmentCardProps {
  messageId: MessageId;
  attachment: AttachmentInfo;
  accountId: string | null;
  warnings: readonly MessageWarning[];
  /** Element id the plug-in preview of this attachment gets (when there is one). */
  previewId: string;
}

export function AttachmentCard({
  messageId,
  attachment,
  accountId,
  warnings,
  previewId,
}: AttachmentCardProps) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const { store } = useComposeContext();
  const canPreview = useHasAttachmentPreview(attachment, accountId) && !attachment.inline;
  const [save, setSave] = useState<SaveState>("idle");
  const name = attachment.filename ?? t("common-unnamed-attachment");
  const type = fileType(attachment.filename, attachment.mime);
  const status = saveStatus(save);
  const onSave = () => {
    setSave("saving");
    saveAttachment(client, messageId, attachment, name)
      .then((outcome) => {
        setSave(outcome === "cancelled" ? "idle" : outcome);
      })
      .catch(() => {
        setSave("failed");
      });
  };
  // A flagged attachment (program, macros, …) asks before it opens.
  const flagged = attachmentWarning(warnings, attachment.index);
  const busy = save === "saving" || save === "opening";
  const runOpen = () => {
    setSave("opening");
    openAttachment(client, messageId, attachment, name)
      .then((outcome) => {
        setSave(outcome === "unsupported" ? "open-unsupported" : outcome);
      })
      .catch(() => {
        setSave("open-failed");
      });
  };
  const onOpen = () => {
    if (busy) return;
    if (flagged === null) runOpen();
    else askToOpenAttachment(store, { name: flagged.filename, risk: flagged.risk, run: runOpen });
  };
  const onPreview = () => {
    const pane = document.getElementById(previewId);
    if (!pane) return;
    pane.scrollIntoView({ block: "nearest" });
    pane.focus({ preventScroll: true });
  };
  return (
    <li
      data-part="attachment-card"
      data-mime={attachment.mime}
      data-inline={attachment.inline ? "" : undefined}
      data-embedded={attachment.shown_in_body ? "" : undefined}
      data-state={save === "idle" ? undefined : save}
    >
      {/* A pointer shortcut for the name's button: out of the tab order and the tree. */}
      <button
        type="button"
        data-part="attachment-card.type"
        data-kind={type.kind}
        aria-hidden="true"
        tabIndex={-1}
        onClick={onOpen}
      >
        {type.label}
      </button>
      <span data-part="attachment-card.text">
        <button
          type="button"
          data-part="attachment-card.name"
          data-action="open"
          title={name}
          onClick={onOpen}
        >
          {name}
        </button>
        <span data-part="attachment-card.size">{formatSize(attachment.size, locale)}</span>
        {/*
          Next to the size, so the card keeps its height (M5a review). The
          live region is always there (empty while idle), so screen readers
          announce "Saving…" and the outcome when they appear.
        */}
        <span
          data-part="attachment-card.status"
          data-state={status === null ? "empty" : save}
          role="status"
          title={status === null ? undefined : t(status.id)}
        >
          {status !== null && (
            <>
              <Icon name={status.icon} part="attachment-card.status.icon" />
              <span data-part="attachment-card.status.text">{t(status.id)}</span>
            </>
          )}
        </span>
      </span>
      <span data-part="attachment-card.actions">
        <AttachmentWarningMark warnings={warnings} index={attachment.index} />
        {canPreview && (
          <button
            type="button"
            data-part="form.button"
            data-variant="icon"
            data-action="preview"
            aria-label={t("message-view-attachment-preview", { name })}
            title={t("message-view-attachment-preview", { name })}
            onClick={onPreview}
          >
            <Icon name="eye" part="form.button.icon" />
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          data-variant="icon"
          data-action="open"
          aria-label={t("message-view-attachment-open", { name })}
          title={t("message-view-attachment-open", { name })}
          disabled={busy || save === "open-unsupported"}
          onClick={onOpen}
        >
          <Icon name="external-link" part="form.button.icon" />
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="icon"
          data-action="save"
          aria-label={t("message-view-attachment-save", { name })}
          title={t("message-view-attachment-save", { name })}
          disabled={busy || save === "unsupported"}
          onClick={onSave}
        >
          <Icon name="download" part="form.button.icon" />
        </button>
        <AttachmentActionMenu messageId={messageId} attachment={attachment} accountId={accountId} />
      </span>
    </li>
  );
}
