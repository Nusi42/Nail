import { act, screen, within } from "@testing-library/react";
import type { ErrorDetails } from "../api/generated";
import { must } from "../test/must";
import { createEnv, renderNode, widget } from "../test/harness";
import { sendPercent } from "../state/uiStore";

/** A status bar that shows only the sync progress. */
function syncStatus() {
  const env = createEnv();
  renderNode(widget("status-bar", { options: { items: "sync-progress" } }), env);
  return env;
}

describe("status-bar sync status", () => {
  it("shows the core's localized reason for a failed synchronization", () => {
    const env = syncStatus();
    act(() => {
      env.store.getState().setSyncError({
        accountId: "work",
        code: "network",
        localized: { id: "sync-error-folders", args: { count: 2, folders: "Lists, Receipts" } },
      });
    });
    expect(screen.getByRole("status", { name: "Status" })).toHaveTextContent(
      "Synchronization failed: 2 folders could not be synchronized: Lists, Receipts",
    );
  });

  it("falls back to the text of the error code", () => {
    const env = syncStatus();
    act(() => {
      env.store.getState().setSyncError({ accountId: "work", code: "auth_failed" });
    });
    expect(screen.getByRole("status", { name: "Status" })).toHaveTextContent(
      /^Synchronization failed: /,
    );
    expect(screen.getByRole("status", { name: "Status" })).not.toHaveTextContent("auth_failed");
  });
});

describe("send progress", () => {
  const sending = {
    id: "out-1",
    draft_id: "d",
    account_id: "acc-personal",
    identity_id: "ada@example.org",
    subject: "Holiday video",
    recipients: [],
    send_at: new Date().toISOString(),
    state: "sending" as const,
    attempts: 1,
    error: null,
    message: null,
  };

  it("shows the upload progress of a large message in the status bar", () => {
    const env = createEnv();
    renderNode(widget("status-bar", { options: { items: "send-status" } }), env);
    act(() => {
      env.store.getState().setLastSend(sending);
    });
    const status = screen.getByRole("status", { name: "Status" });
    expect(status).toHaveTextContent("Sending…");
    act(() => {
      env.store.getState().setSendProgress("out-1", { sentBytes: 3, totalBytes: 8 });
    });
    expect(status).toHaveTextContent("Sending… 37% uploaded");
    act(() => {
      env.store.getState().setSendProgress("out-1", null);
    });
    expect(status).not.toHaveTextContent("uploaded");
  });

  it("shows the upload progress on the outbox entry", async () => {
    const env = createEnv();
    env.transport.data.outbox.push({ ...sending });
    env.store.getState().setSendProgress("out-1", { sentBytes: 5, totalBytes: 10 });
    renderNode(widget("outbox"), env);
    const bar = await screen.findByRole("progressbar", { name: "50% uploaded" });
    expect(bar).toHaveAttribute("data-part", "outbox.item.progress");
    expect(bar).toHaveAttribute("value", "50");
  });
});

describe("sendPercent", () => {
  it("rounds down and stays within 0–100", () => {
    expect(sendPercent({ sentBytes: 0, totalBytes: 0 })).toBe(0);
    expect(sendPercent({ sentBytes: 999, totalBytes: 1000 })).toBe(99);
    expect(sendPercent({ sentBytes: 12, totalBytes: 10 })).toBe(100);
  });
});

describe("Show details (ADR 0190)", () => {
  const details: ErrorDetails = {
    code: "auth_failed",
    protocol: "smtp",
    step: "login",
    command: "AUTH PLAIN <redacted>",
    server_text: "535 5.7.8 Authentication credentials invalid",
    detail: "authentication failed",
    at: "2026-10-08T09:12:01Z",
  };

  it("offers the details of an account that needs attention", async () => {
    const env = createEnv();
    env.transport.data.statuses[1] = {
      ...must(env.transport.data.statuses[1]),
      state: "auth-error",
      error: "auth_failed",
      details,
    };
    renderNode(widget("status-bar", { options: { items: "account-status" } }), env);
    const status = screen.getByRole("status", { name: "Status" });
    await within(status).findByText("Work: Sign-in required");
    act(() => {
      within(status).getByRole("button", { name: "Show details" }).click();
    });
    expect(env.store.getState().windows).toEqual(["error-details:w1"]);
  });

  it("offers the details of a failed send", () => {
    const env = createEnv();
    renderNode(widget("status-bar", { options: { items: "send-status" } }), env);
    act(() => {
      env.store.getState().setLastSend({
        id: "out-2",
        draft_id: "d",
        account_id: "acc-personal",
        identity_id: "ada@example.org",
        subject: "Hi",
        recipients: [],
        send_at: new Date().toISOString(),
        state: "failed",
        attempts: 1,
        error: "auth_failed",
        message: null,
      });
      env.store.getState().setLastSendDetails(details);
    });
    const status = screen.getByRole("status", { name: "Status" });
    expect(within(status).getByRole("button", { name: "Show details" })).toBeVisible();
    // Another message's state forgets the details.
    act(() => {
      env.store.getState().setLastSend(null);
    });
    expect(env.store.getState().lastSendDetails).toBeNull();
  });
});
