import { act, cleanup, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { AuthRequest, AuthResponse, AuthStatus, Request, Response } from "./api/generated";
import { ReauthTransport, reauthStore } from "./api/reauth";
import { configureRemoteAuth, remoteStore } from "./features/remote";
import { AuthTransportError, NO_LOGIN_STATUS } from "./features/remote/authClient";
import { ReauthPrompt, checkRemoteGate } from "./RemoteGate";
import { WindowLayer } from "./layout/WindowLayer";
import { createEnv, renderWithProviders } from "./test/harness";
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
  it("opens the local UI, whose server answers that it has no login", async () => {
    transport(() => Promise.reject(new AuthTransportError(NO_LOGIN_STATUS, "no login")));
    expect(await checkRemoteGate()).toBe("open");
    expect(remoteStore.getState().session).toBeNull();
  });

  it("opens the local UI of an older server, which has no login route", async () => {
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

  override send(request: Request): Promise<Response> {
    this.sent.push(request);
    return Promise.resolve(this.answers.shift() ?? { type: "Done" });
  }
}

/**
 * A request the core answers with `reauth_required` and then `next`,
 * waiting until the UI asks the user to confirm who they are.
 */
async function askedToReauth(next: Response) {
  const inner = new ScriptedTransport();
  inner.answers = [{ type: "Error", code: "reauth_required", message: "fresh login needed" }, next];
  const wrapped = new ReauthTransport(inner);
  const answer = wrapped.send({ type: "ListRemoteUsers" });
  await waitFor(() => {
    expect(reauthStore.getState().pending).toBe(true);
  });
  return { inner, wrapped, answer };
}

describe("re-authentication", () => {
  it("retries a request once the user confirmed who they are", async () => {
    const { inner, answer } = await askedToReauth({ type: "RemoteUsers", users: [] });
    reauthStore.getState().finish(true);
    expect((await answer).type).toBe("RemoteUsers");
    expect(inner.sent.map((r) => r.type)).toEqual(["ListRemoteUsers", "ListRemoteUsers"]);
  });

  it("keeps the error when the user cancels, and passes other answers through", async () => {
    const { wrapped, answer } = await askedToReauth({
      type: "Error",
      code: "forbidden",
      message: "no",
    });
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
    // M5c: the prompt is the modal window `remote-reauth`.
    const env = createEnv();
    renderWithProviders(
      <>
        <ReauthPrompt />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
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
    const dialog = await screen.findByRole("dialog", { name: "Confirm it’s you" });
    expect(dialog).toHaveAttribute("data-window", "remote-reauth");
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText("Password"), "a long enough password");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(resumed).toBe(true);
    });
    expect(sent.some((r) => r.type === "Password" && r.purpose === "reauth")).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
