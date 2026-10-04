/**
 * The HTML (TipTap / ProseMirror) and Markdown (CodeMirror) editors are
 * loaded on demand, in their own chunks: the main bundle stays small and
 * the editors arrive when a composer opens (it starts loading both at
 * once, so switching modes is instant). Until a chunk is there a status
 * line takes the editor's place.
 *
 * `loadHtmlEditor` / `loadMarkdownEditor` also let tests load the modules
 * up front, after which the editors render synchronously.
 */
import { useEffect, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import type * as HtmlEditorModule from "./HtmlEditor";
import type { HtmlEditorProps } from "./HtmlEditor";
import type * as MarkdownEditorModule from "./MarkdownEditor";
import type { MarkdownEditorProps } from "./MarkdownEditor";

type HtmlModule = typeof HtmlEditorModule;
type MarkdownModule = typeof MarkdownEditorModule;

let htmlModule: HtmlModule | null = null;
let markdownModule: MarkdownModule | null = null;
let htmlLoading: Promise<HtmlModule> | null = null;
let markdownLoading: Promise<MarkdownModule> | null = null;

/** Loads the HTML editor chunk (once). */
export function loadHtmlEditor(): Promise<HtmlModule> {
  htmlLoading ??= import("./HtmlEditor").then((m) => (htmlModule = m));
  return htmlLoading;
}

/** Loads the Markdown editor chunk (once). */
export function loadMarkdownEditor(): Promise<MarkdownModule> {
  markdownLoading ??= import("./MarkdownEditor").then((m) => (markdownModule = m));
  return markdownLoading;
}

/** Starts loading both editors without waiting (the composer opened). */
export function prefetchEditors(): void {
  // A failed load shows when the editor is rendered (`useModule`).
  loadHtmlEditor().catch(() => null);
  loadMarkdownEditor().catch(() => null);
}

function useModule<M>(current: M | null, load: () => Promise<M>): M | null | "failed" {
  const [mod, setMod] = useState<M | null | "failed">(current);
  useEffect(() => {
    if (mod !== null) return;
    let live = true;
    load().then(
      (m) => {
        if (live) setMod(m);
      },
      (error: unknown) => {
        console.error("nail: cannot load the editor", error);
        if (live) setMod("failed");
      },
    );
    return () => {
      live = false;
    };
  }, [mod, load]);
  return mod;
}

function Loading({ failed }: { failed: boolean }) {
  const t = useT();
  return (
    <div
      data-part="composer.editor-loading"
      data-state={failed ? "failed" : undefined}
      role={failed ? "alert" : "status"}
    >
      {t(failed ? "composer-editor-failed" : "composer-editor-loading")}
    </div>
  );
}

export function LazyHtmlEditor(props: HtmlEditorProps) {
  const mod = useModule(htmlModule, loadHtmlEditor);
  if (mod === null || mod === "failed") return <Loading failed={mod === "failed"} />;
  return <mod.HtmlEditor {...props} />;
}

export function LazyMarkdownEditor(props: MarkdownEditorProps) {
  const mod = useModule(markdownModule, loadMarkdownEditor);
  if (mod === null || mod === "failed") return <Loading failed={mod === "failed"} />;
  return <mod.MarkdownEditor {...props} />;
}
