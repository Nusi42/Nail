import { act, cleanup, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { appRegistry } from "../registry";
import { createEnv, renderWithProviders } from "../../test/harness";
import { RemoteAlerts } from "./Alerts";
import { configureRemoteAuth, remoteAuth } from "./authConfig";
import { actions, dialogs, effects, events, handleRemoteEvent, widgets } from "./index";
import { runtimeIds } from "./messages";
import { PAIR_DIALOG } from "./PairDevice";
import { remoteKeys } from "./queries";
import { MAX_ALERTS, remoteStore } from "./state";
import { createAda, pairedBrowser } from "./testUtil";
import { wireEvents, actionContext } from "../../test/env";

afterEach(() => {
  // Unmount before resetting the feature's module-level stores.
  cleanup();
  remoteStore.getState().clear();
  configureRemoteAuth(null);
});

describe("remote feature slot", () => {
  it("registers its widgets, dialog, actions, events and alert effect", () => {
    expect(Object.keys(widgets).sort()).toEqual([
      "remote-audit",
      "remote-content",
      "remote-content-allows",
      "remote-devices",
      "remote-settings",
      "remote-users",
    ]);
    for (const kind of Object.keys(widgets)) expect(appRegistry.widgets[kind]).toBe(widgets[kind]);
    expect(appRegistry.dialogs[PAIR_DIALOG]).toBe(dialogs[PAIR_DIALOG]);
    expect(Object.keys(actions).sort()).toEqual([
      "remote.content.allow",
      "remote.logout",
      "remote.pair",
    ]);
    expect(events).toEqual([handleRemoteEvent]);
    expect(effects).toEqual([RemoteAlerts]);
    expect(runtimeIds()).toContain("window-remote-pair");
  });

  it("opens the pairing dialog from the action", async () => {
    const env = createEnv();
    remoteStore.getState().setPairUser("ruser-9");
    await actions["remote.pair"]?.run(actionContext(env), []);
    expect(env.store.getState().windows).toContain(PAIR_DIALOG);
    expect(remoteStore.getState().pairUser).toBeNull();
  });

  it("signs this browser out and notices when its session is ended elsewhere", async () => {
    const env = createEnv();
    wireEvents(env, handleRemoteEvent);
    const ada = await createAda(env.transport);
    const { browser, session } = await pairedBrowser(env.transport, ada.id);
    configureRemoteAuth({ transport: browser });
    expect(remoteAuth().transport).toBe(browser);
    const logout = actions["remote.logout"];
    expect(logout?.enabled?.(actionContext(env), [])).toBe(false);
    remoteStore.getState().setSession(session);
    expect(logout?.enabled?.(actionContext(env), [])).toBe(true);
    await logout?.run(actionContext(env), []);
    expect(browser.session()).toBeNull();
    expect(remoteStore.getState().ended).toBe("logout");

    // Revoked from the desktop: the event ends this page's session.
    const again = await pairedBrowser(env.transport, ada.id, "Tablet");
    remoteStore.getState().setSession(again.session);
    await env.transport.send({ type: "RevokeRemoteSession", session_id: again.session.id });
    expect(remoteStore.getState().ended).toBe("revoked");
  });

  it("keeps the status cache current and refreshes access data on events", async () => {
    const env = createEnv();
    wireEvents(env, handleRemoteEvent);
    env.queryClient.setQueryData(remoteKeys.ui, {
      settings: env.transport.data.features.remote.settings,
      status: { state: "stopped" },
    });
    env.transport.data.features.remote.settings.enabled = true;
    await env.transport.send({
      type: "UpdateRemoteUiSettings",
      settings: env.transport.data.features.remote.settings,
    });
    expect(
      env.queryClient.getQueryData<{ status: { state: string } }>(remoteKeys.ui)?.status.state,
    ).toBe("running");
    env.queryClient.setQueryData(remoteKeys.users, []);
    env.transport.emit({ type: "RemoteAccessChanged" });
    expect(env.queryClient.getQueryState(remoteKeys.users)?.isInvalidated).toBe(true);
    env.queryClient.setQueryData(remoteKeys.allows, []);
    env.transport.emit({ type: "RemoteContentPolicyChanged" });
    expect(env.queryClient.getQueryState(remoteKeys.allows)?.isInvalidated).toBe(true);
  });
});

describe("security alerts", () => {
  it("shows a toast for a new device until it is dismissed (Escape)", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env, handleRemoteEvent);
    renderWithProviders(<RemoteAlerts />, env);
    expect(screen.queryByRole("region", { name: "Security alerts" })).not.toBeInTheDocument();

    const ada = await createAda(env.transport);
    await act(async () => {
      await pairedBrowser(env.transport, ada.id, "Phone", "10.0.0.7");
    });
    const toast = await screen.findByRole("alert", { name: "New device signed in" });
    expect(toast).toHaveTextContent("Phone was paired and signed in as ada from 10.0.0.7.");
    const dismiss = within(toast).getByRole("button", { name: "Dismiss" });
    dismiss.focus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("offers to review devices and keeps at most a few alerts", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env, handleRemoteEvent);
    renderWithProviders(<RemoteAlerts />, env);
    act(() => {
      for (let i = 0; i < MAX_ALERTS + 2; i++) {
        env.transport.emit({
          type: "RemoteSecurityAlert",
          alert: {
            kind: "lockout",
            user_name: i === 0 ? null : `user${String(i)}`,
            ip: i === 0 ? "10.0.0.9" : null,
            until: "2026-09-29T12:00:00Z",
          },
        });
      }
    });
    const toasts = await screen.findAllByRole("alert", { name: "Sign-in locked" });
    expect(toasts).toHaveLength(MAX_ALERTS);
    expect(toasts[0]).toHaveTextContent(/Too many failed sign-ins for user2/);
    const first = toasts[0];
    if (first === undefined) throw new Error("no toast");
    await user.click(within(first).getByRole("button", { name: "Review devices" }));
    expect(env.store.getState().windows).toContain("settings");
    expect(screen.getAllByRole("alert")).toHaveLength(MAX_ALERTS - 1);
  });
});
