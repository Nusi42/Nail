/** Fixtures of the layout editor's tests (imported by the tests only). */
import type { LayoutNode, LayoutNodeRef, ResolvedLayout } from "../../api/generated";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../../api/normalize";
import { must } from "../../test/must";

/** A fresh copy of the default layout. */
export const layout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);

/** A node of a layout window by its path. */
export const ref = (path: number[], window = "mail", breakpoint = 0): LayoutNodeRef => ({
  window,
  breakpoint,
  path,
});

/** The root of the mail window at `breakpoint`. */
export const mailRoot = (l: ResolvedLayout, breakpoint = 0): LayoutNode =>
  must(l.windows.mail?.[breakpoint]).root;
