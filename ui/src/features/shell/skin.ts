/**
 * The M5b look of the navigation rail, its badges and the view host (WP161,
 * ADR 0109), on the token schema only (`var(--nail-…)`), like
 * `../../theme/skin.ts`. The compiler emits it right after the shared skin
 * and before the theme's part rules, so any `theme.kdl` still overrides it.
 *
 * Parts:
 * - `nav-rail` (`data-labels` = `shown` / `hidden`), `nav-rail.logo`,
 *   `nav-rail.item` (`aria-current="page"` / `data-state="active"` on the
 *   active view, `data-view` / `data-action`), `nav-rail.icon`,
 *   `nav-rail.label`, `nav-rail.badge` (`data-badge` = `unread` /
 *   `confirmations`), `nav-rail.spacer`, `nav-rail.separator`,
 *   `nav-rail.account` (holds the shared `avatar` part, coloured by the
 *   avatar palette like every other avatar)
 * - `view-host` (`data-view`), `view-host.view` (`data-view`,
 *   `data-window`), `view-host.missing`
 *
 * Sizes follow the approved proposal: a 42 px button (control + 10 px) on a
 * rail about 60 px wide, a 34 px app mark (`size.avatar`), a 16 px badge.
 */

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;

/** Token shorthands (fallbacks only where the schema may lack the token). */
const T = {
  rail: "var(--nail-color-rail, var(--nail-color-bg-subtle))",
  canvas: "var(--nail-color-canvas, var(--nail-color-bg))",
  raised: "var(--nail-color-bg-raised)",
  bg: "var(--nail-color-bg)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  accent: "var(--nail-color-accent)",
  accentFg: "var(--nail-color-accent-fg)",
  accentSoft:
    "var(--nail-color-accent-soft, color-mix(in srgb, var(--nail-color-accent) 14%, var(--nail-color-bg)))",
  danger: "var(--nail-color-danger)",
  focus: "var(--nail-color-focus-ring)",
  avatarFg: "var(--nail-color-avatar-fg, #ffffff)",
  fontUi: "var(--nail-font-ui)",
  fontDisplay: "var(--nail-font-display, var(--nail-font-ui))",
  label: "var(--nail-font-size-label)",
  small: "var(--nail-font-size-small)",
  heading: "var(--nail-font-size-heading)",
  medium: "var(--nail-font-weight-medium)",
  semibold: "var(--nail-font-weight-semibold)",
  tight: "var(--nail-line-height-tight)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  avatar: "var(--nail-size-avatar)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  rLg: "var(--nail-radius-lg)",
  pill: "var(--nail-radius-pill)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
  disabled: "var(--nail-opacity-disabled)",
} as const;

/** The rail button: control + 10 px (42 px in Comfortable). */
const ITEM = `calc(${T.control} + ${T.sm} + ${T.xxs})`;
/** The badge's height (16 px). */
const BADGE = `calc(${T.md} + ${T.xs})`;

const RAIL = `${p("nav-rail")} {
  display: flex; flex-direction: column; align-items: center; gap: ${T.xs};
  box-sizing: border-box; width: calc(${ITEM} + 2 * ${T.sm}); min-width: calc(${ITEM} + 2 * ${T.sm});
  min-height: 0; padding: ${T.md} 0; overflow-x: hidden; overflow-y: auto;
  font-family: ${T.fontUi}; color: ${T.muted};
  background-color: ${T.rail}; border-right: 1px solid ${T.border};
}
${p("nav-rail")}[data-labels="shown"] { width: calc(${ITEM} * 1.75); min-width: calc(${ITEM} * 1.75); }
${p("nav-rail.logo")} {
  display: grid; place-items: center; flex: 0 0 auto;
  width: ${T.avatar}; height: ${T.avatar}; margin-bottom: ${T.sm};
  border-radius: ${T.rMd}; user-select: none;
  font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: 1;
  color: ${T.accentFg}; background-color: ${T.accent};
}
${p("nav-rail.item")} {
  position: relative; display: grid; place-items: center; flex: 0 0 auto;
  width: ${ITEM}; height: ${ITEM}; padding: 0;
  border: none; border-radius: ${T.rLg};
  font: inherit; color: ${T.muted}; background-color: transparent; cursor: default;
  transition: background-color ${T.fast} ${T.ease}, color ${T.fast} ${T.ease};
}
${p("nav-rail.item")}:hover { color: ${T.fg}; background-color: ${T.raised}; }
${p("nav-rail.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${p("nav-rail.item")}:disabled { opacity: ${T.disabled}; }
${p("nav-rail.item")}[aria-current="page"], ${p("nav-rail.item")}[data-state~="active"] {
  color: ${T.accent}; background-color: ${T.accentSoft};
}
${p("nav-rail.icon")} { width: calc(${T.icon} + ${T.xs}); height: calc(${T.icon} + ${T.xs}); }
${p("nav-rail.label")} {
  position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap;
}
${p("nav-rail")}[data-labels="shown"] ${p("nav-rail.item")} {
  width: calc(${ITEM} * 1.5); height: auto; min-height: ${ITEM};
  grid-auto-flow: row; gap: ${T.xxs}; padding: ${T.xs} ${T.xxs};
}
${p("nav-rail")}[data-labels="shown"] ${p("nav-rail.label")} {
  position: static; width: auto; height: auto; overflow: hidden; clip-path: none;
  max-width: 100%; text-overflow: ellipsis;
  font-size: ${T.label}; font-weight: ${T.medium}; line-height: ${T.tight};
}
${p("nav-rail.badge")} {
  position: absolute; top: calc(${T.xs} + 1px); right: calc(${T.xs} + 1px);
  box-sizing: border-box; min-width: ${BADGE}; height: ${BADGE}; padding: 0 ${T.xs};
  border-radius: ${T.pill};
  font-size: calc(${T.label} - 1.5px); font-weight: ${T.semibold}; line-height: ${BADGE};
  text-align: center; font-variant-numeric: tabular-nums;
  color: ${T.bg}; background-color: ${T.danger};
  box-shadow: 0 0 0 2px ${T.rail};
}
${p("nav-rail.spacer")} { flex: 1 1 auto; min-height: ${T.sm}; }
${p("nav-rail.separator")} {
  flex: 0 0 auto; width: calc(${ITEM} - 2 * ${T.sm}); height: 1px; margin: ${T.xs} 0;
  background-color: ${T.border};
}
${p("nav-rail.account")} { display: grid; place-items: center; flex: 0 0 auto; margin-top: ${T.sm}; }
${p("nav-rail.account")} > ${p("avatar")} {
  display: grid; place-items: center; width: ${T.control}; height: ${T.control}; border-radius: 50%;
  font-size: ${T.small}; font-weight: ${T.semibold}; line-height: 1; letter-spacing: 0.02em;
  color: ${T.avatarFg}; user-select: none;
}`;

const HOST = `${p("view-host")} {
  display: grid; grid-template: minmax(0, 1fr) / minmax(0, 1fr);
  min-width: 0; min-height: 0; overflow: hidden; background-color: ${T.canvas};
}
${p("view-host.view")} {
  display: grid; grid-template: minmax(0, 1fr) / minmax(0, 1fr);
  min-width: 0; min-height: 0; overflow: hidden; outline: none; background-color: ${T.bg};
}
${p("view-host.view")}[hidden] { display: none; }
${p("view-host.missing")} { padding: ${T.md}; font-size: ${T.small}; color: ${T.muted}; }`;

export const SHELL_SKIN_CSS = `${RAIL}\n${HOST}`;
