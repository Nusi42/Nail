import type { JSX } from "react";
import { useT } from "../i18n/I18nProvider";
import { optionList, type WidgetProps } from "../layout/widgetRegistry";
import { useFolders } from "../state/queries";
import { useUi } from "../state/uiStore";

function SyncItem() {
  const t = useT();
  const sync = useUi((s) => s.sync);
  const connection = useUi((s) => s.connection);
  const configError = useUi((s) => s.configError);
  let text: string;
  let state: string;
  if (connection === "reconnecting" || connection === "closed") {
    text = t("status-offline");
    state = "offline";
  } else if (configError) {
    text = t("status-config-error", {
      file: configError.file,
      line: configError.line,
      column: configError.column,
    });
    state = "error";
  } else if (sync && sync.done < sync.total) {
    text = t("status-syncing", { done: sync.done, total: sync.total });
    state = "syncing";
  } else {
    text = t("status-ready");
    state = "idle";
  }
  return (
    <span data-part="status-bar.item" data-item="sync-progress" data-state={state}>
      {text}
    </span>
  );
}

function FolderCountsItem() {
  const t = useT();
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const folders = useFolders(accountId);
  const folder = folders.data?.find((f) => f.id === folderId);
  if (!folder) return null;
  return (
    <span data-part="status-bar.item" data-item="folder-counts">
      {t("status-folder-counts", { total: folder.total, unread: folder.unread })}
    </span>
  );
}

const ITEMS: Record<string, () => JSX.Element | null> = {
  "sync-progress": SyncItem,
  "folder-counts": FolderCountsItem,
};

/** Status line; items come from the `items` option. */
export function StatusBar({ node }: WidgetProps) {
  const items = optionList(node, "items", ["sync-progress"]).filter((item) =>
    Object.hasOwn(ITEMS, item),
  );
  return (
    <div
      data-part="status-bar"
      data-node-id={node.id ?? undefined}
      role="status"
      aria-live="polite"
    >
      {items.map((item) => {
        const Item = ITEMS[item];
        return Item ? <Item key={item} /> : null;
      })}
    </div>
  );
}
