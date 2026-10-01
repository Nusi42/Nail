import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageId, errorMessageIdOf } from "../api/client";
import type {
  AccountConfig,
  AccountTestResult,
  DiscoveredAccount,
  EndpointTestResult,
  ExchangeDiscoveryInfo,
  SecretInput,
} from "../api/generated";
import {
  ExchangeAccountType,
  ExchangeServerReview,
  ExchangeSignInFields,
  exchangeCandidate,
  exchangeCredentialsReady,
  exchangeSecretsFor,
  needsRedirectConfirmation,
  type ExchangeSetupChoice,
} from "../features/exchange";
import { useLabel, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { isEmail } from "../state/addresses";
import { queryKeys, useAccountsConfig, useClient } from "../state/queries";
import { useUi } from "../state/uiStore";
import {
  IncomingFields,
  OutgoingFields,
  SelectField,
  TextField,
  parseSignIn,
  signInChoices,
  signInOf,
  signInValue,
  withSignIn,
  type SignIn,
} from "./forms";

/** Name of the layout window that holds the wizard. */
export const SETUP_WINDOW = "account-setup";

export type SetupStep = "address" | "servers" | "test" | "done";

/** Settings to start from when discovery finds nothing. */
export function manualCandidate(email: string): DiscoveredAccount {
  const domain = email.split("@")[1] ?? "";
  const auth = { method: "password" as const, username: email, secret: null, mechanism: null };
  return {
    source: "guess",
    provider_name: null,
    incoming: {
      protocol: "imap",
      server: { host: domain ? `imap.${domain}` : "", port: 993, tls: "implicit", auth },
      options: { poll_interval_secs: null, compress: null, max_connections: null },
    },
    outgoing: {
      protocol: "smtp",
      server: { host: domain ? `smtp.${domain}` : "", port: 465, tls: "implicit", auth },
      options: { dsn: null },
    },
    warnings: [],
  };
}

/**
 * The proposal to preselect: the best-ranked one without warnings (the core
 * ranks unauthenticated answers last), else the first.
 */
export function preferredCandidate(candidates: DiscoveredAccount[]): number {
  const trusted = candidates.findIndex((c) => c.warnings.length === 0);
  return trusted === -1 ? 0 : trusted;
}

/** A new account from a discovery proposal. */
export function configFrom(
  candidate: DiscoveredAccount,
  email: string,
  name: string,
): AccountConfig {
  return {
    id: "",
    name: name.trim() || email,
    email,
    enabled: true,
    incoming: candidate.incoming,
    outgoing: candidate.outgoing,
    identities: [
      {
        id: email,
        account_id: "",
        email,
        name: name.trim() || null,
        reply_to: null,
        bcc: [],
        signature: null,
        composer_mode: null,
        posting: null,
        mail_style: null,
      },
    ],
    sync: { bodies: null, folders: [] },
    save_sent_copy: null,
  };
}

/** The account's sign-in method (taken from the incoming server). */
export function accountSignIn(config: AccountConfig): SignIn {
  return signInOf(config.incoming.server.auth);
}

/** Applies one sign-in method to both servers. */
export function withAccountSignIn(config: AccountConfig, signIn: SignIn): AccountConfig {
  return {
    ...config,
    incoming: {
      ...config.incoming,
      server: { ...config.incoming.server, auth: withSignIn(config.incoming.server.auth, signIn) },
    } as AccountConfig["incoming"],
    outgoing: config.outgoing && {
      ...config.outgoing,
      server: { ...config.outgoing.server, auth: withSignIn(config.outgoing.server.auth, signIn) },
    },
  };
}

/** Secrets to hand to the core for a password or a finished OAuth flow. */
export function secretsFor(
  config: AccountConfig,
  password: string,
  flowId: string | null,
): SecretInput[] {
  const signIn = accountSignIn(config);
  const slots: SecretInput["endpoint"][] = config.outgoing
    ? ["incoming", "outgoing"]
    : ["incoming"];
  if (signIn.kind === "password" && password !== "") {
    return slots.map((endpoint) => ({ endpoint, value: { kind: "password", password } }));
  }
  if (signIn.kind === "oauth2" && flowId !== null) {
    return slots.map((endpoint) => ({ endpoint, value: { kind: "oauth2", flow_id: flowId } }));
  }
  return [];
}

function EndpointResult({ slot, result }: { slot: string; result: EndpointTestResult }) {
  const t = useT();
  const ok = result.error === null;
  return (
    <li data-part="account-setup.result" data-slot={slot} data-state={ok ? "ok" : "error"}>
      <span data-part="account-setup.result.title">{t(`server-${slot}`)}</span>
      <span data-part="account-setup.result.status">
        {ok ? t("setup-test-ok") : t(errorMessageId(result.error ?? "internal"))}
      </span>
      {result.capabilities.length > 0 && (
        <span data-part="account-setup.result.capabilities">
          {t("setup-test-capabilities", { capabilities: result.capabilities.join(", ") })}
        </span>
      )}
    </li>
  );
}

interface OAuthState {
  flowId: string;
  url: string;
  opened: boolean;
}

/**
 * Account setup: address → discovered settings to confirm or edit → sign-in
 * (password or OAuth) → connection test → save.
 */
export function AccountSetup({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const queryClient = useQueryClient();
  const accountsConfig = useAccountsConfig();
  const oauthResults = useUi((s) => s.oauthResults);
  const selectAccount = useUi((s) => s.selectAccount);
  const closeWindow = useUi((s) => s.closeWindow);

  const [step, setStep] = useState<SetupStep>("address");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [candidates, setCandidates] = useState<DiscoveredAccount[]>([]);
  const [chosen, setChosen] = useState(0);
  const [config, setConfig] = useState<AccountConfig | null>(null);
  const [editing, setEditing] = useState(false);
  const [password, setPassword] = useState("");
  const [oauth, setOAuth] = useState<OAuthState | null>(null);
  const [result, setResult] = useState<AccountTestResult | null>(null);
  const [saved, setSaved] = useState<AccountConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // M3c: Exchange (the kind of account, Autodiscover's question, the
  // trusted redirect).
  const [exchangeChoice, setExchangeChoice] = useState<ExchangeSetupChoice>("auto");
  const [exchangeHost, setExchangeHost] = useState("");
  const [exchangeInfo, setExchangeInfo] = useState<ExchangeDiscoveryInfo | null>(null);
  const [trustedHosts, setTrustedHosts] = useState<string[]>([]);
  const [trusted, setTrusted] = useState(false);

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const providerName = (id: string) =>
    id === "google" || id === "microsoft" ? t(`oauth-provider-${id}`) : id;
  const oauthOutcome = oauth ? oauthResults[oauth.flowId] : undefined;
  const oauthDone = oauth !== null && oauthOutcome === null;
  const signIn = config ? accountSignIn(config) : null;
  const providers = (accountsConfig.data?.oauth_providers ?? []).map((p) => p.id);
  const flowId = oauthDone ? oauth.flowId : null;
  const exchangeIncoming = config?.incoming.protocol === "exchange" ? config.incoming : null;
  const secrets = config
    ? exchangeIncoming
      ? exchangeSecretsFor(config, password, flowId)
      : secretsFor(config, password, flowId)
    : [];
  const credentialsReady = exchangeIncoming
    ? exchangeCredentialsReady(exchangeIncoming.server.auth, password, oauthDone)
    : signIn?.kind === "none" || (signIn?.kind === "password" ? password !== "" : oauthDone);
  const chosenCandidate = candidates[chosen] ?? null;
  const redirectUnconfirmed =
    exchangeIncoming !== null &&
    chosenCandidate !== null &&
    needsRedirectConfirmation(email.trim(), chosenCandidate) &&
    !trusted;

  /** Shows proposals on the servers step. */
  const propose = (address: string, found: DiscoveredAccount[]) => {
    setCandidates(found);
    const index = preferredCandidate(found);
    setChosen(index);
    const first = found[index];
    setConfig(configFrom(first ?? manualCandidate(address), address, name));
    // Unverified settings: show the servers, not just a summary.
    const exchange = first?.incoming.protocol === "exchange";
    setEditing(!exchange && (first === undefined || first.warnings.length > 0));
    setOAuth(null);
    setTrusted(false);
    setStep("servers");
  };

  const lookup = (address: string, hosts: string[], windowsSignIn: boolean) =>
    run(async () => {
      const found = await client.request(
        {
          type: "DiscoverAccount",
          email: address,
          trusted_hosts: hosts,
          windows_sign_in: windowsSignIn,
        },
        "Discovered",
      );
      setExchangeInfo(found.exchange ?? null);
      propose(address, found.candidates);
    });

  const discover = (event: FormEvent) => {
    event.preventDefault();
    const address = email.trim();
    if (!isEmail(address)) {
      setError("setup-error-email");
      return;
    }
    // M3c: the user said what the account is: no lookup.
    if (exchangeChoice !== "auto") {
      setExchangeInfo(null);
      propose(address, [exchangeCandidate(address, exchangeChoice, exchangeHost || null)]);
      return;
    }
    setTrustedHosts([]);
    void lookup(address, [], false);
  };

  /** Autodiscover asked: trust a host / sign in as the Windows user, then look again. */
  const answerPrompt = () => {
    const prompt = exchangeInfo?.prompt;
    if (!prompt) return;
    if (prompt.kind === "confirm-redirect") {
      const hosts = [...trustedHosts, prompt.host];
      setTrustedHosts(hosts);
      void lookup(email.trim(), hosts, false);
    } else {
      void lookup(email.trim(), trustedHosts, true);
    }
  };

  const choose = (index: number) => {
    const candidate = candidates[index];
    if (!candidate) return;
    setChosen(index);
    setConfig(configFrom(candidate, email.trim(), name));
    setOAuth(null);
    setTrusted(false);
  };

  /** The Exchange review changed the proposal (engine, server, URL). */
  const changeCandidate = (candidate: DiscoveredAccount) => {
    setCandidates(candidates.map((c, i) => (i === chosen ? candidate : c)));
    setConfig(configFrom(candidate, email.trim(), name));
    setOAuth(null);
  };

  const startOAuth = () =>
    run(async () => {
      if (signIn?.kind !== "oauth2") return;
      // M3c: Microsoft issues tokens per resource (Graph or EWS).
      const protocol = exchangeIncoming
        ? exchangeIncoming.options.engine === "ews"
          ? ("ews" as const)
          : ("graph" as const)
        : null;
      const started = await client.request(
        { type: "StartOAuth", provider: signIn.provider, login_hint: email.trim(), protocol },
        "OAuthStarted",
      );
      setOAuth({ flowId: started.flow_id, url: started.authorize_url, opened: started.opened });
    });

  const cancelOAuth = () =>
    run(async () => {
      if (!oauth) return;
      await client.request({ type: "CancelOAuth", flow_id: oauth.flowId }, "Done");
      setOAuth(null);
    });

  const test = (event: FormEvent) => {
    event.preventDefault();
    if (!config) return;
    void run(async () => {
      const tested = await client.request(
        { type: "TestAccount", config, secrets },
        "AccountTested",
      );
      setResult(tested.result);
      setStep("test");
    });
  };

  const save = () =>
    run(async () => {
      if (!config) return;
      const { config: stored } = await client.request(
        { type: "AddAccount", config, secrets },
        "AccountSaved",
      );
      setSaved(stored);
      setStep("done");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.accounts }),
        queryClient.invalidateQueries({ queryKey: queryKeys.accountsConfig }),
      ]);
    });

  const finish = () => {
    if (saved) selectAccount(saved.id);
    closeWindow(SETUP_WINDOW);
  };

  const testOk =
    result !== null && result.incoming.error === null && (result.outgoing?.error ?? null) === null;

  return (
    <section
      data-part="account-setup"
      data-node-id={node.id ?? undefined}
      data-step={step}
      aria-label={node.label ? label(node.label) : t("setup-label")}
      aria-busy={busy}
    >
      <ol data-part="account-setup.steps" aria-label={t("setup-steps")}>
        {(["address", "servers", "test", "done"] as const).map((s) => (
          <li
            key={s}
            data-part="account-setup.step"
            data-state={s === step ? "selected" : undefined}
            aria-current={s === step ? "step" : undefined}
          >
            {t(`setup-step-${s}`)}
          </li>
        ))}
      </ol>

      {error && (
        <div data-part="account-setup.error" role="alert">
          {t(error)}
        </div>
      )}

      {step === "address" && (
        <form data-part="account-setup.form" noValidate onSubmit={discover}>
          <p data-part="account-setup.intro">{t("setup-intro")}</p>
          <TextField
            labelId="setup-name"
            name="name"
            value={name}
            onChange={setName}
            autoComplete="name"
          />
          <TextField
            labelId="setup-email"
            name="email"
            type="email"
            value={email}
            required
            autoComplete="email"
            onChange={setEmail}
          />
          <ExchangeAccountType
            choice={exchangeChoice}
            host={exchangeHost}
            onChange={(choice, host) => {
              setExchangeChoice(choice);
              setExchangeHost(host);
            }}
          />
          <div data-part="account-setup.actions">
            <button
              type="submit"
              data-part="form.button"
              data-action="next"
              data-variant="primary"
              disabled={busy}
            >
              {t(busy ? "setup-discovering" : "setup-continue")}
            </button>
          </div>
        </form>
      )}

      {step === "servers" && config && (
        <form data-part="account-setup.form" noValidate onSubmit={test}>
          {exchangeInfo?.prompt && (
            <div
              data-part="account-setup.warning"
              data-warning={exchangeInfo.prompt.kind}
              role="alert"
            >
              <p data-part="account-setup.notice">
                {t(exchangeInfo.prompt.localized.id, exchangeInfo.prompt.localized.args)}
              </p>
              <button
                type="button"
                data-part="form.button"
                data-action="lookup-again"
                disabled={busy}
                onClick={answerPrompt}
              >
                {t(
                  exchangeInfo.prompt.kind === "confirm-redirect"
                    ? "setup-exchange-trust"
                    : "setup-exchange-windows-sign-in",
                  { host: exchangeInfo.prompt.host },
                )}
              </button>
            </div>
          )}
          {candidates.length === 0 ? (
            <p data-part="account-setup.notice">{t("setup-nothing-found")}</p>
          ) : (
            <fieldset data-part="account-setup.candidates">
              <legend data-part="form.section.title">{t("setup-found")}</legend>
              {candidates.map((c, index) => (
                <label key={index} data-part="account-setup.candidate">
                  <input
                    type="radio"
                    data-part="account-setup.candidate.radio"
                    name="candidate"
                    checked={index === chosen}
                    onChange={() => choose(index)}
                  />
                  <span data-part="account-setup.candidate.source">
                    {/* A name from an unverified answer could be anything. */}
                    {(c.warnings.length === 0 ? c.provider_name : null) ??
                      t(`setup-source-${c.source}`)}
                  </span>
                  <span data-part="account-setup.candidate.servers">
                    {t("setup-candidate-servers", {
                      incoming: `${c.incoming.server.host}:${c.incoming.server.port}`,
                      outgoing: c.outgoing
                        ? `${c.outgoing.server.host}:${c.outgoing.server.port}`
                        : "-",
                    })}
                  </span>
                  {c.warnings.map((warning) => (
                    <span
                      key={warning}
                      data-part="account-setup.candidate.warning"
                      data-warning={warning}
                    >
                      {t(`setup-warning-${warning}`)}
                    </span>
                  ))}
                </label>
              ))}
            </fieldset>
          )}
          {(candidates[chosen]?.warnings.length ?? 0) > 0 && (
            <p data-part="account-setup.warning" role="alert">
              {t("setup-warning-check")}
            </p>
          )}
          {exchangeIncoming && chosenCandidate && (
            <ExchangeServerReview
              email={email.trim()}
              candidate={chosenCandidate}
              onChange={changeCandidate}
              confirmed={trusted}
              onConfirm={setTrusted}
            />
          )}
          {!editing && !exchangeIncoming && (
            <button
              type="button"
              data-part="form.button"
              data-action="edit-servers"
              onClick={() => setEditing(true)}
            >
              {t("setup-edit-servers")}
            </button>
          )}
          {editing && !exchangeIncoming && (
            <>
              <IncomingFields
                incoming={config.incoming}
                onChange={(incoming) => setConfig({ ...config, incoming })}
              />
              {config.outgoing && (
                <OutgoingFields
                  outgoing={config.outgoing}
                  onChange={(outgoing) => setConfig({ ...config, outgoing })}
                />
              )}
            </>
          )}
          {exchangeIncoming ? (
            <ExchangeSignInFields
              email={email.trim()}
              engine={exchangeIncoming.options.engine}
              auth={exchangeIncoming.server.auth}
              onChange={(auth) => {
                setConfig({
                  ...config,
                  incoming: {
                    ...exchangeIncoming,
                    server: { ...exchangeIncoming.server, auth },
                  },
                });
                setOAuth(null);
              }}
              password={password}
              onPasswordChange={setPassword}
            />
          ) : (
            <SelectField
              labelId="setup-sign-in"
              name="sign-in"
              value={signIn ? signInValue(signIn) : "password"}
              choices={signInChoices(providers)}
              onChange={(value) => {
                setConfig(withAccountSignIn(config, parseSignIn(value)));
                setOAuth(null);
              }}
            />
          )}
          {signIn?.kind === "password" && !exchangeIncoming && (
            <TextField
              labelId="setup-password"
              name="password"
              type="password"
              value={password}
              autoComplete="current-password"
              hintId="setup-password-hint"
              onChange={setPassword}
            />
          )}
          {signIn?.kind === "oauth2" && (
            <div
              data-part="account-setup.oauth"
              data-state={oauthDone ? "done" : oauth ? "waiting" : undefined}
            >
              {!oauth && (
                <button
                  type="button"
                  data-part="form.button"
                  data-action="oauth-start"
                  disabled={busy}
                  onClick={() => void startOAuth()}
                >
                  {t("setup-oauth-start", { provider: providerName(signIn.provider) })}
                </button>
              )}
              {oauth && oauthOutcome === undefined && (
                <>
                  <span data-part="account-setup.oauth.status" role="status">
                    {t(oauth.opened ? "setup-oauth-waiting" : "setup-oauth-open")}
                  </span>
                  {!oauth.opened && (
                    <a
                      data-part="account-setup.oauth.link"
                      href={oauth.url}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {t("setup-oauth-link")}
                    </a>
                  )}
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="oauth-cancel"
                    onClick={() => void cancelOAuth()}
                  >
                    {t("setup-oauth-cancel")}
                  </button>
                </>
              )}
              {oauthDone && (
                <span data-part="account-setup.oauth.status" role="status">
                  {t("setup-oauth-done")}
                </span>
              )}
              {oauth && oauthOutcome !== undefined && oauthOutcome !== null && (
                <>
                  <span data-part="account-setup.oauth.status" role="alert">
                    {t("setup-oauth-failed", { reason: t(errorMessageId(oauthOutcome)) })}
                  </span>
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="oauth-start"
                    onClick={() => setOAuth(null)}
                  >
                    {t("setup-oauth-retry")}
                  </button>
                </>
              )}
            </div>
          )}
          <div data-part="account-setup.actions">
            <button
              type="button"
              data-part="form.button"
              data-action="back"
              onClick={() => setStep("address")}
            >
              {t("setup-back")}
            </button>
            <button
              type="submit"
              data-part="form.button"
              data-action="next"
              data-variant="primary"
              disabled={busy || !credentialsReady || redirectUnconfirmed}
            >
              {t(busy ? "setup-testing" : "setup-test")}
            </button>
          </div>
        </form>
      )}

      {step === "test" && result && (
        <div data-part="account-setup.form">
          <p data-part="account-setup.notice">
            {t(testOk ? "setup-test-passed" : "setup-test-failed")}
          </p>
          <ul data-part="account-setup.results">
            <EndpointResult slot="incoming" result={result.incoming} />
            {result.outgoing && <EndpointResult slot="outgoing" result={result.outgoing} />}
          </ul>
          <div data-part="account-setup.actions">
            <button
              type="button"
              data-part="form.button"
              data-action="back"
              onClick={() => setStep("servers")}
            >
              {t("setup-back")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="save"
              data-variant={testOk ? "primary" : undefined}
              disabled={busy}
              onClick={() => void save()}
            >
              {t(testOk ? "setup-save" : "setup-save-anyway")}
            </button>
          </div>
        </div>
      )}

      {step === "done" && saved && (
        <div data-part="account-setup.form">
          <p data-part="account-setup.notice" role="status">
            {t("setup-done", { name: saved.name })}
          </p>
          <div data-part="account-setup.actions">
            <button
              type="button"
              data-part="form.button"
              data-action="finish"
              data-variant="primary"
              onClick={finish}
            >
              {t("setup-finish")}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
