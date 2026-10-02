/**
 * Composer extensions (M4a, ADR 0079): plug-in composer actions (insert or
 * transform text) and custom Markdown blocks that the plug-in renders to
 * HTML when the message is sent.
 *
 * Mount points (composer):
 * - `ComposerExtensionBar` in the composer toolbar: a menu of composer
 *   actions (sent as `RunPluginAction` with the draft and selection) and a
 *   menu of Markdown blocks (inserted as a fenced block through
 *   `onInsert`).
 * - `useComposerEdits(draftId, apply)`: the composer applies edits that
 *   plug-ins and agents ask for (`ComposerEditRequested`, queued by the
 *   feature's event handler); `applyComposerEdit` does it for plain text.
 * - `ComposerEditNotices`: who just edited the draft ("shown live").
 * - `PluginMarkdownBlock`: the composer's Markdown preview draws a fenced
 *   block whose info string names a plug-in block with this (the HTML is
 *   made at send time, so the preview shows the source and who renders it).
 */
import { useEffect, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { ComposerSelection, DraftId, LocalizedMessage } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { attributionMessage } from "../agents/model";
import { runPluginAction } from "./ActionMenus";
import { ExtensionMenu } from "./ExtensionMenu";
import {
  blockFence,
  composerActionsFor,
  markdownBlockOwner,
  markdownBlocksFor,
  type BlockOffer,
} from "./extensions";
import { usePlugins } from "./queries";
import { pluginsStoreFor, usePluginsState, type PendingEdit } from "./store";

interface BarProps {
  /** The saved draft (`null` before the first save: `ensureDraft` saves it). */
  draftId: DraftId | null;
  /** Saves the draft if needed; returns its id (`null` = could not save). */
  ensureDraft?(): Promise<DraftId | null>;
  /** The current selection in the draft's source. */
  getSelection(): ComposerSelection;
  /** Inserts Markdown at the cursor (Markdown mode only; omit to hide blocks). */
  onInsert?(markdown: string): void;
}

/** The composer's plug-in menus (actions and Markdown blocks). */
export function ComposerExtensionBar({ draftId, ensureDraft, getSelection, onInsert }: BarProps) {
  const t = useT();
  const client = useClient();
  const store = useUiStoreApi();
  const plugins = usePlugins();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const actions = composerActionsFor(plugins.data ?? []);
  const blocks = onInsert ? markdownBlocksFor(plugins.data ?? []) : [];
  if (actions.length === 0 && blocks.length === 0) return null;

  const run = async (action: string) => {
    setBusy(true);
    setProblem(null);
    try {
      const id = draftId ?? (ensureDraft ? await ensureDraft() : null);
      if (id === null) {
        setProblem({ id: "plugin-composer-save-first" });
        return;
      }
      await runPluginAction(client, store, action, {
        kind: "composer",
        draft_id: id,
        selection: getSelection(),
      });
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-part="plugin-composer" role="group" aria-label={t("plugin-composer-label")}>
      <ExtensionMenu
        kind="composer"
        labelId="plugin-composer-actions"
        items={actions}
        disabled={busy}
        onPick={(item) => void run(item.id)}
      />
      <ExtensionMenu
        kind="markdown-block"
        labelId="plugin-composer-blocks"
        items={blocks}
        disabled={busy}
        onPick={(item) => onInsert?.(blockFence(item as BlockOffer))}
      />
      {problem && (
        <span data-part="plugin-composer.error" role="alert">
          {t(problem.id, problem.args)}
        </span>
      )}
    </div>
  );
}

/**
 * Applies the edits queued for `draftId` as they arrive (oldest first).
 * `apply` gets each edit once; edits for other drafts stay queued.
 */
export function useComposerEdits(
  draftId: DraftId | null,
  apply: (edit: PendingEdit) => void,
): void {
  const store = useUiStoreApi();
  const edits = usePluginsState((s) => s.edits);
  useEffect(() => {
    if (draftId === null || !edits.some((e) => e.draftId === draftId)) return;
    for (const edit of pluginsStoreFor(store).getState().takeEdits(draftId)) apply(edit);
  }, [apply, draftId, edits, store]);
}

/** How long "edited by" stays after the last edit (ms). */
export const EDIT_NOTICE_MS = 6000;

/** Who just edited the draft (a live region next to the composer). */
export function ComposerEditNotices({ draftId }: { draftId: DraftId | null }) {
  const t = useT();
  const latest = usePluginsState((s) =>
    draftId === null ? undefined : s.recent.findLast((e) => e.draftId === draftId),
  );
  const [hidden, setHidden] = useState<number | null>(null);
  useEffect(() => {
    if (!latest) return;
    const seq = latest.seq;
    const timer = setTimeout(() => setHidden(seq), EDIT_NOTICE_MS);
    return () => clearTimeout(timer);
  }, [latest]);
  const shown = latest && latest.seq !== hidden ? latest : null;
  const actor = shown ? attributionMessage(shown.by) : null;
  return (
    <p data-part="plugin-composer.edited" role="status" aria-live="polite">
      {shown && actor && (
        <span data-part="plugin-composer.edited.text" data-actor={shown.by.actor.kind}>
          {t(`plugin-composer-edited-${shown.edit.op}`, { actor: t(actor.id, actor.args) })}
        </span>
      )}
    </p>
  );
}

/** A fenced Markdown block in the composer's preview (info string = `<plugin>.<block>`). */
export function PluginMarkdownBlock({ info, body }: { info: string; body: string }) {
  const t = useT();
  const plugins = usePlugins();
  const owner = markdownBlockOwner(plugins.data ?? [], info);
  return (
    <figure
      data-part="plugin-block"
      data-block={info}
      data-plugin={owner?.pluginId}
      data-state={owner ? "rendered-on-send" : "unknown"}
    >
      <figcaption data-part="plugin-block.caption">
        {owner
          ? t("plugin-block-rendered-by", { block: owner.title, plugin: owner.pluginName })
          : t("plugin-block-unknown", { block: info })}
      </figcaption>
      <pre data-part="plugin-block.source">
        <code>{body}</code>
      </pre>
    </figure>
  );
}
