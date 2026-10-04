/**
 * The desktop settings and screens against the mock core with its
 * simulated desktop host (`mock.ts`): updates and "About", tray and
 * startup, default apps, the first-run screen and the composer notice for
 * drafts from other programs.
 */
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ActionsProvider } from "../../actions/ActionsProvider";
import { NodeView } from "../../layout/LayoutRenderer";
import { useComposeContext } from "../../state/hooks";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { richStoreFor } from "../composer/store";
import { appRegistry } from "../registry";
import { remoteStore } from "../remote";
import {
  ABOUT_DIALOG,
  FIRST_RUN_DIALOG,
  IMPORT_WIZARD_ACTION,
  handleSystemEvent,
  openProgramDraft,
  programDraftMessage,
} from ".";
import { setDefaultApps, setUpdateState, simulateMapiSendMail } from "./mock";
import { AboutNail } from "./AboutNail";
import { FirstRun, FirstRunGate } from "./FirstRun";
import { ProgramDraftNotice } from "./ProgramDraft";
import { activeComposer } from "../composer/sessions";

afterEach(() => {
  // Unmount before resetting module-level stores.
  cleanup();
  remoteStore.getState().clear();
});

/** Forwards the core's events to the feature's handler, like the app. */
function listen(env: Env): () => void {
  return env.client.subscribe((event) => {
    handleSystemEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    });
  });
}

function sent(env: Env, type: string) {
  return env.transport.requests.filter((r) => r.type === type);
}

const release = {
  version: "1.1.0",
  channel: "stable",
  published_at: null,
  notes: "## New\n\n- Faster search\n- <script>alert(1)</script>",
  size_bytes: 48_000_000,
} as const;

describe("update-settings widget", () => {
  it("keeps a newer status from the events over an older answer", async () => {
    // Regression (WP115 desktop smoke): a fast check is ready before the
    // CheckForUpdates answer ("checking") arrives; the answer must not
    // overwrite it.
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    env.transport.data.features.system.autoProgress = false;
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) => {
      const answer = await send(request);
      if (request.type === "CheckForUpdates") {
        setUpdateState(env.transport, { state: "ready-to-install", update: release });
      }
      return answer;
    };
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByRole("button", { name: "Restart to install" })).toBeVisible();
    await waitFor(() => {
      expect(sent(env, "GetUpdateStatus").length).toBeGreaterThan(1);
    });
    expect(screen.getByRole("button", { name: "Restart to install" })).toBeVisible();
    expect(screen.queryByText("Checking for updates…")).toBeNull();
    stop();
  });

  it("checks, downloads with progress and restarts to install", async () => {
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    expect(await screen.findByText("Nail 1.0.0 · Installed with Nail-setup.exe")).toBeVisible();
    expect(screen.getByText("Nail checks for updates automatically.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Check now" }));
    expect(sent(env, "CheckForUpdates")).toHaveLength(1);
    // auto download: the release is found, downloaded in steps, then ready.
    expect(await screen.findByRole("progressbar", { name: "Download progress" })).toBeVisible();
    expect(await screen.findByRole("button", { name: "Restart to install" })).toBeVisible();
    expect(screen.getByText("Version 1.1.0 is ready. Restart Nail to install it.")).toBeVisible();
    const notes = screen.getByRole("region", { name: "What’s new in 1.1.0" });
    expect(within(notes).getByText("Faster search in large folders")).toBeVisible();
    expect(screen.getByText(/Last checked:/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Restart to install" }));
    expect(env.transport.data.features.system.calls).toContain("install");
    expect(
      await screen.findByText("Installing version 1.1.0. Nail restarts in a moment."),
    ).toBeVisible();
    stop();
  });

  it("offers Download when updates are not downloaded automatically", async () => {
    const env = createEnv();
    env.transport.data.features.system.updateSettings.auto_download = false;
    const stop = listen(env);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Check now" }));
    await user.click(await screen.findByRole("button", { name: "Download" }));
    expect(env.transport.data.features.system.calls).toEqual(["check", "download"]);
    expect(await screen.findByRole("button", { name: "Restart to install" })).toBeVisible();
    stop();
  });

  it("shows release notes as text only", async () => {
    const env = createEnv();
    setUpdateState(env.transport, { state: "available", update: release });
    const { container } = renderWithProviders(
      <NodeView node={widget("update-settings")} path="t/0" />,
      env,
    );
    expect(await screen.findByText("Version 1.1.0 is available (48 MB).")).toBeVisible();
    expect(screen.getByText("<script>alert(1)</script>")).toBeVisible();
    expect(container.querySelector("script")).toBeNull();
  });

  it("saves the channel and the automatic options", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "Update channel" }),
      "beta",
    );
    await waitFor(() => {
      expect(env.transport.data.features.system.updateSettings.channel).toBe("beta");
    });
    await user.click(screen.getByRole("checkbox", { name: /Download updates/ }));
    await waitFor(() => {
      expect(env.transport.data.features.system.updateSettings.auto_download).toBe(false);
    });
    expect(sent(env, "SetUpdateSettings")).toHaveLength(2);
  });

  it("explains why updates are off and offers no update buttons (MSI policy)", async () => {
    const env = createEnv();
    Object.assign(env.transport.data.features.system.installation, {
      kind: "msi",
      scope: "per-machine",
      updates_disabled: "msi-managed",
      forced_channel: "stable",
    });
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    expect(await screen.findByText("Your organization installs updates for Nail.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
    expect(screen.getByRole("combobox", { name: "Update channel" })).toBeDisabled();
    expect(screen.getByText("Your organization chose the update channel.")).toBeVisible();
    expect(screen.getByRole("checkbox", { name: /Check for updates/ })).toBeDisabled();
  });

  it("reports a failed check and offers to try again", async () => {
    const env = createEnv();
    env.transport.data.features.system.failNext = "signature";
    const stop = listen(env);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    await user.click(await screen.findByRole("button", { name: "Check now" }));
    expect(
      await screen.findByText("The update’s signature is not valid. Nothing was installed."),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(sent(env, "CheckForUpdates")).toHaveLength(2);
    stop();
  });

  it("shows the core's reason when a request is refused", async () => {
    const env = createEnv();
    env.transport.data.features.system.autoProgress = false;
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("update-settings")} path="t/0" />, env);
    const check = await screen.findByRole("button", { name: "Check now" });
    // The policy changed after the view loaded.
    env.transport.data.features.system.installation.updates_disabled = "policy";
    await user.click(check);
    expect(
      await screen.findByText("Updates are turned off by your organization’s policy."),
    ).toBeVisible();
  });
});

describe("About Nail", () => {
  it("lists the installation and the update state", async () => {
    const env = createEnv();
    renderWithProviders(<AboutNail />, env);
    const facts = await screen.findByRole("list", { name: "Installation" });
    expect(within(facts).getByText("1.0.0")).toBeVisible();
    expect(within(facts).getByText("For you only")).toBeVisible();
    expect(within(facts).getByText("x64")).toBeVisible();
    expect(within(facts).getByText("131.0.2903.70")).toBeVisible();
    expect(await within(facts).findByText("Stable")).toBeVisible();
    expect(screen.getByRole("button", { name: "Check now" })).toBeVisible();
    // No options in "About": they live in the settings.
    expect(screen.queryByRole("combobox", { name: "Update channel" })).toBeNull();
  });

  it("says it is desktop only outside the desktop app", async () => {
    const env = createEnv();
    env.transport.data.features.system.desktop = false;
    renderWithProviders(<AboutNail />, env);
    expect(
      await screen.findByText("This is only available in the Nail desktop app."),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
  });

  it("opens with the action; check-updates opens it when refused", async () => {
    const env = createEnv();
    const ctx = {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: env.transport.data.layout,
      t: (id: string) => id,
      locale: "en",
    };
    await appRegistry.actions["system.about"]?.run(ctx, []);
    expect(env.store.getState().windows).toContain(ABOUT_DIALOG);
    env.store.getState().closeWindow(ABOUT_DIALOG);
    env.transport.data.features.system.autoProgress = false;
    await appRegistry.actions["system.check-updates"]?.run(ctx, []);
    expect(env.store.getState().windows).not.toContain(ABOUT_DIALOG);
    expect(env.transport.data.features.system.updateState.state).toBe("checking");
    await appRegistry.actions["system.check-updates"]?.run(ctx, []);
    expect(env.store.getState().windows).toContain(ABOUT_DIALOG);
    await appRegistry.actions["system.default-apps"]?.run(ctx, []);
    expect(env.transport.data.features.system.calls).toContain("open-settings");
    await appRegistry.actions["system.first-run"]?.run(ctx, []);
    expect(env.store.getState().windows).toContain(FIRST_RUN_DIALOG);
  });
});

describe("desktop-settings widget", () => {
  it("explains a start with Windows the administrator manages", async () => {
    const env = createEnv();
    env.transport.data.features.system.startWithWindowsManaged = true;
    renderWithProviders(<NodeView node={widget("desktop-settings")} path="t/0" />, env);
    const start = await screen.findByRole("checkbox", { name: "Start Nail with Windows" });
    expect(start).toBeChecked();
    expect(start).toBeDisabled();
    expect(start).toHaveAccessibleDescription(
      "Your administrator set Nail to start with Windows for everyone on this computer. Only an administrator can turn this off.",
    );
    // "Start hidden" still applies to the machine's start.
    expect(screen.getByRole("checkbox", { name: /without a window/ })).toBeEnabled();
  });

  it("explains it on the first-run screen too", async () => {
    const env = createEnv();
    env.transport.data.features.system.startWithWindowsManaged = true;
    renderWithProviders(<FirstRun close={vi.fn()} />, env);
    const start = await screen.findByRole("checkbox", { name: /Start Nail with Windows/ });
    expect(start).toBeChecked();
    expect(start).toBeDisabled();
    expect(screen.getByText(/Only an administrator can turn this off/)).toBeVisible();
  });

  it("saves tray and startup settings and shows the dev endpoint", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("desktop-settings")} path="t/0" />, env);
    const tray = await screen.findByRole("checkbox", { name: /notification area$/ });
    expect(tray).toBeChecked();
    const hidden = screen.getByRole("checkbox", { name: /without a window/ });
    expect(hidden).toBeDisabled();
    await user.click(screen.getByRole("checkbox", { name: "Start Nail with Windows" }));
    await waitFor(() => {
      expect(env.transport.data.features.system.desktopSettings.start_with_windows).toBe(true);
    });
    await waitFor(() => expect(hidden).toBeEnabled());
    await user.click(tray);
    await waitFor(() => {
      expect(env.transport.data.features.system.desktopSettings.close_to_tray).toBe(false);
    });
    await user.click(screen.getByRole("checkbox", { name: /nail plugin dev/ }));
    expect(await screen.findByText("Listening on http://127.0.0.1:47811/")).toBeVisible();
    expect(sent(env, "SetDesktopSettings")).toHaveLength(3);
  });

  it("hides the developer switch with show-dev-endpoint=#false", async () => {
    const env = createEnv();
    renderWithProviders(
      <NodeView
        node={widget("desktop-settings", { options: { "show-dev-endpoint": false } })}
        path="t/0"
      />,
      env,
    );
    await screen.findByRole("checkbox", { name: "Start Nail with Windows" });
    expect(screen.queryByRole("checkbox", { name: /nail plugin dev/ })).toBeNull();
  });
});

describe("default-apps widget", () => {
  it("lists the state, opens Settings → Default apps and follows changes", async () => {
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("default-apps")} path="t/0" />, env);
    const list = await screen.findByRole("list", { name: "Links and files" });
    const mailto = within(list).getByText("E-mail links (mailto:)").closest("li");
    expect(mailto).toHaveTextContent("Another app is the default");
    expect(within(list).getByText(/Send to → Mail recipient/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Choose in Default apps" }));
    expect(env.transport.data.features.system.calls).toEqual(["open-settings"]);
    expect(screen.getByText("Choose Nail in Windows Settings, then come back here.")).toBeVisible();
    act(() => {
      setDefaultApps(env.transport, (status) => {
        const entry = status.associations.find((a) => a.target === "mailto");
        if (entry) entry.state = "default";
      });
    });
    await waitFor(() => expect(mailto).toHaveTextContent("Nail is the default"));
    stop();
  });

  it("says when Nail or the Send to component is not registered", async () => {
    const env = createEnv();
    env.transport.data.features.system.installation.mapi_installed = false;
    env.transport.data.features.system.defaultApps.registered = false;
    renderWithProviders(<NodeView node={widget("default-apps")} path="t/0" />, env);
    expect(await screen.findByText(/Nail is not listed in Windows Settings/)).toBeVisible();
    expect(await screen.findByText(/component for “Send to/)).toBeVisible();
  });
});

describe("first-run screen", () => {
  it("is offered once in the desktop app until completed", async () => {
    const env = createEnv();
    env.transport.data.features.system.firstRun.completed = false;
    renderWithProviders(<FirstRunGate />, env);
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(FIRST_RUN_DIALOG);
    });
  });

  it("is not offered when completed, without a desktop host or in a remote session", async () => {
    for (const setup of ["completed", "serve", "remote"] as const) {
      const env = createEnv();
      const system = env.transport.data.features.system;
      system.firstRun.completed = setup === "completed";
      if (setup === "serve") system.desktop = false;
      if (setup === "remote") {
        remoteStore.getState().setSession({ id: "rsess-1" } as never);
      }
      const { unmount } = renderWithProviders(<FirstRunGate />, env);
      await waitFor(() => {
        expect(env.queryClient.isFetching()).toBe(0);
      });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
      expect(env.store.getState().windows).not.toContain(FIRST_RUN_DIALOG);
      unmount();
      remoteStore.getState().clear();
    }
  });

  it("offers default apps, start with Windows and the import offer", async () => {
    const env = createEnv();
    env.transport.data.features.system.firstRun.completed = false;
    env.store.getState().openWindow(FIRST_RUN_DIALOG);
    const close = vi.fn(() => {
      env.store.getState().closeWindow(FIRST_RUN_DIALOG);
    });
    const user = userEvent.setup();
    renderWithProviders(<FirstRun close={close} />, env);
    expect(await screen.findByText("E-mail links now: Another app is the default")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Choose in Default apps" }));
    expect(env.transport.data.features.system.calls).toEqual(["open-settings"]);
    await user.click(screen.getByRole("checkbox", { name: /Start Nail with Windows/ }));
    await waitFor(() => {
      expect(env.transport.data.features.system.desktopSettings).toMatchObject({
        start_with_windows: true,
        start_hidden: true,
      });
    });
    expect(screen.getByRole("button", { name: "Import…" })).toBeVisible();
    expect(
      screen.queryByText("The import assistant comes with a later version of Nail."),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "Get started" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(env.transport.data.features.system.firstRun.completed).toBe(true);
    expect(env.store.getState().windows).not.toContain(FIRST_RUN_DIALOG);
  });

  it("starts the import assistant once it exists (M4c)", async () => {
    const env = createEnv();
    const run = vi.fn();
    const close = vi.fn();
    const user = userEvent.setup();
    renderWithProviders(
      <ActionsProvider
        layout={env.transport.data.layout}
        handlers={{ ...appRegistry.actions, [IMPORT_WIZARD_ACTION]: { run } }}
      >
        <FirstRun close={close} />
      </ActionsProvider>,
      env,
    );
    await user.click(await screen.findByRole("button", { name: "Import…" }));
    await waitFor(() => {
      expect(run).toHaveBeenCalled();
    });
    expect(close).toHaveBeenCalled();
    expect(sent(env, "CompleteFirstRun")).toHaveLength(1);
  });

  it("Skip completes it too; outside the desktop only the import step shows", async () => {
    const env = createEnv();
    env.transport.data.features.system.desktop = false;
    const close = vi.fn();
    const user = userEvent.setup();
    renderWithProviders(<FirstRun close={close} />, env);
    await waitFor(() => {
      expect(env.queryClient.isFetching()).toBe(0);
    });
    expect(screen.queryByRole("button", { name: "Choose in Default apps" })).toBeNull();
    expect(screen.getByText("Bring your mail along")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Skip" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
  });
});

describe("mail from other programs (Simple MAPI)", () => {
  function Opener({ draftId, program }: { draftId: string; program: string | null }) {
    const ctx = useComposeContext();
    return (
      <button
        type="button"
        onClick={() =>
          void openProgramDraft(ctx, { type: "ComposeDraft", draft_id: draftId, program })
        }
      >
        open
      </button>
    );
  }

  it("opens the prepared draft with recipients and files and names the program", async () => {
    const env = createEnv();
    const result = simulateMapiSendMail(env.transport, {
      subject: "Quarterly report",
      body: "See attached.",
      recipients: [{ class: "to", name: "Ada", address: "SMTP:ada@example.org" }],
      attachments: [
        { path: "C:\\Temp\\Q3.xlsx", file_name: "Q3.xlsx", mime: "application/x", data: "AAEC" },
      ],
      ui: "none",
      program: "EXCEL.EXE",
    });
    expect(result).toBe("success");
    const launch = env.transport.data.launches.pop();
    if (launch?.type !== "ComposeDraft") throw new Error("no launch");
    const user = userEvent.setup();
    renderWithProviders(
      <>
        <Opener draftId={launch.draft_id} program={launch.program} />
        <ProgramDraftNotice />
      </>,
      env,
    );
    await user.click(screen.getByRole("button", { name: "open" }));
    await waitFor(() => {
      expect(env.store.getState().composer?.subject).toBe("Quarterly report");
    });
    const composer = env.store.getState().composer;
    expect(composer?.to).toContain("ada@example.org");
    expect(composer?.text.startsWith("See attached.")).toBe(true);
    expect(
      richStoreFor(env.store, activeComposer(env.store.getState().windows))
        .getState()
        .attachments.map((a) => a.filename),
    ).toEqual(["Q3.xlsx"]);
    expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
    const notice = await screen.findByRole("note", { name: "Message from another program" });
    expect(notice).toHaveTextContent("EXCEL.EXE prepared this message. Check it, then press Send.");
    expect(sent(env, "SendDraft")).toHaveLength(0);
    await user.click(within(notice).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("has a text for an unknown program and nothing for other drafts", () => {
    expect(programDraftMessage(null)).toEqual({ id: "system-program-draft-unknown" });
    expect(programDraftMessage(" WINWORD.EXE ")).toEqual({
      id: "mapi-draft-from-program",
      args: { program: "WINWORD.EXE" },
    });
    const env = createEnv();
    const { container } = renderWithProviders(<ProgramDraftNotice draftId="draft-x" />, env);
    expect(container).toBeEmptyDOMElement();
  });
});
