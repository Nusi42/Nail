/**
 * The HTML editor (TipTap / ProseMirror): text formatting, headings, lists,
 * quotes, code, links, inline images and tables, plus a raw-HTML source
 * view. The stored HTML references inline images as `cid:<id>`; the editor
 * shows local previews of freshly added images instead (`toDisplayHtml`).
 */
import type { Editor } from "@tiptap/core";
import Image from "@tiptap/extension-image";
import { TableKit } from "@tiptap/extension-table";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type { DraftAttachment } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { toDisplayHtml, toStoredHtml } from "./draft";
import { FORMAT_COMMANDS, TABLE_COMMANDS, type Command } from "./toolbar";

const editors = new WeakMap<Element, Editor>();

/** The TipTap editor inside a rendered HTML editor (tests, integrations). */
export function htmlEditorIn(root: ParentNode): Editor | null {
  const element = root.querySelector('[data-part="composer.html"]');
  return element ? (editors.get(element) ?? null) : null;
}

export interface HtmlEditorProps {
  value: string;
  onChange(html: string): void;
  label: string;
  /** Local previews of inline images (content id → URL). */
  previews: Readonly<Record<string, string>>;
  /** Uploads images as inline attachments; resolves with them. */
  onInsertImages(files: File[]): Promise<DraftAttachment[]>;
  source: boolean;
  onToggleSource(): void;
}

function ToolButton({
  id,
  pressed,
  disabled,
  onClick,
}: {
  id: string;
  pressed?: boolean | undefined;
  disabled?: boolean | undefined;
  onClick(): void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      data-part="composer.format.button"
      data-command={id}
      data-state={pressed ? "active" : undefined}
      aria-pressed={pressed}
      aria-label={t(`composer-format-${id}`)}
      title={t(`composer-format-${id}`)}
      disabled={disabled}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      <Icon name={`format-${id}`} part="composer.format.icon" />
      <span data-part="composer.format.label">{t(`composer-format-${id}-short`)}</span>
    </button>
  );
}

export function HtmlEditor({
  value,
  onChange,
  label,
  previews,
  onInsertImages,
  source,
  onToggleSource,
}: HtmlEditorProps) {
  const t = useT();
  const ids = useId();
  const previewsRef = useRef(previews);
  const emitted = useRef<string | null>(null);
  const imageInput = useRef<HTMLInputElement>(null);
  const [linkOpen, setLinkOpen] = useState(false);
  const [href, setHref] = useState("");

  useEffect(() => {
    previewsRef.current = previews;
  }, [previews]);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({ link: { openOnClick: false, autolink: true } }),
      Image.configure({ allowBase64: true }),
      TableKit.configure({ table: { resizable: false } }),
    ],
    content: toDisplayHtml(value, previews),
    injectCSS: false,
    immediatelyRender: true,
    shouldRerenderOnTransaction: true,
    editorProps: {
      attributes: {
        "data-part": "composer.html.content",
        role: "textbox",
        "aria-multiline": "true",
        "aria-label": label,
      },
    },
    onUpdate: ({ editor: updated }) => {
      const html = toStoredHtml(updated.getHTML(), previewsRef.current);
      emitted.current = html;
      onChange(html);
    },
  });

  // A new value from outside (mode switch, source view, reopened draft).
  useEffect(() => {
    if (!editor || source || value === emitted.current) return;
    const current = toStoredHtml(editor.getHTML(), previewsRef.current);
    if (current !== value) {
      editor.commands.setContent(toDisplayHtml(value, previewsRef.current), {
        emitUpdate: false,
      });
    }
    emitted.current = value;
  }, [editor, value, source]);

  // Previews that arrive later (inline images of a reopened draft) replace
  // their `cid:` references in the editor; the stored HTML stays the same.
  useEffect(() => {
    if (!editor || source) return;
    const shown = editor.getHTML();
    if (!Object.keys(previews).some((cid) => shown.includes(`cid:${cid}`))) return;
    editor.commands.setContent(toDisplayHtml(toStoredHtml(shown, previews), previews), {
      emitUpdate: false,
    });
  }, [editor, previews, source]);

  const run = (command: Command) => {
    if (editor) command.run(editor.chain().focus()).run();
  };

  const applyLink = (event: FormEvent) => {
    event.preventDefault();
    event.stopPropagation();
    if (!editor) return;
    const url = href.trim();
    const chain = editor.chain().focus().extendMarkRange("link");
    if (url === "") chain.unsetLink().run();
    else chain.setLink({ href: url }).run();
    setLinkOpen(false);
  };

  const insertImages = async (files: File[]) => {
    const added = await onInsertImages(files);
    if (!editor) return;
    for (const attachment of added) {
      const cid = attachment.content_id;
      const src = cid === null ? null : (previewsRef.current[cid] ?? `cid:${cid}`);
      if (src !== null) {
        editor.chain().focus().setImage({ src, alt: attachment.filename }).run();
      }
    }
  };

  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = root.current;
    if (!element || !editor) return;
    editors.set(element, editor);
    return () => {
      editors.delete(element);
    };
  }, [editor]);

  const inTable = editor?.isActive("table") ?? false;

  return (
    <div ref={root} data-part="composer.html" data-state={source ? "source" : undefined}>
      <div data-part="composer.format" role="toolbar" aria-label={t("composer-format-label")}>
        {FORMAT_COMMANDS.map((command) => (
          <ToolButton
            key={command.id}
            id={command.id}
            disabled={source || !editor}
            pressed={command.active && editor ? editor.isActive(...command.active) : undefined}
            onClick={() => run(command)}
          />
        ))}
        <ToolButton
          id="link"
          disabled={source || !editor}
          pressed={editor?.isActive("link") ?? false}
          onClick={() => {
            setHref(String(editor?.getAttributes("link").href ?? ""));
            setLinkOpen(!linkOpen);
          }}
        />
        <ToolButton
          id="image"
          disabled={source || !editor}
          onClick={() => imageInput.current?.click()}
        />
        {inTable &&
          !source &&
          TABLE_COMMANDS.map((command) => (
            <ToolButton key={command.id} id={command.id} onClick={() => run(command)} />
          ))}
        <ToolButton id="source" pressed={source} onClick={onToggleSource} />
        <input
          ref={imageInput}
          data-part="composer.file-input"
          type="file"
          accept="image/*"
          multiple
          hidden
          aria-label={t("composer-format-image")}
          onChange={(event) => {
            const files = [...(event.target.files ?? [])];
            event.target.value = "";
            if (files.length > 0) void insertImages(files);
          }}
        />
      </div>
      {linkOpen && !source && (
        <div data-part="composer.link" role="group" aria-label={t("composer-link-label")}>
          <label data-part="composer.field.label" htmlFor={`${ids}-href`}>
            {t("composer-link-url")}
          </label>
          <input
            id={`${ids}-href`}
            data-part="composer.link.input"
            type="url"
            value={href}
            autoFocus
            onChange={(event) => setHref(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") applyLink(event);
            }}
          />
          <button
            type="button"
            data-part="form.button"
            data-action="apply-link"
            onClick={applyLink}
          >
            {t("composer-link-apply")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel-link"
            onClick={() => setLinkOpen(false)}
          >
            {t("composer-link-cancel")}
          </button>
        </div>
      )}
      {source && (
        <textarea
          data-part="composer.html.source"
          aria-label={t("composer-html-source")}
          spellCheck={false}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      )}
      {/* Stays mounted in source view so the editor keeps its history. */}
      <div data-part="composer.html.editor" hidden={source}>
        <EditorContent editor={editor} />
      </div>
    </div>
  );
}
