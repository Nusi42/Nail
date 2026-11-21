/**
 * "Run on" in the rules window's footer (R2 design review, item 5): the
 * messages Run, Test and Apply act on — the selected messages (with their
 * count; only while some are selected), the current folder (with its name;
 * the default when the window was opened from a message), or where the rule
 * applies (each rule's own folders, else the Inboxes). R1's run panel had
 * the same choices as radio buttons under the editor.
 */
import { useState } from "react";
import { currentTargets, useListSelection } from "../../actions/mailTargets";
import { useT } from "../../i18n/I18nProvider";
import { useFolders } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { listTitle } from "../../widgets/ListHeader";
import { RUN_TARGET_KINDS, type RunTarget, type RunTargetKind } from "./runTargets";
import { useRulesUi } from "./store";

/** One choice of the "Run on" select. */
export interface RunOnChoice {
  kind: RunTargetKind;
  /** Localized label. */
  label: string;
  /** Why it cannot be chosen now (localized), `null` when it can. */
  why: string | null;
}

/** What the footer needs to know about the current list. */
export interface ListFacts {
  /** The number of selected messages (a conversation counts once). */
  selected: number;
  /** The listed folder's name, `null` when the list shows no folder. */
  folderName: string | null;
  /** The list shows search results. */
  searching: boolean;
  /** The window was opened from a message. */
  fromMessage: boolean;
}

/** The choices in the order the select lists them, with why each is unavailable. */
export function runOnChoices(
  facts: ListFacts,
  t: (id: string, args?: Record<string, string | number>) => string,
): RunOnChoice[] {
  const folderWhy = facts.searching
    ? t("rules-target-searching")
    : facts.folderName === null
      ? t("rules-target-no-folder")
      : null;
  return RUN_TARGET_KINDS.map((kind) => {
    switch (kind) {
      case "messages":
        return {
          kind,
          label: t("rules-target-messages", { count: facts.selected }),
          why: facts.selected > 0 ? null : t("rules-target-no-messages"),
        };
      case "folder":
        return {
          kind,
          label: t("rules-target-folder", { folder: facts.folderName ?? "" }),
          why: folderWhy,
        };
      case "scope":
        return { kind, label: t("rules-target-scope"), why: null };
    }
  });
}

/**
 * The choice to start with: the current folder when the window was opened
 * from a message (and the list shows one), else where the rule applies.
 */
export function defaultRunOn(facts: ListFacts): RunTargetKind {
  return facts.fromMessage && !facts.searching && facts.folderName !== null ? "folder" : "scope";
}

/** The footer's "Run on" state and the target it names. */
export function useRunOn() {
  const t = useT();
  const ui = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const fromMessage = useRulesUi((s) => s.origin !== null);
  // The selection's parts, so the count follows the list.
  useUi((s) => s.selectedMessageId);
  useUi((s) => s.selectedThreadId);
  useListSelection((s) => s.picked);
  const folder = useFolders(accountId).data?.find((f) => f.id === folderId);
  const listed = search === null ? listTitle(null, folder, accountId) : null;
  const facts: ListFacts = {
    selected: currentTargets(ui).targets.length,
    folderName:
      folderId === null || listed === null ? null : "id" in listed ? t(listed.id) : listed.text,
    searching: search !== null,
    fromMessage,
  };
  const choices = runOnChoices(facts, t);
  const [chosen, setChosen] = useState<RunTargetKind | null>(null);
  const preferred = chosen ?? defaultRunOn(facts);
  // A choice that is no longer available (the selection went away) falls back.
  const kind = choices.find((c) => c.kind === preferred)?.why === null ? preferred : "scope";
  const choice = choices.find((c) => c.kind === kind);

  /** The target of the chosen kind, read when a button is clicked. */
  const target = (): RunTarget | null => {
    switch (kind) {
      case "scope":
        return { kind: "scope" };
      case "folder":
        return folderId === null ? null : { kind: "folder", folderId };
      case "messages":
        return { kind: "messages", targets: currentTargets(ui).targets };
    }
  };

  return {
    kind,
    choose: setChosen,
    choices,
    /** The chosen target's label (localized). */
    label: choice?.label ?? "",
    target,
  };
}

export type RunOn = ReturnType<typeof useRunOn>;
