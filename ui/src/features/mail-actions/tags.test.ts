/**
 * The label / category windows' tag state over several messages and the
 * toast of a tag change (P3 review).
 */
import type { MessageSummary } from "../../api/generated";
import { createMockData } from "../../api/mockData";
import { must } from "../../test/must";
import { tagToast } from "./run";
import { countTags, tagChange, tagMark } from "./tags";
import { toastMessage } from "./Toast";

function withTags(...tags: string[][]): MessageSummary[] {
  const base = must(createMockData(2).messages[0]);
  return tags.map((t, i) => ({ ...base, id: `m${String(i)}`, tags: t }));
}

describe("tag state of several messages", () => {
  it("marks tags all, some or none, case-insensitively", () => {
    const counts = countTags(withTags(["Travel", "work"], ["travel"], ["Work"]));
    expect(tagMark(counts, "Travel")).toBe("some");
    expect(tagMark(counts, "WORK")).toBe("some");
    expect(tagMark(counts, "Receipts")).toBe("none");
    expect(tagMark(countTags(withTags(["a"], ["A"])), "a")).toBe("all");
  });

  it("adds a checked tag where it is missing and removes every spelling of a cleared one", () => {
    const counts = countTags(withTags(["Travel", "work"], ["travel"], ["Work"]));
    const decided = new Map([
      ["Travel", true],
      ["Work", false],
    ]);
    expect(tagChange(["Travel", "Work", "Receipts"], counts, decided)).toEqual({
      add: ["Travel"],
      remove: ["work", "Work"],
    });
    // Untouched mixed tags stay as they are.
    expect(tagChange(["Travel"], counts, new Map())).toEqual({ add: [], remove: [] });
  });
});

describe("tag change toast", () => {
  it("says what happened and what the tags are called", () => {
    expect(tagToast(["a"], [], "label")).toEqual({ change: "added", noun: "label", count: 1 });
    expect(tagToast([], ["a", "b"], "category")).toEqual({
      change: "removed",
      noun: "category",
      count: 2,
    });
    expect(tagToast(["a"], ["b"])).toEqual({ change: "changed", noun: "tag", count: 2 });
    expect(
      toastMessage({
        kind: "done",
        seq: 1,
        action: "tags",
        conversations: 1,
        messages: 4,
        batchId: "b",
        tags: { change: "removed", noun: "label", count: 1 },
      }),
    ).toEqual({
      id: "mail-actions-done-tags-removed",
      args: { conversations: 1, messages: 4, noun: "label", tags: 1 },
    });
  });
});
