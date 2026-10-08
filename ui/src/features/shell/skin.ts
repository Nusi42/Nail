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
import { p } from "../../theme/selectors";
import { initialsAvatar, QUIET_HOVER, TINY_TEXT, VISUALLY_HIDDEN } from "../../theme/declarations";
import { T } from "../../theme/tokens";

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
${p("nav-rail.item")}:hover { ${QUIET_HOVER} }
${p("nav-rail.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${p("nav-rail.item")}:disabled { opacity: ${T.disabled}; }
${p("nav-rail.item")}[aria-current="page"], ${p("nav-rail.item")}[data-state~="active"] {
  color: ${T.accent}; background-color: ${T.accentSoft};
}
${p("nav-rail.icon")} { width: calc(${T.icon} + ${T.xs}); height: calc(${T.icon} + ${T.xs}); }
${p("nav-rail.label")} { ${VISUALLY_HIDDEN} }
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
  font-size: ${TINY_TEXT}; font-weight: ${T.semibold}; line-height: ${BADGE};
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
${p("nav-rail.account")} > ${p("avatar")} { ${initialsAvatar(T.control, T.small)} }`;

/** A grid that hands its one cell's whole room to its child. */
const FILL =
  "display: grid; grid-template: minmax(0, 1fr) / minmax(0, 1fr); min-width: 0; min-height: 0; overflow: hidden;";

const HOST = `${p("view-host")} {
  ${FILL} background-color: ${T.canvas};
}
${p("view-host.view")} {
  ${FILL} outline: none; background-color: ${T.bg};
}
${p("view-host.view")}[hidden] { display: none; }
${p("view-host.missing")} { padding: ${T.md}; font-size: ${T.small}; color: ${T.muted}; }`;

export const SHELL_SKIN_CSS = `${RAIL}\n${HOST}`;
