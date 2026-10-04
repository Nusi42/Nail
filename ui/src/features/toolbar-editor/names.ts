/**
 * Display names of the default layout's toolbars (`toolbars` in
 * `defaults/layout.kdl`); a toolbar of a user's layout shows its own name.
 */

/** Fluent ids by toolbar name. */
const TITLES: Readonly<Record<string, string>> = {
  main: "toolbar-editor-toolbar-main",
  reader: "toolbar-editor-toolbar-reader",
  calendar: "toolbar-editor-toolbar-calendar",
  customize: "toolbar-editor-toolbar-customize",
};

/** The Fluent id of a toolbar's display name, `null` = show the name itself. */
export function toolbarTitleId(name: string): string | null {
  return Object.hasOwn(TITLES, name) ? (TITLES[name] ?? null) : null;
}
