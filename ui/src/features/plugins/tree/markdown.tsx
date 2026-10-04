/**
 * The `markdown` node: a CommonMark subset drawn as React elements (ADR
 * 0050: "no raw HTML, no remote images; links open outside Nail after a
 * confirmation"). Nothing a plug-in writes is ever parsed as HTML — raw HTML
 * in the source stays literal text, images show their alternative text, and
 * only `http(s):` links become links.
 *
 * Blocks: ATX headings, paragraphs, `-` / `*` / `+` and `1.` lists, `>`
 * quotes, fenced code, thematic breaks. Inline: `code`, `**strong**`,
 * `*emphasis*` / `_emphasis_`, `[text](url)`, `<https://…>`, `![alt](…)`.
 */
import { useState, type ReactNode } from "react";
import { useT } from "../../../i18n/I18nProvider";
import { partAttrs } from "../parts";

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong"; children: Inline[] }
  | { kind: "em"; children: Inline[] }
  | { kind: "link"; href: string | null; children: Inline[] };

export type Block =
  | { kind: "heading"; level: number; inline: Inline[] }
  | { kind: "paragraph"; inline: Inline[] }
  | { kind: "list"; ordered: boolean; items: Inline[][] }
  | { kind: "quote"; blocks: Block[] }
  | { kind: "code"; text: string }
  | { kind: "rule" };

/** Only web links are links (anything else is shown as text). */
export function safeHref(url: string): string | null {
  try {
    const parsed = new URL(url.trim());
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.href : null;
  } catch {
    return null;
  }
}

function findClosing(text: string, from: number, marker: string): number {
  let at = text.indexOf(marker, from);
  while (at >= 0 && at === from) at = text.indexOf(marker, at + 1);
  return at;
}

/** Parses inline Markdown. */
export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let buffer = "";
  const flush = () => {
    if (buffer !== "") out.push({ kind: "text", text: buffer });
    buffer = "";
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const ch = text.charAt(i);
    const after = text.charAt(i + 1);
    if (ch === "\\" && /[\\`*_[\]()<>!#-]/.test(after)) {
      buffer += after;
      i += 2;
      continue;
    }
    if (ch === "`") {
      const end = text.indexOf("`", i + 1);
      if (end > i) {
        flush();
        out.push({ kind: "code", text: text.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }
    if (rest.startsWith("**") || rest.startsWith("__")) {
      const marker = rest.slice(0, 2);
      const end = findClosing(text, i + 2, marker);
      if (end > i + 2) {
        flush();
        out.push({ kind: "strong", children: parseInline(text.slice(i + 2, end)) });
        i = end + 2;
        continue;
      }
    }
    if (ch === "*" || ch === "_") {
      const end = findClosing(text, i + 1, ch);
      if (end > i + 1 && text[i + 1] !== " ") {
        flush();
        out.push({ kind: "em", children: parseInline(text.slice(i + 1, end)) });
        i = end + 1;
        continue;
      }
    }
    const image = /^!\[([^\]]*)\]\(([^)\s]*)\)/.exec(rest);
    if (image) {
      buffer += image[1] ?? "";
      i += image[0].length;
      continue;
    }
    const link = /^\[([^\]]+)\]\(([^)\s]+)\)/.exec(rest);
    if (link) {
      flush();
      out.push({
        kind: "link",
        href: safeHref(link[2] ?? ""),
        children: parseInline(link[1] ?? ""),
      });
      i += link[0].length;
      continue;
    }
    const auto = /^<(https?:\/\/[^\s<>]+)>/.exec(rest);
    if (auto) {
      flush();
      const url = auto[1] ?? "";
      out.push({ kind: "link", href: safeHref(url), children: [{ kind: "text", text: url }] });
      i += auto[0].length;
      continue;
    }
    buffer += ch;
    i += 1;
  }
  flush();
  return out;
}

const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const BULLET = /^\s{0,3}[-*+]\s+(.*)$/;
const ORDERED = /^\s{0,3}\d{1,9}[.)]\s+(.*)$/;
const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const FENCE = /^\s{0,3}(```|~~~)/;

/** Parses block Markdown. */
export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  const peek = (): string | undefined => lines[i];
  for (let line = peek(); line !== undefined; line = peek()) {
    if (line.trim() === "") {
      i += 1;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const marker = fence[1] ?? "";
      const body: string[] = [];
      i += 1;
      for (let next = peek(); next !== undefined; next = peek()) {
        if (next.trimStart().startsWith(marker)) break;
        body.push(next);
        i += 1;
      }
      i += 1;
      blocks.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({
        kind: "heading",
        level: (heading[1] ?? "").length,
        inline: parseInline(heading[2] ?? ""),
      });
      i += 1;
      continue;
    }
    if (RULE.test(line)) {
      blocks.push({ kind: "rule" });
      i += 1;
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      const inner: string[] = [];
      for (let next = peek(); next !== undefined && /^\s{0,3}>/.test(next); next = peek()) {
        inner.push(next.replace(/^\s{0,3}>\s?/, ""));
        i += 1;
      }
      blocks.push({ kind: "quote", blocks: parseMarkdown(inner.join("\n")) });
      continue;
    }
    const bullet = BULLET.exec(line);
    const ordered = ORDERED.exec(line);
    if (bullet || ordered) {
      const re = bullet ? BULLET : ORDERED;
      const items: Inline[][] = [];
      for (let next = peek(); next !== undefined; next = peek()) {
        const match = re.exec(next);
        if (!match) break;
        items.push(parseInline(match[1] ?? ""));
        i += 1;
      }
      blocks.push({ kind: "list", ordered: !bullet, items });
      continue;
    }
    const paragraph: string[] = [];
    for (let next = peek(); next !== undefined; next = peek()) {
      if (
        next.trim() === "" ||
        FENCE.test(next) ||
        HEADING.test(next) ||
        RULE.test(next) ||
        /^\s{0,3}>/.test(next) ||
        BULLET.test(next) ||
        ORDERED.test(next)
      ) {
        break;
      }
      paragraph.push(next.trim());
      i += 1;
    }
    blocks.push({ kind: "paragraph", inline: parseInline(paragraph.join(" ")) });
  }
  return blocks;
}

function Link({ plugin, href, children }: { plugin: string; href: string; children: ReactNode }) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  return (
    <>
      <a
        {...partAttrs("plugin.markdown.link", plugin)}
        href={href}
        rel="noopener noreferrer"
        onClick={(event) => {
          event.preventDefault();
          setConfirming(true);
        }}
      >
        {children}
      </a>
      {confirming && (
        <span {...partAttrs("plugin.markdown.confirm", plugin)} role="group">
          <span {...partAttrs("plugin.markdown.confirm.text", plugin)}>
            {t("plugin-link-confirm", { url: href })}
          </span>
          <button
            type="button"
            {...partAttrs("plugin.markdown.confirm.button", plugin)}
            data-action="open"
            onClick={() => {
              setConfirming(false);
              window.open(href, "_blank", "noopener,noreferrer");
            }}
          >
            {t("plugin-link-open")}
          </button>
          <button
            type="button"
            {...partAttrs("plugin.markdown.confirm.button", plugin)}
            data-action="cancel"
            onClick={() => {
              setConfirming(false);
            }}
          >
            {t("plugin-link-cancel")}
          </button>
        </span>
      )}
    </>
  );
}

function InlineView({ plugin, nodes }: { plugin: string; nodes: readonly Inline[] }) {
  return (
    <>
      {nodes.map((node, index) => {
        switch (node.kind) {
          case "text":
            return <span key={index}>{node.text}</span>;
          case "code":
            return (
              <code key={index} {...partAttrs("plugin.markdown.code", plugin)}>
                {node.text}
              </code>
            );
          case "strong":
            return (
              <strong key={index}>
                <InlineView plugin={plugin} nodes={node.children} />
              </strong>
            );
          case "em":
            return (
              <em key={index}>
                <InlineView plugin={plugin} nodes={node.children} />
              </em>
            );
          case "link":
            return node.href === null ? (
              <span key={index}>
                <InlineView plugin={plugin} nodes={node.children} />
              </span>
            ) : (
              <Link key={index} plugin={plugin} href={node.href}>
                <InlineView plugin={plugin} nodes={node.children} />
              </Link>
            );
        }
      })}
    </>
  );
}

/** Heading levels start below the dialog / panel title (h2): `#` is an h3. */
export function headingTag(level: number): "h3" | "h4" | "h5" | "h6" {
  return (["h3", "h4", "h5", "h6"] as const)[Math.min(Math.max(level, 1), 4) - 1] ?? "h6";
}

function Heading({ plugin, level, inline }: { plugin: string; level: number; inline: Inline[] }) {
  const attrs = { ...partAttrs("plugin.markdown.heading", plugin), "data-level": level };
  const content = <InlineView plugin={plugin} nodes={inline} />;
  switch (headingTag(level)) {
    case "h3":
      return <h3 {...attrs}>{content}</h3>;
    case "h4":
      return <h4 {...attrs}>{content}</h4>;
    case "h5":
      return <h5 {...attrs}>{content}</h5>;
    case "h6":
      return <h6 {...attrs}>{content}</h6>;
  }
}

function BlockView({ plugin, block }: { plugin: string; block: Block }) {
  switch (block.kind) {
    case "heading":
      return <Heading plugin={plugin} level={block.level} inline={block.inline} />;
    case "paragraph":
      return (
        <p {...partAttrs("plugin.markdown.paragraph", plugin)}>
          <InlineView plugin={plugin} nodes={block.inline} />
        </p>
      );
    case "list": {
      const items = block.items.map((item, index) => (
        <li key={index} {...partAttrs("plugin.markdown.item", plugin)}>
          <InlineView plugin={plugin} nodes={item} />
        </li>
      ));
      return block.ordered ? (
        <ol {...partAttrs("plugin.markdown.list", plugin)}>{items}</ol>
      ) : (
        <ul {...partAttrs("plugin.markdown.list", plugin)}>{items}</ul>
      );
    }
    case "quote":
      return (
        <blockquote {...partAttrs("plugin.markdown.quote", plugin)}>
          {block.blocks.map((inner, index) => (
            <BlockView key={index} plugin={plugin} block={inner} />
          ))}
        </blockquote>
      );
    case "code":
      return (
        <pre {...partAttrs("plugin.markdown.code-block", plugin)}>
          <code>{block.text}</code>
        </pre>
      );
    case "rule":
      return <hr {...partAttrs("plugin.markdown.rule", plugin)} />;
  }
}

/** A `markdown` node. */
export function Markdown({ plugin, value }: { plugin: string; value: string }) {
  const blocks = parseMarkdown(value);
  return (
    <div {...partAttrs("plugin.markdown", plugin)}>
      {blocks.map((block, index) => (
        <BlockView key={index} plugin={plugin} block={block} />
      ))}
    </div>
  );
}
