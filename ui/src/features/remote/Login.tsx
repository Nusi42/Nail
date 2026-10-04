/**
 * The remote UI's login page (browser side, ADR 0036). It talks to
 * `POST /api/auth` through an `AuthTransport`, not to `NailClient`: it
 * runs before there is a session.
 *
 * - A browser that is not paired enters the one-time pairing code shown on
 *   the desktop and compares the certificate fingerprint with the one the
 *   desktop shows. Pairing logs it in; it may then add a passkey.
 * - A paired browser signs in with a passkey first; a password (+ TOTP
 *   when enrolled) is the fallback.
 * - `RemoteReauth` asks again before a sensitive action (purpose
 *   `reauth`).
 *
 * The app shell mounts `RemoteLogin` in web mode when `Status` says the
 * browser is not authenticated (WP45).
 */
import { useEffect, useState, type SubmitEvent, type ReactNode } from "react";
import type {
  AuthFailure,
  AuthPurpose,
  AuthResponse,
  AuthStatus,
  RemoteSession,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { AuthTransport } from "./authClient";
import { InputField } from "./fields";
import { groupFingerprint, normalizePairingCode, secondsUntil } from "./model";
import { browserPasskeys, passkeyProblem, type PasskeyApi } from "./passkeys";

/** A message id plus arguments for the page's notice line. */
interface Notice {
  id: string;
  args?: Record<string, string | number>;
  /** An error (role `alert`) or information (role `status`). */
  error: boolean;
}

/** Fluent id of a login failure. */
export function failureMessageId(reason: AuthFailure): string {
  return `remote-login-failed-${reason}`;
}

/** Seconds left of a throttle, counting down once a second. */
function useCountdown(): [number, (seconds: number) => void] {
  const [left, setLeft] = useState(0);
  useEffect(() => {
    if (left <= 0) return;
    const timer = setTimeout(() => {
      setLeft((n) => Math.max(0, n - 1));
    }, 1000);
    return () => {
      clearTimeout(timer);
    };
  }, [left]);
  return [left, setLeft];
}

function NoticeLine({ notice, wait }: { notice: Notice | null; wait: number }) {
  const t = useT();
  if (notice === null && wait === 0) return null;
  return (
    <div data-part="remote-login.notices">
      {notice && (
        <p
          data-part="remote-login.notice"
          data-state={notice.error ? "error" : "info"}
          role={notice.error ? "alert" : "status"}
        >
          {t(notice.id, notice.args)}
        </p>
      )}
      {wait > 0 && (
        <p data-part="remote-login.wait" role="status">
          {t("remote-login-retry-after", { seconds: wait })}
        </p>
      )}
    </div>
  );
}

/** The outcome of one protocol call, for the forms. */
type Step =
  | { kind: "done"; answer: AuthResponse }
  | { kind: "failed"; notice: Notice; retryAfter: number | null };

async function call(run: () => Promise<AuthResponse>): Promise<Step> {
  try {
    const answer = await run();
    if (answer.type === "Failed") {
      return {
        kind: "failed",
        notice: { id: failureMessageId(answer.reason), error: true },
        retryAfter: answer.retry_after_secs,
      };
    }
    return { kind: "done", answer };
  } catch {
    return {
      kind: "failed",
      notice: { id: "remote-login-unreachable", error: true },
      retryAfter: null,
    };
  }
}

/** Shared state of the forms: busy flag, notice and throttle. */
function useAttempt() {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [wait, setWait] = useCountdown();
  const run = async (fn: () => Promise<Step>): Promise<AuthResponse | null> => {
    setBusy(true);
    setNotice(null);
    const step = await fn();
    setBusy(false);
    if (step.kind === "failed") {
      setNotice(step.notice);
      setWait(step.retryAfter ?? 0);
      return null;
    }
    setWait(0);
    return step.answer;
  };
  return { busy, notice, setNotice, wait, run, blocked: busy || wait > 0 };
}

// ---------------------------------------------------------------------------
// Pairing (browser side)

function PairForm({
  auth,
  status,
  onPaired,
}: {
  auth: AuthTransport;
  status: AuthStatus;
  onPaired: (session: RemoteSession) => void;
}) {
  const t = useT();
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const attempt = useAttempt();
  const typed = normalizePairingCode(code);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (attempt.blocked || typed === "") return;
    const answer = await attempt.run(() =>
      call(() =>
        auth.send({
          type: "Pair",
          code: typed,
          device_name: deviceName.trim() || t("remote-pair-default-device"),
        }),
      ),
    );
    if (answer?.type === "LoggedIn") onPaired(answer.session);
  };

  return (
    <form
      data-part="remote-login.form"
      data-step="pair"
      aria-label={t("remote-pair-browser-title")}
      noValidate
      onSubmit={(e) => void submit(e)}
    >
      <h1 data-part="remote-login.title">{t("remote-pair-browser-title")}</h1>
      <p data-part="remote-login.intro">{t("remote-pair-browser-intro")}</p>
      <section
        data-part="remote-login.fingerprint"
        aria-label={t("remote-fingerprint-label")}
        data-state={status.certificate_fingerprint === null ? "none" : "shown"}
      >
        {status.certificate_fingerprint !== null ? (
          <>
            <p data-part="remote-login.fingerprint.intro">{t("remote-pair-browser-compare")}</p>
            <code data-part="remote-fingerprint">
              {groupFingerprint(status.certificate_fingerprint)}
            </code>
            <p data-part="form.hint">{t("remote-pair-browser-mismatch")}</p>
          </>
        ) : (
          <p data-part="remote-login.fingerprint.intro">{t("remote-pair-browser-no-tls")}</p>
        )}
      </section>
      <InputField
        labelId="remote-pair-code"
        hintId="remote-pair-code-hint"
        name="pairing-code"
        value={code}
        onChange={setCode}
        autoComplete="one-time-code"
        focusOnMount
        maxLength={16}
      />
      <InputField
        labelId="remote-pair-device-name"
        hintId="remote-pair-device-name-hint"
        name="device-name"
        value={deviceName}
        onChange={setDeviceName}
        maxLength={64}
      />
      <NoticeLine notice={attempt.notice} wait={attempt.wait} />
      <div data-part="remote-login.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="pair"
          disabled={attempt.blocked || typed === ""}
        >
          {t("remote-pair-browser-submit")}
        </button>
      </div>
    </form>
  );
}

/** After pairing: offer a passkey so the next login needs no password. */
function OfferPasskey({
  auth,
  passkeys,
  session,
  onDone,
}: {
  auth: AuthTransport;
  passkeys: PasskeyApi;
  session: RemoteSession;
  onDone: () => void;
}) {
  const t = useT();
  const [name, setName] = useState(session.device_name);
  const attempt = useAttempt();
  const [added, setAdded] = useState(false);

  const add = async (event: SubmitEvent) => {
    event.preventDefault();
    if (attempt.blocked || name.trim() === "") return;
    const answer = await attempt.run(async () => {
      const start = await call(() =>
        auth.send({ type: "PasskeyRegisterStart", name: name.trim() }),
      );
      if (start.kind === "failed" || start.answer.type !== "PasskeyChallenge") {
        return start.kind === "failed"
          ? start
          : {
              kind: "failed",
              notice: { id: "remote-login-unexpected", error: true },
              retryAfter: null,
            };
      }
      let credential: unknown;
      try {
        credential = await passkeys.register(start.answer.options);
      } catch (e) {
        return {
          kind: "failed",
          notice: { id: `remote-passkey-${passkeyProblem(e)}`, error: true },
          retryAfter: null,
        };
      }
      return call(() =>
        auth.send({
          type: "PasskeyRegisterFinish",
          challenge_id: start.answer.type === "PasskeyChallenge" ? start.answer.challenge_id : "",
          credential,
        }),
      );
    });
    if (answer?.type === "PasskeyRegistered") setAdded(true);
  };

  if (added) {
    return (
      <section
        data-part="remote-login.form"
        data-step="passkey-added"
        aria-label={t("remote-passkey-added-title")}
      >
        <h1 data-part="remote-login.title">{t("remote-passkey-added-title")}</h1>
        <p data-part="remote-login.intro" role="status">
          {t("remote-passkey-added", { name: name.trim() })}
        </p>
        <div data-part="remote-login.actions">
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="continue"
            // eslint-disable-next-line jsx-a11y/no-autofocus -- the step's only action takes the focus when the step replaces the previous one.
            autoFocus
            onClick={onDone}
          >
            {t("remote-login-continue")}
          </button>
        </div>
      </section>
    );
  }

  return (
    <form
      data-part="remote-login.form"
      data-step="offer-passkey"
      aria-label={t("remote-passkey-offer-title")}
      noValidate
      onSubmit={(e) => void add(e)}
    >
      <h1 data-part="remote-login.title">{t("remote-passkey-offer-title")}</h1>
      <p data-part="remote-login.intro">{t("remote-passkey-offer-intro")}</p>
      <InputField
        labelId="remote-passkey-name"
        name="passkey-name"
        value={name}
        onChange={setName}
        focusOnMount
        maxLength={64}
      />
      <NoticeLine notice={attempt.notice} wait={attempt.wait} />
      <div data-part="remote-login.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="add-passkey"
          disabled={attempt.blocked || name.trim() === ""}
        >
          {t("remote-passkey-offer-add")}
        </button>
        <button type="button" data-part="form.button" data-action="skip" onClick={onDone}>
          {t("remote-passkey-offer-skip")}
        </button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// Signing in (login and re-authentication share the forms)

interface SignInProps {
  auth: AuthTransport;
  passkeys: PasskeyApi;
  purpose: AuthPurpose;
  /** Passkeys offered (server and browser). */
  passkeyOffered: boolean;
  /** Fixed login name (re-authentication), else typed. */
  userName: string | null;
  onSuccess: (answer: AuthResponse) => void;
  /** Extra buttons (e.g. "Cancel" of the re-authentication). */
  extra?: ReactNode;
}

function PasskeyStep({
  auth,
  passkeys,
  purpose,
  userName,
  onSuccess,
  onPassword,
  extra,
}: SignInProps & { onPassword: () => void }) {
  const t = useT();
  const attempt = useAttempt();

  const signIn = async () => {
    if (attempt.blocked) return;
    const answer = await attempt.run(async () => {
      const start = await call(() =>
        auth.send({ type: "PasskeyStart", user_name: userName, purpose }),
      );
      if (start.kind === "failed") return start;
      if (start.answer.type !== "PasskeyChallenge") {
        return {
          kind: "failed",
          notice: { id: "remote-login-unexpected", error: true },
          retryAfter: null,
        };
      }
      const challengeId = start.answer.challenge_id;
      let credential: unknown;
      try {
        credential = await passkeys.authenticate(start.answer.options);
      } catch (e) {
        const problem = passkeyProblem(e);
        return {
          kind: "failed",
          notice: { id: `remote-passkey-${problem}`, error: problem !== "cancelled" },
          retryAfter: null,
        };
      }
      return call(() =>
        auth.send({ type: "PasskeyFinish", challenge_id: challengeId, credential }),
      );
    });
    if (answer !== null) onSuccess(answer);
  };

  return (
    <div data-part="remote-login.form" data-step="passkey">
      <p data-part="remote-login.intro">
        {t(purpose === "reauth" ? "remote-reauth-passkey-intro" : "remote-login-passkey-intro")}
      </p>
      <NoticeLine notice={attempt.notice} wait={attempt.wait} />
      <div data-part="remote-login.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="passkey"
          // eslint-disable-next-line jsx-a11y/no-autofocus -- the step's main action takes the focus when the step replaces the previous one.
          autoFocus
          disabled={attempt.blocked}
          onClick={() => void signIn()}
        >
          {t("remote-login-passkey")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="use-password"
          onClick={onPassword}
        >
          {t("remote-login-use-password")}
        </button>
        {extra}
      </div>
    </div>
  );
}

function PasswordStep({
  auth,
  purpose,
  passkeyOffered,
  userName,
  onSuccess,
  onPasskey,
  extra,
}: SignInProps & { onPasskey: () => void }) {
  const t = useT();
  const [name, setName] = useState(userName ?? "");
  const [password, setPassword] = useState("");
  const [totp, setTotp] = useState<string | null>(null);
  const attempt = useAttempt();
  const needsTotp = totp !== null;

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (attempt.blocked || name.trim() === "" || password === "") return;
    if (needsTotp && totp.trim() === "") return;
    const answer = await attempt.run(() =>
      call(() =>
        auth.send({
          type: "Password",
          user_name: name.trim(),
          password,
          totp_code: needsTotp ? totp.replace(/\s/g, "") : null,
          purpose,
        }),
      ),
    );
    if (answer === null) {
      if (needsTotp) setTotp("");
      return;
    }
    if (answer.type === "TotpRequired") {
      setTotp("");
      attempt.setNotice(null);
      return;
    }
    setPassword("");
    onSuccess(answer);
  };

  const restart = () => {
    setTotp(null);
    setPassword("");
    attempt.setNotice(null);
  };

  return (
    <form
      data-part="remote-login.form"
      data-step={needsTotp ? "totp" : "password"}
      aria-label={t(needsTotp ? "remote-login-totp-title" : "remote-login-password-title")}
      noValidate
      onSubmit={(e) => void submit(e)}
    >
      {needsTotp ? (
        <>
          <p data-part="remote-login.intro">{t("remote-login-totp-intro")}</p>
          <InputField
            key="totp"
            labelId="remote-login-totp-code"
            name="totp-code"
            value={totp}
            onChange={setTotp}
            autoComplete="one-time-code"
            inputMode="numeric"
            focusOnMount
            maxLength={10}
          />
        </>
      ) : (
        <>
          <InputField
            labelId="remote-login-user"
            name="user-name"
            value={name}
            onChange={setName}
            autoComplete="username"
            focusOnMount={userName === null}
            readOnly={userName !== null}
          />
          <InputField
            labelId="remote-login-password"
            name="password"
            type="password"
            value={password}
            onChange={setPassword}
            autoComplete="current-password"
            focusOnMount={userName !== null}
          />
        </>
      )}
      <NoticeLine notice={attempt.notice} wait={attempt.wait} />
      <div data-part="remote-login.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action={needsTotp ? "verify" : "sign-in"}
          disabled={
            attempt.blocked ||
            name.trim() === "" ||
            password === "" ||
            (needsTotp && totp.trim() === "")
          }
        >
          {t(
            needsTotp
              ? "remote-login-verify"
              : purpose === "reauth"
                ? "remote-reauth-confirm"
                : "remote-login-sign-in",
          )}
        </button>
        {needsTotp && (
          <button type="button" data-part="form.button" data-action="restart" onClick={restart}>
            {t("remote-login-start-over")}
          </button>
        )}
        {!needsTotp && passkeyOffered && (
          <button
            type="button"
            data-part="form.button"
            data-action="use-passkey"
            onClick={onPasskey}
          >
            {t("remote-login-use-passkey")}
          </button>
        )}
        {extra}
      </div>
    </form>
  );
}

function SignIn(props: SignInProps) {
  const [method, setMethod] = useState<"passkey" | "password">(
    props.passkeyOffered ? "passkey" : "password",
  );
  return method === "passkey" ? (
    <PasskeyStep
      {...props}
      onPassword={() => {
        setMethod("password");
      }}
    />
  ) : (
    <PasswordStep
      {...props}
      onPasskey={() => {
        setMethod("passkey");
      }}
    />
  );
}

// ---------------------------------------------------------------------------
// The page

export interface RemoteLoginProps {
  auth: AuthTransport;
  /** WebAuthn in this browser (default: the real one). */
  passkeys?: PasskeyApi;
  /** Called once the browser has a session. */
  onLoggedIn: (session: RemoteSession) => void;
}

type View =
  | { kind: "loading" }
  | { kind: "unreachable" }
  | { kind: "pair"; status: AuthStatus }
  | { kind: "sign-in"; status: AuthStatus }
  | { kind: "offer-passkey"; session: RemoteSession };

/** The login page of the remote UI. */
export function RemoteLogin({ auth, passkeys = browserPasskeys, onLoggedIn }: RemoteLoginProps) {
  const t = useT();
  const [view, setView] = useState<View>({ kind: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let live = true;
    auth
      .send({ type: "Status" })
      .then((answer) => {
        if (!live) return;
        if (answer.type !== "Status") {
          setView({ kind: "unreachable" });
        } else if (answer.status.authenticated && answer.status.session !== null) {
          onLoggedIn(answer.status.session);
        } else {
          setView({ kind: answer.status.paired ? "sign-in" : "pair", status: answer.status });
        }
      })
      .catch(() => {
        if (live) setView({ kind: "unreachable" });
      });
    return () => {
      live = false;
    };
  }, [auth, onLoggedIn, attempt]);

  let content: ReactNode;
  switch (view.kind) {
    case "loading":
      content = (
        <p data-part="remote-login.loading" role="status">
          {t("remote-login-loading")}
        </p>
      );
      break;
    case "unreachable":
      content = (
        <div data-part="remote-login.form" data-step="unreachable">
          <p data-part="remote-login.notice" data-state="error" role="alert">
            {t("remote-login-unreachable")}
          </p>
          <button
            type="button"
            data-part="form.button"
            data-action="retry"
            onClick={() => {
              setView({ kind: "loading" });
              setAttempt((n) => n + 1);
            }}
          >
            {t("remote-login-retry")}
          </button>
        </div>
      );
      break;
    case "pair": {
      const offer = view.status.methods.passkey && passkeys.supported();
      content = (
        <PairForm
          auth={auth}
          status={view.status}
          onPaired={(session) => {
            if (offer) setView({ kind: "offer-passkey", session });
            else onLoggedIn(session);
          }}
        />
      );
      break;
    }
    case "offer-passkey":
      content = (
        <OfferPasskey
          auth={auth}
          passkeys={passkeys}
          session={view.session}
          onDone={() => {
            onLoggedIn(view.session);
          }}
        />
      );
      break;
    case "sign-in": {
      const offered = view.status.methods.passkey && passkeys.supported();
      content = (
        <section data-part="remote-login.sign-in" aria-label={t("remote-login-title")}>
          <h1 data-part="remote-login.title">{t("remote-login-title")}</h1>
          {view.status.methods.passkey && !passkeys.supported() && (
            <p data-part="remote-login.note" role="note">
              {t("remote-login-no-passkey-support")}
            </p>
          )}
          <SignIn
            auth={auth}
            passkeys={passkeys}
            purpose="login"
            passkeyOffered={offered}
            userName={null}
            onSuccess={(answer) => {
              if (answer.type === "LoggedIn") onLoggedIn(answer.session);
            }}
          />
        </section>
      );
      break;
    }
  }

  return (
    <main data-part="remote-login" data-state={view.kind} aria-label={t("remote-login-label")}>
      {content}
    </main>
  );
}

// ---------------------------------------------------------------------------
// Re-authentication

export interface RemoteReauthProps {
  auth: AuthTransport;
  passkeys?: PasskeyApi;
  /** The session's login name. */
  userName: string;
  /** Whether the server offers passkeys (`AuthStatus.methods.passkey`). */
  passkeyAvailable: boolean;
  /** Called with the new `reauth_until`. */
  onDone: (reauthUntil: string) => void;
  onCancel: () => void;
}

/**
 * "Confirm it's you" before a sensitive action (grants, key export,
 * remote-access changes): the core answered `reauth_required`.
 */
export function RemoteReauth({
  auth,
  passkeys = browserPasskeys,
  userName,
  passkeyAvailable,
  onDone,
  onCancel,
}: RemoteReauthProps) {
  const t = useT();
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape from any field or button inside cancels the re-authentication.
    <section
      data-part="remote-reauth"
      role="group"
      aria-label={t("remote-reauth-title")}
      onKeyDown={(event) => {
        if (event.key === "Escape") onCancel();
      }}
    >
      <h2 data-part="remote-reauth.title">{t("remote-reauth-title")}</h2>
      <p data-part="remote-reauth.intro">{t("remote-reauth-intro", { user: userName })}</p>
      <SignIn
        auth={auth}
        passkeys={passkeys}
        purpose="reauth"
        passkeyOffered={passkeyAvailable && passkeys.supported()}
        userName={userName}
        onSuccess={(answer) => {
          if (answer.type === "Reauthenticated") onDone(answer.reauth_until);
        }}
        extra={
          <button type="button" data-part="form.button" data-action="cancel" onClick={onCancel}>
            {t("remote-cancel")}
          </button>
        }
      />
    </section>
  );
}

/** Seconds a fresh re-authentication is still valid (0 = ask again). */
export function reauthSecondsLeft(status: Pick<AuthStatus, "reauth_until">): number {
  return status.reauth_until === null ? 0 : secondsUntil(status.reauth_until);
}
