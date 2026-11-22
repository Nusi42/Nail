/**
 * The composer's confirmation (M5c WP172; R6, ADR 0239): the modal window
 * `composer-discard:<key>` (the plain name asks about the plain composer),
 * opened by the Discard button of a message with a saved draft or changes —
 * a destructive `ConfirmDialog`: "Discard draft" (red, the draft is
 * deleted) and "Keep editing" (Cancel's role, focused; ×, Esc too).
 *
 * Closing a composer with unsaved changes asks with the window layer's
 * shared prompt instead (`ComposerCloseGuard` in `./Composer.tsx`).
 */
import type { DialogProps } from "../../layout/dialogRegistry";
import { useT } from "../../i18n/I18nProvider";
import { useComposeContext } from "../../state/hooks";
import { useUi } from "../../state/uiStore";
import { ConfirmDialog, DialogFooter } from "../../widgets/dialog";
import { discardRich } from "./flows";
import { composerOf, composerOfConfirm } from "./sessions";

/** The subject of a composer's message, or "New message". */
export function subjectOf(subject: string, t: ReturnType<typeof useT>): string {
  return subject.trim() === "" ? t("window-composer") : subject.trim();
}

/** `composer-discard`: the Discard button of a message worth keeping. */
export function DiscardConfirmDialog({ name, close }: DialogProps) {
  const t = useT();
  const ctx = useComposeContext();
  const window = composerOfConfirm(name);
  const composer = useUi((s) => composerOf(s, window));
  if (!composer) {
    return (
      <div data-part="dialog.confirm" data-confirm="discard" data-state="empty">
        <p data-part="dialog.question">{t("composer-empty")}</p>
        <DialogFooter done={{ onDone: close }} />
      </div>
    );
  }
  return (
    <ConfirmDialog
      kind="discard"
      risk="destructive"
      question={t(
        composer.draftId === null ? "composer-discard-question" : "composer-discard-saved",
        { subject: subjectOf(composer.subject, t) },
      )}
      action={t("composer-discard-draft")}
      onConfirm={() => {
        close();
        void discardRich(ctx, window);
      }}
      cancel={{
        label: t("common-keep-editing"),
        action: "keep-editing",
        onClick: () => {
          close();
          ctx.store.getState().focusWindow(window);
        },
      }}
    />
  );
}
