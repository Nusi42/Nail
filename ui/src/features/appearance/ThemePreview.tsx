/**
 * The theme editor's live preview (M5d WP183): a small mail window drawn
 * with the edited theme's own token values — unsaved drafts included, in
 * the mode the editor shows — so a change is visible before it is written
 * and whichever theme the app wears meanwhile. Each element takes its
 * colours, sizes and corners from a geometry rule (`useGeometry`), like
 * the gallery's swatches; the arrangement comes from the theme's
 * `theme-editor.preview.*` parts.
 */
import { useId, type ReactNode } from "react";
import type { ThemeTokenValue } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useGeometry } from "../../theme/ThemeProvider";
import { resolveTokenValues } from "./tokens";

type Values = ReadonlyMap<string, string>;

/** A token's resolved value, else the app's own (`var(--nail-…)`). */
function token(values: Values, category: string, name: string): string {
  return values.get(`${category}.${name}`) ?? `var(--nail-${category}-${name})`;
}

/** The declarations of each preview element, from the theme's tokens. */
export function previewStyles(values: Values): Record<string, Record<string, string>> {
  const c = (name: string) => token(values, "color", name);
  const radius = (name: string) => token(values, "radius", name);
  const space = (name: string) => token(values, "space", name);
  const size = (name: string) => token(values, "font-size", name);
  const text = { "font-family": token(values, "font", "ui"), "font-size": size("body") };
  return {
    frame: {
      ...text,
      "background-color": c("canvas"),
      color: c("fg"),
      "border-color": c("border"),
    },
    rail: { "background-color": c("rail"), "border-color": c("border") },
    "rail-item": { "background-color": c("accent"), "border-radius": radius("md") },
    sidebar: { "background-color": c("bg-subtle"), padding: space("sm"), gap: space("xs") },
    folder: { color: c("fg"), "border-radius": radius("sm"), padding: space("xs") },
    "folder-selected": {
      color: c("selection-fg"),
      "background-color": c("selection"),
      "border-radius": radius("sm"),
      padding: space("xs"),
    },
    list: { "background-color": c("bg"), "border-color": c("border") },
    row: { "border-color": c("border"), padding: space("sm"), gap: space("sm") },
    "row-selected": {
      "background-color": c("selection"),
      "border-color": c("border"),
      padding: space("sm"),
      gap: space("sm"),
    },
    avatar: {
      "background-color": c("avatar-1"),
      color: c("avatar-fg"),
      "font-size": size("small"),
    },
    unread: { "background-color": c("unread") },
    from: { color: c("fg"), "font-weight": token(values, "font-weight", "semibold") },
    snippet: { color: c("fg-muted"), "font-size": size("small") },
    reader: { "background-color": c("bg"), padding: space("md"), gap: space("sm") },
    title: {
      color: c("fg"),
      "font-size": size("title"),
      "font-weight": token(values, "font-weight", "semibold"),
      "font-family": token(values, "font", "display"),
    },
    body: { color: c("fg"), "line-height": token(values, "line-height", "normal") },
    tag: {
      "background-color": c("bg-raised"),
      color: c("fg-muted"),
      "border-radius": radius("sm"),
      "font-size": size("label"),
      padding: `${space("xxs")} ${space("sm")}`,
    },
    primary: {
      "background-color": c("accent"),
      color: c("accent-fg"),
      "border-radius": radius("md"),
      padding: `${space("xs")} ${space("md")}`,
    },
    secondary: {
      "background-color": c("bg"),
      color: c("fg"),
      "border-color": c("border-strong"),
      "border-radius": radius("md"),
      padding: `${space("xs")} ${space("md")}`,
    },
    field: {
      "background-color": c("bg-subtle"),
      color: c("fg-subtle"),
      "border-color": c("border-strong"),
      "border-radius": radius("sm"),
      padding: `${space("xs")} ${space("sm")}`,
    },
  };
}

function Element({
  geom,
  part,
  styles,
  children,
}: {
  geom: string;
  part: string;
  styles: Record<string, string> | undefined;
  children?: ReactNode;
}) {
  useGeometry(geom, styles ?? {});
  return (
    <span data-part={`theme-editor.preview.${part}`} data-geom={geom}>
      {children}
    </span>
  );
}

/** The preview of `tokens` in `mode` with `drafts` applied. */
export function ThemePreview({
  tokens,
  mode,
  drafts,
  name,
}: {
  tokens: readonly ThemeTokenValue[];
  mode: string | null;
  drafts: ReadonlyMap<string, string | null>;
  /** The theme's display name (the preview's label). */
  name: string;
}) {
  const t = useT();
  const base = useId();
  const styles = previewStyles(resolveTokenValues(tokens, mode, drafts));
  const el = (part: string, key: string, children?: ReactNode, look = part) => (
    <Element key={key} geom={`${base}-${key}`} part={part} styles={styles[look]}>
      {children}
    </Element>
  );
  return (
    <figure data-part="theme-editor.preview" aria-label={t("common-preview-of", { name })}>
      {el(
        "frame",
        "frame",
        <>
          {el("rail", "rail", el("rail-item", "rail-item"))}
          {el(
            "sidebar",
            "sidebar",
            <>
              {el("folder", "inbox", t("common-inbox"), "folder-selected")}
              {el("folder", "sent", t("common-folder-sent"))}
              {el("folder", "archive", t("common-folder-archive"))}
            </>,
          )}
          {el(
            "list",
            "list",
            <>
              {el(
                "row",
                "row-1",
                <>
                  {el("avatar", "avatar-1", t("theme-editor-preview-initials"))}
                  {el(
                    "row-text",
                    "row-text-1",
                    <>
                      {el("from", "from-1", t("theme-editor-preview-from"))}
                      {el("snippet", "snippet-1", t("theme-editor-preview-snippet"))}
                    </>,
                    "none",
                  )}
                  {el("unread", "unread")}
                </>,
                "row-selected",
              )}
              {el(
                "row",
                "row-2",
                <>
                  {el("avatar", "avatar-2", t("theme-editor-preview-initials-2"))}
                  {el(
                    "row-text",
                    "row-text-2",
                    <>
                      {el("from", "from-2", t("theme-editor-preview-from-2"))}
                      {el("snippet", "snippet-2", t("theme-editor-preview-snippet-2"))}
                    </>,
                    "none",
                  )}
                </>,
              )}
            </>,
          )}
          {el(
            "reader",
            "reader",
            <>
              {el("title", "title", t("theme-editor-preview-snippet"))}
              {el("tag", "tag", t("common-work"))}
              {el("body", "body", t("theme-editor-preview-body"))}
              {el(
                "actions",
                "actions",
                <>
                  {el("primary", "primary", t("common-reply"))}
                  {el("secondary", "secondary", t("common-archive"))}
                </>,
                "none",
              )}
              {el("field", "field", t("common-search-mail"))}
            </>,
          )}
        </>,
      )}
      <figcaption data-part="theme-editor.preview.caption">
        {t("theme-editor-preview-caption")}
      </figcaption>
    </figure>
  );
}
