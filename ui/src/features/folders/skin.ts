/**
 * The look of the folder windows (ADR 0196) on the token schema only
 * (`var(--nail-…)`), like `../../theme/skin.ts`; the theme compiler emits
 * it after the shared skin. The folder list is the shared `folder-picker`
 * one.
 *
 * Parts: `folder-dialog` (`data-dialog` = `name` / `move` / `delete`):
 * `.intro`, `.note`, `.actions`.
 */
import { p } from "../../theme/selectors";
import { T } from "../../theme/tokens";
import { FOLDER_DIALOG_PART } from "./names";

const d = (part: string) => p(`${FOLDER_DIALOG_PART}.${part}`);

export const FOLDERS_SKIN_CSS = `${p(FOLDER_DIALOG_PART)} {
  display: flex; flex-direction: column; gap: ${T.sm}; min-height: 0; height: 100%;
}
${d("intro")} { margin: 0; overflow-wrap: anywhere; }
${d("note")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${p(FOLDER_DIALOG_PART)} ${p("folder-picker.list")} { flex: 1 1 auto; min-height: 0; overflow: auto; }
${d("actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; margin-top: auto; }
`;
