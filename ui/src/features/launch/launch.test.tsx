import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { IMPORT_DIALOG, pimStoreFor, SOURCES_DIALOG } from "../pim";
import { remoteStore } from "../remote";
import { LaunchRequests } from "./LaunchRequests";
import { systemStoreFor } from "../system";

afterEach(() => {
  // Unmount before resetting the remote feature's module-level store.
  cleanup();
  remoteStore.getState().clear();
});

/** Renders the launch handler and waits until it took the queued launches. */
async function launchesTaken(env: Env): Promise<void> {
  renderWithProviders(<LaunchRequests />, env);
  await waitFor(() => {
    expect(env.transport.requests.some((r) => r.type === "TakeLaunchRequests")).toBe(true);
  });
}

describe("launch requests", () => {
  it("opens the composer for a mailto: link taken at start-up", async () => {
    const env = createEnv();
    env.transport.data.launches.push({
      type: "Compose",
      mailto: {
        to: ["ada@example.org"],
        cc: ["bob@example.org"],
        bcc: [],
        subject: "Hello there",
        body: "Line 1",
        in_reply_to: null,
      },
    });
    renderWithProviders(<LaunchRequests />, env);
    await waitFor(() => {
      expect(env.store.getState().composer).not.toBeNull();
    });
    const composer = env.store.getState().composer;
    expect(composer?.to).toContain("ada@example.org");
    expect(composer?.cc).toContain("bob@example.org");
    expect(composer?.subject).toBe("Hello there");
    expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
    // Each request is handed out once.
    expect(env.transport.data.launches).toEqual([]);
  });

  it("opens a draft another program prepared (Simple MAPI, M4b)", async () => {
    const env = createEnv();
    const created = await env.transport.send({
      type: "NewDraft",
      identity_id: null,
      account_id: null,
      mode: "plain",
      context: null,
    });
    if (created.type !== "Draft") throw new Error(created.type);
    const saved = await env.transport.send({
      type: "SaveDraft",
      draft: {
        ...created.draft,
        subject: "Quarterly report",
        to: [{ name: "Ada", email: "ada@example.org" }],
      },
    });
    if (saved.type !== "DraftSaved") throw new Error(saved.type);
    env.transport.data.launches.push({
      type: "ComposeDraft",
      draft_id: saved.draft_id,
      program: "EXCEL.EXE",
    });
    renderWithProviders(<LaunchRequests />, env);
    await waitFor(() => {
      expect(env.store.getState().composer?.subject).toBe("Quarterly report");
    });
    expect(env.store.getState().composer?.to).toContain("ada@example.org");
    expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
    // The program is noted for the composer's notice (WP114 / WP115).
    expect(systemStoreFor(env.store).getState().programDrafts).toEqual({
      [saved.draft_id]: "EXCEL.EXE",
    });
  });

  it("fills the plain composer when the core has no rich composer", async () => {
    const env = createEnv();
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) =>
      request.type === "NewDraft"
        ? { type: "Error", code: "unsupported", message: "no composer" }
        : send(request);
    env.transport.data.launches.push({
      type: "Compose",
      mailto: {
        to: ["ada@example.org"],
        cc: [],
        bcc: ["dan@example.org"],
        subject: "Plain",
        body: "Body text",
        in_reply_to: null,
      },
    });
    renderWithProviders(<LaunchRequests />, env);
    await waitFor(() => {
      expect(env.store.getState().composer?.subject).toBe("Plain");
    });
    const composer = env.store.getState().composer;
    expect(composer?.to).toBe("ada@example.org");
    expect(composer?.bcc).toBe("dan@example.org");
    expect(composer?.showBcc).toBe(true);
    expect(composer?.text.startsWith("Body text")).toBe(true);
  });

  it("imports and shows an opened .eml file after a later start", async () => {
    const env = createEnv();
    const message = env.transport.data.messages[3];
    if (message === undefined) throw new Error("no message");
    env.transport.data.messageFiles.set("C:\\Downloads\\offer.eml", message.id);
    await launchesTaken(env);
    act(() => {
      env.transport.pushLaunch([{ type: "OpenMessageFile", path: "C:\\Downloads\\offer.eml" }]);
    });
    await waitFor(() => {
      expect(env.store.getState().selectedMessageId).toBe(message.id);
    });
    expect(env.store.getState().activeFolderId).toBe(message.folder_id);
  });

  it("opens the import dialog for .ics / .vcf files and the sources for webcal links", async () => {
    const env = createEnv();
    env.transport.data.launches.push({ type: "ImportCalendarFile", path: "C:\\cal\\team.ics" });
    renderWithProviders(<LaunchRequests />, env);
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(IMPORT_DIALOG);
    });
    expect(pimStoreFor(env.store).getState().importPath).toBe("C:\\cal\\team.ics");
    act(() => {
      env.transport.pushLaunch([
        {
          type: "SubscribeCalendar",
          url: "webcal://example.org/h.ics",
          https_url: "https://example.org/h.ics",
        },
      ]);
    });
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(SOURCES_DIALOG);
    });
    expect(pimStoreFor(env.store).getState().subscribeUrl).toBe("webcal://example.org/h.ics");
  });

  it("says when a file cannot be opened and can be dismissed with the keyboard", async () => {
    const env = createEnv();
    await launchesTaken(env);
    act(() => {
      env.transport.pushLaunch([{ type: "OpenMessageFile", path: "C:\\missing.eml" }]);
    });
    const notice = await screen.findByRole("status", { name: "Opening links and files" });
    await waitFor(() => expect(notice).toHaveTextContent("The file could not be opened"));
    // The core's localized reason, not its English diagnostic.
    expect(notice).toHaveTextContent(/The file .?C:\\missing\.eml.? cannot be read/);
    expect(notice).not.toHaveTextContent("file C:\\missing.eml not found");
    const user = userEvent.setup();
    await user.tab();
    expect(within(notice).getByRole("button", { name: "Dismiss" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.queryByRole("status", { name: "Opening links and files" })).toBeNull();
  });

  it("does nothing in a remote-UI session (desktop only)", async () => {
    remoteStore.getState().setSession({ id: "rsess-1" } as never);
    const env = createEnv();
    env.transport.data.launches.push({ type: "ImportCalendarFile", path: "C:\\a.ics" });
    renderWithProviders(<LaunchRequests />, env);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(env.transport.requests.some((r) => r.type === "TakeLaunchRequests")).toBe(false);
  });
});
