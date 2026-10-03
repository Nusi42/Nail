/**
 * Deterministic in-memory mail data for `MockTransport` (tests and `?mock=1`).
 * The theme and layout are the reference fixtures from `nail-api`.
 *
 * Besides mail, the mock holds what `accounts.kdl` and the store hold for the
 * real core: the accounts configuration (accounts, identities, virtual
 * accounts), connection states, drafts and the outbox.
 */
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import defaultThemeJson from "../../../crates/nail-api/fixtures/default-theme.json";
import type {
  Account,
  AccountConfig,
  AccountStatus,
  AccountsConfig,
  Address,
  DiscoveredAccount,
  Draft,
  ErrorCode,
  Folder,
  FolderRole,
  Identity,
  LaunchRequest,
  MessageBody,
  MessageSummary,
  OutboxEntry,
  ResolvedLayout,
  ResolvedTheme,
  UiContext,
} from "./generated";
import { normalizeLayout, normalizeTheme } from "./normalize";
import { createComposerMockState, type ComposerMockState } from "../features/composer/mock";
import {
  createCustomizationMockState,
  type CustomizationMockState,
} from "../features/customization/mock";
import { createAgentsMockState, type AgentsMockState } from "../features/agents/mock";
import { createSystemMockState, type SystemMockState } from "../features/system/mock";
import {
  createPortabilityMockState,
  type PortabilityMockState,
} from "../features/portability/mock";
import { createPaletteMockState, type PaletteMockState } from "../features/palette/mock";
import { createAppearanceMockState, type AppearanceMockState } from "../features/appearance/mock";
import { createAttachmentsMockState, type AttachmentsMockState } from "./mockAttachments";
import { createPrintMockState, type PrintMockState } from "../features/print/mock";
import { createVaultMockState, type VaultMockState } from "../features/security/vaultMock";
import { createExchangeMockState, type ExchangeMockState } from "../features/exchange/mock";
import { createGmailMockState, type GmailMockState } from "../features/gmail/mock";
import { createMcpMockState, type McpMockState } from "../features/mcp/mock";
import { createPimMockState, type PimMockState } from "../features/pim/mock";
import { createPluginsMockState, type PluginsMockState } from "../features/plugins/mock";
import { createRemoteMockState, type RemoteMockState } from "../features/remote/mock";
import { createRulesMockState, type RulesMockState } from "../features/rules/mock";
import { createSchedulingMockState, type SchedulingMockState } from "../features/scheduling/mock";
import { createSecurityMockState, type SecurityMockState } from "../features/security/mock";
import { createSieveMockState, type SieveMockState } from "../features/sieve/mock";
import { createTriageMockState, type TriageMockState } from "../features/triage/mock";

export interface MockData {
  /** Real accounts, in display order. */
  accounts: Account[];
  /** Folders of the real accounts. */
  folders: Folder[];
  messages: MessageSummary[];
  bodies: Map<string, MessageBody>;
  theme: ResolvedTheme;
  layout: ResolvedLayout;
  locale: string | null;
  /** The `accounts.kdl` model. */
  accountsConfig: AccountsConfig;
  /** Connection state per real account. */
  statuses: AccountStatus[];
  drafts: Map<string, Draft>;
  outbox: OutboxEntry[];
  /** Discovery answers by domain; unknown domains get a guess. */
  discovery: Record<string, DiscoveredAccount[]>;
  /** OAuth flows by id (a `done` flow has "tokens" the core can use). */
  oauthFlows: Map<string, "pending" | "done" | "failed" | "cancelled">;
  /** Failed queued writes (op id → account), for `RetryOp`. */
  failedOps: Map<string, string>;
  /** The last reported UI context. */
  uiContext: UiContext;
  /** Launch requests waiting for the UI (`TakeLaunchRequests`). */
  launches: LaunchRequest[];
  /** `.eml` files the mock can "import": path → message id shown. */
  messageFiles: Map<string, string>;
  /** State of the feature mocks (`src/features/<name>/mock.ts`). */
  features: MockFeatures;
  /** Behaviour switches for tests. */
  behaviour: MockBehaviour;
}

/** Each feature's mock state; only that feature's `mock.ts` touches it. */
export interface MockFeatures {
  composer: ComposerMockState;
  rules: RulesMockState;
  mcp: McpMockState;
  customization: CustomizationMockState;
  security: SecurityMockState;
  remote: RemoteMockState;
  sieve: SieveMockState;
  triage: TriageMockState;
  plugins: PluginsMockState;
  pim: PimMockState;
  scheduling: SchedulingMockState;
  gmail: GmailMockState;
  exchange: ExchangeMockState;
  agents: AgentsMockState;
  system: SystemMockState;
  portability: PortabilityMockState;
  palette: PaletteMockState;
  /** Themes and selection (M5a, `features/appearance/mock.ts`). */
  appearance: AppearanceMockState;
  /** Attachments of received mail: downloads and saves (M5a, `./mockAttachments.ts`). */
  attachments: AttachmentsMockState;
  print: PrintMockState;
  /** Recovery key and smartcards (`features/security/vaultMock.ts`). */
  vault: VaultMockState;
}

export function emptyUiContext(): UiContext {
  return {
    account_id: null,
    folder_id: null,
    message_id: null,
    thread_id: null,
    selected_message_ids: [],
    selected_text: null,
    draft_id: null,
    draft_cursor: null,
  };
}

/** Knobs tests use to make the mock core fail or slow down. */
export interface MockBehaviour {
  /** Error of every queued write (`OpStatus` `failed`); `null` = succeed. */
  opError: ErrorCode | null;
  /** Error of every submission (`SendStatus` `failed`); `null` = succeed. */
  sendError: ErrorCode | null;
  /** Result of OAuth sign-ins; `null` = success. */
  oauthError: ErrorCode | null;
  /** Delay of asynchronous core work (op replay, OAuth, sending) in ms. */
  workMs: number;
}

export const defaultTheme = (): ResolvedTheme => normalizeTheme(defaultThemeJson);
export const defaultLayout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);

const PEOPLE: Address[] = [
  { name: "Grace Hopper", email: "grace@navy.example" },
  { name: "Alan Turing", email: "alan@bletchley.example" },
  { name: "Katherine Johnson", email: "katherine@nasa.example" },
  { name: null, email: "noreply@shop.example" },
  { name: "Edsger Dijkstra", email: "ewd@tue.example" },
  { name: "Barbara Liskov", email: "liskov@mit.example" },
];

const SUBJECTS = [
  "Compiler notes for Monday",
  "Re: The bombe schedule",
  "Trajectory numbers",
  "Your order has shipped",
  "Go To statement considered harmful",
  "Abstraction and substitution",
  "Lunch?",
  "Quarterly report draft",
];

/** Base time of the generated mailbox: newest message. */
export const MOCK_NOW = Date.parse("2026-09-29T08:00:00Z");

interface FolderSpec {
  name: string;
  role: FolderRole;
  count: number;
  parent?: string;
}

const FOLDERS: FolderSpec[] = [
  { name: "Inbox", role: "inbox", count: 0 },
  { name: "Receipts", role: "other", count: 3, parent: "Inbox" },
  { name: "Sent", role: "sent", count: 4 },
  { name: "Drafts", role: "drafts", count: 0 },
  { name: "Archive", role: "archive", count: 6 },
  { name: "Junk", role: "junk", count: 1 },
  { name: "Trash", role: "trash", count: 0 },
];

/** The folders a newly added mock account starts with. */
export const NEW_ACCOUNT_FOLDERS: readonly FolderSpec[] = FOLDERS.filter((f) => !f.parent).map(
  (f) => ({ ...f, count: 0 }),
);

/** A mailing-list message delivered to both accounts (virtual accounts dedupe it). */
export const LIST_MESSAGE_ID = "rust-announce-42@lists.example";

function pick<T>(items: readonly T[], index: number): T {
  return items[index % items.length] as T;
}

/** Folder id of a mock folder: `<account>/<path>`. */
export function mockFolderId(accountId: string, path: string): string {
  return `${accountId}/${path}`;
}

/** Builds a message body for a summary. */
export function mockBody(
  summary: MessageSummary,
  extra: { text: string; html?: string | null; messageId?: string; cc?: Address[] },
): MessageBody {
  return {
    id: summary.id,
    headers: {
      subject: summary.subject,
      from: summary.from ? [summary.from] : [],
      to: summary.to,
      cc: extra.cc ?? [],
      reply_to: [],
      date: summary.date,
      message_id: extra.messageId ?? `${summary.id.replaceAll("/", ".")}@mock.example`,
    },
    text: extra.text,
    sanitized_html: extra.html ?? null,
    attachments: [],
    remote_content_blocked: false,
  };
}

function identity(accountId: string, email: string, name: string | null, sig: string | null) {
  const id: Identity = {
    id: email,
    account_id: accountId,
    email,
    name,
    reply_to: null,
    bcc: [],
    signature: sig === null ? null : { text: sig, html: null, markdown: null },
    composer_mode: null,
    posting: null,
    mail_style: null,
  };
  return id;
}

/** A complete IMAP + SMTP account configuration (password login). */
export function mockAccountConfig(
  id: string,
  name: string,
  email: string,
  host: string,
  identities: Identity[],
): AccountConfig {
  const auth = { method: "password" as const, username: email, secret: null, mechanism: null };
  return {
    id,
    name,
    email,
    enabled: true,
    incoming: {
      protocol: "imap",
      server: { host: `imap.${host}`, port: 993, tls: "implicit", auth },
      options: { poll_interval_secs: null, compress: null, max_connections: null },
    },
    outgoing: {
      protocol: "smtp",
      server: { host: `smtp.${host}`, port: 465, tls: "implicit", auth },
      options: { dsn: null },
    },
    identities,
    sync: { bodies: null, folders: [] },
    save_sent_copy: null,
  };
}

/** Builds the mock mailbox. `inboxSize` controls the personal inbox (virtualization). */
export function createMockData(inboxSize = 250): MockData {
  const accounts: Account[] = [
    { id: "acc-personal", name: "Personal", email: "ada@example.org", kind: "imap" },
    { id: "acc-work", name: "Work", email: "ada@work.example", kind: "imap" },
  ];
  const folders: Folder[] = [];
  const messages: MessageSummary[] = [];
  const bodies = new Map<string, MessageBody>();

  for (const account of accounts) {
    const inboxCount = account.id === "acc-personal" ? inboxSize : 12;
    for (const spec of FOLDERS) {
      const path = spec.parent ? `${spec.parent}/${spec.name}` : spec.name;
      const id = mockFolderId(account.id, path);
      const parentId = spec.parent ? mockFolderId(account.id, spec.parent) : null;
      const count = spec.role === "inbox" ? inboxCount : spec.count;
      let unread = 0;
      for (let i = 0; i < count; i++) {
        const seq = messages.length;
        const from = pick(PEOPLE, seq);
        const subject = pick(SUBJECTS, seq + i);
        const seen = i % 3 !== 0;
        if (!seen) unread++;
        const messageId = `${id}#${i + 1}`;
        const date = new Date(MOCK_NOW - i * 37 * 60_000).toISOString();
        const text = `Hello Ada,\n\n${subject} (message ${i + 1} in ${spec.name}).\n\n> Earlier text\n>> Older text\n\nBest,\n${from.name ?? from.email}`;
        const summary: MessageSummary = {
          id: messageId,
          folder_id: id,
          account_id: account.id,
          subject,
          from,
          to: [{ name: "Ada Lovelace", email: account.email }],
          date,
          flags: { seen, flagged: i % 7 === 0, answered: i % 11 === 0, draft: false },
          has_attachments: i % 5 === 0,
          preview: text.replace(/\s+/g, " ").slice(0, 120),
          // Messages with the same subject close together form a conversation
          // (subjects repeat every four messages of a folder).
          thread_id: `t-${id}-${(seq + i) % SUBJECTS.length}-${Math.floor(i / 12)}`,
          tags: i % 9 === 0 ? ["work"] : [],
          duplicates: [],
        };
        messages.push(summary);
        const body = mockBody(summary, {
          text,
          html:
            i % 4 === 0
              ? `<p>Hello <b>Ada</b>,</p><p>${subject}.</p><blockquote><p>Earlier text</p></blockquote>`
              : null,
          cc: i % 6 === 0 ? [pick(PEOPLE, seq + 1)] : [],
        });
        body.attachments =
          i % 5 === 0
            ? [
                {
                  index: 0,
                  filename: "report.pdf",
                  mime: "application/pdf",
                  size: 48_213,
                  inline: false,
                },
              ]
            : [];
        body.remote_content_blocked = i % 8 === 0;
        bodies.set(messageId, body);
      }
      folders.push({
        id,
        account_id: account.id,
        parent_id: parentId,
        name: spec.name,
        path,
        role: spec.role,
        unread,
        total: count,
      });
    }
  }

  // The same mailing-list post, archived in both accounts.
  for (const account of accounts) {
    const folderId = mockFolderId(account.id, "Archive");
    const summary: MessageSummary = {
      id: `${folderId}#list`,
      folder_id: folderId,
      account_id: account.id,
      subject: "[rust-announce] Rust 1.97 released",
      from: { name: "Rust Announce", email: "announce@lists.example" },
      to: [{ name: null, email: "rust-announce@lists.example" }],
      date: new Date(MOCK_NOW - 3 * 86_400_000).toISOString(),
      flags: { seen: true, flagged: false, answered: false, draft: false },
      has_attachments: false,
      preview: "The Rust team is happy to announce a new version.",
      thread_id: `t-list-${account.id}`,
      tags: [],
      duplicates: [],
    };
    messages.push(summary);
    bodies.set(
      summary.id,
      mockBody(summary, {
        text: "The Rust team is happy to announce a new version.",
        messageId: LIST_MESSAGE_ID,
      }),
    );
    const archive = folders.find((f) => f.id === folderId);
    if (archive) archive.total++;
  }

  const personalIds = [
    identity("acc-personal", "ada@example.org", "Ada Lovelace", "Ada"),
    identity("acc-personal", "ada+lists@example.org", "Ada Lovelace", null),
  ];
  const workIds = [
    identity("acc-work", "ada@work.example", "Ada Lovelace", "Ada Lovelace\nAnalyst"),
  ];

  return {
    accounts,
    folders,
    messages,
    bodies,
    theme: defaultTheme(),
    layout: defaultLayout(),
    locale: null,
    accountsConfig: {
      accounts: [
        mockAccountConfig(
          "acc-personal",
          "Personal",
          "ada@example.org",
          "example.org",
          personalIds,
        ),
        mockAccountConfig("acc-work", "Work", "ada@work.example", "work.example", workIds),
      ],
      virtual_accounts: [],
      oauth_providers: [
        {
          id: "google",
          client_id: "mock-client",
          client_secret: null,
          auth_url: null,
          token_url: null,
          scopes: [],
        },
      ],
      undo_send_secs: 10,
      pim_sources: [],
    },
    statuses: accounts.map((a) => ({
      account_id: a.id,
      state: "online",
      since: new Date(MOCK_NOW).toISOString(),
      error: null,
      message: null,
    })),
    drafts: new Map(),
    outbox: [],
    oauthFlows: new Map(),
    discovery: {
      "gmail.com": [
        {
          source: "ispdb",
          provider_name: "Google Mail",
          incoming: {
            protocol: "imap",
            server: {
              host: "imap.gmail.com",
              port: 993,
              tls: "implicit",
              auth: {
                method: "oauth2",
                username: "",
                provider: "google",
                secret: null,
                mechanism: null,
              },
            },
            options: { poll_interval_secs: null, compress: null, max_connections: null },
          },
          outgoing: {
            protocol: "smtp",
            server: {
              host: "smtp.gmail.com",
              port: 465,
              tls: "implicit",
              auth: {
                method: "oauth2",
                username: "",
                provider: "google",
                secret: null,
                mechanism: null,
              },
            },
            options: { dsn: null },
          },
          warnings: [],
        },
      ],
    },
    failedOps: new Map(),
    uiContext: emptyUiContext(),
    launches: [],
    messageFiles: new Map(),
    features: {
      composer: createComposerMockState(),
      rules: createRulesMockState(),
      mcp: createMcpMockState(),
      customization: createCustomizationMockState(),
      security: createSecurityMockState(),
      remote: createRemoteMockState(),
      sieve: createSieveMockState(),
      triage: createTriageMockState(),
      plugins: createPluginsMockState(),
      pim: createPimMockState(),
      scheduling: createSchedulingMockState(),
      gmail: createGmailMockState(),
      exchange: createExchangeMockState(),
      agents: createAgentsMockState(),
      system: createSystemMockState(),
      portability: createPortabilityMockState(),
      palette: createPaletteMockState(),
      appearance: createAppearanceMockState(),
      attachments: createAttachmentsMockState(),
      print: createPrintMockState(),
      vault: createVaultMockState(),
    },
    behaviour: { opError: null, sendError: null, oauthError: null, workMs: 20 },
  };
}
