/**
 * The Fluent catalogs shipped in `locales/<lang>/*.ftl`, bundled as raw text
 * by Vite.
 */
import type { Catalogs } from "./localizer";

const files = import.meta.glob<string>("@locales/*/*.ftl", {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Groups glob results by their locale directory. */
export function groupCatalogs(entries: Record<string, string>): Catalogs {
  const catalogs: Catalogs = {};
  for (const path of Object.keys(entries).sort()) {
    const match = /\/([^/]+)\/[^/]+\.ftl$/.exec(path);
    const locale = match?.[1];
    if (!locale) continue;
    (catalogs[locale] ??= []).push(entries[path] ?? "");
  }
  return catalogs;
}

/** All shipped catalogs. */
export const shippedCatalogs: Catalogs = groupCatalogs(files);
