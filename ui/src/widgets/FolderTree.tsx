/**
 * The folder pane (`folder-tree`).
 *
 * Grouped (M5b WP162, ADRs 0109 / 0111) as soon as the layout sets one
 * of the M5b options (`sections`, `favorites`, `collapsible`,
 * `account-colors`): the New message button on top, then the sections of
 * `folderPaneSections` — Favorites (the unified folders of every account),
 * one collapsible section per account with its colour dot and address, and
 * the smart folders only when there is one. Collapsed sections are
 * remembered in the core's UI state.
 *
 * Without those options it stays the pre-M5b tree of the active account
 * (older layouts keep working; the default layout opts in).
 *
 * One ARIA tree: sections are expandable tree items; Up / Down / Home / End
 * move, Right / Left expand / collapse (or step into / out of a section),
 * Enter and Space choose.
 */
import { folderRoleLabelId, rolesFirst } from "./folderNames";
import { useQueryClient } from "@tanstack/react-query";
import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { errorMessageIdOf } from "../api/client";
import {
  FAVORITE_FOLDER_MESSAGE_IDS,
  type Account,
  type AccountState,
  type FavoriteFolder,
  type Folder,
  type SavedSearch,
} from "../api/generated";
import { useActions } from "../actions/ActionsProvider";
import { MailboxFolderBadge } from "../features/exchange";
import { isActiveSearch, useSavedSearches } from "../features/customization/SmartFolders";
import { useLabel, useT } from "../i18n/I18nProvider";
import { useLayoutConfig } from "../layout/LayoutContext";
import { optionBool, type WidgetNode, type WidgetProps } from "../layout/widgetRegistry";
import {
  useAccountStatuses,
  useAccounts,
  useClient,
  useFolders,
  useOutbox,
} from "../state/queries";
import { UI_STATE_COLLAPSED_SECTIONS, stringList, useUiStateValue } from "../state/uiState";
import { useUi } from "../state/uiStore";
import { useAccountName } from "./AccountSwitcher";
import { Icon } from "./Icon";
import {
  DEFAULT_FAVORITES,
  FAVORITE_FOLDERS,
  FOLDER_SECTION_KINDS,
  folderPaneSections,
  wordsOption,
  type FolderPaneSection,
} from "./folderPane";
import {
  FAVORITE_ICONS,
  FAVORITE_TARGETS,
  favoriteCount,
  toggleCollapsedSection,
  unifiedFolder,
} from "./folderPaneState";
import { ALL_ACCOUNTS } from "../api/contract";

/** Folders in tree order (parents before children) with their depth. */
/** What a folder pane section reads and changes: the selected account, folder and search. */
function useFolderSelection() {
  const activeAccountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const selectAccount = useUi((s) => s.selectAccount);
  const selectFolder = useUi((s) => s.selectFolder);
  return { activeAccountId, activeFolderId, search, selectAccount, selectFolder };
}

export function flattenFolders(
  folders: readonly Folder[],
  options: { rolesFirst?: boolean } = {},
): { folder: Folder; depth: number }[] {
  const byParent = new Map<string | null, Folder[]>();
  const ids = new Set(folders.map((f) => f.id));
  for (const folder of folders) {
    const parent = folder.parent_id !== null && ids.has(folder.parent_id) ? folder.parent_id : null;
    const list = byParent.get(parent) ?? [];
    list.push(folder);
    byParent.set(parent, list);
  }
  const out: { folder: Folder; depth: number }[] = [];
  const visit = (parent: string | null, depth: number) => {
    const siblings = byParent.get(parent) ?? [];
    for (const folder of options.rolesFirst ? rolesFirst(siblings) : siblings) {
      out.push({ folder, depth });
      if (depth < 32) visit(folder.id, depth + 1);
    }
  };
  visit(null, 0);
  return out;
}

/** The M5b options that switch the pane to sections. */
export const GROUPED_OPTIONS = ["sections", "favorites", "collapsible", "account-colors"] as const;

/** Whether the node asks for the grouped folder pane. */
export function isGrouped(node: WidgetNode): boolean {
  return GROUPED_OPTIONS.some((key) => Object.hasOwn(node.options, key));
}

/** The folders of the active account, or the grouped folder pane. */
export function FolderTree(props: WidgetProps) {
  return isGrouped(props.node) ? <FolderPane {...props} /> : <AccountFolderTree {...props} />;
}

interface FolderRowProps {
  folder: Folder;
  depth: number;
  /** ARIA level of the row (one deeper inside a section). */
  level: number;
  selected: boolean;
  tabbable: boolean;
  showUnread: boolean;
  onChoose: () => void;
  rowRef?: (el: HTMLLIElement | null) => void;
}

/** One folder row (the same part in both modes: menus, tests, themes). */
function FolderRow({
  folder,
  depth,
  level,
  selected,
  tabbable,
  showUnread,
  onChoose,
  rowRef,
}: FolderRowProps) {
  const t = useT();
  const roleLabel = folderRoleLabelId(folder);
  return (
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the tree handles the keyboard for its items (roving tabindex, Enter / Space).
    <li
      ref={rowRef}
      role="treeitem"
      aria-level={level}
      aria-selected={selected}
      tabIndex={tabbable ? 0 : -1}
      data-part="folder-tree.item"
      data-state={selected ? "selected" : undefined}
      data-role={folder.role}
      data-depth={depth}
      data-folder={folder.id}
      data-account={folder.account_id}
      data-key={`folder:${folder.id}`}
      onClick={onChoose}
    >
      <Icon name={folder.role === "other" ? "folder" : folder.role} part="folder-tree.item.icon" />
      <span data-part="folder-tree.item.name" title={roleLabel ? folder.name : undefined}>
        {roleLabel ? t(roleLabel) : folder.name}
      </span>
      <MailboxFolderBadge folder={folder} />
      {showUnread && folder.unread > 0 && (
        <span
          data-part="folder-tree.item.unread-count"
          aria-label={t("folder-tree-unread", { count: folder.unread })}
        >
          {folder.unread}
        </span>
      )}
    </li>
  );
}

/** The pre-M5b tree: the folders of the active account, with unread counts. */
function AccountFolderTree({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const selectFolder = useUi((s) => s.selectFolder);
  const folders = useFolders(accountId);
  const showUnread = optionBool(node, "show-unread", true);
  const items = useRef(new Map<string, HTMLLIElement>());
  const rows = flattenFolders(folders.data ?? [], { rolesFirst: true });

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const index = rows.findIndex((r) => r.folder.id === activeFolderId);
    let next: number;
    if (event.key === "ArrowDown") next = Math.min(rows.length - 1, index + 1);
    else if (event.key === "ArrowUp") next = Math.max(0, index - 1);
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = rows.length - 1;
    else return;
    event.preventDefault();
    const target = rows[next];
    if (target) {
      selectFolder(target.folder.id);
      items.current.get(target.folder.id)?.focus();
    }
  };

  const focusable = activeFolderId ?? rows[0]?.folder.id;

  return (
    <nav
      data-part="folder-tree"
      data-key-context="folders"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("common-folders")}
    >
      {folders.isError && (
        <div data-part="folder-tree.error">{t(errorMessageIdOf(folders.error))}</div>
      )}
      <ul
        data-part="folder-tree.list"
        role="tree"
        aria-label={t("common-folders")}
        onKeyDown={onKeyDown}
      >
        {rows.map(({ folder, depth }) => (
          <FolderRow
            key={folder.id}
            folder={folder}
            depth={depth}
            level={depth + 1}
            selected={folder.id === activeFolderId}
            tabbable={folder.id === focusable}
            showUnread={showUnread}
            onChoose={() => {
              selectFolder(folder.id);
            }}
            rowRef={(el) => {
              if (el) items.current.set(folder.id, el);
              else items.current.delete(folder.id);
            }}
          />
        ))}
      </ul>
    </nav>
  );
}

// ---------------------------------------------------------------------------
// Grouped pane

/** Shared by every section of one pane. */
interface PaneContext {
  /** Key of the tree item that takes Tab (roving tab index). */
  focusKey: string | null;
  collapsible: boolean;
  showUnread: boolean;
  toggle: (id: string) => void;
}

interface SectionProps {
  section: FolderPaneSection;
  pane: PaneContext;
  /** Visible heading. */
  title: string;
  /** Colour dot slot (account sections with `account-colors`). */
  colorSlot: number | null;
  /** Shown next to the heading while collapsed (unread inbox mail). */
  collapsedCount: number | null;
  /** Keys of the rows inside (a collapsed section takes their Tab stop). */
  childKeys: readonly string[];
  /** An account section's account kind and connection state (M5b integration). */
  account?: { kind: Account["kind"]; state: AccountState | null };
  children: ReactNode;
}

/** A section: an expandable tree item with a heading and its rows. */
function Section({
  section,
  pane,
  title,
  colorSlot,
  collapsedCount,
  childKeys,
  account,
  children,
}: SectionProps) {
  const t = useT();
  const headingId = useId();
  const statusId = useId();
  // Not online: the state shows as a tag beside the title.
  const state: AccountState | null =
    account?.state && account.state !== "online" ? account.state : null;
  const offline = state !== null;
  const key = `section:${section.id}`;
  const expanded = !section.collapsed;
  const tabbable =
    pane.focusKey === key ||
    (!expanded && pane.focusKey !== null && childKeys.includes(pane.focusKey));
  return (
    <li
      role="treeitem"
      aria-level={1}
      aria-labelledby={offline ? `${headingId} ${statusId}` : headingId}
      aria-expanded={pane.collapsible ? expanded : undefined}
      aria-selected={false}
      tabIndex={tabbable ? 0 : -1}
      data-part="folder-tree.section"
      data-section={section.id}
      data-kind={section.kind}
      data-state={expanded ? undefined : "collapsed"}
      data-account={section.accountId ?? undefined}
      data-account-kind={account?.kind}
      data-status={account?.state ?? undefined}
      data-key={key}
    >
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions -- the section's tree item expands and collapses with the arrow keys; this is its pointer target. */}
      <div
        data-part={
          section.accountId !== null ? "folder-tree.account" : "folder-tree.section.header"
        }
        data-account={section.accountId ?? undefined}
        data-section={section.id}
        onClick={
          pane.collapsible
            ? () => {
                pane.toggle(section.id);
              }
            : undefined
        }
      >
        {pane.collapsible && (
          <Icon
            name={expanded ? "chevron-down" : "chevron-right"}
            part="folder-tree.section.chevron"
          />
        )}
        {colorSlot !== null && (
          <span
            data-part="folder-tree.section.dot"
            data-color={colorSlot - 1}
            title={t("account-color-slot", { slot: colorSlot })}
            aria-hidden="true"
          />
        )}
        <span data-part="folder-tree.section.title" id={headingId} title={title}>
          {title}
        </span>
        {/* Not online: the state as a tag beside the title (only the title is cut),
            also for screen readers (part of the section's name). */}
        {state !== null && (
          <span
            data-part="folder-tree.section.status"
            data-state={state}
            id={statusId}
            title={t(`account-state-${state}`)}
          >
            {t(`account-state-${state}`)}
          </span>
        )}
        {!expanded && collapsedCount !== null && collapsedCount > 0 && (
          <span
            data-part="folder-tree.section.unread-count"
            aria-label={t("folder-tree-unread", { count: collapsedCount })}
          >
            {collapsedCount}
          </span>
        )}
      </div>
      {expanded && (
        <ul data-part="folder-tree.group" role="group">
          {children}
        </ul>
      )}
    </li>
  );
}

function FavoritesSection({
  section,
  pane,
  favorites,
}: {
  section: FolderPaneSection;
  pane: PaneContext;
  favorites: readonly FavoriteFolder[];
}) {
  const t = useT();
  const { dispatch } = useActions();
  const folders = useFolders(ALL_ACCOUNTS).data ?? [];
  const { activeAccountId, activeFolderId, search, selectAccount, selectFolder } =
    useFolderSelection();
  const setSearch = useUi((s) => s.setSearch);
  const setSearchInput = useUi((s) => s.setSearchInput);

  const outbox = useOutbox(favorites.includes("outbox")).data;
  const rows = favorites.flatMap((favorite) => {
    const target = FAVORITE_TARGETS[favorite];
    // The Outbox shows only while mail waits there (M5b review).
    if (favorite === "outbox" && (outbox?.length ?? 0) === 0) return [];
    const folder = target.kind === "folder" ? unifiedFolder(folders, target.role) : null;
    // A unified folder no account has (no Sent anywhere) is left out.
    if (target.kind === "folder" && folder === null) return [];
    let selected = false;
    if (target.kind === "folder") {
      selected =
        search === null && activeAccountId === ALL_ACCOUNTS && activeFolderId === folder?.id;
    } else if (target.kind === "search") {
      selected =
        search !== null &&
        search.text === target.text &&
        search.accountId === null &&
        search.folderId === null;
    }
    const choose = () => {
      if (target.kind === "folder" && folder) {
        selectAccount(ALL_ACCOUNTS);
        selectFolder(folder.id);
      } else if (target.kind === "search") {
        setSearchInput(target.text);
        setSearch({ text: target.text, accountId: null, folderId: null, error: null });
      } else if (target.kind === "action") {
        void dispatch(target.action);
      }
    };
    const count =
      favorite === "outbox" ? (outbox?.length ?? null) : favoriteCount(favorite, folders);
    return [{ favorite, selected, choose, count }];
  });

  return (
    <Section
      section={section}
      pane={pane}
      title={t("folder-section-favorites")}
      colorSlot={null}
      collapsedCount={null}
      childKeys={rows.map((r) => `favorite:${r.favorite}`)}
    >
      {rows.map(({ favorite, selected, choose, count }) => {
        const key = `favorite:${favorite}`;
        return (
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the tree handles the keyboard for its items (roving tabindex, Enter / Space).
          <li
            key={favorite}
            role="treeitem"
            aria-level={2}
            aria-selected={selected}
            tabIndex={pane.focusKey === key ? 0 : -1}
            data-part="folder-tree.favorite"
            data-favorite={favorite}
            data-state={selected ? "selected" : undefined}
            data-key={key}
            onClick={choose}
          >
            <Icon name={FAVORITE_ICONS[favorite]} part="folder-tree.item.icon" />
            <span data-part="folder-tree.item.name">
              {t(FAVORITE_FOLDER_MESSAGE_IDS[favorite])}
            </span>
            {pane.showUnread && count !== null && count > 0 && (
              <span
                data-part="folder-tree.item.unread-count"
                aria-label={
                  favorite === "drafts"
                    ? t("folder-tree-drafts", { count })
                    : favorite === "outbox"
                      ? t("folder-tree-outbox", { count })
                      : t("folder-tree-unread", { count })
                }
              >
                {count}
              </span>
            )}
          </li>
        );
      })}
    </Section>
  );
}

function AccountSection({
  section,
  pane,
  account,
  showColor,
}: {
  section: FolderPaneSection;
  pane: PaneContext;
  account: Account;
  showColor: boolean;
}) {
  const t = useT();
  const nameOf = useAccountName();
  const folders = useFolders(account.id);
  const status = useAccountStatuses().data?.find((s) => s.account_id === account.id);
  const { activeAccountId, activeFolderId, search, selectAccount, selectFolder } =
    useFolderSelection();
  const rows = flattenFolders(folders.data ?? [], { rolesFirst: true });
  const name = nameOf(account);
  const title =
    account.email !== "" && account.email !== name
      ? t("folder-tree-account-title", { name, email: account.email })
      : name;
  const inbox = rows.find((r) => r.folder.role === "inbox" && r.depth === 0)?.folder;

  return (
    <Section
      section={section}
      pane={pane}
      title={title}
      colorSlot={showColor ? section.colorSlot : null}
      collapsedCount={inbox?.unread ?? null}
      childKeys={rows.map((r) => `folder:${r.folder.id}`)}
      account={{ kind: account.kind, state: status?.state ?? null }}
    >
      {folders.isError && (
        <li data-part="folder-tree.error" role="none">
          {t(errorMessageIdOf(folders.error))}
        </li>
      )}
      {rows.map(({ folder, depth }) => (
        <FolderRow
          key={folder.id}
          folder={folder}
          depth={depth}
          level={depth + 2}
          selected={
            search === null && activeAccountId === account.id && activeFolderId === folder.id
          }
          tabbable={pane.focusKey === `folder:${folder.id}`}
          showUnread={pane.showUnread}
          onChoose={() => {
            selectAccount(account.id);
            selectFolder(folder.id);
          }}
        />
      ))}
    </Section>
  );
}

function SmartFoldersSection({
  section,
  pane,
  saved,
}: {
  section: FolderPaneSection;
  pane: PaneContext;
  saved: readonly SavedSearch[];
}) {
  const t = useT();
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const setSearchInput = useUi((s) => s.setSearchInput);
  return (
    <Section
      section={section}
      pane={pane}
      title={t("folder-section-smart-folders")}
      colorSlot={null}
      collapsedCount={null}
      childKeys={saved.map((s, index) => `smart:${s.id ?? `saved-${String(index)}`}`)}
    >
      {saved.map((s, index) => {
        const id = s.id ?? `saved-${String(index)}`;
        const key = `smart:${id}`;
        const selected = isActiveSearch(s, search);
        return (
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the tree handles the keyboard for its items (roving tabindex, Enter / Space).
          <li
            key={id}
            role="treeitem"
            aria-level={2}
            aria-selected={selected}
            tabIndex={pane.focusKey === key ? 0 : -1}
            data-part="folder-tree.smart-folder"
            data-search={s.id ?? undefined}
            data-state={selected ? "selected" : undefined}
            data-key={key}
            onClick={() => {
              setSearchInput(s.query);
              setSearch({
                text: s.query,
                accountId: s.account_id,
                folderId: s.folder_id,
                error: null,
              });
            }}
          >
            <Icon name="search" part="folder-tree.item.icon" />
            <span data-part="folder-tree.item.name">{s.name}</span>
          </li>
        );
      })}
    </Section>
  );
}

/** The New message button on top of the pane. */
function ComposeButton() {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const { dispatch, isEnabled } = useActions();
  const def = Object.hasOwn(layout.actions, "mail.compose")
    ? layout.actions["mail.compose"]
    : undefined;
  const text = def?.label ? label(def.label) : t("mail-action-compose");
  return (
    <button
      type="button"
      data-part="folder-tree.compose"
      data-action="mail.compose"
      disabled={!isEnabled("mail.compose")}
      onClick={() => void dispatch("mail.compose")}
    >
      <Icon name={def?.icon ?? "pen-line"} part="folder-tree.compose.icon" />
      <span data-part="folder-tree.compose.label">{text}</span>
    </button>
  );
}

/** The visible tree items of a tree, in order. */
function treeItems(tree: HTMLElement): HTMLElement[] {
  return [...tree.querySelectorAll<HTMLElement>('[role="treeitem"]')];
}

/** The grouped folder pane (M5b). */
function FolderPane({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const queryClient = useQueryClient();
  const accountsQuery = useAccounts();
  const saved = useSavedSearches().data ?? [];
  const collapsed = stringList(useUiStateValue(UI_STATE_COLLAPSED_SECTIONS));
  const activeFolderId = useUi((s) => s.activeFolderId);
  const tree = useRef<HTMLUListElement>(null);
  const [focused, setFocused] = useState<string | null>(null);

  const kinds = wordsOption(node.options.sections, FOLDER_SECTION_KINDS, FOLDER_SECTION_KINDS);
  const favorites = wordsOption(node.options.favorites, FAVORITE_FOLDERS, DEFAULT_FAVORITES);
  const collapsible = optionBool(node, "collapsible", true);
  const accountColors = optionBool(node, "account-colors", true);
  const showUnread = optionBool(node, "show-unread", true);
  const compose = optionBool(node, "compose", true);
  const accounts = (accountsQuery.data ?? []).filter((a) => a.id !== ALL_ACCOUNTS);
  const sections = folderPaneSections(
    kinds,
    favorites,
    accounts.map((a) => a.id),
    saved.length,
    collapsible ? collapsed : [],
  );

  // The tree item that takes Tab: the one last focused, else the selected
  // folder, else the first section (a collapsed section stands in for its
  // rows; the effect below covers keys that disappeared).
  const focusKey =
    focused ??
    (activeFolderId !== null
      ? `folder:${activeFolderId}`
      : sections[0]
        ? `section:${sections[0].id}`
        : null);

  // Keep exactly one Tab stop: while the item that should have it is not
  // shown (its folders are loading, it was removed), the first item stands
  // in; it gives the stop back as soon as the right one renders.
  const standIn = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const el = tree.current;
    if (!el) return;
    const items = treeItems(el);
    const stops = items.filter((i) => i.tabIndex === 0);
    const patched = standIn.current;
    if (stops.length === 0 && items[0]) {
      items[0].tabIndex = 0;
      standIn.current = items[0];
    } else if (stops.length > 1 && patched && patched.dataset.key !== focusKey) {
      patched.tabIndex = -1;
      standIn.current = null;
    }
  });

  const pane: PaneContext = {
    focusKey,
    collapsible,
    showUnread,
    toggle: (id) => void toggleCollapsedSection(client, queryClient, id),
  };

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const el = tree.current;
    const item = (event.target as HTMLElement).closest<HTMLElement>('[role="treeitem"]');
    if (!el || !item || event.altKey || event.ctrlKey || event.metaKey) return;
    const items = treeItems(el);
    const at = items.indexOf(item);
    const isSection = item.dataset.part === "folder-tree.section";
    const expanded = item.getAttribute("aria-expanded");
    const parentSection = item.parentElement?.closest<HTMLElement>('[role="treeitem"]') ?? null;
    let target: HTMLElement | null | undefined = null;
    switch (event.key) {
      case "ArrowDown":
        target = items[at + 1];
        break;
      case "ArrowUp":
        target = items[at - 1];
        break;
      case "Home":
        target = items[0];
        break;
      case "End":
        target = items[items.length - 1];
        break;
      case "ArrowRight":
        if (isSection && expanded === "false") pane.toggle(item.dataset.section ?? "");
        else if (isSection) target = items[at + 1];
        break;
      case "ArrowLeft":
        if (isSection && expanded === "true") pane.toggle(item.dataset.section ?? "");
        else if (!isSection) target = parentSection;
        break;
      case "Enter":
      case " ":
        if (isSection) {
          if (collapsible) pane.toggle(item.dataset.section ?? "");
        } else {
          item.click();
        }
        break;
      default:
        return;
    }
    event.preventDefault();
    target?.focus();
  };

  return (
    <nav
      data-part="folder-tree"
      data-mode="sections"
      data-key-context="folders"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("common-folders")}
    >
      {compose && <ComposeButton />}
      {accountsQuery.isError && (
        <div data-part="folder-tree.error">{t(errorMessageIdOf(accountsQuery.error))}</div>
      )}
      <ul
        ref={tree}
        data-part="folder-tree.list"
        role="tree"
        aria-label={t("common-folders")}
        onKeyDown={onKeyDown}
        onFocus={(event) => {
          const key = (event.target as HTMLElement).dataset.key;
          if (key) setFocused(key);
        }}
      >
        {sections.map((section) => {
          if (section.kind === "favorites") {
            return (
              <FavoritesSection
                key={section.id}
                section={section}
                pane={pane}
                favorites={favorites}
              />
            );
          }
          if (section.kind === "smart-folders") {
            return (
              <SmartFoldersSection key={section.id} section={section} pane={pane} saved={saved} />
            );
          }
          const account = accounts.find((a) => a.id === section.accountId);
          return account ? (
            <AccountSection
              key={section.id}
              section={section}
              pane={pane}
              account={account}
              showColor={accountColors}
            />
          ) : null;
        })}
      </ul>
    </nav>
  );
}
