/**
 * Plug-in extensions in the composer (M4a WP105, ADR 0084): the plug-ins'
 * composer actions and Markdown blocks in the toolbar, edits that plug-ins
 * and AI agents (`edit_open_draft`) send to the open draft, who made them,
 * and the plug-in blocks of a Markdown draft below the preview (their HTML
 * is rendered by the plug-in when the message is sent).
 */
import { useCallback, type RefObject } from "react";
import type { ComposerEdit, ComposerMode, ComposerSelection } from "../../api/generated";
import type { ComposeContext } from "../../state/compose";
import { useUi } from "../../state/uiStore";
import {
  applyComposerEdit,
  ComposerEditNotices,
  ComposerExtensionBar,
  PluginMarkdownBlock,
  useComposerEdits,
} from "../plugins";
import { saveRich } from "./flows";
import { composerOf, patchComposer } from "./sessions";
import { richStoreFor, useComposerWindow, useRich } from "./store";

/** What an edit changes: the source text of the draft's mode. */
export interface EditableBody {
  text: string;
  html: string;
  markdown: string;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Plain text as HTML paragraphs (an edit in the HTML editor). */
export function textAsHtml(text: string): string {
  return text
    .split(/\n{2,}/)
    .map((p) => `<p>${escapeHtml(p).replace(/\n/g, "<br>")}</p>`)
    .join("");
}

/**
 * Applies a plug-in's or agent's edit to the body of `mode`. Plain text and
 * Markdown use the selection; the HTML editor appends inserts at the end
 * (its selection is not known here) and replaces all on `replace-all`.
 */
export function applyEditToBody(
  mode: ComposerMode,
  body: EditableBody,
  selection: Pick<ComposerSelection, "start" | "end">,
  edit: Pick<ComposerEdit, "op" | "text" | "format">,
): EditableBody {
  switch (mode) {
    case "plain":
      return { ...body, text: applyComposerEdit(body.text, selection, edit).text };
    case "markdown":
      return { ...body, markdown: applyComposerEdit(body.markdown, selection, edit).text };
    case "html": {
      const html = edit.format === "html" ? edit.text : textAsHtml(edit.text);
      return { ...body, html: edit.op === "replace-all" ? html : `${body.html}${html}` };
    }
  }
}

/** A fenced plug-in block (`<plugin>.<block>` info string) in Markdown. */
export interface PluginFence {
  info: string;
  body: string;
}

/** The plug-in blocks of a Markdown body, in order (closed fences only). */
export function pluginFences(markdown: string): PluginFence[] {
  const fences: PluginFence[] = [];
  const lines = markdown.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const open = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)/.exec(lines[i] ?? "");
    const marker = open?.[1];
    if (!open || !marker) continue;
    const info = open[2] ?? "";
    let end = -1;
    for (let j = i + 1; j < lines.length; j++) {
      const closing = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[j] ?? "")?.[1];
      if (closing && closing[0] === marker[0] && closing.length >= marker.length) {
        end = j;
        break;
      }
    }
    if (end < 0) break;
    if (/^[^.\s]+\.[^.\s]+$/.test(info)) {
      fences.push({ info, body: lines.slice(i + 1, end).join("\n") });
    }
    i = end;
  }
  return fences;
}

function selectionOf(
  mode: ComposerMode,
  body: EditableBody,
  textarea: HTMLTextAreaElement | null,
): ComposerSelection {
  if (mode === "plain" && textarea) {
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    return { text: body.text.slice(start, end), start, end };
  }
  const source = mode === "markdown" ? body.markdown : body.html;
  return { text: "", start: source.length, end: source.length };
}

/** The composer's plug-in toolbar and the edits sent to its draft. */
export function ComposerPlugins({
  ctx,
  mode,
  textarea,
}: {
  ctx: ComposeContext;
  mode: ComposerMode;
  textarea: RefObject<HTMLTextAreaElement | null>;
}) {
  const window = useComposerWindow();
  const composer = useUi((s) => composerOf(s, window));
  const rich = useRich((s) => s);
  const draftId = composer?.draftId ?? null;
  const body: EditableBody = {
    text: composer?.text ?? "",
    html: rich.html,
    markdown: rich.markdown,
  };
  const apply = useCallback(
    (pending: { edit: ComposerEdit }) => {
      const state = composerOf(ctx.store.getState(), window);
      if (!state) return;
      const store = richStoreFor(ctx.store, window);
      const current = store.getState();
      const now: EditableBody = {
        text: state.text,
        html: current.html,
        markdown: current.markdown,
      };
      const next = applyEditToBody(
        current.mode,
        now,
        selectionOf(current.mode, now, textarea.current),
        pending.edit,
      );
      if (next.text !== now.text) {
        patchComposer(ctx.store, window, { text: next.text, error: null });
      }
      if (next.html !== now.html || next.markdown !== now.markdown) {
        store.setState({ html: next.html, markdown: next.markdown });
      }
    },
    [ctx.store, textarea, window],
  );
  useComposerEdits(draftId, apply);
  if (!composer) return null;
  return (
    <>
      <ComposerExtensionBar
        draftId={draftId}
        ensureDraft={() => saveRich(ctx, { quiet: true, window })}
        getSelection={() => selectionOf(mode, body, textarea.current)}
        {...(mode === "markdown"
          ? {
              onInsert: (markdown: string) =>
                richStoreFor(ctx.store, window).setState({
                  markdown: `${rich.markdown}${rich.markdown && !rich.markdown.endsWith("\n") ? "\n" : ""}${markdown}`,
                }),
            }
          : {})}
      />
      <ComposerEditNotices draftId={draftId} />
    </>
  );
}

/** The plug-in blocks of a Markdown draft (shown with the preview). */
export function PluginBlocks({ markdown }: { markdown: string }) {
  const fences = pluginFences(markdown);
  if (fences.length === 0) return null;
  return (
    <div data-part="composer.plugin-blocks">
      {fences.map((fence, i) => (
        <PluginMarkdownBlock key={`${i}-${fence.info}`} info={fence.info} body={fence.body} />
      ))}
    </div>
  );
}
