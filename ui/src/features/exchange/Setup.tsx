/**
 * Exchange in account setup and account settings (ADR 0072; mount points
 * for `widgets/AccountSetup.tsx` / `widgets/AccountSettings.tsx`):
 *
 * - `ExchangeAccountType` (address step): find the settings automatically,
 *   or say it is Microsoft 365 / Exchange Online (Graph) or an Exchange
 *   server of the organization (EWS, with its host) — then
 *   `exchangeCandidate()` replaces discovery.
 * - `ExchangeServerReview` (servers step, for an Exchange proposal): what
 *   Autodiscover found (engine, server, EWS address), a warning when it
 *   sent the setup to a server outside the address's domain (the user must
 *   confirm before signing in there) or uses EWS against Exchange Online,
 *   and the engine choice.
 * - `ExchangeSignInFields`: how the account signs in — for Exchange
 *   servers as the signed-in Windows user (no password stored), another
 *   Windows account (`DOMAIN\user`, Negotiate / Kerberos / NTLM), a
 *   password (Basic over TLS) or Microsoft OAuth; Graph only with OAuth.
 * - `ExchangeOptionsFields` (account settings): polling, streaming (EWS),
 *   rules on the server, the EWS address.
 * - `ExchangeStatusPanel` (account settings): engine, server version, how
 *   changes arrive, last sync, throttling, folders, queued changes,
 *   warnings.
 */
import { useId } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  AuthConfig,
  DiscoveredAccount,
  ExchangeEngine,
  ExchangeOptions,
  SspiPackage,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { formatInstant } from "../pim/fmt";
import { localZone } from "../pim/time";
import { Problem } from "./common";
import {
  exchangeHost,
  exchangeSignInOf,
  exchangeSignIns,
  exchangeWarnings,
  redirectedHost,
  withEngine,
  withExchangeSignIn,
  type ExchangeSignIn,
} from "./model";
import { useExchangeStatus } from "./queries";
import { ENGINES } from "./messages";

/** What the user says the account is on the address step. */
export type ExchangeSetupChoice = "auto" | ExchangeEngine;

const CHOICES: readonly ExchangeSetupChoice[] = ["auto", "graph", "ews"];
const PACKAGES: readonly SspiPackage[] = ["negotiate", "kerberos", "ntlm"];

/** Mount point `ExchangeAccountType` (address step). */
export function ExchangeAccountType({
  choice,
  host,
  onChange,
}: {
  choice: ExchangeSetupChoice;
  /** The Exchange server's host (EWS). */
  host: string;
  onChange: (choice: ExchangeSetupChoice, host: string) => void;
}) {
  const t = useT();
  const ids = { group: useId(), host: useId(), hint: useId() };
  return (
    <fieldset data-part="exchange.setup.type" data-choice={choice}>
      <legend data-part="form.section.title">{t("exchange-setup-type")}</legend>
      {CHOICES.map((c) => (
        <label key={c} data-part="form.check" data-field={`type-${c}`}>
          <input
            type="radio"
            name={ids.group}
            checked={choice === c}
            onChange={() => {
              onChange(c, host);
            }}
          />
          <span data-part="form.check.label">{t(`exchange-setup-type-${c}`)}</span>
        </label>
      ))}
      {choice === "ews" && (
        <div data-part="form.field" data-field="exchange-host">
          <label data-part="form.label" htmlFor={ids.host}>
            {t("exchange-setup-host")}
          </label>
          <input
            id={ids.host}
            data-part="form.input"
            value={host}
            autoComplete="off"
            aria-describedby={ids.hint}
            onChange={(e) => {
              onChange(choice, e.target.value.trim());
            }}
          />
          <p id={ids.hint} data-part="form.hint">
            {t("exchange-setup-host-hint")}
          </p>
        </div>
      )}
    </fieldset>
  );
}

/** Whether the user must confirm the proposal's server before signing in. */
export function needsRedirectConfirmation(email: string, candidate: DiscoveredAccount): boolean {
  return redirectedHost(email, candidate) !== null;
}

/** Mount point `ExchangeServerReview` (servers step, Exchange proposals only). */
export function ExchangeServerReview({
  email,
  candidate,
  onChange,
  confirmed,
  onConfirm,
}: {
  email: string;
  candidate: DiscoveredAccount;
  onChange: (candidate: DiscoveredAccount) => void;
  /** The user trusts a redirected server. */
  confirmed: boolean;
  onConfirm: (confirmed: boolean) => void;
}) {
  const t = useT();
  const ids = { engine: useId(), url: useId(), confirm: useId() };
  const incoming = candidate.incoming;
  if (incoming.protocol !== "exchange") return null;
  const engine = incoming.options.engine;
  const redirected = redirectedHost(email, candidate);
  const warnings = exchangeWarnings(incoming);
  const domain = email.split("@")[1] ?? "";
  const setUrl = (url: string) => {
    onChange({
      ...candidate,
      incoming: { ...incoming, options: { ...incoming.options, url: url.trim() || null } },
    });
  };
  const setHost = (host: string) => {
    onChange({ ...candidate, incoming: { ...incoming, server: { ...incoming.server, host } } });
  };
  return (
    <section data-part="exchange.setup.review" data-engine={engine}>
      <h3 data-part="exchange.setup.title">{t("common-microsoft-exchange")}</h3>
      <dl data-part="exchange.setup.details">
        <dt data-part="exchange.setup.term">{t("exchange-setup-source")}</dt>
        <dd data-part="exchange.setup.value">{t(`setup-source-${candidate.source}`)}</dd>
        <dt data-part="exchange.setup.term">{t("exchange-setup-engine")}</dt>
        <dd data-part="exchange.setup.value">{t(`exchange-engine-${engine}`)}</dd>
        <dt data-part="exchange.setup.term">{t("common-server")}</dt>
        <dd data-part="exchange.setup.value">{exchangeHost(candidate)}</dd>
        {engine === "ews" && incoming.options.url && (
          <>
            <dt data-part="exchange.setup.term">{t("exchange-setup-url")}</dt>
            <dd data-part="exchange.setup.value">{incoming.options.url}</dd>
          </>
        )}
      </dl>
      {redirected !== null && (
        <div data-part="exchange.setup.redirect" role="alert">
          <p data-part="exchange.setup.warning" data-warning="redirect">
            {t("exchange-setup-redirect", { host: redirected, domain })}
          </p>
          <label data-part="form.check" data-field="trust-redirect">
            <input
              id={ids.confirm}
              data-part="form.checkbox"
              type="checkbox"
              checked={confirmed}
              onChange={(e) => {
                onConfirm(e.target.checked);
              }}
            />
            <span data-part="form.check.label">
              {t("exchange-setup-redirect-confirm", { host: redirected })}
            </span>
          </label>
        </div>
      )}
      {warnings.map((w) => (
        <p key={w} data-part="exchange.setup.warning" data-warning={w} role="alert">
          {t(`exchange-warning-${w}`)}
        </p>
      ))}
      <fieldset data-part="form.section" data-section="engine">
        <legend data-part="form.section.title">{t("exchange-setup-engine-choice")}</legend>
        {ENGINES.map((e) => (
          <label key={e} data-part="form.check" data-field={`engine-${e}`}>
            <input
              type="radio"
              name={ids.engine}
              checked={engine === e}
              onChange={() => {
                onChange(withEngine(candidate, e, email));
              }}
            />
            <span data-part="form.check.label">{t(`exchange-engine-${e}`)}</span>
          </label>
        ))}
      </fieldset>
      {engine === "ews" && (
        <>
          <div data-part="form.field" data-field="exchange-server">
            <label data-part="form.label" htmlFor={`${ids.url}-host`}>
              {t("exchange-setup-host")}
            </label>
            <input
              id={`${ids.url}-host`}
              data-part="form.input"
              value={incoming.server.host}
              onChange={(e) => {
                setHost(e.target.value.trim());
              }}
            />
          </div>
          <EwsUrlField
            id={ids.url}
            value={incoming.options.url ?? ""}
            hint={t("exchange-setup-url-hint", { host: incoming.server.host || "…" })}
            onChange={setUrl}
          />
        </>
      )}
    </section>
  );
}

/** Mount point `ExchangeSignInFields`: the sign-in of an Exchange account. */
export function ExchangeSignInFields({
  email,
  engine,
  auth,
  onChange,
  password,
  onPasswordChange,
}: {
  email: string;
  engine: ExchangeEngine;
  auth: AuthConfig;
  onChange: (auth: AuthConfig) => void;
  password: string;
  onPasswordChange: (password: string) => void;
}) {
  const t = useT();
  const ids = { group: useId(), login: useId(), pkg: useId(), password: useId(), hint: useId() };
  const choices = exchangeSignIns(engine);
  const current = exchangeSignInOf(auth);
  const signIn: ExchangeSignIn = choices.includes(current) ? current : (choices[0] ?? "oauth2");
  const login =
    auth.method === "windows"
      ? (auth.username ?? "")
      : auth.method === "password" || auth.method === "oauth2"
        ? auth.username
        : "";
  const pkg: SspiPackage = auth.method === "windows" ? (auth.package ?? "negotiate") : "negotiate";
  const setLogin = (username: string) => {
    if (auth.method === "windows") onChange({ ...auth, username: username || null });
    else if (auth.method === "password") onChange({ ...auth, username });
  };
  return (
    <fieldset data-part="exchange.setup.sign-in" data-sign-in={signIn}>
      <legend data-part="form.section.title">{t("common-sign-in")}</legend>
      {engine === "graph" && <p data-part="form.hint">{t("exchange-sign-in-graph-hint")}</p>}
      {choices.length > 1 &&
        choices.map((c) => (
          <label key={c} data-part="form.check" data-field={`sign-in-${c}`}>
            <input
              type="radio"
              name={ids.group}
              checked={signIn === c}
              onChange={() => {
                onChange(withExchangeSignIn(auth, c, email));
              }}
            />
            <span data-part="form.check.label">{t(`exchange-sign-in-${c}`)}</span>
          </label>
        ))}
      {signIn === "windows-current" && (
        <p data-part="form.hint">{t("exchange-sign-in-windows-current-hint")}</p>
      )}
      {(signIn === "windows-account" || signIn === "password") && (
        <div data-part="form.field" data-field="login">
          <label data-part="form.label" htmlFor={ids.login}>
            {t(
              signIn === "windows-account"
                ? "exchange-sign-in-windows-login"
                : "exchange-sign-in-login",
            )}
          </label>
          <input
            id={ids.login}
            data-part="form.input"
            value={login}
            autoComplete="username"
            onChange={(e) => {
              setLogin(e.target.value.trim());
            }}
          />
        </div>
      )}
      {(signIn === "windows-current" || signIn === "windows-account") && (
        <div data-part="form.field" data-field="package">
          <label data-part="form.label" htmlFor={ids.pkg}>
            {t("exchange-sign-in-package")}
          </label>
          <select
            id={ids.pkg}
            data-part="form.select"
            value={pkg}
            onChange={(e) => {
              if (auth.method !== "windows") return;
              onChange({
                ...auth,
                package: e.target.value === "negotiate" ? null : (e.target.value as SspiPackage),
              });
            }}
          >
            {PACKAGES.map((p) => (
              <option key={p} value={p}>
                {t(`exchange-sspi-${p}`)}
              </option>
            ))}
          </select>
        </div>
      )}
      {(signIn === "windows-account" || signIn === "password") && (
        <div data-part="form.field" data-field="password">
          <label data-part="form.label" htmlFor={ids.password}>
            {t("common-password")}
          </label>
          <input
            id={ids.password}
            data-part="form.input"
            type="password"
            value={password}
            autoComplete="current-password"
            aria-describedby={ids.hint}
            onChange={(e) => {
              onPasswordChange(e.target.value);
            }}
          />
          <p id={ids.hint} data-part="form.hint">
            {t(
              signIn === "password"
                ? "exchange-sign-in-basic-hint"
                : "exchange-sign-in-keychain-hint",
            )}
          </p>
        </div>
      )}
      {signIn === "oauth2" && <p data-part="form.hint">{t("exchange-sign-in-oauth-hint")}</p>}
    </fieldset>
  );
}

/** Mount point `ExchangeOptionsFields` (account settings of an Exchange account). */
export function ExchangeOptionsFields({
  options,
  onChange,
}: {
  options: ExchangeOptions;
  onChange: (options: ExchangeOptions) => void;
}) {
  const t = useT();
  const ids = { poll: useId(), max: useId(), url: useId() };
  const number = (text: string) => (/^\d+$/.test(text.trim()) ? Number(text.trim()) : null);
  return (
    <fieldset data-part="form.section" data-section="exchange">
      <legend data-part="form.section.title">{t("common-exchange")}</legend>
      <p data-part="form.hint">{t(`exchange-engine-${options.engine}`)}</p>
      <div data-part="form.field" data-field="poll-interval">
        <label data-part="form.label" htmlFor={ids.poll}>
          {t("exchange-options-poll")}
        </label>
        <input
          id={ids.poll}
          data-part="form.input"
          data-width="short"
          type="number"
          inputMode="numeric"
          min={10}
          aria-describedby={`${ids.poll}-hint`}
          value={options.poll_interval_secs ?? ""}
          onChange={(e) => {
            onChange({ ...options, poll_interval_secs: number(e.target.value) });
          }}
        />
        <p id={`${ids.poll}-hint`} data-part="form.hint">
          {t("exchange-options-default", { seconds: 30 })}
        </p>
      </div>
      <div data-part="form.field" data-field="max-poll-interval">
        <label data-part="form.label" htmlFor={ids.max}>
          {t("exchange-options-max-poll")}
        </label>
        <input
          id={ids.max}
          data-part="form.input"
          data-width="short"
          type="number"
          inputMode="numeric"
          min={10}
          aria-describedby={`${ids.max}-hint`}
          value={options.max_poll_interval_secs ?? ""}
          onChange={(e) => {
            onChange({ ...options, max_poll_interval_secs: number(e.target.value) });
          }}
        />
        <p id={`${ids.max}-hint`} data-part="form.hint">
          {t("exchange-options-default", { seconds: 300 })}
        </p>
      </div>
      {options.engine === "ews" && (
        <>
          <EwsUrlField
            id={ids.url}
            value={options.url ?? ""}
            onChange={(url) => {
              onChange({ ...options, url: url.trim() || null });
            }}
          />
          <label data-part="form.check" data-field="streaming">
            <input
              data-part="form.checkbox"
              type="checkbox"
              checked={options.streaming !== false}
              onChange={(e) => {
                onChange({ ...options, streaming: e.target.checked ? null : false });
              }}
            />
            <span data-part="form.check.label">{t("exchange-options-streaming")}</span>
          </label>
        </>
      )}
      <label data-part="form.check" data-field="exchange-server-rules">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={options.server_rules !== false}
          onChange={(e) => {
            onChange({ ...options, server_rules: e.target.checked ? null : false });
          }}
        />
        <span data-part="form.check.label">{t("exchange-rules-server-switch")}</span>
      </label>
    </fieldset>
  );
}

/** Mount point `ExchangeStatusPanel` (account settings). Nothing for other accounts. */
export function ExchangeStatusPanel({ accountId }: { accountId: string }) {
  const t = useT();
  const format = useDateFormat();
  const status = useExchangeStatus(accountId);
  const tz = localZone();
  const when = (utc: string) =>
    formatInstant(format, utc, tz, { dateStyle: "medium", timeStyle: "short" });
  if (status.isError) return <Problem message={errorMessageOf(status.error)} />;
  const s = status.data;
  if (!s) return null;
  return (
    <section data-part="exchange.status" aria-label={t("exchange-status-label")}>
      <dl data-part="exchange.status.details">
        <dt data-part="exchange.status.term">{t("exchange-setup-engine")}</dt>
        <dd data-part="exchange.status.value">{t(`exchange-engine-${s.engine}`)}</dd>
        {s.server_version && (
          <>
            <dt data-part="exchange.status.term">{t("exchange-status-version")}</dt>
            <dd data-part="exchange.status.value">{s.server_version}</dd>
          </>
        )}
        <dt data-part="exchange.status.term">{t("exchange-status-push")}</dt>
        <dd data-part="exchange.status.value">{t(`exchange-push-${s.push}`)}</dd>
        <dt data-part="exchange.status.term">{t("exchange-status-last-sync")}</dt>
        <dd data-part="exchange.status.value">
          {s.last_sync_at ? when(s.last_sync_at) : t("common-never-inline")}
        </dd>
        <dt data-part="exchange.status.term">{t("exchange-status-folders")}</dt>
        <dd data-part="exchange.status.value">
          {t("exchange-status-folders-value", { synced: s.folders_synced, total: s.folders_total })}
        </dd>
        <dt data-part="exchange.status.term">{t("exchange-status-pending")}</dt>
        <dd data-part="exchange.status.value">{s.pending_ops}</dd>
        {s.throttled_until && (
          <>
            <dt data-part="exchange.status.term">{t("exchange-status-throttled")}</dt>
            <dd data-part="exchange.status.value">{when(s.throttled_until)}</dd>
          </>
        )}
      </dl>
      {s.warnings.length > 0 && (
        <ul data-part="exchange.status.warnings">
          {s.warnings.map((w) => (
            <li key={w} data-part="exchange.status.warning" data-warning={w} role="alert">
              {t(`exchange-warning-${w}`)}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** The EWS URL field (empty: Autodiscover finds it), with an optional hint. */
function EwsUrlField({
  id,
  value,
  hint,
  onChange,
}: {
  id: string;
  value: string;
  hint?: string;
  onChange: (url: string) => void;
}) {
  const t = useT();
  return (
    <div data-part="form.field" data-field="exchange-url">
      <label data-part="form.label" htmlFor={id}>
        {t("exchange-setup-url")}
      </label>
      <input
        id={id}
        data-part="form.input"
        type="url"
        value={value}
        aria-describedby={hint === undefined ? undefined : `${id}-hint`}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      />
      {hint !== undefined && (
        <p id={`${id}-hint`} data-part="form.hint">
          {hint}
        </p>
      )}
    </div>
  );
}
