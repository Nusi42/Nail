/**
 * Placements proposed on install (ADR 0050): the core writes the plug-in's
 * suggested toolbar / menu items, key bindings and panels into a copy of
 * the user's `layout.kdl` and compares it with the real file
 * (`PreviewPluginPlacements` → the merge dialog's `ConfigMergePreview`).
 * The user picks the changes to take — all proposed ones are preselected —
 * and applies them with the merge dialog's request (`ApplyConfigMerge`,
 * which backs up the file first; "Undo" reverts to that backup). Nothing is
 * written without "Add to layout".
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import type { ConfigBackup, LocalizedMessage, MergeChoice, MergeItem } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { differs } from "../customization/merge";
import { backupsKey } from "../customization/MergeDialog";

interface Props {
  pluginId: string;
  /** Called when the user is done (applied, skipped or nothing to place). */
  onDone(): void;
}

/** The merge choices: checked items taken from the proposal, the rest kept. */
export function placementChoices(
  items: readonly MergeItem[],
  skipped: ReadonlySet<string>,
): MergeChoice[] {
  return items
    .filter(differs)
    .map((item) => ({ item_id: item.id, take: skipped.has(item.id) ? "mine" : "theirs" }));
}

type Stage = { kind: "choose" } | { kind: "applied"; backup: ConfigBackup | null };

export function Placements({ pluginId, onDone }: Props) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const [stage, setStage] = useState<Stage>({ kind: "choose" });
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const proposal = useQuery({
    queryKey: ["plugins", "placements", pluginId],
    queryFn: () =>
      client.request({ type: "PreviewPluginPlacements", plugin_id: pluginId }, "PluginPlacements"),
    gcTime: 0,
    staleTime: 0,
  });

  if (proposal.isPending) {
    return (
      <section data-part="plugin-placements" aria-busy="true">
        <p data-part="plugin-placements.status" role="status">
          {t("plugin-placements-loading")}
        </p>
      </section>
    );
  }

  if (proposal.isError) {
    const unsupported =
      proposal.error instanceof NailError && proposal.error.code === "unsupported";
    const message = unsupported
      ? { id: "plugin-placements-unavailable" }
      : errorMessageOf(proposal.error);
    return (
      <section data-part="plugin-placements">
        <p data-part="plugin-placements.error" role="alert">
          {t(message.id, message.args)}
        </p>
        <div data-part="plugin-placements.actions">
          <button type="button" data-part="form.button" data-action="done" onClick={onDone}>
            {t("plugin-placements-close")}
          </button>
        </div>
      </section>
    );
  }

  const { preview, incoming } = proposal.data;
  const items = preview.sections.flatMap((s) => s.items).filter(differs);

  if (stage.kind === "applied") {
    const undo = async () => {
      if (!stage.backup) return;
      setBusy(true);
      setProblem(null);
      try {
        await client.request(
          { type: "RevertConfig", file: stage.backup.file, backup_id: stage.backup.id },
          "Done",
        );
        void queryClient.invalidateQueries({ queryKey: backupsKey(stage.backup.file) });
        setStage({ kind: "applied", backup: null });
        onDone();
      } catch (error) {
        setProblem(errorMessageOf(error));
      } finally {
        setBusy(false);
      }
    };
    return (
      <section data-part="plugin-placements" data-stage="applied">
        <p data-part="plugin-placements.status" role="status">
          {t("plugin-placements-applied")}
        </p>
        {problem && (
          <p data-part="plugin-placements.error" role="alert">
            {t(problem.id, problem.args)}
          </p>
        )}
        <div data-part="plugin-placements.actions">
          {stage.backup && (
            <button
              type="button"
              data-part="form.button"
              data-action="undo"
              disabled={busy}
              onClick={() => void undo()}
            >
              {t("plugin-placements-undo")}
            </button>
          )}
          <button type="button" data-part="form.button" data-action="done" onClick={onDone}>
            {t("plugin-placements-close")}
          </button>
        </div>
      </section>
    );
  }

  if (preview.errors.length > 0 || items.length === 0) {
    return (
      <section data-part="plugin-placements">
        <p
          data-part={
            preview.errors.length > 0 ? "plugin-placements.error" : "plugin-placements.status"
          }
          role={preview.errors.length > 0 ? "alert" : "status"}
        >
          {preview.errors.length > 0
            ? t("plugin-placements-invalid", { count: preview.errors.length })
            : t("plugin-placements-nothing")}
        </p>
        <div data-part="plugin-placements.actions">
          <button type="button" data-part="form.button" data-action="done" onClick={onDone}>
            {t("plugin-placements-close")}
          </button>
        </div>
      </section>
    );
  }

  const apply = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const r = await client.request(
        {
          type: "ApplyConfigMerge",
          file: preview.file,
          incoming,
          choices: placementChoices(items, skipped),
          default: "mine",
        },
        "ConfigMergeApplied",
      );
      void queryClient.invalidateQueries({ queryKey: backupsKey(preview.file) });
      setStage({ kind: "applied", backup: r.backup });
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const taken = items.filter((i) => !skipped.has(i.id)).length;
  return (
    <section data-part="plugin-placements" data-stage="choose" aria-busy={busy}>
      <p data-part="plugin-placements.intro">{t("plugin-placements-intro")}</p>
      {problem && (
        <p data-part="plugin-placements.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <ul data-part="plugin-placements.items">
        {items.map((item) => (
          <li
            key={item.id}
            data-part="plugin-placements.item"
            data-change={item.change}
            data-state={skipped.has(item.id) ? "skipped" : "taken"}
            data-conflict={item.conflicts.length > 0 ? "" : undefined}
          >
            <label data-part="plugin-placements.choice">
              <input
                type="checkbox"
                data-item={item.id}
                checked={!skipped.has(item.id)}
                onChange={(event) => {
                  const next = new Set(skipped);
                  if (event.target.checked) next.delete(item.id);
                  else next.add(item.id);
                  setSkipped(next);
                }}
              />
              <code data-part="plugin-placements.key">{item.key}</code>
            </label>
            {item.theirs !== null && <pre data-part="plugin-placements.text">{item.theirs}</pre>}
            {item.conflicts.length > 0 && (
              <ul data-part="plugin-placements.conflicts">
                {item.conflicts.map((conflict, index) => (
                  <li key={index} data-part="plugin-placements.conflict" data-kind={conflict.kind}>
                    {conflict.localized
                      ? t(conflict.localized.id, conflict.localized.args)
                      : t(`merge-conflict-${conflict.kind}`)}
                  </li>
                ))}
              </ul>
            )}
          </li>
        ))}
      </ul>
      <div data-part="plugin-placements.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="apply"
          data-variant="primary"
          disabled={busy || taken === 0}
          onClick={() => void apply()}
        >
          {t("plugin-placements-apply", { count: taken })}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="skip"
          disabled={busy}
          onClick={onDone}
        >
          {t("plugin-placements-skip")}
        </button>
      </div>
    </section>
  );
}
