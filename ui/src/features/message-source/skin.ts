/**
 * The look of the source window (ADR 0194) on the token schema only
 * (`var(--nail-…)`), like `../../theme/skin.ts`. The theme compiler emits
 * it after the shared skin, so any `theme.kdl` part rule still wins. The
 * Original / Decoded switcher is the shared one (`skinResponsive.ts`).
 *
 * Parts: `message-source` (`data-view`, `data-state~="wrapped"`):
 * `.controls`, `.views`, `.view`, `.note`, `.text`, `.status`, `.actions`.
 */
import { SCROLL_BOX } from "../../theme/declarations";
import { p } from "../../theme/selectors";
import { T } from "../../theme/tokens";
import { SOURCE_PART } from "./names";

const s = (part: string) => p(`${SOURCE_PART}.${part}`);

export const SOURCE_SKIN_CSS = `${p(SOURCE_PART)} {
  display: flex; flex-direction: column; gap: ${T.sm}; min-height: 0; height: 100%;
}
${s("controls")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.md}; }
${s("note")}, ${s("status")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${s("status")}:empty { display: none; }
${s("text")} {
  ${SCROLL_BOX} padding: ${T.xs};
  font-family: ${T.fontMono}; font-size: ${T.small}; white-space: pre; user-select: text;
}
${p(SOURCE_PART)}[data-state~="wrapped"] ${s("text")} { white-space: pre-wrap; overflow-wrap: anywhere; }
${s("text")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${s("actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; margin-top: auto; }
`;
