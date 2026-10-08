/**
 * The look of the connection log feature (ADR 0190) on the token schema
 * only (`var(--nail-…)`), like `../../theme/skin.ts`. The theme compiler
 * emits it after the shared skin, so any `theme.kdl` part rule still wins.
 * Window sizes come from the window specs (`index.ts`), never from here.
 *
 * Parts:
 * - `connection-log` (`data-scope`, `data-state` = `live` / `paused`):
 *   `.header`, `.title`, `.note`, `.controls`, `.filters`, `.actions`,
 *   `.status` (`data-tone`), `.error`, `.empty`, `.list`, `.entry`
 *   (`data-level`, `data-direction`, `data-protocol`, `data-seq`,
 *   `data-highlight`) with `.entry.time`, `.entry.protocol`,
 *   `.entry.connection`, `.entry.direction`, `.entry.level`, `.entry.text`;
 * - `error-details` (`data-step`, `data-code`): `.summary`, `.fields`,
 *   `.field` (`data-field`), `.label`, `.value`, `.code`, `.hint`,
 *   `.status`, `.actions`;
 * - `connection-log-account` (Settings → Accounts): `.heading`, `.empty`,
 *   `.problems`, `.problem` (`data-step`), `.problem.text`;
 * - `connection-log-settings` (Settings → Diagnostics): `.heading`, `.path`;
 * - `application-log`: `.path`, `.empty`, `.lines`, `.actions`.
 */
import { ACCENT_BAR, FIELD_BASIS } from "../../theme/declarations";
import { p } from "../../theme/selectors";
import { T } from "../../theme/tokens";
import { ACCOUNT_PART, APP_LOG_PART, DETAILS_PART, SETTINGS_PART, VIEWER_PART } from "./names";

const v = (part: string) => p(`${VIEWER_PART}.${part}`);
const d = (part: string) => p(`${DETAILS_PART}.${part}`);
const a = (part: string) => p(`${ACCOUNT_PART}.${part}`);
const l = (part: string) => p(`${APP_LOG_PART}.${part}`);

/** Monospace text that keeps its spaces and breaks anywhere (spaces wrap instead of hanging past the box). */
const CODE = `font-family: ${T.fontMono}; font-size: ${T.small}; white-space: break-spaces; overflow-wrap: anywhere;`;

export const CONNLOG_SKIN_CSS = `${p(VIEWER_PART)}, ${p(DETAILS_PART)}, ${p(APP_LOG_PART)} {
  display: flex; flex-direction: column; gap: ${T.sm}; min-height: 0; height: 100%;
}
${v("header")} { display: flex; flex-direction: column; gap: ${T.xxs}; }
${v("title")} { margin: 0; font-size: inherit; font-weight: ${T.semibold}; }
${v("note")}, ${v("status")}, ${v("empty")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${v("status")}[data-tone="error"], ${v("error")} { margin: 0; color: ${T.danger}; }
${v("controls")} { display: flex; flex-wrap: wrap; align-items: flex-end; justify-content: space-between; gap: ${T.sm}; }
${v("filters")}, ${v("actions")} { display: flex; flex-wrap: wrap; align-items: flex-end; gap: ${T.sm}; }
${v("list")} {
  flex: 1 1 auto; min-height: 8em; overflow: auto; margin: 0; padding: 0; list-style: none;
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.subtle};
}
${v("list")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${v("entry")} {
  display: flex; flex-wrap: wrap; align-items: baseline; gap: 0 ${T.sm};
  padding: ${T.xxs} ${T.xs}; border-bottom: 1px solid ${T.border}; border-left: ${ACCENT_BAR} solid transparent;
}
${v("entry")}[data-level="warning"] { border-left-color: ${T.warning}; }
${v("entry")}[data-level="error"] { border-left-color: ${T.danger}; }
${v("entry")}[data-highlight] { background-color: ${T.accentSoft}; border-left-color: ${T.accent}; }
${v("entry.time")}, ${v("entry.protocol")}, ${v("entry.connection")}, ${v("entry.direction")}, ${v("entry.level")} {
  flex: none; color: ${T.muted}; font-size: ${T.small}; font-variant-numeric: tabular-nums;
}
${v("entry.level")} { color: ${T.fg}; font-weight: ${T.semibold}; }
${v("entry.text")} { flex: 1 1 24em; min-width: 0; color: ${T.fg}; ${CODE} }
${d("summary")} { margin: 0; font-weight: ${T.semibold}; overflow-wrap: anywhere; }
${d("fields")} { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: ${T.xs} ${T.md}; margin: 0; }
${d("field")} { display: contents; }
${d("label")} { color: ${T.muted}; }
${d("value")} { margin: 0; min-width: 0; overflow-wrap: anywhere; }
${d("code")} { display: block; padding: ${T.xxs} ${T.xs}; border-radius: ${T.rSm}; background-color: ${T.subtle}; ${CODE} }
${d("hint")}, ${d("status")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${d("actions")}, ${l("actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; margin-top: auto; }
${p(ACCOUNT_PART)}, ${p(SETTINGS_PART)} { display: flex; flex-direction: column; align-items: flex-start; gap: ${T.xs}; margin-top: ${T.md}; }
${a("heading")}, ${p(`${SETTINGS_PART}.heading`)} { margin: ${T.sm} 0 0; font-size: inherit; font-weight: ${T.semibold}; }
${a("empty")}, ${p(`${SETTINGS_PART}.path`)}, ${l("path")}, ${l("empty")} { margin: 0; color: ${T.muted}; overflow-wrap: anywhere; }
${a("problems")} { display: flex; flex-direction: column; gap: ${T.xs}; align-self: stretch; margin: 0; padding: 0; list-style: none; }
${a("problem")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; }
${a("problem.text")} { flex: 1 1 ${FIELD_BASIS}; min-width: 0; overflow-wrap: anywhere; }
${l("lines")} {
  flex: 1 1 auto; min-height: 8em; overflow: auto; margin: 0; padding: ${T.xs};
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.subtle}; ${CODE}
}
${l("lines")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
`;
