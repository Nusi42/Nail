/**
 * Opening attachments (ADR 0211): a click on the card's name or badge or its
 * Open button asks the core; in a browser the file downloads instead; a
 * flagged attachment asks first. Plain-text links go the way of HTML ones.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { MessageWarning } from "../api/generated";
import { recordRequests } from "../test/env";
import { createEnv, renderAsDialog, renderWithProviders, settleQueries } from "../test/harness";
import { FIRST, INBOX } from "../test/mockIds";
import { must } from "../test/must";
import { stubProperties } from "../test/stub";
import { AttachmentCard } from "./AttachmentCard";
import {
  MAIL_OPEN_ATTACHMENT_DIALOG,
  OpenAttachmentDialog,
  pendingAttachment,
} from "./OpenAttachment";

const FLAGGED: MessageWarning = {
  kind: "dangerous-attachment",
  index: 0,
  filename: "report.pdf.exe",
  risk: "executable",
};

function setup(desktop: boolean) {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  env.transport.data.features.portability.desktop = desktop;
  const attachment = must(env.transport.data.bodies.get(FIRST)?.attachments[0]);
  const rendered = (warnings: readonly MessageWarning[] = []) =>
    renderWithProviders(
      <ul>
        <AttachmentCard
          messageId={FIRST}
          attachment={attachment}
          accountId="acc-personal"
          warnings={warnings}
          previewId="preview"
        />
      </ul>,
      env,
    );
  return { env, attachment, rendered };
}

describe("opening an attachment from its card", () => {
  it("in the desktop app a click on the name or the badge, and the Open button, ask the core", async () => {
    const { env, rendered } = setup(true);
    const requests = recordRequests(env);
    rendered();
    const card = screen.getByRole("listitem");
    const name = within(card).getByText("report.pdf");
    expect(name).toHaveAttribute("data-part", "attachment-card.name");
    expect(name).toHaveAttribute("data-action", "open");
    expect(name.tagName).toBe("BUTTON");
    fireEvent.click(name);
    await waitFor(() => {
      expect(within(card).getByRole("status")).toHaveTextContent("Opened.");
    });
    expect(requests.filter((r) => r.type === "OpenAttachment")).toEqual([
      { type: "OpenAttachment", message_id: FIRST, index: 0 },
    ]);
    expect(env.transport.data.features.attachments.opened).toEqual([`${FIRST}#0`]);
    expect(card).toHaveAttribute("data-state", "opened");

    fireEvent.click(within(card).getByText("PDF"));
    await waitFor(() => {
      expect(env.transport.data.features.attachments.opened).toHaveLength(2);
    });
    await waitFor(() => {
      expect(within(card).getByRole("status")).toHaveTextContent("Opened.");
    });
    fireEvent.click(within(card).getByRole("button", { name: "Open report.pdf" }));
    await waitFor(() => {
      expect(env.transport.data.features.attachments.opened).toHaveLength(3);
    });
    // The badge is a pointer shortcut only: the name and Open stay in the tab order.
    expect(within(card).getByText("PDF")).toHaveAttribute("tabindex", "-1");
    expect(within(card).getByText("PDF")).toHaveAttribute("aria-hidden", "true");
    // Nothing was saved or downloaded.
    expect(env.transport.data.features.attachments.saved).toEqual([]);
    expect(env.transport.data.features.attachments.downloaded).toEqual([]);
    await settleQueries(env);
  });

  it("in a browser the file downloads instead", async () => {
    const { env, rendered } = setup(false);
    const restoreUrl = stubProperties(URL, {
      createObjectURL: vi.fn(() => "blob:mock"),
      revokeObjectURL: vi.fn(),
    });
    const clicked: HTMLAnchorElement[] = [];
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      clicked.push(this);
    });
    try {
      rendered();
      fireEvent.click(screen.getByRole("button", { name: "Open report.pdf" }));
      await waitFor(() => {
        expect(screen.getByRole("status")).toHaveTextContent("Downloaded.");
      });
      expect(env.transport.data.features.attachments.opened).toEqual([]);
      expect(env.transport.data.features.attachments.downloaded).toEqual([`${FIRST}#0`]);
      expect(clicked.map((a) => a.download)).toEqual(["report.pdf"]);
    } finally {
      click.mockRestore();
      restoreUrl();
    }
    await settleQueries(env);
  });

  it("says so when the core fails to open it", async () => {
    const { env, rendered } = setup(true);
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) =>
      request.type === "OpenAttachment"
        ? { type: "Error", code: "internal", message: "no handler" }
        : send(request);
    rendered();
    fireEvent.click(screen.getByRole("button", { name: "Open report.pdf" }));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("The attachment could not be opened.");
    });
    expect(screen.getByRole("listitem")).toHaveAttribute("data-state", "open-failed");
    await settleQueries(env);
  });

  it("asks first for a flagged attachment: Cancel opens nothing, Open anyway opens it", async () => {
    const { env, rendered } = setup(true);
    rendered([FLAGGED]);
    const card = screen.getByRole("listitem");
    fireEvent.click(within(card).getByText("report.pdf"));
    expect(env.store.getState().windows).toContain(MAIL_OPEN_ATTACHMENT_DIALOG);
    expect(pendingAttachment(env.store)).toMatchObject({
      name: "report.pdf.exe",
      risk: "executable",
    });
    expect(env.transport.data.features.attachments.opened).toEqual([]);

    const first = renderAsDialog(env, OpenAttachmentDialog, MAIL_OPEN_ATTACHMENT_DIALOG);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("Opening this attachment may harm your computer.");
    expect(dialog).toHaveTextContent("“report.pdf.exe” is a program.");
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    await act(async () => {
      await Promise.resolve();
    });
    expect(cancel).toHaveFocus();
    await userEvent.click(cancel);
    expect(pendingAttachment(env.store)).toBeNull();
    expect(env.transport.data.features.attachments.opened).toEqual([]);
    first.unmount();

    fireEvent.click(within(card).getByRole("button", { name: "Open report.pdf" }));
    renderAsDialog(env, OpenAttachmentDialog, MAIL_OPEN_ATTACHMENT_DIALOG);
    await userEvent.click(screen.getByRole("button", { name: "Open anyway" }));
    await waitFor(() => {
      expect(env.transport.data.features.attachments.opened).toEqual([`${FIRST}#0`]);
    });
    await waitFor(() => {
      expect(card).toHaveAttribute("data-state", "opened");
    });
    await settleQueries(env);
  });
});
