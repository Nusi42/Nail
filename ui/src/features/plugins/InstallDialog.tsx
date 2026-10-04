/**
 * The install dialog (`window.open "plugin-install"`, ADR 0050):
 *
 * 1. Source: a `.nailplugin` file, an https URL, or (development) an
 *    unpacked folder.
 * 2. Review: `PreviewPluginInstall` shows the manifest — name, version,
 *    author, what it adds — its problems (installing is refused while there
 *    are any) and every permission it asks for, each allowed or denied
 *    (mail permissions optionally limited to accounts).
 * 3. Install: `InstallPlugin` with the previewed package hash, so exactly
 *    the reviewed package is installed (`LoadDevPlugin` for a folder).
 * 4. Placements: the plug-in's suggested toolbar / menu / key / panel
 *    placements, confirmed like a merge (`./Placements.tsx`).
 */
import { useId, useState } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import type {
  LocalizedMessage,
  PluginGrant,
  PluginInfo,
  PluginInstallPreview,
  PluginManifest,
  PluginPackageSource,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useAccounts, useClient } from "../../state/queries";
import { TextField } from "../../widgets/forms";
import { contributionItems } from "./extensions";
import { allGranted, grantsValid, orderedGrants } from "./grants";
import { Permissions } from "./Permissions";
import { Placements } from "./Placements";
import { usePluginsState } from "./store";

/** Dialog name. */
export const INSTALL_DIALOG = "plugin-install";

export type SourceKind = PluginPackageSource["kind"];
export const SOURCE_KINDS: readonly SourceKind[] = ["file", "url", "dev"];

/** The package source from the form, or `null` when the text is empty. */
export function sourceOf(kind: SourceKind, text: string): PluginPackageSource | null {
  const value = text.trim();
  if (value === "") return null;
  switch (kind) {
    case "file":
      return { kind: "file", path: value };
    case "url":
      return { kind: "url", url: value };
    case "dev":
      return { kind: "dev", folder: value };
  }
}

/** Whether a manifest suggests anything for the layout. */
export function hasPlacements(manifest: PluginManifest): boolean {
  const c = manifest.contributes;
  return (c?.placements?.length ?? 0) > 0 || (c?.keys?.length ?? 0) > 0;
}

type Stage =
  | { kind: "source" }
  | { kind: "review"; preview: PluginInstallPreview; grants: PluginGrant[]; enable: boolean }
  | { kind: "placements"; plugin: PluginInfo }
  | { kind: "done"; plugin: PluginInfo };

function Contributions({ manifest }: { manifest: PluginManifest }) {
  const t = useT();
  const items = contributionItems(manifest);
  if (items.length === 0) return null;
  return (
    <section data-part="plugin-install.adds">
      <h3 data-part="plugin-install.heading">{t("plugin-install-adds")}</h3>
      <ul data-part="plugin-install.contributions">
        {items.map((item) => (
          <li key={item.key} data-part="plugin-install.contribution" data-kind={item.kind}>
            {item.kind === "hook"
              ? t(item.id, { hook: t(item.args.hook ?? "") })
              : t(item.id, item.args)}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Review({
  stage,
  busy,
  onChange,
  onInstall,
  onBack,
}: {
  stage: Extract<Stage, { kind: "review" }>;
  busy: boolean;
  onChange: (stage: Extract<Stage, { kind: "review" }>) => void;
  onInstall: () => void;
  onBack: () => void;
}) {
  const t = useT();
  const accounts = useAccounts();
  const { preview } = stage;
  const manifest = preview.manifest;
  const blocked = manifest === null || preview.issues.length > 0;
  return (
    <div data-part="plugin-install.review">
      {manifest && (
        <section data-part="plugin-install.summary">
          <h3 data-part="plugin-install.name">
            {t("plugin-install-name", { name: manifest.name, version: manifest.version })}
          </h3>
          {manifest.author !== undefined && (
            <p data-part="plugin-install.author">
              {t("plugin-install-author", { author: manifest.author })}
            </p>
          )}
          {manifest.description !== undefined && (
            <p data-part="plugin-install.description">{manifest.description}</p>
          )}
          {manifest.homepage !== undefined && (
            <p data-part="plugin-install.homepage">
              {t("plugin-install-homepage", { url: manifest.homepage })}
            </p>
          )}
          <p data-part="plugin-install.package">
            {t("plugin-install-package", {
              size: preview.size,
              hash: preview.package_sha256.slice(0, 12),
            })}
          </p>
          {preview.installed_version !== null && (
            <p data-part="plugin-install.update" role="status">
              {t("plugin-install-update", {
                installed: preview.installed_version,
                version: manifest.version,
              })}
            </p>
          )}
          {preview.source.kind === "dev" && (
            <p data-part="plugin-install.dev">{t("plugin-install-dev-note")}</p>
          )}
        </section>
      )}
      {preview.issues.length > 0 && (
        <section data-part="plugin-install.issues" role="alert">
          <h3 data-part="plugin-install.heading">
            {t("plugin-install-issues", { count: preview.issues.length })}
          </h3>
          <ul data-part="plugin-install.issue-list">
            {preview.issues.map((issue, index) => (
              <li key={index} data-part="plugin-install.issue" data-kind={issue.kind}>
                {t(`plugin-manifest-${issue.kind}`, {
                  path: issue.path,
                  detail: issue.detail ?? "",
                })}
              </li>
            ))}
          </ul>
        </section>
      )}
      {manifest && !blocked && (
        <>
          <Contributions manifest={manifest} />
          <section data-part="plugin-install.permissions">
            <h3 data-part="plugin-install.heading">{t("plugin-install-permissions")}</h3>
            <Permissions
              permissions={manifest.permissions ?? []}
              grants={stage.grants}
              accounts={accounts.data ?? []}
              disabled={busy}
              onChange={(grants) => {
                onChange({ ...stage, grants });
              }}
            />
          </section>
          {preview.source.kind !== "dev" && (
            <label data-part="plugin-install.enable">
              <input
                type="checkbox"
                data-field="enable"
                checked={stage.enable}
                disabled={busy}
                onChange={(event) => {
                  onChange({ ...stage, enable: event.target.checked });
                }}
              />
              {t("plugin-install-enable")}
            </label>
          )}
        </>
      )}
      <div data-part="plugin-install.actions">
        {!blocked && (
          <button
            type="button"
            data-part="form.button"
            data-action="install"
            data-variant="primary"
            disabled={busy || !grantsValid(stage.grants)}
            onClick={onInstall}
          >
            {t(
              preview.installed_version === null
                ? "plugin-install-install"
                : "plugin-install-update-button",
            )}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          data-action="back"
          disabled={busy}
          onClick={onBack}
        >
          {t("plugin-install-back")}
        </button>
      </div>
    </div>
  );
}

/** Dialog `plugin-install`. */
export function InstallDialog({ close }: DialogProps) {
  const t = useT();
  const client = useClient();
  const installMode = usePluginsState((s) => s.installMode);
  const [kind, setKind] = useState<SourceKind>(installMode === "dev" ? "dev" : "file");
  const [text, setText] = useState("");
  const [stage, setStage] = useState<Stage>({ kind: "source" });
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const kindName = useId();

  const preview = async () => {
    const source = sourceOf(kind, text);
    if (!source) return;
    setBusy(true);
    setProblem(null);
    try {
      const r = await client.request(
        { type: "PreviewPluginInstall", source },
        "PluginInstallPreview",
      );
      setStage({
        kind: "review",
        preview: r.preview,
        grants: allGranted(r.preview.manifest?.permissions ?? []),
        enable: true,
      });
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    if (stage.kind !== "review" || !stage.preview.manifest) return;
    const { preview: p, enable } = stage;
    const manifest = stage.preview.manifest;
    const grants = orderedGrants(manifest.permissions ?? [], stage.grants);
    setBusy(true);
    setProblem(null);
    try {
      const r =
        p.source.kind === "dev"
          ? await client.request(
              { type: "LoadDevPlugin", folder: p.source.folder, grants },
              "Plugin",
            )
          : await client.request(
              {
                type: "InstallPlugin",
                source: p.source,
                package_sha256: p.package_sha256,
                grants,
                enable,
              },
              "Plugin",
            );
      setStage(
        hasPlacements(r.plugin.manifest)
          ? { kind: "placements", plugin: r.plugin }
          : { kind: "done", plugin: r.plugin },
      );
    } catch (error) {
      setProblem(
        error instanceof NailError && error.code === "conflict"
          ? { id: "plugin-install-changed" }
          : errorMessageOf(error),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-part="plugin-install" data-stage={stage.kind} aria-busy={busy}>
      {problem && (
        <p data-part="plugin-install.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {stage.kind === "source" && (
        <form
          data-part="plugin-install.source"
          onSubmit={(event) => {
            event.preventDefault();
            void preview();
          }}
        >
          <p data-part="plugin-install.intro">{t("plugin-install-intro")}</p>
          <fieldset data-part="plugin-install.kinds">
            <legend data-part="plugin-install.kinds.legend">{t("plugin-install-source")}</legend>
            {SOURCE_KINDS.map((k) => (
              <label key={k} data-part="plugin-install.kind">
                <input
                  type="radio"
                  name={kindName}
                  data-kind={k}
                  checked={kind === k}
                  onChange={() => {
                    setKind(k);
                  }}
                />
                {t(`plugin-install-source-${k}`)}
              </label>
            ))}
          </fieldset>
          <TextField
            labelId={`plugin-install-location-${kind}`}
            name="location"
            value={text}
            required
            onChange={setText}
          />
          <div data-part="plugin-install.actions">
            <button
              type="submit"
              data-part="form.button"
              data-action="preview"
              data-variant="primary"
              disabled={busy || text.trim() === ""}
            >
              {t("plugin-install-preview")}
            </button>
            <button type="button" data-part="form.button" data-action="cancel" onClick={close}>
              {t("plugin-install-cancel")}
            </button>
          </div>
        </form>
      )}
      {stage.kind === "review" && (
        <Review
          stage={stage}
          busy={busy}
          onChange={setStage}
          onInstall={() => void install()}
          onBack={() => {
            setProblem(null);
            setStage({ kind: "source" });
          }}
        />
      )}
      {stage.kind === "placements" && (
        <>
          <p data-part="plugin-install.status" role="status">
            {t("plugin-install-done", { name: stage.plugin.manifest.name })}
          </p>
          <Placements
            pluginId={stage.plugin.id}
            onDone={() => {
              setStage({ kind: "done", plugin: stage.plugin });
            }}
          />
        </>
      )}
      {stage.kind === "done" && (
        <>
          <p data-part="plugin-install.status" role="status">
            {t("plugin-install-done", { name: stage.plugin.manifest.name })}
          </p>
          <div data-part="plugin-install.actions">
            <button type="button" data-part="form.button" data-action="close" onClick={close}>
              {t("plugin-install-close")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
