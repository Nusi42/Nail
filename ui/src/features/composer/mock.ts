/**
 * The mock core's answers to the composer requests (WP21), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.composer` ([`createComposerMockState`]).
 *
 * The real work (Markdown rendering, HTML ↔ Markdown, quoting) is the
 * core's (`nail-compose`); this mock only imitates it closely enough for UI
 * tests and the `?mock=1` dev mode.
 */
import type {
  Address,
  ComposerMode,
  Draft,
  DraftContext,
  Identity,
  Request,
  Response,
} from "../../api/generated";
import type { MockTransport } from "../../api/mock";
import { chooseIdentity } from "../../api/mockAccounts";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const COMPOSER_REQUESTS = [
  "NewDraft",
  "RenderPreview",
  "ConvertDraft",
  "UploadAttachment",
  "SuggestAddresses",
] as const satisfies readonly Request["type"][];

export type ComposerRequest = Extract<Request, { type: (typeof COMPOSER_REQUESTS)[number] }>;

export function isComposerRequest(request: Request): request is ComposerRequest {
  return (COMPOSER_REQUESTS as readonly string[]).includes(request.type);
}

/** An uploaded file in the mock blob store. */
export interface MockBlob {
  filename: string;
  mime: string;
  /** Base64 content. */
  data: string;
}

/** Mock-core state of this feature. */
export interface ComposerMockState {
  /** Uploaded attachments by blob hash. */
  blobs: Map<string, MockBlob>;
  /** Addresses collected besides those in the mock mail (autocomplete). */
  addresses: Address[];
}

export function createComposerMockState(): ComposerMockState {
  return { blobs: new Map(), addresses: [] };
}

/** Largest attachment the mock accepts (the core's limit is configurable). */
export const MOCK_MAX_ATTACHMENT = 25 * 1024 * 1024;

export function answerComposer(core: MockCore, request: ComposerRequest): Response {
  switch (request.type) {
    case "NewDraft":
      return newDraft(core, request.identity_id, request.account_id, request.mode, request.context);
    case "RenderPreview":
      return { type: "PreviewRendered", html: mailDocument(renderMarkdown(request.markdown)) };
    case "ConvertDraft":
      return convertDraft(request.draft, request.mode);
    case "UploadAttachment":
      return upload(core, request.filename, request.mime, request.data, request.content_id);
    case "SuggestAddresses":
      return {
        type: "AddressSuggestions",
        addresses: suggest(core, request.prefix, request.limit),
      };
  }
}

// ---- Text helpers (a tiny stand-in for nail-compose) ----

export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function inline(text: string): string {
  return escapeHtml(text)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\*([^*]+)\*/g, "<em>$1</em>")
    .replace(/~~([^~]+)~~/g, "<del>$1</del>")
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>');
}

/** Markdown subset → HTML fragment: headings, lists, quotes, code, paragraphs. */
export function renderMarkdown(markdown: string): string {
  const out: string[] = [];
  const lines = markdown.replaceAll("\r\n", "\n").split("\n");
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length > 0) out.push(`<p>${paragraph.map(inline).join("<br>")}</p>`);
    paragraph = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i] ?? "";
    const fence = /^```(\w*)/.exec(line);
    if (fence) {
      flush();
      const code: string[] = [];
      for (i++; i < lines.length && !(lines[i] ?? "").startsWith("```"); i++) {
        code.push(lines[i] ?? "");
      }
      const lang = fence[1] ? ` class="language-${fence[1]}"` : "";
      out.push(`<pre><code${lang}>${escapeHtml(code.join("\n"))}</code></pre>`);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      const level = (heading[1] ?? "#").length;
      out.push(`<h${level}>${inline(heading[2] ?? "")}</h${level}>`);
      continue;
    }
    if (/^[-*]\s+/.test(line)) {
      flush();
      const items: string[] = [];
      for (; i < lines.length && /^[-*]\s+/.test(lines[i] ?? ""); i++) {
        items.push(`<li>${inline((lines[i] ?? "").replace(/^[-*]\s+/, ""))}</li>`);
      }
      i--;
      out.push(`<ul>${items.join("")}</ul>`);
      continue;
    }
    if (line.startsWith(">")) {
      flush();
      const quoted: string[] = [];
      for (; i < lines.length && (lines[i] ?? "").startsWith(">"); i++) {
        quoted.push((lines[i] ?? "").replace(/^> ?/, ""));
      }
      i--;
      out.push(`<blockquote>${renderMarkdown(quoted.join("\n"))}</blockquote>`);
      continue;
    }
    if (line.trim() === "") flush();
    else paragraph.push(line);
  }
  flush();
  return out.join("\n");
}

function mailDocument(fragment: string): string {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"></head><body style="font-family: sans-serif;">${fragment}</body></html>`;
}

function textToHtml(text: string): string {
  return text
    .split(/\n{2,}/)
    .filter((block) => block.trim() !== "")
    .map((block) => `<p>${block.split("\n").map(escapeHtml).join("<br>")}</p>`)
    .join("");
}

function decodeEntities(text: string): string {
  return text
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&nbsp;", " ")
    .replaceAll("&amp;", "&");
}

function htmlToText(html: string): string {
  return decodeEntities(
    html
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/<\/(p|div|h\d|li|blockquote)>/gi, "\n\n")
      .replace(/<[^>]+>/g, ""),
  )
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function htmlToMarkdown(html: string): { markdown: string; lossy: boolean } {
  const lossy = /<(u|font|sub|sup|mark)\b|\sstyle="|colspan=/i.test(html);
  const markdown = htmlToText(
    html
      .replace(/<(strong|b)>/gi, "**")
      .replace(/<\/(strong|b)>/gi, "**")
      .replace(/<(em|i)>/gi, "*")
      .replace(/<\/(em|i)>/gi, "*")
      .replace(/<h(\d)[^>]*>/gi, (_, n: string) => `${"#".repeat(Number(n))} `)
      .replace(/<li[^>]*>/gi, "- "),
  );
  return { markdown, lossy };
}

/** `ConvertDraft` as the core would answer it. */
export function convertDraft(draft: Draft, mode: ComposerMode): Response {
  const source =
    draft.mode === "html"
      ? (draft.html ?? "")
      : draft.mode === "markdown"
        ? (draft.markdown ?? "")
        : draft.text;
  let body = source;
  let lossy = false;
  if (draft.mode !== mode) {
    if (draft.mode === "plain") body = mode === "html" ? textToHtml(source) : source;
    else if (draft.mode === "markdown" && mode === "html") body = renderMarkdown(source);
    else if (draft.mode === "markdown") lossy = /[*_`#>[\]~]|^\s*[-+] /m.test(source);
    else if (mode === "markdown") ({ markdown: body, lossy } = htmlToMarkdown(source));
    else {
      body = htmlToText(source);
      lossy = /<(?!\/?(p|br|div)\b)[a-z]/i.test(source);
    }
  }
  return {
    type: "DraftConverted",
    draft: {
      ...draft,
      mode,
      text: mode === "plain" ? body : "",
      html: mode === "html" ? body : null,
      markdown: mode === "markdown" ? body : null,
      markdown_part: mode === "markdown" && draft.markdown_part,
    },
    lossy,
  };
}

// ---- NewDraft ----

function identities(core: MockCore): Identity[] {
  return core.data.accountsConfig.accounts.flatMap((a) => a.identities);
}

function signatureBlock(identity: Identity, mode: ComposerMode): string {
  const sig = identity.signature;
  if (!sig) return "";
  if (mode === "html") {
    const html = sig.html ?? (sig.text ? textToHtml(sig.text) : null);
    return html ? `<div class="signature"><p>-- </p>${html}</div>` : "";
  }
  const text = mode === "markdown" ? (sig.markdown ?? sig.text) : sig.text;
  return text ? `-- \n${text}` : "";
}

function prefixed(subject: string, prefix: string): string {
  return subject.toLowerCase().startsWith(prefix.toLowerCase()) ? subject : `${prefix} ${subject}`;
}

function newDraft(
  core: MockCore,
  identityId: string | null,
  accountId: string | null,
  mode: ComposerMode | null,
  context: DraftContext | null,
): Response {
  const original = context ? core.data.bodies.get(context.message_id) : undefined;
  if (context && !original) {
    return errorResponse("not_found", `message ${context.message_id} not found`);
  }
  let id = identityId;
  if (id === null) {
    const chosen = chooseIdentity(core.data, accountId, [], context?.message_id ?? null);
    id = chosen.type === "IdentityChosen" ? chosen.identity_id : null;
  }
  const identity = identities(core).find((i) => i.id === id);
  if (!identity) return errorResponse("invalid_request", `unknown identity ${id ?? ""}`);
  const m: ComposerMode = mode ?? identity.composer_mode ?? "plain";
  const sig = signatureBlock(identity, m);
  const draft: Draft = {
    id: null,
    identity_id: identity.id,
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    mode: m,
    text: "",
    html: null,
    markdown: null,
    attachments: [],
    context,
    signature: "identity",
    markdown_part: false,
  };
  let body = m === "html" ? `<p></p>${sig}` : sig === "" ? "" : `\n\n${sig}`;
  if (context && original) {
    const h = original.headers;
    const sender = h.from[0];
    const header = `On ${h.date ?? ""}, ${sender?.name ?? sender?.email ?? ""} wrote:`;
    if (context.mode === "forward") {
      draft.subject = prefixed(h.subject, "Fwd:");
      body =
        m === "html"
          ? `<p></p>${sig}<p>Forwarded message</p>${original.sanitized_html ?? textToHtml(original.text)}`
          : `${body}\n\nForwarded message\n\n${original.text}`;
    } else {
      draft.subject = prefixed(h.subject, "Re:");
      const own = new Set(identities(core).map((i) => i.email.toLowerCase()));
      const answered = h.reply_to.length > 0 ? h.reply_to : h.from;
      draft.to = answered.filter((a) => !own.has(a.email.toLowerCase()));
      if (context.mode === "reply-all") {
        const seen = new Set(draft.to.map((a) => a.email.toLowerCase()));
        draft.cc = [...h.to, ...h.cc].filter((a) => {
          const key = a.email.toLowerCase();
          if (seen.has(key) || own.has(key)) return false;
          seen.add(key);
          return true;
        });
      }
      const quoted = original.text
        .split("\n")
        .map((l) => (l.startsWith(">") ? `>${l}` : `> ${l}`))
        .join("\n");
      body =
        m === "html"
          ? `<p>${escapeHtml(header)}</p><blockquote type="cite">${original.sanitized_html ?? textToHtml(original.text)}</blockquote><p></p>${sig}`
          : `${header}\n${quoted}${sig === "" ? "\n\n" : `\n\n\n\n${sig}`}`;
    }
  }
  if (m === "html") draft.html = body;
  else if (m === "markdown") draft.markdown = body;
  else draft.text = body;
  return { type: "Draft", draft };
}

// ---- Attachments and addresses ----

function base64Size(data: string): number {
  const clean = data.replace(/\s/g, "");
  const padding = clean.endsWith("==") ? 2 : clean.endsWith("=") ? 1 : 0;
  return Math.floor((clean.length * 3) / 4) - padding;
}

function upload(
  core: MockCore,
  filename: string,
  mime: string,
  data: string,
  contentId: string | null,
): Response {
  if (!/^[A-Za-z0-9+/=\s]*$/.test(data)) {
    return errorResponse("invalid_request", "attachment data is not base64");
  }
  const size = base64Size(data);
  if (size > MOCK_MAX_ATTACHMENT) return errorResponse("too_large", `${filename}: ${size} bytes`);
  const blob = core.nextId("blob-");
  core.data.features.composer.blobs.set(blob, { filename, mime, data });
  return {
    type: "AttachmentUploaded",
    attachment: { filename, mime, size, blob, content_id: contentId },
  };
}

/** Collected addresses: correspondents of the mock mail plus extra ones. */
function collected(core: MockCore): Address[] {
  const counts = new Map<string, { address: Address; count: number }>();
  const add = (a: Address) => {
    const key = a.email.toLowerCase();
    const entry = counts.get(key);
    if (entry) {
      entry.count++;
      if (!entry.address.name && a.name) entry.address = a;
    } else counts.set(key, { address: a, count: 1 });
  };
  for (const m of core.data.messages) {
    if (m.from) add(m.from);
    m.to.forEach(add);
  }
  core.data.features.composer.addresses.forEach(add);
  const own = new Set(identities(core).map((i) => i.email.toLowerCase()));
  return [...counts.values()]
    .filter((e) => !own.has(e.address.email.toLowerCase()))
    .sort((a, b) => b.count - a.count || a.address.email.localeCompare(b.address.email))
    .map((e) => e.address);
}

/** `SuggestAddresses`: prefix of the address or of a word of the name. */
export function suggest(core: MockCore, prefix: string, limit: number): Address[] {
  const p = prefix.trim().toLowerCase();
  if (p === "") return [];
  return collected(core)
    .filter(
      (a) =>
        a.email.toLowerCase().startsWith(p) ||
        (a.name ?? "")
          .toLowerCase()
          .split(/\s+/)
          .some((w) => w.startsWith(p)) ||
        (a.name ?? "").toLowerCase().startsWith(p),
    )
    .slice(0, limit);
}

// ---- Rich drafts (until the shared mock stores them) ----

/**
 * `SaveDraft` for rich drafts. The shared mock (`api/mockOutbox.ts`) still
 * refuses HTML / Markdown / attachments as M1a did; WP25 routes rich drafts
 * here. Tests install it with [`acceptRichDrafts`].
 */
export function saveRichDraft(core: MockCore, draft: Draft): Response {
  if (!identities(core).some((i) => i.id === draft.identity_id)) {
    return errorResponse("invalid_request", `unknown identity ${draft.identity_id}`);
  }
  if (draft.id !== null && !core.data.drafts.has(draft.id)) {
    return errorResponse("not_found", `draft ${draft.id} not found`);
  }
  for (const a of draft.attachments) {
    if (!core.data.features.composer.blobs.has(a.blob)) {
      return errorResponse("invalid_request", `unknown attachment blob ${a.blob}`);
    }
  }
  const id = draft.id ?? core.nextId("draft-");
  core.data.drafts.set(id, structuredClone({ ...draft, id }));
  return { type: "DraftSaved", draft_id: id };
}

/** Makes a `MockTransport` store rich drafts (see [`saveRichDraft`]). */
export function acceptRichDrafts(transport: MockTransport): void {
  const send = transport.send.bind(transport);
  transport.send = async (request: Request): Promise<Response> => {
    if (
      request.type === "SaveDraft" &&
      (request.draft.html !== null ||
        request.draft.markdown !== null ||
        request.draft.attachments.length > 0)
    ) {
      transport.requests.push(request);
      return saveRichDraft(transport, request.draft);
    }
    return send(request);
  };
}
