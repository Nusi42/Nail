/**
 * Choices of the merge dialog: per item "keep mine" or "take theirs", with
 * whole-section and whole-file shortcuts. Only items that differ need a
 * choice; the core treats every item without one as `default`.
 */
import type {
  ConfigMergePreview,
  MergeChoice,
  MergeItem,
  MergeSection,
  MergeTake,
} from "../../api/generated";

/** Explicit choices by item id. */
export type Choices = Readonly<Record<string, MergeTake>>;

/** Whether the item differs (unchanged items have nothing to choose). */
export function differs(item: MergeItem): boolean {
  return item.change !== "unchanged";
}

/** The side an item ends up with. */
export function takeOf(choices: Choices, item: MergeItem, fallback: MergeTake): MergeTake {
  return choices[item.id] ?? fallback;
}

/** Sets every differing item of a section (or the whole file) to one side. */
export function chooseAll(
  choices: Choices,
  preview: ConfigMergePreview,
  take: MergeTake,
  section?: MergeSection,
): Choices {
  const next: Record<string, MergeTake> = { ...choices };
  for (const s of preview.sections) {
    if (section !== undefined && s.section !== section) continue;
    for (const item of s.items) {
      if (differs(item)) next[item.id] = take;
    }
  }
  return next;
}

/** The request form of the choices (only differing items of the preview). */
export function choiceList(choices: Choices, preview: ConfigMergePreview): MergeChoice[] {
  const out: MergeChoice[] = [];
  for (const s of preview.sections) {
    for (const item of s.items) {
      const take = choices[item.id];
      if (take !== undefined && differs(item)) out.push({ item_id: item.id, take });
    }
  }
  return out;
}

/** Counts for the summary line. */
export function summarize(
  preview: ConfigMergePreview,
  choices: Choices,
  fallback: MergeTake,
): { differing: number; taken: number; conflicts: number } {
  let differing = 0;
  let taken = 0;
  let conflicts = 0;
  for (const s of preview.sections) {
    for (const item of s.items) {
      if (!differs(item)) continue;
      differing += 1;
      if (takeOf(choices, item, fallback) === "theirs") {
        taken += 1;
        conflicts += item.conflicts.length;
      }
    }
  }
  return { differing, taken, conflicts };
}
