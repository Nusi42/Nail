/**
 * A plug-in panel: the layout widget `plugin-panel "<plugin>.<panel>"`
 * (ADR 0050). Its content is the panel's component tree, fetched with the
 * UI context (`GetPluginPanel`, again when the selection changes) and
 * replaced when the plug-in re-renders it (`PluginPanelChanged`, handled in
 * `index.ts`). `button` nodes send `PluginUiAction` with the panel's values.
 * Without an enabled plug-in providing the panel it shows a short notice.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { JsonValue, LocalizedMessage, PluginUiNode } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionString, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { isRunnable } from "./commands";
import { clientIdOf, uiContextOf } from "./context";
import { parsePluginPanelId } from "./ids";
import { partAttrs } from "./parts";
import { pluginKeys, usePlugins } from "./queries";
import { PluginTree } from "./tree/PluginTree";
import { mergeValues, type Values } from "./tree/values";

function PanelTree({
  panel,
  plugin,
  tree,
}: {
  panel: string;
  plugin: string;
  tree: PluginUiNode[];
}) {
  const t = useT();
  const client = useClient();
  const [values, setValues] = useState<Values>(() => mergeValues(tree, {}, new Date()));
  const [lastTree, setLastTree] = useState(tree);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  if (lastTree !== tree) {
    setLastTree(tree);
    setValues((previous) => mergeValues(tree, previous, new Date()));
  }
  const action = async (componentId: string) => {
    setBusy(true);
    setProblem(null);
    try {
      await client.request(
        {
          type: "PluginUiAction",
          target: { kind: "panel", panel },
          component_id: componentId,
          values,
          client_id: clientIdOf(client),
        },
        "Done",
      );
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      {problem && (
        <p {...partAttrs("plugin.panel.error", plugin)} role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <PluginTree
        plugin={plugin}
        nodes={tree}
        values={values}
        issues={{}}
        busy={busy}
        onChange={(id: string, value: JsonValue) => {
          setValues((previous) => ({ ...previous, [id]: value }));
        }}
        onAction={(id) => void action(id)}
      />
    </>
  );
}

export function PluginPanel({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const panel = optionString(node, "panel", "");
  const parsed = parsePluginPanelId(panel);
  const plugins = usePlugins();
  const plugin = parsed ? plugins.data?.find((p) => p.id === parsed.plugin) : undefined;
  const def = plugin?.manifest.contributes?.panels?.find((p) => p.id === parsed?.panel);
  const available = plugin !== undefined && def !== undefined && isRunnable(plugin);

  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const messageId = useUi((s) => s.selectedMessageId);
  const threadId = useUi((s) => s.selectedThreadId);
  const draftId = useUi((s) => s.composer?.draftId ?? null);
  const contextKey = JSON.stringify([accountId, folderId, messageId, threadId, draftId]);
  const context = useMemo(
    () =>
      uiContextOf({
        activeAccountId: accountId,
        activeFolderId: folderId,
        selectedMessageId: messageId,
        selectedThreadId: threadId,
        composer: draftId === null ? null : { draftId },
      }),
    [accountId, folderId, messageId, threadId, draftId],
  );

  const content = useQuery({
    queryKey: pluginKeys.panel(panel, contextKey),
    queryFn: async () =>
      (
        await client.request(
          { type: "GetPluginPanel", panel, context, client_id: clientIdOf(client) },
          "PluginPanel",
        )
      ).tree,
    enabled: available,
  });

  const title = def?.title ?? (node.label ? label(node.label) : t("plugin-panel-label"));
  const error = content.error ? errorMessageOf(content.error) : null;
  return (
    <section
      data-part="plugin-panel"
      data-panel={panel}
      data-plugin={parsed?.plugin}
      data-state={available ? undefined : "missing"}
      data-node-id={node.id ?? undefined}
      aria-label={title}
      aria-busy={content.isFetching}
    >
      {!available ? (
        <p data-part="plugin-panel.missing">{t("plugin-panel-missing")}</p>
      ) : error ? (
        <p {...partAttrs("plugin.panel.error", parsed?.plugin ?? "")} role="alert">
          {t(error.id, error.args)}
        </p>
      ) : content.data ? (
        <PanelTree panel={panel} plugin={parsed?.plugin ?? ""} tree={content.data} />
      ) : null}
    </section>
  );
}
