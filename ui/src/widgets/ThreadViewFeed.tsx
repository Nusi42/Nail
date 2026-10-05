/**
 * The reader as a card feed (P2 Design A, WP203; `thread-view mode="cards"`):
 * the conversation's subject and participants, then its messages as cards,
 * oldest → newest, without indentation. An open card shows the reader's
 * per-message widgets for its own message — security badges and warnings,
 * the remote-content banner, the junk verdict, the body state and
 * `message-view` (`card=`, default all six) — through `CardScope`; a
 * collapsed card is one line with avatar, sender, preview and date. More
 * than four collapsed cards in the middle fold into "N older messages".
 * The rules are in `./ThreadViewModel.ts`.
 *
 * Accessibility: `role="feed"` with an `article` per card (`aria-setsize` /
 * `aria-posinset`), each card's header a button with `aria-expanded`.
 * Keys (`keys context="reader"`): j / k, o (Enter on a header), ; / :,
 * n / p — the `conversation.*` actions of `features/conversation-reader` —
 * and r / a / f on the focused card: focusing or clicking into a card
 * selects its message, so the reader's actions act on it.
 *
 * Without a conversation to show (nothing selected, no thread, the
 * conversation failing, a message outside it) the widgets show the selected
 * message alone, as the reader did before the feed; while the conversation
 * loads the reader stays empty.
 */
import { Fragment, useEffect, useId, useMemo, useRef, type ReactNode } from "react";
import type { Address, ConversationMessage, ThreadParticipant } from "../api/generated";
import { useActions } from "../actions/ActionsProvider";
import { openContextMenu } from "../features/menus";
import { FULL_DATE_TIME, formatDateTime, formatRelative } from "../i18n/format";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { useLayoutConfig } from "../layout/LayoutContext";
import {
  optionBool,
  optionList,
  optionString,
  useWidgetRegistry,
  type WidgetNode,
  type WidgetProps,
} from "../layout/widgetRegistry";
import { displayName } from "../state/addresses";
import { useNow } from "../state/clock";
import { useConversation } from "../state/queries";
import { useUi, useUiStoreApi } from "../state/uiStore";
import { Icon } from "./Icon";
import { Avatar } from "./primitives";
import {
  CARD_WIDGETS,
  feedItems,
  feedStore,
  initiallyOpen,
  primaryMessage,
  useFeed,
} from "./ThreadViewModel";
import { CardScope } from "./ThreadViewScope";

/** The per-card actions in a card's header. */
export const CARD_ACTIONS = ["mail.reply", "mail.reply-all", "mail.forward"] as const;

const CARD_ACTION_TITLES: Record<(typeof CARD_ACTIONS)[number], string> = {
  "mail.reply": "mail-action-reply",
  "mail.reply-all": "mail-action-reply-all",
  "mail.forward": "mail-action-forward",
};

function isCardWidget(kind: string): kind is (typeof CARD_WIDGETS)[number] {
  return (CARD_WIDGETS as readonly string[]).includes(kind);
}

/**
 * The widget nodes of an open card (`card=` kinds; `headers=` and
 * `mark-read=` go to its `message-view`). `single` = the nodes for a
 * message shown alone (its `message-view` with its default headers).
 */
export function cardNodes(node: WidgetNode, single: boolean): WidgetNode[] {
  const kinds = [...new Set(optionList(node, "card", CARD_WIDGETS).filter(isCardWidget))];
  return kinds.map((kind) => ({
    type: "widget",
    id: null,
    label: null,
    kind,
    options:
      kind === "message-view"
        ? {
            ...(single ? {} : { headers: optionString(node, "headers", "") }),
            "mark-read": optionBool(node, "mark-read", true),
          }
        : {},
    rows: [],
    source: null,
  }));
}

function Widgets({ nodes, nodeKey }: { nodes: readonly WidgetNode[]; nodeKey: string }) {
  const registry = useWidgetRegistry();
  return (
    <>
      {nodes.map((n) => {
        const Widget = Object.hasOwn(registry, n.kind) ? registry[n.kind] : undefined;
        return Widget ? <Widget key={n.kind} node={n} nodeKey={`${nodeKey}/${n.kind}`} /> : null;
      })}
    </>
  );
}

/** The names of a card's recipients, "me" for the user's addresses. */
function recipientNames(
  to: readonly Address[],
  mine: ReadonlySet<string>,
  me: string,
  separator: string,
): string {
  return to
    .map((a) => (mine.has(a.email.toLowerCase()) ? me : displayName(a) || a.email))
    .join(separator);
}

function participantName(p: ThreadParticipant, me: string): string {
  if (p.me) return me;
  return p.name ?? p.email;
}

interface CardProps {
  threadId: string;
  /** The conversation's subject (a card shows its own only when it differs). */
  subject: string;
  message: ConversationMessage;
  index: number;
  total: number;
  open: boolean;
  selected: boolean;
  mine: ReadonlySet<string>;
  nodes: readonly WidgetNode[];
  nodeKey: string;
}

function Card({
  threadId,
  subject,
  message: m,
  index,
  total,
  open,
  selected,
  mine,
  nodes,
  nodeKey,
}: CardProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const now = useNow();
  const ids = useId();
  const store = useUiStoreApi();
  const { dispatch } = useActions();
  const layout = useLayoutConfig();
  const actionDefs = layout.actions;
  const me = t("conversations-me");
  const select = () => {
    if (store.getState().selectedMessageId !== m.id) store.getState().selectMessage(m.id);
  };
  const states = [
    open ? "expanded" : null,
    m.flags.seen ? null : "unread",
    selected ? "selected" : null,
    m.flags.flagged ? "flagged" : null,
    m.from_me ? "from-me" : null,
  ].filter((s) => s !== null);
  const date = new Date(m.date);
  const valid = !Number.isNaN(date.getTime());
  const sender = m.from_me ? me : displayName(m.from) || t("composer-unknown-sender");
  return (
    <article
      data-part="thread-view.card"
      data-state={states.join(" ") || undefined}
      data-message={m.id}
      data-account={m.account_id}
      aria-setsize={total}
      aria-posinset={index + 1}
      aria-labelledby={`${ids}-toggle`}
      aria-current={selected ? "true" : undefined}
      tabIndex={-1}
      onPointerDownCapture={select}
      onFocusCapture={select}
    >
      <div data-part="thread-view.card.header">
        <button
          type="button"
          id={`${ids}-toggle`}
          data-part="thread-view.card.toggle"
          data-feed-stop=""
          aria-expanded={open}
          onClick={() => {
            feedStore.getState().setOpen(threadId, [m.id], !open);
          }}
        >
          <Avatar
            part="thread-view.card.avatar"
            name={m.from?.name ?? null}
            email={m.from?.email ?? ""}
          />
          <span data-part="thread-view.card.from">{sender}</span>
          {open ? (
            m.to.length > 0 && (
              <span data-part="thread-view.card.recipients">
                {t("thread-view-card-to", {
                  names: recipientNames(m.to, mine, me, t("conversations-names-separator")),
                })}
              </span>
            )
          ) : (
            <span data-part="thread-view.card.preview">{m.preview}</span>
          )}
          <time
            data-part="thread-view.card.date"
            dateTime={valid ? date.toISOString() : undefined}
            title={valid ? formatDateTime(date, locale, FULL_DATE_TIME) : undefined}
          >
            {valid ? formatRelative(date, now, locale) : m.date}
          </time>
        </button>
        {open && (
          <div
            data-part="thread-view.card.actions"
            role="group"
            aria-label={t("thread-view-card-actions", { name: sender })}
          >
            {CARD_ACTIONS.map((id) => {
              const def = actionDefs[id];
              const text = def?.label ? label(def.label) : t(CARD_ACTION_TITLES[id]);
              return (
                <button
                  key={id}
                  type="button"
                  data-part="thread-view.card.action"
                  data-action={id}
                  aria-label={text}
                  title={text}
                  onClick={() => {
                    select();
                    void dispatch(id);
                  }}
                >
                  <Icon name={def?.icon ?? null} part="thread-view.card.action.icon" />
                </button>
              );
            })}
            {/* P3: the card's own menu (`context:thread-view.message`):
                its actions act on this message, not the conversation. */}
            <button
              type="button"
              data-part="thread-view.card.action"
              data-action="more"
              aria-label={t("mail-actions-card-more")}
              title={t("mail-actions-card-more")}
              aria-haspopup="menu"
              onClick={(event) => {
                select();
                const rect = event.currentTarget.getBoundingClientRect();
                openContextMenu(store, layout, event.currentTarget, {
                  point: { x: rect.left, y: rect.bottom },
                  keyboard: event.detail === 0,
                });
              }}
            >
              <Icon name="ellipsis" part="thread-view.card.action.icon" />
            </button>
          </div>
        )}
      </div>
      {open && (
        <div data-part="thread-view.card.body">
          <CardScope messageId={m.id} subject={subject}>
            <Widgets nodes={nodes} nodeKey={`${nodeKey}/${m.id}`} />
          </CardScope>
        </div>
      )}
    </article>
  );
}

/**
 * "N older messages": a line of its own between two cards (an `article` of
 * the feed, so the feed owns only articles). Opening it moves the focus to
 * the first card it revealed, so the keyboard stays in the feed.
 */
function Fold({
  threadId,
  messages,
}: {
  threadId: string;
  messages: readonly ConversationMessage[];
}) {
  const t = useT();
  const ids = useId();
  const first = messages[0];
  return (
    <article data-part="thread-view.fold-row" aria-labelledby={`${ids}-fold`}>
      <button
        type="button"
        id={`${ids}-fold`}
        data-part="thread-view.fold"
        data-feed-stop=""
        aria-expanded={false}
        onClick={() => {
          const s = feedStore.getState();
          s.unfold(threadId);
          if (first !== undefined) s.requestFocus(threadId, first.id);
        }}
      >
        {t("conversations-older", { count: messages.length })}
      </button>
    </article>
  );
}

/** `thread-view mode="cards"`. */
export function ThreadFeed({ node, nodeKey }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ids = useId();
  const threadId = useUi((s) => s.selectedThreadId);
  const selectedId = useUi((s) => s.selectedMessageId);
  // The conversation as the list shows it; while searching, across folders.
  const folderId = useUi((s) => (s.search === null ? s.activeFolderId : null));
  const query = useConversation(threadId, folderId);
  const messages = useMemo(() => query.data?.messages ?? [], [query.data]);
  const feedNodes = useMemo(() => cardNodes(node, false), [node]);
  const singleNodes = useMemo(() => cardNodes(node, true), [node]);
  const showFeed =
    threadId !== null &&
    query.isSuccess &&
    selectedId !== null &&
    messages.some((m) => m.id === selectedId);

  // What starts open, decided once per message (reading never closes a card).
  const initial = useMemo(() => initiallyOpen(messages, selectedId), [messages, selectedId]);
  useEffect(() => {
    if (!showFeed) return;
    feedStore.getState().seed(
      threadId,
      messages.map((m) => m.id),
      Object.fromEntries(messages.map((m) => [m.id, initial.has(m.id)])),
    );
  }, [showFeed, threadId, messages, initial]);
  const opened = useFeed((s) => (threadId === null ? undefined : s.open[threadId]));
  const unfolded = useFeed((s) => threadId !== null && s.unfolded[threadId] === true);
  const focus = useFeed((s) => s.focus);
  const isOpen = (m: ConversationMessage) => opened?.[m.id] ?? initial.has(m.id);

  // Opening a conversation shows its primary card (the newest unread, or
  // the list's choice) at the top of the reader.
  const section = useRef<HTMLElement>(null);
  const scrolledFor = useRef<string | null>(null);
  const primary = showFeed ? primaryMessage(messages, selectedId) : null;
  useEffect(() => {
    const el = section.current;
    if (!showFeed || el === null || primary === null || scrolledFor.current === threadId) return;
    scrolledFor.current = threadId;
    const card = el.querySelector<HTMLElement>(`[data-message="${CSS.escape(primary)}"]`);
    if (card && card.previousElementSibling !== null) el.scrollTop = card.offsetTop;
  }, [showFeed, threadId, primary]);

  // Focus requested by n / p across a fold, once the fold opened.
  useEffect(() => {
    const el = section.current;
    if (el === null || focus?.threadId !== threadId) return;
    el.querySelector<HTMLElement>(
      `[data-message="${CSS.escape(focus.messageId)}"] [data-part="thread-view.card.toggle"]`,
    )?.focus();
  }, [focus, threadId]);

  if (!showFeed && threadId !== null && selectedId !== null && query.isPending) {
    // The conversation is on its way (a local query): an empty reader for
    // that moment, so the message does not show alone first and then move
    // into its card.
    return (
      <section
        data-part="thread-view"
        data-mode="cards"
        data-node-id={node.id ?? undefined}
        data-key-context="reader"
        aria-label={node.label ? label(node.label) : t("thread-view-label")}
        aria-busy="true"
      />
    );
  }
  if (!showFeed) {
    return (
      <div
        data-part="thread-view"
        data-mode="single"
        data-node-id={node.id ?? undefined}
        aria-busy={query.isFetching}
      >
        <Widgets nodes={singleNodes} nodeKey={nodeKey} />
      </div>
    );
  }

  const thread = query.data.thread;
  const items = feedItems(messages, isOpen, selectedId, unfolded);
  const mine = new Set<string>(
    [
      ...thread.participants.filter((p) => p.me).map((p) => p.email),
      ...messages.filter((m) => m.from_me).map((m) => m.from?.email ?? ""),
    ]
      .filter((e) => e !== "")
      .map((e) => e.toLowerCase()),
  );
  const me = t("conversations-me");
  const anyClosed = items.some((i) => i.kind === "fold" || !isOpen(i.message));
  const participants: ReactNode = thread.participants.map((p, i) => (
    <Fragment key={`${p.email}-${String(i)}`}>
      {i > 0 && (
        <span data-part="thread-view.participants.separator">
          {t("conversations-names-separator")}
        </span>
      )}
      <span
        data-part="thread-view.participant"
        data-state={
          [p.me ? "me" : null, p.unread ? "unread" : null].filter(Boolean).join(" ") || undefined
        }
      >
        {participantName(p, me)}
      </span>
    </Fragment>
  ));

  return (
    <section
      ref={section}
      data-part="thread-view"
      data-mode="cards"
      data-node-id={node.id ?? undefined}
      data-thread={threadId}
      data-key-context="reader"
      aria-label={node.label ? label(node.label) : t("thread-view-label")}
    >
      <header data-part="thread-view.header">
        <h1 data-part="thread-view.subject" id={`${ids}-subject`}>
          {thread.subject || t("message-no-subject")}
        </h1>
        <p data-part="thread-view.participants">{participants}</p>
        <span data-part="thread-view.count">
          {t("conversations-count", { count: messages.length })}
        </span>
        {messages.length > 1 && (
          <button
            type="button"
            data-part="thread-view.expand-all"
            onClick={() => {
              const s = feedStore.getState();
              s.setOpen(
                threadId,
                messages.map((m) => m.id),
                anyClosed,
              );
              if (anyClosed) s.unfold(threadId);
            }}
          >
            {t(anyClosed ? "conversations-expand-all" : "conversations-collapse-all")}
          </button>
        )}
      </header>
      <div
        role="feed"
        data-part="thread-view.feed"
        aria-labelledby={`${ids}-subject`}
        aria-busy={query.isFetching}
      >
        {items.map((item) =>
          item.kind === "fold" ? (
            <Fold key={`fold-${String(item.index)}`} threadId={threadId} messages={item.messages} />
          ) : (
            <Card
              key={item.message.id}
              threadId={threadId}
              subject={thread.subject}
              message={item.message}
              index={item.index}
              total={messages.length}
              open={isOpen(item.message)}
              selected={item.message.id === selectedId}
              mine={mine}
              nodes={feedNodes}
              nodeKey={nodeKey}
            />
          ),
        )}
      </div>
    </section>
  );
}
