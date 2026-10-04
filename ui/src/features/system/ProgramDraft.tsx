/**
 * Mail from other programs (Simple MAPI "Send to → Mail recipient", Office
 * "Send as attachment"; `LaunchRequest::ComposeDraft`, ADR 0087). The core
 * already saved the draft with the program's recipients, subject, text and
 * files; `openProgramDraft` opens it in the composer and remembers which
 * program asked, and `ProgramDraftNotice` (mounted in the composer) says
 * so: "WINWORD.EXE prepared this message. Check it, then press Send." Nail
 * never sends for another program — also not when it asked to send
 * without a window.
 */
import type { DraftId, LaunchRequest } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { ComposeContext } from "../../state/compose";
import { useUi } from "../../state/uiStore";
import { openRichDraft } from "../composer";
import { systemStoreFor, useSystemState } from "./store";
import { composerOf, useComposerWindowOf } from "../composer/windowOf";

/** A notice for the app's launch notice area: message id and arguments. */
export interface ProgramDraftMessage {
  id: string;
  args?: Record<string, string>;
}

/** The notice's message for a program (or an unknown one). */
export function programDraftMessage(program: string | null): ProgramDraftMessage {
  return program === null || program.trim() === ""
    ? { id: "system-program-draft-unknown" }
    : { id: "mapi-draft-from-program", args: { program: program.trim() } };
}

/**
 * Handles `LaunchRequest::ComposeDraft` (called by `features/launch`):
 * opens the draft in the composer and notes the program for the composer
 * notice. Returns `null` (the composer shows the notice itself).
 */
export async function openProgramDraft(
  ctx: ComposeContext,
  request: Extract<LaunchRequest, { type: "ComposeDraft" }>,
): Promise<null> {
  systemStoreFor(ctx.store).getState().noteProgramDraft(request.draft_id, request.program);
  await openRichDraft(ctx, request.draft_id);
  return null;
}

/**
 * The composer's notice for a draft another program prepared; renders
 * nothing for other drafts. `draftId` defaults to the open composer's.
 */
export function ProgramDraftNotice({ draftId }: { draftId?: DraftId | null }) {
  const t = useT();
  // The composer of this notice's own window (M5c: several composers).
  const window = useComposerWindowOf();
  const open = useUi((s) => composerOf(s, window)?.draftId ?? null);
  const id = draftId === undefined ? open : draftId;
  const drafts = useSystemState((s) => s.programDrafts);
  const forget = useSystemState((s) => s.forgetProgramDraft);
  if (id === null || !Object.hasOwn(drafts, id)) return null;
  const message = programDraftMessage(drafts[id] ?? null);
  return (
    <div data-part="program-draft" role="note" aria-label={t("system-program-draft-label")}>
      <p data-part="program-draft.text">{t(message.id, message.args)}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="dismiss"
        onClick={() => {
          forget(id);
        }}
      >
        {t("system-program-draft-dismiss")}
      </button>
    </div>
  );
}
