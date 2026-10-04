/**
 * Fluent message ids of the layout editor for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids starting with one of
 * `prefixes` must exist in the English catalogs (`layout-editor.ftl`;
 * contract ids in `core-editors.ftl`); ids built at runtime are listed by
 * `runtimeIds`.
 */
import type { DockSide } from "../../api/generated";
import { PALETTE_WIDGETS, type DropZone } from "./model";
import { NAMED_KINDS } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["layout-editor-"];

/** Literals with those prefixes that are not message ids (widget kinds). */
export const notMessages: readonly string[] = ["layout-editor-settings"];

const SIDES: readonly DockSide[] = ["left", "right", "top", "bottom"];
const ZONES: readonly DropZone[] = [...SIDES, "swap", "into"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...ZONES.flatMap((z) => [
      `layout-editor-zone-${z}`,
      `layout-editor-target-${z}`,
      `layout-editor-dropped-${z}`,
    ]),
    ...NAMED_KINDS.map((k) => `layout-editor-kind-${k}`),
    ...PALETTE_WIDGETS.flatMap((w) => [
      `layout-editor-widget-${w.kind}`,
      `layout-editor-widget-${w.kind}-description`,
    ]),
  ];
}
