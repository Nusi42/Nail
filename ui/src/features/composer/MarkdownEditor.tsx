/**
 * The Markdown source editor (CodeMirror 6). The document is controlled:
 * edits go out through `onChange`, and a changed `value` (mode switch,
 * signature swap, reopened draft) replaces the document.
 */
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { useEffect, useRef, useState } from "react";

/** The content attributes, among them the accessible name. */
function contentAttributes(label: string) {
  return EditorView.contentAttributes.of({
    "data-part": "composer.markdown.content",
    "aria-label": label,
    "aria-multiline": "true",
    spellcheck: "true",
  });
}

/** Keys the composer uses itself (send, save) stay out of CodeMirror's keymap. */
const RESERVED = new Set(["Mod-Enter", "Mod-s"]);

export interface MarkdownEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** Accessible name of the text area. */
  label: string;
}

export function MarkdownEditor({ value, onChange, label }: MarkdownEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const initial = useRef(value);
  // The editor is created once; a new label (a language switch) is swapped
  // in through this compartment.
  const labelRef = useRef(label);
  const [attributes] = useState(() => new Compartment());

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    if (!host.current) return;
    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: initial.current,
        extensions: [
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap].filter((b) => !RESERVED.has(b.key ?? ""))),
          markdown(),
          syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
          EditorView.lineWrapping,
          EditorView.editorAttributes.of({ "data-part": "composer.markdown.editor" }),
          attributes.of(contentAttributes(labelRef.current)),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = editor;
    return () => {
      editor.destroy();
      view.current = null;
    };
  }, [attributes]);

  useEffect(() => {
    labelRef.current = label;
    view.current?.dispatch({
      effects: attributes.reconfigure(contentAttributes(label)),
    });
  }, [attributes, label]);

  useEffect(() => {
    const editor = view.current;
    if (!editor) return;
    const current = editor.state.doc.toString();
    if (current !== value) {
      editor.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }
  }, [value]);

  return <div data-part="composer.markdown.source" ref={host} />;
}

/** The CodeMirror view inside a rendered Markdown editor (tests, integrations). */
export function markdownViewIn(root: ParentNode): EditorView | null {
  const element = root.querySelector('[data-part="composer.markdown.editor"]');
  return element instanceof HTMLElement ? EditorView.findFromDOM(element) : null;
}
