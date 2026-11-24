/**
 * The demo-multi Exchange accounts in e2e (M3c, ADR 0077): the core's clock
 * for the mock tenants, the mock Graph tenant's state (`graph-*` controls)
 * and switching an Exchange account on.
 */
import { expect, type Page } from "@playwright/test";
import { control, type NailServer } from "./nail";
import { syncAccount } from "./views";

/**
 * The core's clock for the Exchange tests: before every dated item of the
 * mock tenants (their first reminders are due 2026-10-05 06:45 UTC), so no
 * reminder comes due — or is caught up — while a test runs, whatever the
 * day the suite runs on. Reminder toasts are `pim.spec.ts`'s subject.
 */
export const FIXTURE_NOW = Date.UTC(2026, 9, 1, 8, 0) / 1000;

/** A message of the mock Graph tenant. */
export interface GraphMessage {
  id: string;
  folder: string;
  subject: string;
  read: boolean;
  flag: string;
  categories: string[];
  inference: string;
}

/** The messages of the mock Graph tenant (its own mailbox, or `mailbox`). */
export const graphMessagesOf = (server: NailServer, mailbox?: string) =>
  control<GraphMessage[]>(server, "graph-messages", mailbox ? { mailbox } : {});

/** The mock Graph tenant's message `subject`. */
export const graphMessageOf = async (server: NailServer, subject: string) =>
  (await graphMessagesOf(server)).find((m) => m.subject === subject);

/** The mock Graph tenant's categories, automatic replies and inbox rules. */
export const graphSettingsOf = (server: NailServer) =>
  control<{
    categories: { displayName: string }[];
    automatic_replies: Record<string, unknown>;
    rules: Record<string, unknown>[];
  }>(server, "graph-settings");

/** Settings → Accounts → Contoso → "Synchronize this account". */
export async function enableContoso(page: Page): Promise<void> {
  await enableAccount(page, "Contoso", 2);
}

/**
 * Settings → Accounts → `name` (an Exchange account) → "Synchronize this
 * account"; waits for `messages` rows in its inbox.
 */
export async function enableAccount(page: Page, name: string, messages: number): Promise<void> {
  await syncAccount(page, name, messages, async (settings) => {
    // An Exchange account shows its engine and options.
    await expect(settings.getByRole("combobox", { name: "Protocol" })).toHaveValue("exchange");
  });
}
