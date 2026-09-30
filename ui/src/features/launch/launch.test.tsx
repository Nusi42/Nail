import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { createEnv, renderWithProviders } from "../../test/harness";
import { remoteStore } from "../remote";
import { LaunchRequests } from "./LaunchRequests";

afterEach(() => remoteStore.getState().clear());

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
    await waitFor(() => expect(env.store.getState().composer).not.toBeNull());
    const composer = env.store.getState().composer;
    expect(composer?.to).toContain("ada@example.org");
    expect(composer?.cc).toContain("bob@example.org");
    expect(composer?.subject).toBe("Hello there");
    expect(env.store.getState().windows).toContain("composer");
    // Each request is handed out once.
    expect(env.transport.data.launches).toEqual([]);
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
    await waitFor(() => expect(env.store.getState().composer?.subject).toBe("Plain"));
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
    renderWithProviders(<LaunchRequests />, env);
    await waitFor(() =>
      expect(env.transport.requests.some((r) => r.type === "TakeLaunchRequests")).toBe(true),
    );
    act(() =>
      env.transport.pushLaunch([{ type: "OpenMessageFile", path: "C:\\Downloads\\offer.eml" }]),
    );
    await waitFor(() => expect(env.store.getState().selectedMessageId).toBe(message.id));
    expect(env.store.getState().activeFolderId).toBe(message.folder_id);
  });

  it("says what it cannot open yet and can be dismissed with the keyboard", async () => {
    const env = createEnv();
    env.transport.data.launches.push({ type: "ImportContactsFile", path: "C:\\a.vcf" });
    renderWithProviders(<LaunchRequests />, env);
    const notice = await screen.findByRole("status", { name: "Opening links and files" });
    expect(notice).toHaveTextContent("Contacts cannot be imported yet");
    act(() => env.transport.pushLaunch([{ type: "OpenMessageFile", path: "C:\\missing.eml" }]));
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
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(env.transport.requests.some((r) => r.type === "TakeLaunchRequests")).toBe(false);
  });
});
