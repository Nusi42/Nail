/**
 * Small labelled form controls and the server editor shared by the account
 * setup wizard and the account settings. Labels are Fluent ids; styling is
 * the theme's (`form.*` parts).
 */
import { useId, type ReactNode } from "react";
import type {
  AuthConfig,
  IncomingServer,
  MailProtocol,
  OutgoingServer,
  ServerEndpoint,
  TlsMode,
} from "../api/generated";
import { useT } from "../i18n/I18nProvider";

/**
 * A control that is small on purpose (ADR 0160): the skin sizes it
 * (`theme/skin.ts` `CONTROL_WIDTHS`) instead of filling its field's column.
 */
export type ControlWidth = "short" | "medium" | "content";

interface FieldProps {
  /** Fluent id of the label. */
  labelId: string;
  /** Arguments of the label message. */
  args?: Record<string, string | number>;
  /** Fluent id of a hint shown under the control. */
  hintId?: string;
  name: string;
}

export function TextField({
  labelId,
  args,
  hintId,
  name,
  value,
  onChange,
  type = "text",
  required = false,
  autoComplete = "off",
}: FieldProps & {
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "email" | "password";
  required?: boolean;
  autoComplete?: string;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId, args)}
      </label>
      <input
        id={id}
        data-part="form.input"
        type={type}
        name={name}
        value={value}
        required={required}
        autoComplete={autoComplete}
        aria-describedby={hintId ? `${id}-hint` : undefined}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      {hintId && (
        <span data-part="form.hint" id={`${id}-hint`}>
          {t(hintId)}
        </span>
      )}
    </div>
  );
}

export function TextAreaField({
  labelId,
  args,
  name,
  value,
  onChange,
}: FieldProps & { value: string; onChange: (value: string) => void }) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId, args)}
      </label>
      <textarea
        id={id}
        data-part="form.textarea"
        name={name}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </div>
  );
}

export function NumberField({
  labelId,
  args,
  hintId,
  name,
  value,
  onChange,
  min = 0,
  max,
}: FieldProps & {
  value: number | null;
  onChange: (value: number | null) => void;
  min?: number;
  max?: number;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId, args)}
      </label>
      <input
        id={id}
        data-part="form.input"
        data-width="short"
        type="number"
        name={name}
        min={min}
        max={max}
        value={value ?? ""}
        aria-describedby={hintId ? `${id}-hint` : undefined}
        onChange={(event) => {
          const n = event.target.value === "" ? null : Number(event.target.value);
          onChange(n === null || Number.isNaN(n) ? null : n);
        }}
      />
      {hintId && (
        <span data-part="form.hint" id={`${id}-hint`}>
          {t(hintId)}
        </span>
      )}
    </div>
  );
}

export interface Choice {
  value: string;
  /** Fluent id, or literal text when `literal` is set (e.g. an address). */
  label: string;
  literal?: boolean;
}

export function SelectField({
  labelId,
  args,
  name,
  value,
  choices,
  onChange,
  width,
}: FieldProps & {
  value: string;
  choices: readonly Choice[];
  onChange: (value: string) => void;
  /** Leave out to fill the field's column. */
  width?: ControlWidth;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId, args)}
      </label>
      <select
        id={id}
        data-part="form.select"
        data-width={width}
        name={name}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      >
        {choices.map((c) => (
          <option key={c.value} value={c.value}>
            {c.literal ? c.label : t(c.label)}
          </option>
        ))}
      </select>
    </div>
  );
}

export function CheckField({
  labelId,
  args,
  name,
  checked,
  onChange,
}: FieldProps & { checked: boolean; onChange: (checked: boolean) => void }) {
  const t = useT();
  return (
    <label data-part="form.check" data-field={name}>
      <input
        data-part="form.checkbox"
        type="checkbox"
        name={name}
        checked={checked}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
      <span data-part="form.check.label">{t(labelId, args)}</span>
    </label>
  );
}

/** A titled group of fields. */
export function Section({ titleId, children }: { titleId: string; children: ReactNode }) {
  const t = useT();
  return (
    <fieldset data-part="form.section">
      <legend data-part="form.section.title">{t(titleId)}</legend>
      {children}
    </fieldset>
  );
}

// ---------------------------------------------------------------------------
// Servers

/** Default port of a protocol for a TLS mode (mirrors `nail_api::default_port`). */
export function defaultPort(protocol: MailProtocol, tls: TlsMode): number {
  const implicit = tls === "implicit";
  switch (protocol) {
    case "imap":
      return implicit ? 993 : 143;
    case "pop3":
      return implicit ? 995 : 110;
    case "smtp":
      return implicit ? 465 : 587;
    case "gmail":
    case "graph":
    case "ews":
      return implicit ? 443 : 80;
  }
}

const TLS_CHOICES: Choice[] = [
  { value: "implicit", label: "server-tls-implicit" },
  { value: "starttls", label: "server-tls-starttls" },
  { value: "plaintext", label: "server-tls-plaintext" },
];

/** Changes the TLS mode, moving the port along when it was the old default. */
export function withTls(
  protocol: MailProtocol,
  server: ServerEndpoint,
  tls: TlsMode,
): ServerEndpoint {
  const port =
    server.port === defaultPort(protocol, server.tls) ? defaultPort(protocol, tls) : server.port;
  return { ...server, tls, port };
}

/** Login name of an auth config (empty for `none`). */
export function usernameOf(auth: AuthConfig): string {
  if (auth.method === "password" || auth.method === "oauth2") return auth.username;
  if (auth.method === "client-certificate" || auth.method === "windows") {
    return auth.username ?? "";
  }
  return "";
}

function withUsername(auth: AuthConfig, username: string): AuthConfig {
  if (auth.method === "password" || auth.method === "oauth2") return { ...auth, username };
  if (auth.method === "client-certificate" || auth.method === "windows") {
    return { ...auth, username: username || null };
  }
  return auth;
}

/** How the user signs in: a password, or an OAuth provider id. */
export type SignIn = { kind: "password" } | { kind: "oauth2"; provider: string } | { kind: "none" };

export function signInOf(auth: AuthConfig): SignIn {
  if (auth.method === "oauth2") return { kind: "oauth2", provider: auth.provider };
  if (auth.method === "none") return { kind: "none" };
  return { kind: "password" };
}

/** Applies a sign-in method to an auth config, keeping the login name. */
export function withSignIn(auth: AuthConfig, signIn: SignIn): AuthConfig {
  const username = usernameOf(auth);
  switch (signIn.kind) {
    case "password":
      return { method: "password", username, secret: null, mechanism: null };
    case "oauth2":
      return {
        method: "oauth2",
        username,
        provider: signIn.provider,
        secret: null,
        mechanism: null,
      };
    case "none":
      return { method: "none" };
  }
}

/** Switches an incoming server between IMAP and POP3 with fresh options. */
export function withProtocol(server: IncomingServer, protocol: "imap" | "pop3"): IncomingServer {
  if (server.protocol === protocol) return server;
  const endpoint = { ...server.server, port: defaultPort(protocol, server.server.tls) };
  return protocol === "imap"
    ? {
        protocol,
        server: endpoint,
        options: { poll_interval_secs: null, compress: null, max_connections: null },
      }
    : {
        protocol,
        server: endpoint,
        options: {
          poll_interval_secs: null,
          leave_on_server: null,
          delete_after_days: null,
          delete_when_deleted_locally: null,
        },
      };
}

/** Host, port, security and login name of one server. */
export function EndpointFields({
  name,
  protocol,
  server,
  onChange,
}: {
  name: string;
  protocol: MailProtocol;
  server: ServerEndpoint;
  onChange: (server: ServerEndpoint) => void;
}) {
  return (
    <>
      <TextField
        labelId="server-host"
        name={`${name}-host`}
        value={server.host}
        required
        onChange={(host) => {
          onChange({ ...server, host: host.trim() });
        }}
      />
      <NumberField
        labelId="server-port"
        name={`${name}-port`}
        value={server.port}
        min={1}
        max={65535}
        onChange={(port) => {
          onChange({ ...server, port: port ?? defaultPort(protocol, server.tls) });
        }}
      />
      <SelectField
        labelId="server-tls"
        name={`${name}-tls`}
        value={server.tls}
        choices={TLS_CHOICES}
        onChange={(tls) => {
          onChange(withTls(protocol, server, tls as TlsMode));
        }}
      />
      {server.auth.method !== "none" && (
        <TextField
          labelId="server-username"
          name={`${name}-username`}
          value={usernameOf(server.auth)}
          onChange={(username) => {
            onChange({ ...server, auth: withUsername(server.auth, username) });
          }}
        />
      )}
    </>
  );
}

/** IMAP and POP3 accounts switch between each other. */
const MAIL_PROTOCOL_CHOICES: readonly Choice[] = [
  { value: "imap", label: "server-protocol-imap" },
  { value: "pop3", label: "server-protocol-pop3" },
];

/** A Gmail API account stays one: no IMAP / POP3 conversion from here. */
const GMAIL_PROTOCOL_CHOICES: readonly Choice[] = [
  { value: "gmail", label: "server-protocol-gmail" },
];

/** The mail protocol of an incoming server (an Exchange account's engine, M3c). */
export function mailProtocolOf(incoming: IncomingServer): MailProtocol {
  if (incoming.protocol === "exchange") {
    return incoming.options.engine === "ews" ? "ews" : "graph";
  }
  return incoming.protocol;
}

/** An Exchange account stays one too (M3c; its engine is an account option). */
const EXCHANGE_PROTOCOL_CHOICES: readonly Choice[] = [
  { value: "exchange", label: "server-protocol-exchange" },
];

/** The protocol choices for an incoming server. */
export function protocolChoices(incoming: IncomingServer): readonly Choice[] {
  if (incoming.protocol === "gmail") return GMAIL_PROTOCOL_CHOICES;
  if (incoming.protocol === "exchange") return EXCHANGE_PROTOCOL_CHOICES;
  return MAIL_PROTOCOL_CHOICES;
}

/** Editor for the incoming server (protocol + endpoint). */
export function IncomingFields({
  incoming,
  onChange,
}: {
  incoming: IncomingServer;
  onChange: (incoming: IncomingServer) => void;
}) {
  return (
    <Section titleId="server-incoming">
      <SelectField
        labelId="server-protocol"
        name="incoming-protocol"
        value={incoming.protocol}
        choices={protocolChoices(incoming)}
        onChange={(protocol) => {
          if (protocol === "imap" || protocol === "pop3") {
            onChange(withProtocol(incoming, protocol));
          }
        }}
      />
      <EndpointFields
        name="incoming"
        protocol={mailProtocolOf(incoming)}
        server={incoming.server}
        onChange={(server) => {
          onChange({ ...incoming, server });
        }}
      />
    </Section>
  );
}

/** Editor for the outgoing (SMTP) server. */
export function OutgoingFields({
  outgoing,
  onChange,
}: {
  outgoing: OutgoingServer;
  onChange: (outgoing: OutgoingServer) => void;
}) {
  return (
    <Section titleId="server-outgoing">
      <EndpointFields
        name="outgoing"
        protocol="smtp"
        server={outgoing.server}
        onChange={(server) => {
          onChange({ ...outgoing, server });
        }}
      />
    </Section>
  );
}

/** The sign-in choices: password, the built-in OAuth providers and configured ones. */
export function signInChoices(providers: readonly string[]): Choice[] {
  const ids = [...new Set(["google", "microsoft", ...providers])];
  return [
    { value: "password", label: "sign-in-password" },
    ...ids.map((id) =>
      id === "google" || id === "microsoft"
        ? { value: `oauth2:${id}`, label: `sign-in-oauth-${id}` }
        : // A provider configured in accounts.kdl is shown by its id.
          { value: `oauth2:${id}`, label: id, literal: true },
    ),
  ];
}

export function signInValue(signIn: SignIn): string {
  return signIn.kind === "oauth2" ? `oauth2:${signIn.provider}` : signIn.kind;
}

export function parseSignIn(value: string): SignIn {
  if (value.startsWith("oauth2:")) return { kind: "oauth2", provider: value.slice(7) };
  return value === "none" ? { kind: "none" } : { kind: "password" };
}
