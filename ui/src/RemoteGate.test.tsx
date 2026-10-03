import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { AuthRequest, AuthResponse, AuthStatus, Request, Response } from "./api/generated";
import { ReauthTransport, reauthStore } from "./api/reauth";
import { configureRemoteAuth, remoteStore } from "./features/remote";
import { AuthTransportError } from "./features/remote/authClient";
import { I18nProvider } from "./i18n/I18nProvider";
import { ReauthPrompt, checkRemoteGate } from "./RemoteGate";
import { MockTransport } from "./api/mock";

const session = {
  id: "rsess-1",
  user_id: "ruser-1",
  user_name: "ada",
} as unknown as NonNullable<AuthStatus["session"]>;

function status(authenticated: boolean): AuthStatus {
  return {
    authenticated,
    paired: authenticated,
    session: authenticated ? session : null,
    reauth_until: null,
    csrf_token: "csrf",
    methods: { passkey: false, password: true, pairing: true } as AuthStatus["methods"],
    certificate_fingerprint: null,
  };
}

function transport(answer: (request: AuthRequest) => Promise<AuthResponse>) {
  const sent: AuthRequest[] = [];
  configureRemoteAuth({
    transport: {
      send: (request) => {
        sent.push(request);
        return answer(request);
      },
    },
  });
  return sent;
}

afterEach(() => {
  // Unmount before resetting the feature's module-level stores.
  cleanup();
  configureRemoteAuth(null);
  remoteStore.getState().clear();
  reauthStore.getState().finish(false);
});

describe("the remote login gate", () => {
  it("opens the local UI, which has no login route", async () => {
    transport(() => Promise.reject(new AuthTransportError(404, "not found")));
    expect(await checkRemoteGate()).toBe("open");
    expect(remoteStore.getState().session).toBeNull();
  });

  it("opens for a browser with a session and remembers it", async () => {
    transport(() => Promise.resolve({ type: "Status", status: status(true) }));
    expect(await checkRemoteGate()).toBe("open");
    expect(remoteStore.getState().session?.id).toBe("rsess-1");
  });

  it("shows the login page otherwise, also when the server is unreachable", async () => {
    transport(() => Promise.resolve({ type: "Status", status: status(false) }));
    expect(await checkRemoteGate()).toBe("login");
    transport(() => Promise.reject(new AuthTransportError(null, "offline")));
    expect(await checkRemoteGate()).toBe("login");
  });
});

class ScriptedTransport extends MockTransport {
  answers: Response[] = [];
  sent: Request[] = [];

  override async send(request: Request): Promise<Response> {
    this.sent.push(request);
    return this.answers.shift() ?? { type: "Done" };
  }
}

describe("re-authentication", () => {
  it("retries a request once the user confirmed who they are", async () => {
    const inner = new ScriptedTransport();
    inner.answers = [
      { type: "Error", code: "reauth_required", message: "fresh login needed" },
      { type: "RemoteUsers", users: [] },
    ];
    const wrapped = new ReauthTransport(inner);
    const answer = wrapped.send({ type: "ListRemoteUsers" });
    await waitFor(() => expect(reauthStore.getState().pending).toBe(true));
    reauthStore.getState().finish(true);
    expect((await answer).type).toBe("RemoteUsers");
    expect(inner.sent.map((r) => r.type)).toEqual(["ListRemoteUsers", "ListRemoteUsers"]);
  });

  it("keeps the error when the user cancels, and passes other answers through", async () => {
    const inner = new ScriptedTransport();
    inner.answers = [
      { type: "Error", code: "reauth_required", message: "fresh login needed" },
      { type: "Error", code: "forbidden", message: "no" },
    ];
    const wrapped = new ReauthTransport(inner);
    const answer = wrapped.send({ type: "ListRemoteUsers" });
    await waitFor(() => expect(reauthStore.getState().pending).toBe(true));
    reauthStore.getState().finish(false);
    expect(await answer).toMatchObject({ code: "reauth_required" });
    expect(await wrapped.send({ type: "ListRemoteUsers" })).toMatchObject({ code: "forbidden" });
  });

  it("asks for the password in a dialog and resumes after it", async () => {
    const sent = transport((request) =>
      Promise.resolve(
        request.type === "Password"
          ? { type: "Reauthenticated", reauth_until: "2026-09-29T12:00:00Z" }
          : { type: "Status", status: status(true) },
      ),
    );
    remoteStore.getState().setSession(session);
    render(
      <I18nProvider locale="en">
        <ReauthPrompt />
      </I18nProvider>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    let resumed: boolean | null = null;
    act(() => {
      void reauthStore
        .getState()
        .request()
        .then((ok) => {
          resumed = ok;
        });
    });
    await screen.findByRole("dialog");
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText("Password"), "a long enough password");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(resumed).toBe(true));
    expect(sent.some((r) => r.type === "Password" && r.purpose === "reauth")).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
