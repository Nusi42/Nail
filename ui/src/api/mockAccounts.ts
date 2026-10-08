/**
 * Account behaviour of the mock core: discovery, connection tests, OAuth
 * sign-in, account / virtual-account CRUD, identities and status.
 *
 * Deterministic rules for tests:
 * - Discovery: domains ending in `.invalid` find nothing, `data.discovery`
 *   answers known domains, any other domain gets `imap.<domain>` /
 *   `smtp.<domain>` guesses (source `autoconfig` for `example.*`).
 * - Tests: hosts containing `unreachable` fail with `network`, `badtls` with
 *   `tls`; the password `wrong` fails with `auth_failed`.
 */
import type {
  AccountConfig,
  DiscoveredAccount,
  EndpointTestResult,
  Identity,
  IncomingServer,
  OutgoingServer,
  Response,
  SecretInput,
  MessageId,
  ServerEndpoint,
  VirtualAccountConfig,
} from "./generated";
import { mockFolderId, NEW_ACCOUNT_FOLDERS, type MockData } from "./mockData";
import { errorResponse, virtualConfig, type MockCore } from "./mockMail";
import { ALL_ACCOUNTS } from "./contract";

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
const ID_RE = /^[a-z0-9][a-z0-9._-]{0,63}$/;

function guess(email: string, domain: string): DiscoveredAccount {
  const auth = { method: "password" as const, username: email, secret: null, mechanism: null };
  return {
    source: domain.startsWith("example.") ? "autoconfig" : "guess",
    provider_name: null,
    incoming: {
      protocol: "imap",
      server: { host: `imap.${domain}`, port: 993, tls: "implicit", auth },
      options: { poll_interval_secs: null, compress: null, max_connections: null },
    },
    outgoing: {
      protocol: "smtp",
      server: { host: `smtp.${domain}`, port: 465, tls: "implicit", auth },
      options: { dsn: null },
    },
    warnings: [],
  };
}

function withUsername<T extends IncomingServer | OutgoingServer>(server: T, email: string): T {
  const auth = server.server.auth;
  if (auth.method === "password" || auth.method === "oauth2") {
    return { ...server, server: { ...server.server, auth: { ...auth, username: email } } };
  }
  return server;
}

export function discover(d: MockData, email: string): Response {
  const address = email.trim();
  if (!EMAIL_RE.test(address)) return errorResponse("invalid_request", "not an email address");
  const domain = (address.split("@")[1] ?? "").toLowerCase();
  let candidates: DiscoveredAccount[];
  if (domain.endsWith(".invalid")) candidates = [];
  else if (Object.hasOwn(d.discovery, domain)) candidates = d.discovery[domain] ?? [];
  else candidates = [guess(address, domain)];
  return {
    type: "Discovered",
    email: address,
    candidates: candidates.map((c) => ({
      ...c,
      incoming: withUsername(c.incoming, address),
      outgoing: c.outgoing ? withUsername(c.outgoing, address) : null,
    })),
  };
}

const CAPABILITIES: Record<string, string[]> = {
  imap: ["IMAP4rev1", "IDLE", "CONDSTORE", "QRESYNC", "MOVE", "UIDPLUS", "SPECIAL-USE"],
  pop3: ["TOP", "UIDL", "SASL PLAIN", "STLS"],
  smtp: ["SIZE 35882577", "8BITMIME", "PIPELINING", "AUTH PLAIN LOGIN"],
};

function testEndpoint(
  core: MockCore,
  protocol: string,
  server: ServerEndpoint,
  secret: SecretInput | undefined,
): EndpointTestResult {
  const fail = (error: EndpointTestResult["error"], message: string): EndpointTestResult => ({
    error,
    message,
    capabilities: [],
  });
  if (server.host.includes("unreachable")) return fail("network", "connection refused");
  if (server.host.includes("badtls")) return fail("tls", "certificate not trusted");
  const value = secret?.value;
  if (value?.kind === "password" && value.password === "wrong") {
    return fail("auth_failed", "authentication failed");
  }
  if (value?.kind === "oauth2" && core.data.oauthFlows.get(value.flow_id) !== "done") {
    return fail("auth_failed", "the sign-in has not finished");
  }
  return { error: null, message: null, capabilities: CAPABILITIES[protocol] ?? [] };
}

export function testAccount(
  core: MockCore,
  config: AccountConfig,
  secrets: SecretInput[],
): Response {
  const secretFor = (slot: "incoming" | "outgoing") => secrets.find((s) => s.endpoint === slot);
  return {
    type: "AccountTested",
    result: {
      incoming: testEndpoint(
        core,
        config.incoming.protocol,
        config.incoming.server,
        secretFor("incoming"),
      ),
      outgoing: config.outgoing
        ? testEndpoint(
            core,
            "smtp",
            config.outgoing.server,
            secretFor("outgoing") ?? secretFor("incoming"),
          )
        : null,
    },
  };
}

export function startOAuth(core: MockCore, provider: string): Response {
  const known =
    provider === "google" ||
    provider === "microsoft" ||
    core.data.accountsConfig.oauth_providers.some((p) => p.id === provider);
  if (!known) return errorResponse("config", `no OAuth client configured for ${provider}`);
  const flowId = core.nextId("oauth-");
  const flows = core.data.oauthFlows;
  flows.set(flowId, "pending");
  core.later(() => {
    if (flows.get(flowId) !== "pending") return;
    const error = core.data.behaviour.oauthError;
    flows.set(flowId, error ? "failed" : "done");
    core.emit({
      type: "OAuthFinished",
      flow_id: flowId,
      error,
      message: error ? "mock sign-in failed" : null,
    });
  });
  return {
    type: "OAuthStarted",
    flow_id: flowId,
    authorize_url: `http://127.0.0.1:1/mock-oauth/${provider}?flow=${flowId}`,
    opened: true,
  };
}

export function cancelOAuth(core: MockCore, flowId: string): Response {
  if (core.data.oauthFlows.get(flowId) !== "pending") return { type: "Done" };
  core.data.oauthFlows.set(flowId, "cancelled");
  core.emit({ type: "OAuthFinished", flow_id: flowId, error: "cancelled", message: null });
  return { type: "Done" };
}

function idTaken(d: MockData, id: string): boolean {
  return (
    id === ALL_ACCOUNTS ||
    d.accountsConfig.accounts.some((a) => a.id === id) ||
    d.accountsConfig.virtual_accounts.some((v) => v.id === id)
  );
}

/** Derives a free id from a name or address, as the core does. */
function deriveId(d: MockData, source: string): string {
  const base =
    source
      .toLowerCase()
      .replace(/@.*$/, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 48) || "account";
  let id = base;
  for (let n = 2; idTaken(d, id); n++) id = `${base}-${String(n)}`;
  return id;
}

function validateAccount(config: AccountConfig): string | null {
  if (!EMAIL_RE.test(config.email)) return "invalid email address";
  if (config.incoming.server.host.trim() === "") return "incoming host missing";
  if (config.outgoing?.server.host.trim() === "") return "outgoing host missing";
  if (config.identities.some((i) => !EMAIL_RE.test(i.email))) return "invalid identity address";
  return null;
}

function defaultIdentity(config: AccountConfig): Identity[] {
  if (config.identities.length > 0) {
    return config.identities.map((i) => ({ ...i, id: i.id || i.email, account_id: config.id }));
  }
  return [
    {
      id: config.email,
      account_id: config.id,
      email: config.email,
      name: null,
      reply_to: null,
      bcc: [],
      signature: null,
      composer_mode: null,
      posting: null,
      mail_style: null,
    },
  ];
}

export function addAccount(core: MockCore, input: AccountConfig): Response {
  const d = core.data;
  const problem = validateAccount(input);
  if (problem) return errorResponse("invalid_request", problem);
  if (input.id !== "" && (idTaken(d, input.id) || !ID_RE.test(input.id))) {
    return errorResponse("conflict", `account id ${input.id} is taken or invalid`);
  }
  const id = input.id || deriveId(d, input.email.split("@")[1]?.split(".")[0] ?? input.email);
  const config: AccountConfig = { ...input, id, name: input.name || input.email };
  config.identities = defaultIdentity(config);
  d.accountsConfig.accounts.push(config);
  d.accounts.push({ id, name: config.name, email: config.email, kind: config.incoming.protocol });
  const specs =
    config.incoming.protocol === "pop3"
      ? NEW_ACCOUNT_FOLDERS.filter(
          (f) => f.role === "inbox" || f.role === "sent" || f.role === "trash",
        )
      : NEW_ACCOUNT_FOLDERS;
  for (const spec of specs) {
    d.folders.push({
      id: mockFolderId(id, spec.name),
      account_id: id,
      parent_id: null,
      name: spec.name,
      path: spec.name,
      role: spec.role,
      unread: 0,
      total: 0,
    });
  }
  const status = {
    account_id: id,
    state: "connecting" as const,
    since: new Date().toISOString(),
    error: null,
    message: null,
  };
  d.statuses.push(status);
  core.emit({ type: "AccountsChanged" });
  core.emit({ type: "AccountStatus", status });
  core.later(() => {
    const online = { ...status, state: "online" as const, since: new Date().toISOString() };
    d.statuses = d.statuses.map((s) => (s.account_id === id ? online : s));
    core.emit({ type: "AccountStatus", status: online });
  });
  return { type: "AccountSaved", config };
}

export function updateAccount(core: MockCore, input: AccountConfig): Response {
  const d = core.data;
  const index = d.accountsConfig.accounts.findIndex((a) => a.id === input.id);
  if (index < 0) return errorResponse("not_found", `account ${input.id} not found`);
  const problem = validateAccount(input);
  if (problem) return errorResponse("invalid_request", problem);
  const config = { ...input, identities: defaultIdentity(input) };
  d.accountsConfig.accounts[index] = config;
  d.accounts = d.accounts.map((a) =>
    a.id === config.id ? { ...a, name: config.name, email: config.email } : a,
  );
  const state = config.enabled ? "online" : "disabled";
  d.statuses = d.statuses.map((s) =>
    s.account_id === config.id ? { ...s, state, error: null, message: null } : s,
  );
  core.emit({ type: "AccountsChanged" });
  return { type: "AccountSaved", config };
}

export function removeAccount(core: MockCore, accountId: string): Response {
  const d = core.data;
  if (!d.accountsConfig.accounts.some((a) => a.id === accountId)) {
    return errorResponse(
      accountId === ALL_ACCOUNTS ? "invalid_request" : "not_found",
      `account ${accountId} cannot be removed`,
    );
  }
  d.accountsConfig.accounts = d.accountsConfig.accounts.filter((a) => a.id !== accountId);
  d.accounts = d.accounts.filter((a) => a.id !== accountId);
  d.folders = d.folders.filter((f) => f.account_id !== accountId);
  d.messages = d.messages.filter((m) => m.account_id !== accountId);
  d.statuses = d.statuses.filter((s) => s.account_id !== accountId);
  for (const va of d.accountsConfig.virtual_accounts) {
    va.members = va.members.filter((m) => m !== accountId);
  }
  core.emit({ type: "AccountsChanged" });
  return { type: "Done" };
}

/**
 * Whether an identity rule pattern matches an address: `*@example.com`
 * (a domain), `*@*.example.com` (subdomains) or a full address;
 * case-insensitive (as `IdentityRule::matches` in the core).
 */
export function patternMatches(pattern: string, email: string): boolean {
  const p = pattern.trim().toLowerCase();
  const e = email.trim().toLowerCase();
  if (p.startsWith("*@*.")) {
    const domain = e.split("@")[1] ?? "";
    return domain.endsWith(`.${p.slice(4)}`);
  }
  if (p.startsWith("*@")) return (e.split("@")[1] ?? "") === p.slice(2);
  return p === e;
}

/**
 * `ChooseIdentity`, as the core decides: the virtual account's identity
 * rules, else the identity a replied-to message was sent to, else the
 * default (a virtual account's `default_identity`, else the first).
 */
export function chooseIdentity(
  d: MockData,
  accountId: string | null,
  recipients: readonly string[],
  replyTo: MessageId | null,
): Response {
  const chosen = (id: string | null): Response => ({ type: "IdentityChosen", identity_id: id });
  const va = accountId === null ? undefined : virtualConfig(d, accountId);
  const rule = va?.identity_rules.find((r) => recipients.some((x) => patternMatches(r.pattern, x)));
  if (rule) return chosen(rule.identity);
  const body = replyTo === null ? undefined : d.bodies.get(replyTo);
  if (body) {
    const addressed = [...body.headers.to, ...body.headers.cc].map((a) => a.email.toLowerCase());
    const all = d.accountsConfig.accounts.flatMap((a) => a.identities);
    const hit = all.find((i) => addressed.includes(i.email.toLowerCase()));
    if (hit) return chosen(hit.id);
  }
  const listed = listIdentities(d, accountId);
  if (listed.type !== "Identities") return listed;
  const preferred = va?.default_identity;
  const identity = listed.identities.find((i) => i.id === preferred) ?? listed.identities[0];
  return chosen(identity?.id ?? null);
}

export function listIdentities(d: MockData, accountId: string | null): Response {
  const all = d.accountsConfig.accounts.flatMap((a) => a.identities);
  if (accountId === null) return { type: "Identities", identities: all };
  const va = virtualConfig(d, accountId);
  if (va) {
    return {
      type: "Identities",
      identities: va.members.flatMap(
        (m) => d.accountsConfig.accounts.find((a) => a.id === m)?.identities ?? [],
      ),
    };
  }
  const account = d.accountsConfig.accounts.find((a) => a.id === accountId);
  if (!account) return errorResponse("not_found", `account ${accountId} not found`);
  return { type: "Identities", identities: account.identities };
}

function validateVirtual(d: MockData, config: VirtualAccountConfig): string | null {
  if (config.name.trim() === "") return "name missing";
  const real = new Set(d.accountsConfig.accounts.map((a) => a.id));
  if (config.members.length === 0) return "a virtual account needs members";
  if (config.members.some((m) => !real.has(m))) return "members must be real accounts";
  if (config.folder_mappings.some((m) => m.name.trim() === "" || m.sources.length === 0)) {
    return "folder mappings need a name and a source";
  }
  const identities = new Set(
    d.accountsConfig.accounts.flatMap((a) => a.identities.map((i) => i.id)),
  );
  if (config.default_identity !== null && !identities.has(config.default_identity)) {
    return "unknown default identity";
  }
  if (config.identity_rules.some((r) => !identities.has(r.identity) || !r.pattern.includes("@"))) {
    return "invalid identity rule";
  }
  return null;
}

export function addVirtualAccount(core: MockCore, input: VirtualAccountConfig): Response {
  const d = core.data;
  const problem = validateVirtual(d, input);
  if (problem) return errorResponse("invalid_request", problem);
  if (input.id !== "" && (idTaken(d, input.id) || !ID_RE.test(input.id))) {
    return errorResponse("conflict", `id ${input.id} is taken or invalid`);
  }
  const config = { ...input, id: input.id || deriveId(d, input.name) };
  d.accountsConfig.virtual_accounts.push(config);
  core.emit({ type: "AccountsChanged" });
  return { type: "VirtualAccountSaved", config };
}

export function updateVirtualAccount(core: MockCore, input: VirtualAccountConfig): Response {
  const d = core.data;
  const index = d.accountsConfig.virtual_accounts.findIndex((v) => v.id === input.id);
  if (index < 0) return errorResponse("not_found", `virtual account ${input.id} not found`);
  const problem = validateVirtual(d, input);
  if (problem) return errorResponse("invalid_request", problem);
  d.accountsConfig.virtual_accounts[index] = input;
  core.emit({ type: "AccountsChanged" });
  return { type: "VirtualAccountSaved", config: input };
}

export function removeVirtualAccount(core: MockCore, id: string): Response {
  const d = core.data;
  if (!d.accountsConfig.virtual_accounts.some((v) => v.id === id)) {
    return errorResponse(
      id === ALL_ACCOUNTS ? "invalid_request" : "not_found",
      `cannot remove ${id}`,
    );
  }
  d.accountsConfig.virtual_accounts = d.accountsConfig.virtual_accounts.filter((v) => v.id !== id);
  core.emit({ type: "AccountsChanged" });
  return { type: "Done" };
}
