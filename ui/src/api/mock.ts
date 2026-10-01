/**
 * In-memory transport answering every request of the contract from
 * `MockData`, like a core with two IMAP accounts would. Used by unit tests
 * and by `?mock=1` in the dev server.
 *
 * Writes are applied at once and "replayed" asynchronously with `OpStatus`
 * events; sending honours the undo-send delay with real timers (tests use
 * fake timers). `data.behaviour` makes the mock fail on purpose.
 */
import { Listeners, type ConnectionStatus, type Transport } from "./client";
import type { LaunchRequest, Event as NailEvent, Request, Response } from "./generated";
import * as accounts from "./mockAccounts";
import { createMockData, type MockData } from "./mockData";
import * as mail from "./mockMail";
import { errorResponse, type MockCore } from "./mockMail";
import * as outbox from "./mockOutbox";
import { parseQuery } from "./mockSearch";
import {
  answerComposer,
  answerDraftAttachment,
  isComposerRequest,
} from "../features/composer/mock";
import { answerCustomization, isCustomizationRequest } from "../features/customization/mock";
import { answerExchange, isExchangeRequest } from "../features/exchange/mock";
import { answerGmail, isGmailRequest } from "../features/gmail/mock";
import { answerMcp, isMcpRequest } from "../features/mcp/mock";
import { answerPim, isPimRequest } from "../features/pim/mock";
import { answerPlugins, isPluginsRequest } from "../features/plugins/mock";
import { answerRemote, decorateRemoteContent, isRemoteRequest } from "../features/remote/mock";
import { answerRules, isRulesRequest } from "../features/rules/mock";
import {
  answerScheduling,
  decorateInvitation,
  isSchedulingRequest,
} from "../features/scheduling/mock";
import { answerSecurity, decorateMessageBody, isSecurityRequest } from "../features/security/mock";
import { answerSieve, isSieveRequest } from "../features/sieve/mock";
import { answerSync, isSyncRequest } from "../features/sync/mock";
import { answerTriage, decorateJunk, isTriageRequest } from "../features/triage/mock";

export interface MockTransportOptions {
  data?: MockData;
  /** Artificial latency per request in ms (0 = next microtask). */
  latencyMs?: number;
}

export class MockTransport implements Transport, MockCore {
  readonly data: MockData;
  /** Every request received, in order (for assertions). */
  readonly requests: Request[] = [];
  private readonly latencyMs: number;
  private readonly events = new Listeners<NailEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private closed = false;
  private counter = 0;

  constructor(options: MockTransportOptions = {}) {
    this.data = options.data ?? createMockData();
    this.latencyMs = options.latencyMs ?? 0;
  }

  async send(request: Request): Promise<Response> {
    this.requests.push(request);
    if (this.latencyMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, this.latencyMs));
    }
    return this.answer(request);
  }

  /** Pushes an event to all subscribers, as the core would. */
  /** Like the desktop shell: queues launch requests and tells the UI. */
  pushLaunch(requests: LaunchRequest[]): void {
    this.data.launches.push(...requests);
    this.emit({ type: "LaunchRequested" });
  }

  emit(event: NailEvent): void {
    this.events.emit(event);
  }

  later(fn: () => void, ms = this.data.behaviour.workMs): void {
    setTimeout(() => {
      if (!this.closed) fn();
    }, ms);
  }

  nextId(prefix: string): string {
    this.counter++;
    return `${prefix}${this.counter}`;
  }

  onEvent(listener: (event: NailEvent) => void): () => void {
    return this.events.add(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    return this.statuses.add(listener);
  }

  status(): ConnectionStatus {
    return this.closed ? "closed" : "connected";
  }

  close(): void {
    this.closed = true;
    this.statuses.emit("closed");
  }

  private answer(request: Request): Response {
    const d = this.data;
    if (this.closed) {
      return errorResponse("unavailable", "mock transport closed");
    }
    // Feature packages answer their own requests (src/features/*/mock.ts).
    if (isComposerRequest(request)) return answerComposer(this, request);
    if (isRulesRequest(request)) return answerRules(this, request);
    if (isMcpRequest(request)) return answerMcp(this, request);
    if (isCustomizationRequest(request)) return answerCustomization(this, request);
    if (isSecurityRequest(request)) return answerSecurity(this, request);
    if (isRemoteRequest(request)) return answerRemote(this, request);
    if (isSieveRequest(request)) return answerSieve(this, request);
    if (isTriageRequest(request)) return answerTriage(this, request);
    if (isSyncRequest(request)) return answerSync(this, request);
    if (isPluginsRequest(request)) return answerPlugins(this, request);
    if (isPimRequest(request)) return answerPim(this, request);
    if (isSchedulingRequest(request)) return answerScheduling(this, request);
    if (isGmailRequest(request)) return answerGmail(this, request);
    if (isExchangeRequest(request)) return answerExchange(this, request);
    switch (request.type) {
      case "Ping":
        return { type: "Pong", version: "0.0.0-mock", protocol: 1 };
      case "GetConfig":
        return { type: "Config", theme: d.theme, layout: d.layout, locale: d.locale };
      case "ListAccounts":
        return { type: "Accounts", accounts: mail.listAccounts(d) };
      case "ListFolders": {
        const folders = mail.listFolders(d, request.account_id);
        return folders ? { type: "Folders", folders } : notFound(`account ${request.account_id}`);
      }
      case "ListMessages": {
        const all = mail.folderMessages(d, request.folder_id);
        if (!all) return notFound(`folder ${request.folder_id}`);
        return {
          type: "Messages",
          folder_id: request.folder_id,
          offset: request.offset,
          total: all.length,
          messages: all.slice(request.offset, request.offset + request.limit),
        };
      }
      case "GetMessage": {
        const body = d.bodies.get(request.id);
        return body
          ? {
              type: "Message",
              message: decorateJunk(
                this,
                decorateRemoteContent(
                  this,
                  decorateInvitation(this, decorateMessageBody(this, body)),
                ),
              ),
            }
          : notFound(`message ${request.id}`);
      }
      case "SyncNow": {
        const account = request.account_id ?? d.accounts[0]?.id ?? "";
        queueMicrotask(() => {
          this.emit({ type: "SyncProgress", account_id: account, done: 0, total: 1 });
          this.emit({ type: "SyncProgress", account_id: account, done: 1, total: 1 });
        });
        return { type: "SyncStarted" };
      }
      case "DiscoverAccount":
        return accounts.discover(d, request.email);
      case "TestAccount":
        return accounts.testAccount(this, request.config, request.secrets);
      case "StartOAuth":
        return accounts.startOAuth(this, request.provider);
      case "CancelOAuth":
        return accounts.cancelOAuth(this, request.flow_id);
      case "GetAccountsConfig":
        return { type: "AccountsConfig", config: structuredClone(d.accountsConfig) };
      case "AddAccount":
        return accounts.addAccount(this, request.config);
      case "UpdateAccount":
        return accounts.updateAccount(this, request.config);
      case "RemoveAccount":
        return accounts.removeAccount(this, request.account_id);
      case "ListIdentities":
        return accounts.listIdentities(d, request.account_id);
      case "ChooseIdentity":
        return accounts.chooseIdentity(d, request.account_id, request.recipients, request.reply_to);
      case "ListVirtualAccounts":
        return {
          type: "VirtualAccounts",
          virtual_accounts: structuredClone(d.accountsConfig.virtual_accounts),
        };
      case "AddVirtualAccount":
        return accounts.addVirtualAccount(this, request.config);
      case "UpdateVirtualAccount":
        return accounts.updateVirtualAccount(this, request.config);
      case "RemoveVirtualAccount":
        return accounts.removeVirtualAccount(this, request.account_id);
      case "ListAccountStatuses":
        return { type: "AccountStatuses", statuses: d.statuses.map((s) => ({ ...s })) };
      case "UpdateFlags":
        return mail.updateFlags(this, request.message_ids, request.change);
      case "MoveMessages":
        return mail.moveMessages(this, request.message_ids, request.target);
      case "DeleteMessages":
        return mail.deleteMessages(this, request.message_ids, request.permanent);
      case "ListThreads": {
        const threads = mail.listThreads(d, request.folder_id);
        if (!threads) return notFound(`folder ${request.folder_id}`);
        return {
          type: "Threads",
          folder_id: request.folder_id,
          offset: request.offset,
          total: threads.length,
          threads: threads.slice(request.offset, request.offset + request.limit),
        };
      }
      case "GetThread":
        return mail.getThread(d, request.thread_id);
      case "Search":
        return mail.search(d, request.query);
      case "CheckQuery":
        return { type: "QueryChecked", error: parseQuery(request.text).error };
      case "SaveDraft":
        return outbox.saveDraft(this, request.draft);
      case "GetDraft":
        return outbox.getDraft(this, request.draft_id);
      case "GetDraftAttachment":
        return answerDraftAttachment(this, request);
      case "DeleteDraft":
        return outbox.deleteDraft(this, request.draft_id);
      case "ListDrafts":
        return outbox.listDrafts(this);
      case "SendDraft":
        return outbox.sendDraft(
          this,
          request.draft_id,
          request.delay_secs,
          request.send_at,
          request.crypto,
        );
      case "CancelSend":
        return outbox.cancelSend(this, request.outbox_id);
      case "ListOutbox":
        return outbox.listOutbox(this);
      case "RetryOp":
        return mail.retryOp(this, request.op_id);
      case "ReportUiContext":
        d.uiContext = structuredClone(request.context);
        return { type: "Done" };
      case "GetUiContext":
        return { type: "UiContext", context: structuredClone(d.uiContext) };
      case "TakeLaunchRequests":
        return { type: "LaunchRequests", requests: d.launches.splice(0) };
      case "ImportMessageFile": {
        const id = d.messageFiles.get(request.path);
        const message = id === undefined ? undefined : d.messages.find((m) => m.id === id);
        return message
          ? { type: "MessageImported", message: structuredClone(message) }
          : {
              type: "Error",
              code: "not_found",
              message: `file ${request.path} not found`,
              localized: {
                id: "message-file-unreadable",
                args: { path: request.path, detail: "not found" },
              },
            };
      }
    }
  }
}

function notFound(what: string): Response {
  return errorResponse("not_found", `${what} not found`);
}
