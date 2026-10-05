import type { ConversationMessage } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import { displayName } from "../state/addresses";
import { formatRelative } from "../template/template";

export interface ChildRowProps {
  /** The element id (`aria-activedescendant` of the list). */
  id: string;
  /** Index among the rendered rows (the virtualizer's `data-index`). */
  index: number;
  /** The virtualizer's measuring ref. */
  measure: (element: Element | null) => void;
  message: ConversationMessage;
  /** The conversation row's key (its thread). */
  parentKey: string | null;
  selected: boolean;
  /** `data-state` tokens (`selected`, `unread`, `flagged`, `from-me`). */
  states: string | undefined;
  posinset: number;
  setsize: number;
  locale: string;
  now: number;
  /** A click (Ctrl / Shift + click pick it besides others; P3). */
  onChoose: (event: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => void;
}

/**
 * One message of an expanded conversation (Design B, P2 WP202): one line
 * with the sender ("me" for the user), the preview and the date, a rail
 * under the conversation's avatar. Choosing it opens the message alone in
 * the reader.
 */
export function ChildRowView({
  id,
  index,
  measure,
  message,
  parentKey,
  selected,
  states,
  posinset,
  setsize,
  locale,
  now,
  onChoose,
}: ChildRowProps) {
  const t = useT();
  const sender = message.from_me
    ? t("conversations-me")
    : displayName(message.from) || t("composer-unknown-sender");
  const preview = message.preview || message.subject || t("message-no-subject");
  return (
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus -- the tree handles the keyboard and the focus for its rows (aria-activedescendant).
    <div
      id={id}
      ref={measure}
      data-index={index}
      role="treeitem"
      aria-level={2}
      aria-posinset={posinset}
      aria-setsize={setsize}
      aria-selected={selected}
      data-part="message-list.child"
      data-state={states}
      data-account={message.account_id}
      data-message={message.id}
      data-folder={message.folder_id}
      data-thread={parentKey ?? undefined}
      onClick={onChoose}
    >
      <span data-part="message-list.child.rail" aria-hidden="true" />
      <span data-part="message-list.child.from">{sender}</span>
      <span data-part="message-list.child.preview">{preview}</span>
      <span data-part="message-list.child.date">
        {formatRelative(new Date(message.date), now, locale)}
      </span>
    </div>
  );
}
