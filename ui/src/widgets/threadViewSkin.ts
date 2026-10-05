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
  /** 28 px in Comfortable (size.avatar 34 scaled by 28 / 34), 23 px in Compact. */
  avatar: "var(--nail-skin-avatar-sm)",
} as const;

/** The strip's rows: half the list's row padding above and below. */
const ROW_PAD = `calc(${T.row} / 2)`;

export const THREAD_VIEW_SKIN_CSS = `:root { --nail-skin-avatar-sm: calc(var(--nail-size-avatar) * 28 / 34); }
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
