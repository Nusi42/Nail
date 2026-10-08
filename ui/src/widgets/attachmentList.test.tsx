/**
 * The reader's attachment list leaves out the images the body shows
 * (`AttachmentInfo.shown_in_body`, ADR 0162) behind a disclosure button.
 */
import { act, fireEvent, screen, within } from "@testing-library/react";
import type { AttachmentInfo } from "../api/generated";
import { violations } from "../test/axe";
import {
  createEnv,
  renderNode,
  settleQueries,
  widget,
  stubElementSizesEachTest,
} from "../test/harness";
import { must } from "../test/must";
import { listedAttachments } from "./AttachmentList";
import { FIRST, INBOX } from "../test/mockIds";

const SECOND = `${INBOX}#2`;

const LOGO: AttachmentInfo = {
  index: 0,
  filename: null,
  mime: "image/png",
  size: 70,
  inline: true,
  shown_in_body: true,
};
const REPORT: AttachmentInfo = {
  index: 1,
  filename: "report.pdf",
  mime: "application/pdf",
  size: 48_213,
  inline: false,
  shown_in_body: false,
};

stubElementSizesEachTest();

/** An env whose messages `id` have `attachments`; `FIRST` is selected. */
function envWith(bodies: Readonly<Record<string, AttachmentInfo[]>>) {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  for (const [id, attachments] of Object.entries(bodies)) {
    must(env.transport.data.bodies.get(id)).attachments = attachments;
  }
  env.store.getState().selectMessage(FIRST);
  return env;
}

const cardNames = (list: HTMLElement) =>
  within(list)
    .getAllByRole("listitem")
    .map((card) => card.querySelector('[data-part="attachment-card.name"]')?.textContent);

describe("attachment list (ADR 0162)", () => {
  it("lists all attachments, or those the body does not show", () => {
    const message = { attachments: [LOGO, REPORT] };
    expect(listedAttachments(message, false)).toEqual([REPORT]);
    expect(listedAttachments(message, true)).toEqual([LOGO, REPORT]);
  });

  it("hides the images the body shows until the user asks for them", async () => {
    const env = envWith({ [FIRST]: [LOGO, REPORT] });
    const { container } = renderNode(widget("message-view"), env);
    const list = await screen.findByRole("list", { name: "Attachments" });
    expect(cardNames(list)).toEqual(["report.pdf"]);
    expect(screen.queryByText("Unnamed attachment")).toBeNull();
    const toggle = screen.getByRole("button", { name: "Show all attachments (2)" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAttribute("data-part", "message-view.attachments.toggle");
    expect(toggle.parentElement).toHaveAttribute("data-part", "message-view.attachment-list");
    expect(await violations(container)).toEqual([]);

    fireEvent.click(toggle);
    const hide = screen.getByRole("button", { name: "Hide images shown in the message" });
    expect(hide).toBe(toggle);
    expect(hide).toHaveAttribute("aria-expanded", "true");
    expect(cardNames(list)).toEqual(["Unnamed attachment", "report.pdf"]);
    // Still downloadable, and marked for the theme.
    const logo = must(screen.getByText("Unnamed attachment").closest("li"));
    expect(logo).toHaveAttribute("data-embedded", "");
    expect(within(logo).getByRole("button", { name: "Save Unnamed attachment" })).toBeEnabled();
    expect(await violations(container)).toEqual([]);

    fireEvent.click(hide);
    expect(cardNames(list)).toEqual(["report.pdf"]);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await settleQueries(env);
  });

  it("shows only the button when the body shows every attachment; another message starts collapsed", async () => {
    const env = envWith({ [FIRST]: [LOGO], [SECOND]: [LOGO] });
    const { container } = renderNode(widget("message-view"), env);
    const toggle = await screen.findByRole("button", { name: "Show all attachments (1)" });
    expect(screen.queryByRole("list", { name: "Attachments" })).toBeNull();
    expect(await violations(container)).toEqual([]);

    fireEvent.click(toggle);
    const list = screen.getByRole("list", { name: "Attachments" });
    expect(cardNames(list)).toEqual(["Unnamed attachment"]);

    act(() => {
      env.store.getState().selectMessage(SECOND);
    });
    expect(
      await screen.findByRole("button", { name: "Show all attachments (1)", expanded: false }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Attachments" })).toBeNull();
    await settleQueries(env);
  });

  it("has no button when the body shows no attachment", async () => {
    const env = envWith({ [FIRST]: [REPORT] });
    renderNode(widget("message-view"), env);
    const list = await screen.findByRole("list", { name: "Attachments" });
    expect(cardNames(list)).toEqual(["report.pdf"]);
    expect(screen.queryByRole("button", { name: /Show all attachments/ })).toBeNull();
    await settleQueries(env);
  });
});
