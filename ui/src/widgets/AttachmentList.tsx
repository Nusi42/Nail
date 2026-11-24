/**
 * The reader's attachment cards (ADR 0162). Images the HTML body already
 * shows (`AttachmentInfo.shown_in_body`: a `cid:` logo, a photo placed in
 * the text) are left out until the user asks for them: a disclosure button
 * lists them too, so they stay downloadable. With nothing hidden the list
 * is the plain card list. The iCalendar part of an invitation is left out
 * altogether: the invitation card shows it (R7, ADR 0246).
 */
import { useState } from "react";
import type { MessageBody } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import { AttachmentCard } from "./AttachmentCard";
import { Icon } from "./Icon";
import { shownByInvitationCard } from "../features/scheduling/model";
import { useInvitationCard } from "../features/scheduling/queries";

export interface AttachmentListProps {
  message: MessageBody;
  accountId: string | null;
  /** Element id of the plug-in preview of attachment `index`. */
  previewId: (index: number) => string;
}

/**
 * The attachments listed: all of them when `expanded`, else those the body
 * does not show — never the iCalendar part the invitation card shows.
 */
export function listedAttachments(
  message: Pick<MessageBody, "attachments" | "invitation">,
  expanded: boolean,
  carded = message.invitation !== undefined,
): MessageBody["attachments"] {
  const own = message.attachments.filter((a) => !shownByInvitationCard(carded, a));
  return expanded ? own : own.filter((a) => !a.shown_in_body);
}

export function AttachmentList({ message, accountId, previewId }: AttachmentListProps) {
  const t = useT();
  // Collapsed again for every other message.
  const [expandedFor, setExpandedFor] = useState<string | null>(null);
  const expanded = expandedFor === message.id;
  const carded = useInvitationCard(message);
  const all = listedAttachments(message, true, carded);
  const total = all.length;
  if (total === 0) return null;
  const embedded = all.some((a) => a.shown_in_body);
  const listed = listedAttachments(message, expanded, carded);
  return (
    <div data-part="message-view.attachment-list" data-state={expanded ? "expanded" : undefined}>
      {listed.length > 0 && (
        <ul data-part="message-view.attachments" aria-label={t("common-attachments")}>
          {listed.map((a) => (
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
      {embedded && (
        <button
          type="button"
          data-part="message-view.attachments.toggle"
          aria-expanded={expanded}
          onClick={() => {
            setExpandedFor(expanded ? null : message.id);
          }}
        >
          <Icon
            name={expanded ? "chevron-up" : "chevron-down"}
            part="message-view.attachments.toggle.icon"
          />
          <span data-part="message-view.attachments.toggle.text">
            {expanded
              ? t("message-view-attachments-hide-embedded")
              : t("message-view-attachments-show-all", { count: total })}
          </span>
        </button>
      )}
    </div>
  );
}
