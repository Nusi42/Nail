/** Links in mail (ADR 0206): the status overlay, following a link, the confirmation. */
import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageWarning } from "../api/generated";
import { mailActionsStoreFor } from "../features/mail-actions/store";
import { actionContext, recordRequests } from "../test/env";
import { createEnv, renderAsDialog, renderWithProviders } from "../test/harness";
import { must } from "../test/must";
import { LINK_STATUS_CHARS, shortenMiddle } from "./mailLinks";
import { scopedUiStore } from "./ThreadViewScope";
import {
  followMailLink,
  MAIL_OPEN_LINK_DIALOG,
  MailLinkStatus,
  OpenLinkDialog,
  pendingMailLink,
} from "./ReaderLinks";

const TRACKING = "https://track.example.net/e/abc/click";

const MISMATCH: MessageWarning = {
  kind: "link-mismatch",
  link: 1,
  href: "https://login.example.net/",
  shown_host: "bank.example.com",
  target_host: "login.example.net",
};

let open: ReturnType<typeof vi.fn>;
beforeEach(() => {
  open = vi.fn(() => null);
  vi.stubGlobal("open", open);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the link status text", () => {
  it("shortens long targets in the middle and keeps short ones", () => {
    expect(shortenMiddle(TRACKING)).toBe(TRACKING);
    const long = `https://example.com/${"a".repeat(100)}/end`;
    const short = shortenMiddle(long);
    expect(Array.from(short)).toHaveLength(LINK_STATUS_CHARS);
    expect(short.startsWith("https://example.com/aaa")).toBe(true);
    expect(short.endsWith("aaa/end")).toBe(true);
    expect(short).toContain("…");
    expect(shortenMiddle("abcdefghij", 5)).toBe("ab…ij");
    // Characters, not UTF-16 units: an emoji is never cut in half.
    expect(shortenMiddle("😀😀😀😀😀😀", 3)).toBe("😀…😀");
  });
});

describe("the link status overlay", () => {
  it("shows the target, the full value in its accessible text, and no warning for a clean link", () => {
    const long = `https://example.com/${"x".repeat(120)}`;
    const { container } = renderWithProviders(
      <MailLinkStatus link={{ href: long, index: 0 }} warnings={[MISMATCH]} />,
      createEnv(),
    );
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("data-part", "message-view.link-status");
    expect(status).not.toHaveAttribute("data-state");
    expect(container.querySelector('[data-part="message-view.link-status.url"]')).toHaveTextContent(
      shortenMiddle(long),
    );
    expect(status).toHaveTextContent(`Link to ${long}`);
    expect(container.querySelector('[data-part="message-view.link-status.warning"]')).toBeNull();
    // Mounted in a dock of its own: it never takes room in the reader.
    expect(container.firstElementChild).toHaveAttribute("data-part", "message-view.link-dock");
  });

  it("shows the core's warning of a flagged link", () => {
    renderWithProviders(
      <MailLinkStatus link={{ href: MISMATCH.href, index: 1 }} warnings={[MISMATCH]} />,
      createEnv(),
    );
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("data-state", "warning");
    expect(status).toHaveTextContent(
      "A link shows bank.example.com but goes to login.example.net.",
    );
  });
});

describe("following a link", () => {
  it("opens a clean link in a new tab without opener or referrer (web mode)", async () => {
    const env = createEnv();
    const requests = recordRequests(env);
    await followMailLink(actionContext(env), { href: TRACKING, index: 0 }, [MISMATCH]);
    expect(requests).toContainEqual({ type: "OpenLink", url: TRACKING });
    expect(open).toHaveBeenCalledWith(TRACKING, "_blank", "noopener,noreferrer");
  });

  it("opens Nail's composer for a mailto: link", async () => {
    const env = createEnv();
    await followMailLink(actionContext(env), { href: "mailto:ada@example.org", index: 0 }, []);
    const composers = Object.values(env.store.getState().composers);
    expect(composers.map((c) => c.to)).toContainEqual("ada@example.org");
    expect(open).not.toHaveBeenCalled();
  });

  it("shows the core's refusal in the toast", async () => {
    const env = createEnv();
    await followMailLink(actionContext(env), { href: "javascript:alert(1)", index: 0 }, []);
    expect(mailActionsStoreFor(env.store).getState().toast?.kind).toBe("error");
    expect(open).not.toHaveBeenCalled();
  });

  it("asks first for a flagged link: Cancel opens nothing, Open anyway opens it", async () => {
    const env = createEnv();
    const requests = recordRequests(env);
    const link = { href: MISMATCH.href, index: 1 };
    await followMailLink(actionContext(env), link, [MISMATCH]);
    expect(env.store.getState().windows).toContain(MAIL_OPEN_LINK_DIALOG);
    expect(requests.filter((r) => r.type === "OpenLink")).toEqual([]);
    expect(pendingMailLink(env.store)?.link).toEqual(link);

    const { unmount } = renderAsDialog(env, OpenLinkDialog, MAIL_OPEN_LINK_DIALOG);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("This link may not go where it seems to.");
    expect(dialog).toHaveTextContent(
      "A link shows bank.example.com but goes to login.example.net.",
    );
    expect(dialog).toHaveTextContent(`Real target: ${MISMATCH.href}`);
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await act(async () => {
      await Promise.resolve();
    });
    expect(cancel).toHaveFocus();
    await userEvent.click(cancel);
    expect(pendingMailLink(env.store)).toBeNull();
    expect(open).not.toHaveBeenCalled();
    unmount();

    await followMailLink(actionContext(env), link, [MISMATCH]);
    renderAsDialog(env, OpenLinkDialog, MAIL_OPEN_LINK_DIALOG);
    await userEvent.click(screen.getByRole("button", { name: "Open anyway" }));
    await vi.waitFor(() => {
      expect(open).toHaveBeenCalledWith(MISMATCH.href, "_blank", "noopener,noreferrer");
    });
    expect(must(requests.find((r) => r.type === "OpenLink"))).toEqual({
      type: "OpenLink",
      url: MISMATCH.href,
    });
  });

  it("asks in the app's window for a link clicked in a card of the feed", async () => {
    const env = createEnv();
    const card = scopedUiStore(env.store, "card-message");
    const link = { href: MISMATCH.href, index: 1 };
    await followMailLink({ ...actionContext(env), store: card }, link, [MISMATCH]);
    expect(env.store.getState().windows).toContain(MAIL_OPEN_LINK_DIALOG);
    expect(pendingMailLink(env.store)?.link).toEqual(link);
  });
});
