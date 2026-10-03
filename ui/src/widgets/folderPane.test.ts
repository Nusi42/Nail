/**
 * Folder-pane sections (M5b): mirrors
 * `nail_api::views::tests::folder_pane_sections_follow_the_rules` and
 * `account_colours_cycle`.
 */
import {
  DEFAULT_FAVORITES,
  FAVORITE_FOLDERS,
  FOLDER_SECTION_KINDS,
  accountColorSlot,
  folderPaneSections,
  wordsOption,
} from "./folderPane";

describe("folder pane sections (M5b)", () => {
  const accounts = ["work", "home"];
  const ids = (s: { id: string }[]) => s.map((x) => x.id);

  it("cycles the account colours", () => {
    expect(Array.from({ length: 10 }, (_v, i) => accountColorSlot(i))).toEqual([
      1, 2, 3, 4, 5, 6, 7, 8, 1, 2,
    ]);
  });

  it("follows the rules", () => {
    let sections = folderPaneSections(FOLDER_SECTION_KINDS, DEFAULT_FAVORITES, accounts, 0, []);
    expect(ids(sections)).toEqual(["favorites", "account:work", "account:home"]);
    expect(sections[1]?.colorSlot).toBe(1);
    expect(sections[2]?.colorSlot).toBe(2);
    expect(sections[2]?.accountId).toBe("home");
    sections = folderPaneSections(FOLDER_SECTION_KINDS, DEFAULT_FAVORITES, accounts, 2, [
      "account:home",
      "smart-folders",
    ]);
    expect(ids(sections)).toEqual(["favorites", "account:work", "account:home", "smart-folders"]);
    expect(sections.map((s) => s.collapsed)).toEqual([false, false, true, true]);
    sections = folderPaneSections(
      ["smart-folders", "favorites", "accounts"],
      [],
      accounts.slice(0, 1),
      1,
      [],
    );
    expect(ids(sections)).toEqual(["smart-folders", "account:work"]);
  });

  it("reads word options", () => {
    expect(wordsOption(undefined, FAVORITE_FOLDERS, DEFAULT_FAVORITES)).toEqual([
      ...DEFAULT_FAVORITES,
    ]);
    expect(wordsOption("drafts  starred drafts all-inboxes", FAVORITE_FOLDERS, [])).toEqual([
      "drafts",
      "all-inboxes",
    ]);
    expect(wordsOption("", FOLDER_SECTION_KINDS, FOLDER_SECTION_KINDS)).toEqual([]);
  });
});
