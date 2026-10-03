/**
 * M5a WP153 fixes in the mail panes: list rows with avatar, unread dot and
 * tag line (hidden in Compact), attachment cards, and the reader column.
 */
import { vi } from "vitest";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { MessageSummary, ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { createEnv, renderNode, settleQueries, stubElementSizes, widget } from "../test/harness";
import { colorSlot } from "../theme/skin";
import { fileType, formatSize } from "./AttachmentCard";
import { extrasOf, messageTags } from "./MessageList";

const INBOX = "acc-personal/Inbox";
const FIRST = `${INBOX}#1`;

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

function listEnv() {
  return createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
}

function row(container: HTMLElement, id: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(
    `[data-part="message-list.row"][data-message="${id}"]`,
  );
  if (!el) throw new Error(`no row ${id}`);
  return el;
}

describe("message list rows (M5a)", () => {
  it("shows an avatar with initials and a stable colour, and an unread dot only on unread rows", async () => {
    const env = listEnv();
    const { container } = renderNode(widget("message-list"), env);
    await screen.findAllByRole("option");
    const first = env.transport.data.messages.find((m) => m.id === FIRST) as MessageSummary;
    const unread = row(container, FIRST);
    const avatar = unread.querySelector('[data-part="message-list.row.avatar"]');
    expect(avatar).toHaveAttribute("aria-hidden", "true");
    expect(avatar).toHaveAttribute("data-color", String(colorSlot(first.from?.email ?? "")));
    expect(avatar?.textContent).toMatch(/^\p{Lu}{1,2}$/u);
    expect(unread.querySelector('[data-part="message-list.row.unread-dot"]')).not.toBeNull();
    expect(unread).toHaveAttribute("data-view", "rows");
    const read = row(container, `${INBOX}#2`);
    expect(read.querySelector('[data-part="message-list.row.unread-dot"]')).toBeNull();
    // The template lines sit in the row body next to the avatar.
    expect(
      unread.querySelector(
        '[data-part="message-list.row.body"] [data-part="message-list.row.line"]',
      ),
    ).not.toBeNull();
  });

  it("puts account, attachments, flag and labels on the tag line", async () => {
    const env = listEnv();
    const { container } = renderNode(widget("message-list"), env);
    await waitFor(() =>
      expect(
        row(container, FIRST).querySelector('[data-part="tag"][data-kind="account"]'),
      ).not.toBeNull(),
    );
    const line = row(container, FIRST).querySelector(
      '[data-part="message-list.row.tag-line"]',
    ) as HTMLElement;
    const kinds = [...line.querySelectorAll('[data-part="tag"]')].map((t) =>
      t.getAttribute("data-kind"),
    );
    expect(kinds).toEqual(["account", "attachments", "flagged", "label"]);
    const account = within(line).getByText("Personal").closest('[data-part="tag"]');
    // M5b: the account colour follows the account order, like the folder
    // pane's sections (Personal is the first account: slot 1, data-color 0).
    expect(account?.querySelector('[data-part="tag.dot"]')).toHaveAttribute("data-color", "0");
    expect(within(line).getByText("Attachment")).toBeInTheDocument();
    expect(within(line).getByText("Flagged")).toBeInTheDocument();
    expect(within(line).getByText("work")).toBeInTheDocument();
    // A plain read message without attachments still names its account.
    const plain = row(container, `${INBOX}#2`).querySelector(
      '[data-part="message-list.row.tag-line"]',
    );
    expect(plain?.querySelectorAll('[data-part="tag"]')).toHaveLength(1);
  });

  it("shows the attachment count, invitation and warning when the core sends them", () => {
    const base: MessageSummary = {
      id: "m",
      folder_id: "f",
      account_id: "a",
      subject: "",
      from: null,
      to: [],
      date: "2026-10-03T00:00:00Z",
      flags: { seen: true, flagged: false, answered: false, draft: false },
      has_attachments: false,
      preview: "",
      thread_id: null,
      tags: [],
      duplicates: [],
    };
    expect(messageTags(base)).toMatchObject({ attachments: false, attachmentCount: null });
    const extra = { ...base, attachment_count: 3, invitation: true, warning: true };
    expect(messageTags(extra)).toMatchObject({
      attachments: true,
      attachmentCount: 3,
      invitation: true,
      warning: true,
    });
    expect(extrasOf({ attachment_count: "3", invitation: "yes" })).toEqual({});
  });

  it("hides the tag line in Compact (theme density)", async () => {
    const env = listEnv();
    const compact: ResolvedTheme = { ...defaultTheme(), density: "compact" };
    const { container } = renderNode(widget("message-list"), env, { theme: compact });
    await waitFor(() =>
      expect(
        row(container, FIRST).querySelector('[data-part="message-list.row.tag-line"]'),
      ).not.toBeNull(),
    );
    expect(document.documentElement.dataset.nailDensity).toBe("compact");
    const line = row(container, FIRST).querySelector(
      '[data-part="message-list.row.tag-line"]',
    ) as HTMLElement;
    expect(getComputedStyle(line).display).toBe("none");
  });

  it("keeps the tag line in Comfortable", async () => {
    const env = listEnv();
    const { container } = renderNode(widget("message-list"), env);
    await waitFor(() =>
      expect(
        row(container, FIRST).querySelector('[data-part="message-list.row.tag-line"]'),
      ).not.toBeNull(),
    );
    const line = row(container, FIRST).querySelector(
      '[data-part="message-list.row.tag-line"]',
    ) as HTMLElement;
    expect(getComputedStyle(line).display).toBe("flex");
  });

  it("uses plain rows without avatar in the column view", async () => {
    const env = listEnv();
    const { container } = renderNode(
      widget("message-list", { options: { columns: "from subject date" } }),
      env,
    );
    await screen.findAllByRole("option");
    const r = row(container, FIRST);
    expect(r).toHaveAttribute("data-view", "columns");
    expect(r.querySelector('[data-part="message-list.row.avatar"]')).toBeNull();
  });
});

describe("attachment cards (M5a)", () => {
  it("derive the type badge and its colour group", () => {
    expect(fileType("INV-2026-09-5521.pdf", "application/pdf")).toEqual({
      label: "PDF",
      kind: "pdf",
    });
    expect(fileType("photo.JPEG", "image/jpeg")).toEqual({ label: "JPEG", kind: "image" });
    expect(fileType("sheet.xlsx", "application/octet-stream")).toEqual({
      label: "XLSX",
      kind: "spreadsheet",
    });
    expect(fileType(null, "image/png")).toEqual({ label: "PNG", kind: "image" });
    expect(fileType("invite", "text/calendar")).toEqual({ label: "CALE", kind: "calendar" });
    expect(fileType("data.bin", "application/octet-stream")).toEqual({
      label: "BIN",
      kind: "other",
    });
    expect(formatSize(48_213, "en")).toBe("47 kB");
    expect(formatSize(10, "en")).toBe("1 kB");
    expect(formatSize(5 * 1024 * 1024, "en")).toBe("5 MB");
  });

  it("show type, name and size apart; in a browser Save downloads the file", async () => {
    const env = listEnv();
    env.store.getState().selectMessage(FIRST);
    const createObjectURL = vi.fn(() => "blob:mock");
    const revokeObjectURL = vi.fn();
    // jsdom has no object URLs.
    const original = { create: URL.createObjectURL, revoke: URL.revokeObjectURL };
    URL.createObjectURL = createObjectURL;
    URL.revokeObjectURL = revokeObjectURL;
    const clicked: HTMLAnchorElement[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push(this);
    });
    try {
      renderNode(widget("message-view"), env);
      const list = await screen.findByRole("list", { name: "Attachments" });
      const card = within(list).getByRole("listitem");
      expect(card).toHaveAttribute("data-part", "attachment-card");
      expect(within(card).getByText("PDF")).toHaveAttribute("data-kind", "pdf");
      expect(within(card).getByText("report.pdf")).toHaveAttribute(
        "data-part",
        "attachment-card.name",
      );
      expect(within(card).getByText("47 kB")).toHaveAttribute("data-part", "attachment-card.size");
      // No plug-in renders PDFs in the default mock: no preview action.
      expect(within(card).queryByRole("button", { name: "Preview report.pdf" })).toBeNull();
      fireEvent.click(within(card).getByRole("button", { name: "Save report.pdf" }));
      const status = await within(card).findByRole("status");
      expect(status).toHaveTextContent("Downloaded.");
      // In the text column next to the size, so the card keeps its height.
      expect(status.parentElement).toHaveAttribute("data-part", "attachment-card.text");
      expect(status.previousElementSibling).toHaveAttribute("data-part", "attachment-card.size");
      expect(env.transport.data.features.attachments.downloaded).toEqual([`${FIRST}#0`]);
      expect(createObjectURL).toHaveBeenCalledTimes(1);
      expect(clicked.map((a) => a.download)).toEqual(["report.pdf"]);
      await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:mock"));
    } finally {
      click.mockRestore();
      URL.createObjectURL = original.create;
      URL.revokeObjectURL = original.revoke;
    }
    await settleQueries(env);
  });

  it("in the desktop app Save asks where and the core writes the file", async () => {
    const env = listEnv();
    env.store.getState().selectMessage(FIRST);
    const portability = env.transport.data.features.portability;
    portability.desktop = true;
    portability.pickAnswers = [null, "C:\\Users\\Ada\\report.pdf"];
    renderNode(widget("message-view"), env);
    const save = await screen.findByRole("button", { name: "Save report.pdf" });
    // Cancelled in the Save dialog: nothing happens.
    await act(async () => {
      fireEvent.click(save);
    });
    await waitFor(() => expect(save).toBeEnabled());
    expect(screen.queryByRole("status")).toBeNull();
    expect(env.transport.data.features.attachments.saved).toEqual([]);
    fireEvent.click(save);
    expect(await screen.findByRole("status")).toHaveTextContent("Saved.");
    expect(env.transport.data.features.attachments.saved).toEqual([
      `${FIRST}#0 → C:\\Users\\Ada\\report.pdf`,
    ]);
    await settleQueries(env);
  });

  it("say so when the core cannot save attachments", async () => {
    const env = listEnv();
    env.store.getState().selectMessage(FIRST);
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) =>
      request.type === "GetMessageAttachment"
        ? { type: "Error", code: "unsupported", message: "no" }
        : send(request);
    renderNode(widget("message-view"), env);
    const save = await screen.findByRole("button", { name: "Save report.pdf" });
    await act(async () => {
      fireEvent.click(save);
    });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Saving attachments is not available in this version of Nail yet.",
    );
    expect(save).toBeDisabled();
    await settleQueries(env);
  });

  it("report a failed save", async () => {
    const env = listEnv();
    env.store.getState().selectMessage(FIRST);
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) =>
      request.type === "GetMessageAttachment"
        ? { type: "Error", code: "not_found", message: "gone" }
        : send(request);
    renderNode(widget("message-view"), env);
    fireEvent.click(await screen.findByRole("button", { name: "Save report.pdf" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "The attachment could not be saved.",
    );
    await settleQueries(env);
  });
});

describe("reader column (M5a)", () => {
  it("puts header, attachments and body into one centred column", async () => {
    const env = listEnv();
    // #2 has a plain-text body.
    env.store.getState().selectMessage(`${INBOX}#2`);
    const { container } = renderNode(widget("message-view"), env);
    const text = await waitFor(() => {
      const el = container.querySelector('[data-part="message-view.text"]');
      if (!el) throw new Error("no text yet");
      return el as HTMLElement;
    });
    const column = container.querySelector('[data-part="message-view.column"]');
    expect(column).not.toBeNull();
    expect(column?.contains(container.querySelector('[data-part="message-view.header"]'))).toBe(
      true,
    );
    expect(column?.contains(text)).toBe(true);
    // Plain text keeps its line breaks.
    expect(text.tagName).toBe("PRE");
    expect(getComputedStyle(text).whiteSpace).toBe("pre-wrap");
    await settleQueries(env);
  });
});
