/**
 * One attachment of the reader as a card (M5a WP153): a type badge, the
 * name, the size and the actions — preview (when a plug-in renders this
 * type), save, and the plug-ins' attachment actions. Replaces the chips
 * whose size ran into the name.
 */
import { useState } from "react";
import type { AttachmentInfo, MessageId, MessageWarning } from "../api/generated";
import { saveAttachment, type SaveOutcome } from "./attachmentSave";
import { AttachmentWarningMark } from "../features/security";
import { AttachmentActionMenu, useHasAttachmentPreview } from "../features/plugins";
import { useLocale, useT } from "../i18n/I18nProvider";
import { useClient } from "../state/queries";
import { Icon } from "./Icon";

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

/** The colour group of the type badge (`data-kind`). */
export type FileKind =
  "pdf" | "image" | "document" | "spreadsheet" | "archive" | "calendar" | "other";

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

/** What saving an attachment has come to. */
export type SaveState = "idle" | "saving" | Exclude<SaveOutcome, "cancelled"> | "failed";

/**
 * The status line of a card: its Fluent id and icon, `null` while idle.
 * "Saving…" shows at once, the outcome replaces it.
 */
export function saveStatus(state: SaveState): { id: string; icon: string } | null {
  switch (state) {
    case "idle":
      return null;
    case "saving":
      return { id: "attachment-card-saving", icon: "download" };
    case "saved":
    case "downloaded":
      return { id: `message-view-attachment-save-${state}`, icon: "check" };
    case "unsupported":
    case "failed":
      return { id: `message-view-attachment-save-${state}`, icon: "circle-alert" };
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
  const canPreview = useHasAttachmentPreview(attachment, accountId) && !attachment.inline;
  const [save, setSave] = useState<SaveState>("idle");
  const name = attachment.filename ?? t("message-view-attachment-unnamed");
  const type = fileType(attachment.filename, attachment.mime);
  const status = saveStatus(save);
  const onSave = () => {
    setSave("saving");
    saveAttachment(client, messageId, attachment, name)
      .then((outcome) => setSave(outcome === "cancelled" ? "idle" : outcome))
      .catch(() => setSave("failed"));
  };
  const onPreview = () => {
    const pane = document.getElementById(previewId);
    if (!pane) return;
    pane.scrollIntoView?.({ block: "nearest" });
    pane.focus({ preventScroll: true });
  };
  return (
    <li
      data-part="attachment-card"
      data-mime={attachment.mime}
      data-inline={attachment.inline ? "" : undefined}
      data-state={save === "idle" ? undefined : save}
    >
      <span data-part="attachment-card.type" data-kind={type.kind} aria-hidden="true">
        {type.label}
      </span>
      <span data-part="attachment-card.text">
        <span data-part="attachment-card.name" title={name}>
          {name}
        </span>
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
          data-action="save"
          aria-label={t("message-view-attachment-save", { name })}
          title={t("message-view-attachment-save", { name })}
          disabled={save === "saving" || save === "unsupported"}
          onClick={onSave}
        >
          <Icon name="download" part="form.button.icon" />
        </button>
        <AttachmentActionMenu messageId={messageId} attachment={attachment} accountId={accountId} />
      </span>
    </li>
  );
}
