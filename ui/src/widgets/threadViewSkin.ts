/**
 * The reader's conversation strip (`thread-view.*`; P2 quick win, ADR 0148),
 * on the token schema only (`var(--nail-…)`), like `../theme/skin.ts`. Until
 * P2 the strip was styled by `defaults/theme.kdl` (a grey band of at most
 * 40 % of the window with its own scroll bar); now it sits in the reader's
 * column on the reader's background: rows padded by `space.row`, 28 px
 * avatars (`size.avatar` scaled, smaller in Compact), body-size text, the
 * newest three messages with the older ones folded into "N earlier
 * messages" (`thread-view.fold`), no scroll area of its own.
 *
 * The compiler emits this after the shared skin and before the theme's part
 * rules (`../theme/compile.ts`), so a `theme.kdl` still overrides any rule
 * here. WP-reader (P2) owns this file and replaces the strip with the card
 * feed (`thread-view.card*`, `thread-view.participants`; `thread-view.item`
 * stays an alias for one release).
 */

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;

const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  accent: "var(--nail-color-accent)",
  focus: "var(--nail-color-focus-ring)",
  quote: "var(--nail-color-quote-1, var(--nail-color-border))",
  fontReader: "var(--nail-font-reader, var(--nail-font-ui))",
  small: "var(--nail-font-size-small)",
  body: "var(--nail-font-size-body)",
  medium: "var(--nail-font-weight-medium)",
  semibold: "var(--nail-font-weight-semibold)",
  normal: "var(--nail-line-height-normal)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  xxl: "var(--nail-space-xxl)",
  pane: "var(--nail-space-pane)",
  row: "var(--nail-space-row)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  disabled: "var(--nail-opacity-disabled)",
  fontUi: "var(--nail-font-ui)",
  fontDisplay: "var(--nail-font-display, var(--nail-font-ui))",
  heading: "var(--nail-font-size-heading)",
  tight: "var(--nail-line-height-tight)",
  lg: "var(--nail-space-lg)",
  xl: "var(--nail-space-xl)",
  rLg: "var(--nail-radius-lg)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  /** 28 px in Comfortable (size.avatar 34 scaled by 28 / 34), 23 px in Compact. */
  avatar: "var(--nail-skin-avatar-sm)",
} as const;

/** The strip's rows: half the list's row padding above and below. */
const ROW_PAD = `calc(${T.row} / 2)`;

const STRIP = `:root { --nail-skin-avatar-sm: calc(var(--nail-size-avatar) * 28 / 34); }
${p("thread-view")} {
  box-sizing: border-box; width: 100%; max-width: calc(68ch + 2 * ${T.xxl}); margin-inline: auto;
  padding: ${T.sm} ${T.xxl} ${T.xs}; border-bottom: 1px solid ${T.border};
  font-size: ${T.body}; line-height: ${T.normal}; color: ${T.fg}; background-color: ${T.bg};
}
@container nail-reader (max-width: 640px) {
  ${p("thread-view")} { padding-inline: ${T.pane}; }
}
${p("thread-view.header")} {
  display: flex; flex-wrap: wrap; align-items: baseline; column-gap: ${T.sm}; padding: ${T.xs} ${T.sm};
}
${p("thread-view.subject")} { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; font-weight: ${T.semibold}; }
${p("thread-view.count")} { flex: 0 0 auto; font-size: ${T.small}; color: ${T.muted}; }
${p("thread-view.fold")} {
  display: block; box-sizing: border-box; width: 100%; padding: ${ROW_PAD} ${T.sm}; border: none; border-radius: ${T.rMd};
  font: inherit; font-size: ${T.small}; font-weight: ${T.medium}; text-align: start; color: ${T.accent};
  background-color: transparent; cursor: pointer;
}
${p("thread-view.fold")}:hover { background-color: ${T.subtle}; }
${p("thread-view.fold")}:focus-visible, ${p("thread-view.item.toggle")}:focus-visible, ${p("thread-view.item.open")}:focus-visible {
  outline: 2px solid ${T.focus}; outline-offset: -2px;
}
${p("thread-view.item")} { border-radius: ${T.rMd}; }
${p("thread-view.item")} + ${p("thread-view.item")} { border-top: 1px solid ${T.border}; }
${p("thread-view.item")}[data-state~="selected"] { background-color: ${T.subtle}; }
${p("thread-view.item.header")} { display: flex; align-items: center; gap: ${T.sm}; padding-inline-end: ${T.sm}; }
${p("thread-view.item.toggle")} {
  display: flex; flex: 1 1 auto; align-items: center; gap: ${T.md}; min-width: 0;
  padding: ${ROW_PAD} ${T.sm}; border: none; border-radius: ${T.rMd};
  font: inherit; text-align: start; white-space: nowrap; overflow: hidden;
  color: inherit; background-color: transparent; cursor: pointer;
}
${p("thread-view.item.avatar")} {
  display: grid; flex: 0 0 auto; place-items: center; width: ${T.avatar}; height: ${T.avatar}; border-radius: 50%;
  font-size: calc(${T.avatar} * 0.4); font-weight: ${T.semibold}; line-height: 1;
  color: var(--nail-color-avatar-fg, #ffffff); user-select: none;
}
${p("thread-view.item.from")} { flex: 0 1 auto; min-width: 0; max-width: 50%; overflow: hidden; text-overflow: ellipsis; }
${p("thread-view.item")}[data-state~="unread"] ${p("thread-view.item.from")} { font-weight: ${T.semibold}; }
${p("thread-view.item.date")} { flex: 0 0 auto; font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted}; }
${p("thread-view.item.preview")} { flex: 1 1 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: ${T.muted}; }
${p("thread-view.item.open")} {
  flex: 0 0 auto; padding: ${T.xxs} ${T.sm}; border: 1px solid ${T.border}; border-radius: ${T.rSm};
  font: inherit; font-size: ${T.small}; color: ${T.fg}; background-color: ${T.bg}; cursor: pointer;
}
${p("thread-view.item.open")}:hover { background-color: ${T.subtle}; }
${p("thread-view.item.open")}:disabled { opacity: ${T.disabled}; cursor: default; }
${p("thread-view.text")} { padding: 0 ${T.sm} ${T.sm} calc(${T.sm} + ${T.avatar} + ${T.md}); }
${p("thread-view.body")} { font-family: ${T.fontReader}; font-size: ${T.body}; }
${p("thread-view.quote")} { padding-inline-start: ${T.sm}; border-inline-start: 3px solid ${T.quote}; color: ${T.muted}; }
${p("thread-view.quote-toggle")} { font-size: ${T.small}; color: ${T.accent}; cursor: pointer; }`;

/** The reader's per-message widgets an open card holds besides `message-view` (`ThreadViewFeed.tsx`). */
const CARD_SLOTS = [
  "security-message",
  "security-warnings",
  "remote-content",
  "junk-verdict",
  "sync-message-body",
]
  .map(p)
  .join(", ");

/** A part inside an open card. */
const inCard = (part: string) => `${p("thread-view.card.body")} ${p(part)}`;

/**
 * The card feed (P2 Design A, `thread-view mode="cards"`, WP203): the
 * reader pane scrolls as a whole; the subject header and the cards share one
 * centred column; cards are bordered boxes without indentation, a
 * collapsed card one line, each card as tall as its content (the quote
 * toggle sits right under the text); a fold "N older messages" is a line of
 * its own between cards (`thread-view.fold-row`). Inside a card the
 * reader's widgets lose their own column, scroll area and subject; the
 * frame of an HTML body is as tall as its mail (P3, ADR 0164).
 * `data-mode="single"` is the selected message alone (no conversation).
 */
const FEED = `${p("stack")}:has(> ${p("thread-view")}[data-mode]) { container: nail-reader / inline-size; }
${p("thread-view")}[data-mode] {
  display: flex; flex-direction: column; box-sizing: border-box; width: 100%; max-width: none; height: 100%; min-height: 0;
  margin: 0; padding: ${T.lg} ${T.xxl} ${T.xl}; border: none; position: relative;
  overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable both-edges;
  font-family: ${T.fontUi}; font-size: ${T.body}; line-height: ${T.normal}; color: ${T.fg}; background-color: ${T.bg};
}
${p("thread-view")}[data-mode="single"] { padding: 0; }
${p("thread-view")}[data-mode="single"] > ${p("message-view")} { flex: 1 1 auto; }
${p("thread-view")}[data-mode="cards"] > ${p("thread-view.header")}, ${p("thread-view.feed")} {
  box-sizing: border-box; width: 100%; max-width: calc(68ch + 2 * ${T.xl}); margin-inline: auto;
}
${p("thread-view")}[data-mode="cards"] > ${p("thread-view.header")} {
  display: flex; flex-wrap: wrap; align-items: baseline; gap: ${T.xs} ${T.md}; padding: 0 0 ${T.md};
}
${p("thread-view")}[data-mode="cards"] ${p("thread-view.subject")} {
  flex: 1 0 100%; margin: 0; overflow-wrap: anywhere;
  font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight};
}
${p("thread-view.participants")} { flex: 1 1 auto; min-width: 0; margin: 0; font-size: ${T.small}; color: ${T.muted}; overflow-wrap: anywhere; }
${p("thread-view.participant")}[data-state~="unread"] { font-weight: ${T.semibold}; color: ${T.fg}; }
${p("thread-view")}[data-mode="cards"] ${p("thread-view.count")} { flex: 0 0 auto; }
${p("thread-view.expand-all")} {
  flex: 0 0 auto; padding: ${T.xxs} ${T.sm}; border: none; border-radius: ${T.rSm};
  font: inherit; font-size: ${T.small}; font-weight: ${T.medium}; color: ${T.accent}; background-color: transparent; cursor: pointer;
}
${p("thread-view.expand-all")}:hover { background-color: ${T.subtle}; }
${p("thread-view.feed")} { display: flex; flex: 1 0 auto; flex-direction: column; gap: ${T.sm}; }
${p("thread-view.card")} {
  display: flex; flex-direction: column; box-sizing: border-box; min-width: 0;
  border: 1px solid ${T.border}; border-radius: ${T.rLg}; background-color: ${T.bg};
}
${p("thread-view.card")}:focus { outline: none; }
${p("thread-view.card")}[data-state~="selected"] { border-color: ${T.accent}; }
${p("thread-view.fold-row")} { display: flex; align-items: center; gap: ${T.sm}; }
${p("thread-view.fold-row")}::before, ${p("thread-view.fold-row")}::after {
  content: ""; flex: 1 1 0; border-top: 1px dashed ${T.border};
}
${p("thread-view.fold-row")} > ${p("thread-view.fold")} {
  flex: 0 0 auto; width: auto; padding: ${ROW_PAD} ${T.md}; border-radius: ${T.rLg};
}
${p("thread-view.card.header")} { display: flex; align-items: center; gap: ${T.xs}; min-width: 0; padding-inline-end: ${T.sm}; }
${p("thread-view.card.toggle")} {
  display: flex; flex: 1 1 auto; align-items: center; gap: ${T.md}; min-width: 0;
  padding: ${ROW_PAD} ${T.md}; border: none; border-radius: ${T.rLg};
  font: inherit; text-align: start; white-space: nowrap; overflow: hidden;
  color: inherit; background-color: transparent; cursor: pointer;
}
${p("thread-view.card")}:not([data-state~="expanded"]) ${p("thread-view.card.toggle")}:hover { background-color: ${T.subtle}; }
${p("thread-view.card.avatar")} {
  display: grid; flex: 0 0 auto; place-items: center; width: ${T.avatar}; height: ${T.avatar}; border-radius: 50%;
  font-size: calc(${T.avatar} * 0.4); font-weight: ${T.semibold}; line-height: 1;
  color: var(--nail-color-avatar-fg, #ffffff); user-select: none;
}
${p("thread-view.card.from")} { flex: 0 1 auto; min-width: 0; max-width: 45%; overflow: hidden; text-overflow: ellipsis; font-weight: ${T.medium}; }
${p("thread-view.card")}[data-state~="unread"] ${p("thread-view.card.from")} { font-weight: ${T.semibold}; }
${p("thread-view.card.recipients")}, ${p("thread-view.card.preview")} {
  flex: 1 1 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: ${T.muted};
}
${p("thread-view.card.recipients")} { font-size: ${T.small}; }
${p("thread-view.card.date")} {
  flex: 0 0 auto; margin-inline-start: auto; font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted};
}
${p("thread-view.card.actions")} { display: flex; flex: 0 0 auto; gap: ${T.xxs}; }
${p("thread-view.card.action")} {
  display: inline-grid; place-items: center; width: ${T.control}; height: ${T.control}; padding: 0;
  border: none; border-radius: ${T.rSm}; color: ${T.muted}; background-color: transparent; cursor: pointer;
}
${p("thread-view.card.action")}:hover { color: ${T.fg}; background-color: ${T.subtle}; }
${p("thread-view.card.action.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("thread-view.card.toggle")}:focus-visible, ${p("thread-view.card.action")}:focus-visible, ${p("thread-view.expand-all")}:focus-visible {
  outline: 2px solid ${T.focus}; outline-offset: -2px;
}
${p("thread-view.card.body")} {
  display: flex; flex: 1 0 auto; flex-direction: column; gap: ${T.sm}; min-width: 0;
  padding: 0 ${T.md} ${T.md} calc(${T.md} + ${T.avatar} + ${T.md});
}
${p("thread-view.card.body")} > :is(${CARD_SLOTS}) { width: 100%; margin: 0; }
${p("thread-view.card.body")} > ${p("message-view")} {
  flex: 1 0 auto; height: auto; overflow: visible; scrollbar-gutter: auto; background-color: transparent;
}
${inCard("message-view.column")} { flex: 1 0 auto; max-width: none; margin: 0; padding: 0; gap: ${T.md}; }
${inCard("message-view.header")}:empty { display: none; }
${inCard("message-view.text")} { display: grid; align-content: start; gap: ${T.xs}; padding-bottom: 0; white-space: normal; }
${inCard("message-view.text-block")}, ${inCard("message-view.quote")} {
  margin: 0; font: inherit; white-space: pre-wrap; overflow-wrap: anywhere;
}
${inCard("message-view.quote")} { padding-inline-start: ${T.sm}; border-inline-start: 3px solid ${T.quote}; color: ${T.muted}; }
${inCard("message-view.quote-toggle")} { width: fit-content; font-size: ${T.small}; color: ${T.accent}; cursor: pointer; }
${inCard("message-view.quote-toggle")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${inCard("message-view.body")} { background-color: transparent; }
@container nail-reader (max-width: 640px) {
  ${p("thread-view")}[data-mode="cards"] { padding: ${T.md} ${T.pane} ${T.lg}; }
  ${p("thread-view.card.body")} { padding-inline-start: ${T.md}; }
  ${p("thread-view.card.toggle")} { gap: ${T.sm}; padding-inline: ${T.sm}; }
  ${p("thread-view.card.recipients")} { display: none; }
}
@container nail-reader (max-width: 480px) {
  /* Phone width: the sender keeps its room; Reply all / Forward stay in the reader's bar. */
  ${p("thread-view.card.action")}:not(:first-child) { display: none; }
  ${p("thread-view.card.from")} { max-width: none; }
}`;

export const THREAD_VIEW_SKIN_CSS = `${STRIP}
${FEED}`;
