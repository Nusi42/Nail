/**
 * Plug-in actions on attachments and links (M4a, ADR 0079): menus listing
 * the actions of running plug-ins that apply ("Save to…", "Open in…").
 * Picking one sends `RunPluginAction`; what the plug-in does arrives as
 * events (dialogs, notifications, composer edits).
 *
 * Mount points: `AttachmentActionMenu` next to each attachment in the
 * reader's attachment list; `LinkActionMenu` where a link is offered to
 * the user (link warning, link context menu). `linkActionsFor` /
 * `runPluginAction` serve other menus (e.g. the layout's context menus).
 */
import { useState } from "react";
import type { NailClient } from "../../api/client";
import { errorMessageOf } from "../../api/client";
import type {
  AttachmentInfo,
  LocalizedMessage,
  MessageId,
  PluginActionTarget,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import type { UiStore } from "../../state/uiStore";
import { clientIdOf, uiContextOf } from "./context";
import { ExtensionMenu } from "./ExtensionMenu";
import { attachmentActionsFor, linkActionsFor, type Offer } from "./extensions";
import { usePlugins } from "./queries";

/** Runs a plug-in action (`<plugin>.<name>`) at `target` with this UI's context. */
export async function runPluginAction(
  client: NailClient,
  store: UiStore,
  action: string,
  target: PluginActionTarget,
): Promise<void> {
  await client.request(
    {
      type: "RunPluginAction",
      action,
      target,
      context: uiContextOf(store.getState()),
      client_id: clientIdOf(client),
    },
    "Done",
  );
}

/** A menu plus the problem of the last action, if any. */
function ActionMenu({
  kind,
  labelId,
  labelArgs,
  items,
  target,
}: {
  kind: string;
  labelId: string;
  labelArgs: Record<string, string>;
  items: Offer[];
  target: PluginActionTarget;
}) {
  const t = useT();
  const client = useClient();
  const store = useUiStoreApi();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  if (items.length === 0) return null;
  const pick = async (item: Offer) => {
    setBusy(true);
    setProblem(null);
    try {
      await runPluginAction(client, store, item.id, target);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <span data-part="plugin-actions" data-menu={kind} aria-busy={busy}>
      <ExtensionMenu
        kind={kind}
        labelId={labelId}
        labelArgs={labelArgs}
        items={items}
        disabled={busy}
        onPick={(item) => void pick(item)}
      />
      {problem && (
        <span data-part="plugin-actions.error" role="alert">
          {t(problem.id, problem.args)}
        </span>
      )}
    </span>
  );
}

/** The plug-in actions of one attachment (reader attachment list). */
export function AttachmentActionMenu({
  messageId,
  attachment,
  accountId = null,
}: {
  messageId: MessageId;
  attachment: AttachmentInfo;
  accountId?: string | null;
}) {
  const plugins = usePlugins();
  const items = attachmentActionsFor(plugins.data ?? [], attachment, accountId);
  return (
    <ActionMenu
      kind="attachment"
      labelId="plugin-attachment-actions"
      labelArgs={{ name: attachment.filename ?? attachment.mime }}
      items={items}
      target={{ kind: "attachment", message_id: messageId, index: attachment.index }}
    />
  );
}

/** The plug-in actions of one link (`http(s)` or `mailto`). */
export function LinkActionMenu({
  url,
  text = null,
  messageId = null,
}: {
  url: string;
  text?: string | null;
  messageId?: MessageId | null;
}) {
  const plugins = usePlugins();
  const items = linkActionsFor(plugins.data ?? [], url);
  return (
    <ActionMenu
      kind="link"
      labelId="plugin-link-actions"
      labelArgs={{ url }}
      items={items}
      target={{ kind: "link", url, text, message_id: messageId }}
    />
  );
}
