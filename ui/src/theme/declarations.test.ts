/**
 * The shared CSS declarations (Q2, SSOT): each is a well-formed declaration
 * list the skins can drop into a rule, and the lengths the skins share with
 * defaults/theme.kdl stay equal to the theme's (the pins named in
 * `features/pim/parts.ts` and `theme/skinResponsive.ts`).
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { HOUR_HEIGHT, TIME_GUTTER } from "../features/pim/parts";
import { expectInsertable } from "../test/css";
import {
  ACCENT_BAR,
  AVATAR_FG,
  avatarDisk,
  CAPS,
  CAPS_TIGHT,
  floatingWidth,
  HC_RING,
  initialsAvatar,
  INITIALS_TRACKING,
  INLINE_GAP,
  INLINE_ROW,
  LINE_BOX,
  noticeStack,
  QUIET_BUTTON,
  QUIET_HOVER,
  READER_MEASURE,
  READER_NARROW,
  SECTION_LABEL,
  smallTag,
  TINY_TEXT,
} from "./declarations";
import { FREEBUSY_NAME_COLUMN } from "./skinResponsive";

const theme = readFileSync(resolve(process.cwd(), "../defaults/theme.kdl"), "utf8");

/** The declarations of `part "<name>"` in defaults/theme.kdl (one-line rules). */
function themePart(name: string): string {
  const line = theme.split("\n").find((l) => l.trimStart().startsWith(`part "${name}" {`));
  expect(line, name).toBeDefined();
  return line ?? "";
}

describe("shared declarations", () => {
  it("are declaration lists a rule accepts", () => {
    const lists = [
      CAPS,
      CAPS_TIGHT,
      SECTION_LABEL,
      INITIALS_TRACKING,
      avatarDisk("2rem"),
      initialsAvatar("2rem", "1rem"),
      smallTag("4px", "red", "blue"),
      INLINE_ROW,
      QUIET_BUTTON,
      QUIET_HOVER,
      noticeStack("left"),
      noticeStack("right"),
      `color: ${AVATAR_FG};`,
      `font-size: ${TINY_TEXT};`,
      `max-width: ${floatingWidth("560px")};`,
      `min-height: ${LINE_BOX}; gap: ${INLINE_GAP}; border-left: ${ACCENT_BAR} solid; outline: ${HC_RING} solid;`,
      `max-width: ${READER_MEASURE};`,
    ];
    for (const list of lists) {
      expect(list.trim()).toMatch(/;$/);
      expect(list).not.toMatch(/[{}]/);
    }
    expectInsertable(lists.map((list) => `[data-part="x"] { ${list} }`));
    expectInsertable([`@container nail-reader (max-width: ${READER_NARROW}) { p { margin: 0; } }`]);
  });

  it("keeps notices off the window's edges on both sides", () => {
    expect(noticeStack("left")).toContain("left: 16px;");
    expect(noticeStack("right")).toContain("right: 16px;");
    expect(floatingWidth("420px")).toBe("min(420px, calc(100% - 2 * 16px))");
  });

  it("puts the label size, weight and colour on section labels", () => {
    expect(SECTION_LABEL).toContain("font-size: var(--nail-font-size-label);");
    expect(SECTION_LABEL).toContain(CAPS);
    expect(SECTION_LABEL).toContain("color: var(--nail-color-fg-muted);");
  });
});

describe("lengths shared with defaults/theme.kdl", () => {
  it("gives the time grid the theme's hour-label column and hour height", () => {
    expect(themePart("calendar.head")).toContain(
      `grid-template-columns "${TIME_GUTTER} minmax(0, 1fr)"`,
    );
    expect(themePart("calendar.body")).toContain(
      `grid-template-columns "${TIME_GUTTER} minmax(0, 1fr)"`,
    );
    expect(themePart('calendar.head[zones=\\"2\\"]')).toContain(
      `grid-template-columns "${String(2 * parseFloat(TIME_GUTTER))}px minmax(0, 1fr)"`,
    );
    expect(`${/height (\d+);/.exec(themePart("calendar.slot"))?.[1] ?? ""}px`).toBe(HOUR_HEIGHT);
  });

  it("starts the free/busy scale after the theme's name column", () => {
    expect(themePart("freebusy.row")).toContain(
      `grid-template-columns "${FREEBUSY_NAME_COLUMN} 1fr"`,
    );
    expect(themePart("freebusy.scale")).toContain(
      `margin-left ${String(parseFloat(FREEBUSY_NAME_COLUMN))};`,
    );
  });

  it("measures the reader column like the theme's message text", () => {
    expect(theme).toMatch(new RegExp(`max-width "${READER_MEASURE}"`));
  });
});
