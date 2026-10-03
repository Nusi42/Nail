/**
 * The composer's confirmations (M5c WP172) — the only modal composer
 * windows (`{ confirmation: true }` in `./index.ts`), one per composer
 * (`composer-close:<key>`, `composer-discard:<key>`; the plain names ask
 * about the plain composer):
 *
 * - `composer-close`: the composer has unsaved changes and is being closed —
 *   "Save draft" (then close), "Close without saving" (the last saved draft
 *   stays), "Keep editing" (the default).
 * - `composer-discard`: the Discard button of a message with a saved draft
 *   or changes — "Discard" (the draft is deleted), "Keep editing".
 */
import { useEffect, useRef, useState } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useT } from "../../i18n/I18nProvider";
import { useComposeContext } from "../../state/hooks";
import { useUi } from "../../state/uiStore";
import { closeComposerNow, discardRich, saveRich } from "./flows";
import { composerOf, composerOfConfirm } from "./sessions";

function useComposerOfDialog(name: string) {
  const window = composerOfConfirm(name);
  const composer = useUi((s) => composerOf(s, window));
  return { window, composer };
}

/**
 * Focuses the safe choice ("Keep editing") when the confirmation opens
 * (after the window frame focused its first button).
 */
function useSafeFocus() {
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => queueMicrotask(() => ref.current?.focus()), []);
  return ref;
}

function Gone({ close }: { close(): void }) {
  const t = useT();
  return (
    <div data-part="composer.confirm" data-state="empty">
      <p data-part="composer.confirm.question">{t("composer-empty")}</p>
      <div data-part="composer.confirm.actions">
        <button type="button" data-part="form.button" onClick={close}>
          {t("composer-confirm-ok")}
        </button>
      </div>
    </div>
  );
}

function subjectOf(subject: string, t: ReturnType<typeof useT>): string {
  return subject.trim() === "" ? t("window-composer") : subject.trim();
}

/** `composer-close`: closing a composer with unsaved changes. */
export function CloseConfirmDialog({ name, close }: DialogProps) {
  const t = useT();
  const ctx = useComposeContext();
  const { window, composer } = useComposerOfDialog(name);
  const keep = useSafeFocus();
  const [busy, setBusy] = useState(false);
  if (!composer) return <Gone close={close} />;

  const save = async () => {
    setBusy(true);
    const saved = await saveRich(ctx, { window });
    setBusy(false);
    close();
    if (saved !== null) closeComposerNow(ctx, window);
    else ctx.store.getState().focusWindow(window);
  };

  return (
    <div data-part="composer.confirm" data-confirm="close" aria-busy={busy}>
      <p data-part="composer.confirm.question">
        {t("composer-close-question", { subject: subjectOf(composer.subject, t) })}
      </p>
      <div data-part="composer.confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="save-close"
          data-variant="primary"
          disabled={busy}
          onClick={() => void save()}
        >
          {t("composer-close-save")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="close-unsaved"
          disabled={busy}
          onClick={() => {
            close();
            closeComposerNow(ctx, window);
          }}
        >
          {t("composer-close-discard")}
        </button>
        <button
          ref={keep}
          type="button"
          data-part="form.button"
          data-action="keep-editing"
          disabled={busy}
          onClick={() => {
            close();
            ctx.store.getState().focusWindow(window);
          }}
        >
          {t("composer-confirm-keep")}
        </button>
      </div>
    </div>
  );
}

/** `composer-discard`: the Discard button of a message worth keeping. */
export function DiscardConfirmDialog({ name, close }: DialogProps) {
  const t = useT();
  const ctx = useComposeContext();
  const { window, composer } = useComposerOfDialog(name);
  const keep = useSafeFocus();
  if (!composer) return <Gone close={close} />;
  return (
    <div data-part="composer.confirm" data-confirm="discard">
      <p data-part="composer.confirm.question">
        {t(composer.draftId === null ? "composer-discard-question" : "composer-discard-saved", {
          subject: subjectOf(composer.subject, t),
        })}
      </p>
      <div data-part="composer.confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="discard"
          data-variant="danger"
          onClick={() => {
            close();
            void discardRich(ctx, window);
          }}
        >
          {t("composer-discard")}
        </button>
        <button
          ref={keep}
          type="button"
          data-part="form.button"
          data-action="keep-editing"
          onClick={() => {
            close();
            ctx.store.getState().focusWindow(window);
          }}
        >
          {t("composer-confirm-keep")}
        </button>
      </div>
    </div>
  );
}
