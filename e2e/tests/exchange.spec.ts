import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailPerTest } from "../support/nail";
import {
  accountSectionNamed,
  readerAction,
  runFromPalette,
  showView,
  viewRegion,
  messageRows,
  syncAccount,
  DEMO_MULTI_CONVERSATIONS,
  openMail,
  moveSelectionTo,
  messageList,
  saveVacationReply,
} from "../support/views";
import { defined } from "../support/defined";
import { eventChip, syncCalendars } from "../support/calendar";
import { expectSentThrough, newMessage } from "../support/compose";

/**
 * Wave M3c (ADR 0077) against `nail serve --seed demo-multi`: Exchange
 * Online through the mock Graph server (the "Contoso" account, switched off
 * at first; signed in through the mock OAuth2 server) and an on-premises
 * Exchange through the mock EWS server, both found by the mock Autodiscover
 * (`autodiscover.contoso.example`, `autodiscover.corp.example`). Controls:
 * `graph-*` (the mock Graph tenant), `ews-*` (the mock EWS server).
 */

/**
 * The core's clock for these tests: before every dated item of the mock
 * tenants (their first reminders are due 2026-10-05 06:45 UTC), so no
 * reminder comes due — or is caught up — while a test runs, whatever the
 * day the suite runs on. Reminder toasts are `pim.spec.ts`'s subject.
 */
const FIXTURE_NOW = Date.UTC(2026, 9, 1, 8, 0) / 1000;

/**
 * The page's date for tests that answer a meeting in the calendar: the
 * fixtures' meetings are on 2026-10-07 (EWS) and 2026-10-08 (Graph) and the
 * calendar opens on the browser's today, so the agenda shows them whatever
 * day the suite runs on.
 */
const PAGE_NOW = new Date("2026-10-07T08:00:00Z");

const server = nailPerTest({ seed: "demo-multi" }, async (s) => {
  await control(s, "clock-set", { at: FIXTURE_NOW });
});

interface GraphMessage {
  id: string;
  folder: string;
  subject: string;
  read: boolean;
  flag: string;
  categories: string[];
  inference: string;
}

const graphMessages = (mailbox?: string) =>
  control<GraphMessage[]>(server, "graph-messages", mailbox ? { mailbox } : {});
const graphMessage = async (subject: string) =>
  (await graphMessages()).find((m) => m.subject === subject);
const graphFolders = () =>
  control<{ id: string; name: string; well_known: string | null }[]>(server, "graph-folders");
const graphSettings = () =>
  control<{
    categories: { displayName: string }[];
    automatic_replies: Record<string, unknown>;
    rules: Record<string, unknown>[];
  }>(server, "graph-settings");
const graphPim = () =>
  control<{
    events: Record<string, unknown>[];
    tasks: { list: string; task: Record<string, unknown> }[];
  }>(server, "graph-pim");

/** Waits until Graph has the event `subject` and `check` holds for it. */
async function expectGraphEvent(
  subject: string,
  check: (event: Record<string, unknown>) => boolean,
) {
  await expect
    .poll(async () => (await graphPim()).events.some((e) => e.subject === subject && check(e)), {
      timeout: 30_000,
    })
    .toBe(true);
}

interface EwsItem {
  id: string;
  subject: string | null;
  read: boolean;
  categories: string[];
  start: string | null;
  start_time_zone: string | null;
}

const ewsItems = (folder = "inbox") => control<EwsItem[]>(server, "ews-messages", { folder });
const ewsSettings = () =>
  control<{
    oof: { state: string; internal: string; external: string };
    rules: { name: string | null; enabled: boolean }[];
  }>(server, "ews-settings");

/** Settings → Accounts → Contoso → "Synchronize this account". */
async function enableContoso(page: Page) {
  await enableAccount(page, "Contoso", 2);
}

/** Settings → Accounts → `name` (an Exchange account) → "Synchronize this
 * account"; waits for `messages` rows in its inbox. */
async function enableAccount(page: Page, name: string, messages: number) {
  await syncAccount(page, name, messages, async (settings) => {
    // An Exchange account shows its engine and options.
    await expect(settings.getByRole("combobox", { name: "Protocol" })).toHaveValue("exchange");
  });
}

/** Opens the Add account dialog and looks up `email`. */
async function lookUp(page: Page, email: string): Promise<Locator> {
  await runFromPalette(page, "add account", "Add account");
  const dialog = page.getByRole("dialog", { name: "Add account" });
  await dialog.getByRole("textbox", { name: "Your name" }).fill("Ada");
  await dialog.getByRole("textbox", { name: "Email address" }).fill(email);
  await dialog.getByRole("button", { name: "Continue" }).click();
  return dialog;
}

/** Test, add and go to the new account's inbox. */
async function finishSetup(page: Page, dialog: Locator) {
  await dialog.getByRole("button", { name: "Test connection" }).click();
  await expect(dialog.getByText("Nail can connect to your servers.")).toBeVisible({
    timeout: 20_000,
  });
  await dialog.getByRole("button", { name: "Add account" }).click();
  await expect(dialog.getByText(/“Ada” was added/)).toBeVisible();
  await dialog.getByRole("button", { name: "Go to the inbox" }).click();
  await expect(dialog).toBeHidden();
}

test("an Exchange Online account is added through Autodiscover and syncs", async ({ page }) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  // Another address of the demo tenant (Ada's is configured already).
  const dialog = await lookUp(page, "grace@contoso.example");
  // Autodiscover: Exchange Online → Microsoft Graph.
  const review = dialog.locator('[data-part="exchange.setup.review"]');
  await expect(review).toHaveAttribute("data-engine", "graph");
  await expect(
    review
      .locator('[data-part="exchange.setup.value"]')
      .getByText("Microsoft Graph (Exchange Online)"),
  ).toBeVisible();
  // The demo's Graph server is outside contoso.example: the user confirms it.
  await expect(dialog.getByRole("button", { name: "Test connection" })).toBeDisabled();
  await review.getByRole("checkbox", { name: /^I trust/ }).check();
  // Sign in with the (mock) Microsoft identity platform.
  await dialog.getByRole("button", { name: /^Sign in with/ }).click();
  const link = dialog.getByRole("link", { name: "Open sign-in page" });
  await expect(link).toBeVisible();
  const authorize = await link.getAttribute("href");
  expect(authorize).toBeTruthy();
  // Plays the browser: the mock signs in and redirects to Nail's listener.
  await page.request.get(defined(authorize, "the sign-in link"));
  await finishSetup(page, dialog);
  const grace = accountSectionNamed(page, "grace@contoso.example");
  await expect(grace).toHaveAttribute("data-account-kind", "exchange");
  await expect(grace).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await expect(messageRows(page).filter({ hasText: "Engine review on Monday" })).toHaveCount(1, {
    timeout: 30_000,
  });
});

test("an on-premises Exchange account is added through Autodiscover (EWS)", async ({ page }) => {
  await page.goto(server.url);
  // Another address of the demo organization (Ada's is configured already).
  const dialog = await lookUp(page, "grace@corp.example");
  const review = dialog.locator('[data-part="exchange.setup.review"]');
  await expect(review).toHaveAttribute("data-engine", "ews");
  await expect(
    review
      .locator('[data-part="exchange.setup.value"]')
      .getByText("Exchange Web Services (Exchange Server)"),
  ).toBeVisible();
  await review.getByRole("checkbox", { name: /^I trust/ }).check();
  // A password (Basic over the loopback connection of the demo).
  await dialog.getByRole("radio", { name: "With a password" }).check();
  await dialog.getByRole("textbox", { name: "User name" }).fill("ada@corp.example");
  await dialog.getByLabel("Password", { exact: true }).fill("corp-secret");
  await finishSetup(page, dialog);
  const ada = accountSectionNamed(page, "grace@corp.example");
  await expect(ada).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await expect(messageRows(page).filter({ hasText: "Quarterly numbers" })).toHaveCount(1, {
    timeout: 30_000,
  });
  // Server-side mail arrives through streaming notifications.
  await control(server, "ews-deliver", {
    raw: "From: Grace <grace@corp.example>\nTo: ada@corp.example\nSubject: Pushed by EWS\nMessage-ID: <push@corp.example>\n\nHello\n",
  });
  await expect(messageRows(page).filter({ hasText: "Pushed by EWS" })).toHaveCount(1, {
    timeout: 30_000,
  });
});

test("Exchange mail syncs, takes flags, moves, categories as tags and sends", async ({ page }) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await enableContoso(page);

  // Focused Inbox tabs filter the inbox.
  const tabs = page.locator('[data-part="focused-tabs"]');
  await tabs.getByRole("button", { name: "Other", exact: true }).click();
  await expect(messageRows(page)).toHaveCount(1);
  await expect(messageRows(page).first()).toContainText("Fabrikam newsletter, October");
  await tabs.getByRole("button", { name: "All", exact: true }).click();
  await expect(messageRows(page)).toHaveCount(2);

  // Flag: replayed to Graph.
  await messageRows(page).filter({ hasText: "Fabrikam newsletter" }).click();
  await readerAction(page, "Flag");
  await expect
    .poll(async () => (await graphMessage("Fabrikam newsletter, October"))?.flag)
    .toBe("flagged");

  // A category on the server shows as a tag with its colour.
  await messageRows(page).filter({ hasText: "Engine review on Monday" }).click();
  await expect(
    page.locator('[data-part="exchange.categories"]').getByText("Red category"),
  ).toBeVisible();

  // Move to Inbox/Receipts: the server moves it too.
  await moveSelectionTo(page, "Receipts");
  await expect(messageRows(page)).toHaveCount(1);
  const receipts = defined(
    (await graphFolders()).find((f) => f.name === "Receipts"),
    "the Receipts folder",
  );
  await expect
    .poll(async () => (await graphMessage("Engine review on Monday"))?.folder)
    .toBe(receipts.id);

  // A new category written as a tag on a message.
  await page.getByRole("button", { name: "New category" }).click();
  const editor = page.getByRole("dialog", { name: "Category" });
  await editor.getByRole("textbox", { name: "Name" }).fill("Urgent");
  await editor.getByRole("button", { name: "Create" }).click();
  await expect(editor).toBeHidden();
  await expect
    .poll(async () => (await graphSettings()).categories.map((c) => c.displayName))
    .toContain("Urgent");
  // Tags are categories on the message.
  await messageRows(page).filter({ hasText: "Fabrikam newsletter" }).click();
  await readerAction(page, "Categories…");
  const apply = page.getByRole("dialog", { name: "Apply categories" });
  await apply.getByRole("checkbox", { name: "Urgent" }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
  await expect
    .poll(async () => (await graphMessage("Fabrikam newsletter, October"))?.categories)
    .toContain("Urgent");

  // Sending goes through Graph.
  await messageList(page).focus();
  const composer = await newMessage(page, {
    to: "bob@contoso.example",
    subject: "Sent through Microsoft Graph",
    text: "Hello Bob",
  });
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expectSentThrough(server, "graph-sent", "Sent through Microsoft Graph");
});

test("a shared mailbox appears and automatic replies are set and read back", async ({ page }) => {
  await page.goto(server.url);
  await enableContoso(page);
  // A shared mailbox: its folders under @support@contoso.example.
  const mailboxes = page.locator('[data-part="exchange-mailboxes"]');
  await mailboxes.getByRole("button", { name: "Add shared mailbox…" }).click();
  const dialog = page.getByRole("dialog", { name: "Shared mailbox" });
  await dialog.getByRole("textbox", { name: "Mailbox address" }).fill("support@contoso.example");
  await dialog.getByRole("button", { name: "Open mailbox" }).click();
  await expect(dialog).toBeHidden();
  const tree = page.locator('[data-part="folder-tree"]');
  await expect(tree.locator('[data-part="exchange.mailbox-badge"]')).toBeVisible({
    timeout: 30_000,
  });
  // M5b: section (1) → mailbox (2) → its Inbox (3).
  await tree.getByRole("treeitem", { name: /^Inbox/, level: 3 }).click();
  await expect(messageRows(page).filter({ hasText: "Ticket 4711: login fails" })).toHaveCount(1, {
    timeout: 30_000,
  });

  // Automatic replies: on, with a text, on the server.
  await saveVacationReply(page, "Contoso", "Back on Monday.");
  // The demo's replies were scheduled: the dates stay, the text is new.
  await expect
    .poll(async () => String((await graphSettings()).automatic_replies.internalReplyMessage), {
      timeout: 30_000,
    })
    .toContain("Back on Monday.");
  // Read back through the Exchange editor, from the banner: the demo's
  // replies are on around today.
  await showView(page, "Mail");
  await page
    .locator('[data-part="automatic-replies-banner"]')
    .getByRole("button", { name: /^Edit/ })
    .click();
  const replies = page.getByRole("dialog", { name: "Automatic replies" });
  await expect(replies.getByRole("textbox", { name: /in my organization/ })).toHaveValue(
    /Back on Monday\./,
  );
});

test("a Nail rule is placed as an Exchange inbox rule", async ({ page }) => {
  await page.goto(server.url);
  await enableContoso(page);
  await messageRows(page).filter({ hasText: "Fabrikam newsletter" }).click();
  await readerAction(page, "Create rule from message");
  const rules = page.getByRole("dialog", { name: "Filter rules" });
  const editor = rules.getByRole("form", { name: "Rule" });
  await expect(editor.locator('[data-part="rules.condition"]').first()).toBeVisible();
  // Filed into Inbox/Receipts: something Exchange can run itself.
  await editor.getByRole("button", { name: "Add action" }).click();
  await editor.getByRole("textbox", { name: "Folder path" }).fill("Inbox/Receipts");
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor.getByText("The rule is saved.")).toBeVisible();
  // The rule list shows where it runs for the Exchange account.
  await expect(rules.locator('[data-part="exchange.rule-badge"]').first()).toBeVisible({
    timeout: 30_000,
  });
  await expect.poll(async () => (await graphSettings()).rules.length, { timeout: 30_000 }).toBe(3);
});

test("a room is booked, a meeting answered and a To Do task synced", async ({ page }) => {
  await page.clock.setFixedTime(PAGE_NOW);
  await page.goto(server.url);
  await enableContoso(page);
  // To Do: a task in the account's list is created on Graph.
  await showView(page, "Tasks");
  const tasks = viewRegion(page, "Tasks");
  await tasks.getByLabel("List").selectOption({ label: "Tasks" });
  await tasks.getByLabel(/^Add a task to/).fill("Order the badges");
  await tasks.getByRole("button", { name: "Add", exact: true }).click();
  await expect
    .poll(async () => (await graphPim()).tasks.some((t) => t.task.title === "Order the badges"), {
      timeout: 30_000,
    })
    .toBe(true);
  await page.keyboard.press("Escape");

  // A meeting in the Exchange calendar with a room from the room finder.
  await showView(page, "Calendar");
  const calendar = viewRegion(page, "Calendar");
  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title", { exact: true }).fill("Design review");
  await editor.getByLabel("Calendar", { exact: true }).selectOption({ label: "Calendar" });
  await editor.getByRole("button", { name: "Find a room…" }).click();
  const room = editor.getByRole("button", { name: "Add room Conf Room Baker" });
  await expect(room).toBeVisible({ timeout: 20_000 });
  await room.click();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expectGraphEvent("Design review", (e) =>
    JSON.stringify(e).includes("baker@contoso.example"),
  );

  // An invitation in the Exchange calendar is answered through Graph's
  // meeting API (no iMIP mail).
  await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  await eventChip(calendar, "Architecture review").click();
  await expect(editor).toBeVisible();
  await editor.getByRole("button", { name: "Accept", exact: true }).click();
  const answers = async () =>
    (await control<{ action: string }[]>(server, "graph-meetings")).map((a) => a.action);
  await expect.poll(answers, { timeout: 30_000 }).toContain("accept");
  expect(await control<string[]>(server, "graph-sent")).toEqual([]);

  // A sync keeps the answer, and a later edit of the own copy is an
  // update, not a second answer to the organizer.
  await page.keyboard.press("Escape");
  await expect(editor).toBeHidden();
  await syncCalendars(page);
  await eventChip(calendar, "Architecture review").click();
  await expect(editor.getByRole("button", { name: "Accept", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(
    editor
      .getByRole("group", { name: "People and rooms" })
      .getByRole("listitem")
      .filter({ hasText: "(you)" }),
  ).toContainText("Accepted");
  await editor.getByLabel("Show me as", { exact: true }).selectOption({ label: "Free" });
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expectGraphEvent("Architecture review", (e) => e.showAs === "free");
  expect(await answers()).toEqual(["accept"]);
});

test("Exchange Server (EWS) mail moves, takes categories, sends, sets automatic replies and answers a meeting", async ({
  page,
}) => {
  await page.clock.setFixedTime(PAGE_NOW);
  await page.goto(server.url);
  await enableAccount(page, "Corp", 3);

  // Move to Archive: replayed through EWS (MoveItem).
  await messageRows(page).filter({ hasText: "Quarterly numbers" }).click();
  await moveSelectionTo(page, "Archive");
  await expect(messageRows(page)).toHaveCount(2);
  await expect
    .poll(async () => (await ewsItems("Archive")).map((i) => i.subject), { timeout: 30_000 })
    .toContain("Quarterly numbers");

  // A category of the server's master list, set as a tag.
  await messageRows(page).filter({ hasText: "Password expires in 7 days" }).click();
  await readerAction(page, "Categories…");
  const apply = page.getByRole("dialog", { name: "Apply categories" });
  await apply.getByRole("checkbox", { name: "Work" }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
  await expect
    .poll(
      async () =>
        (await ewsItems()).find((i) => i.subject === "Password expires in 7 days")?.categories,
      { timeout: 30_000 },
    )
    .toContain("Work");

  // Sending goes through EWS (SendAndSaveCopy).
  await messageList(page).focus();
  const composer = await newMessage(page, {
    to: "bob@corp.example",
    subject: "Sent through EWS",
    text: "Hello Bob",
  });
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expectSentThrough(server, "ews-sent", "Sent through EWS");

  // Automatic replies through the vacation editor (SetUserOofSettings).
  await saveVacationReply(page, "Corp", "Away until Monday.");
  await expect
    .poll(async () => (await ewsSettings()).oof.internal, { timeout: 30_000 })
    .toContain("Away until Monday.");
  await page.keyboard.press("Escape");

  // An invitation in the Exchange Server calendar is answered through EWS
  // (AcceptItem), not by iMIP mail.
  await showView(page, "Calendar");
  const calendar = viewRegion(page, "Calendar");
  await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  await calendar
    .getByRole("button", { name: /^Board review,/ })
    .first()
    .click({ timeout: 30_000 });
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await expect(editor).toBeVisible();
  await editor.getByRole("button", { name: "Accept", exact: true }).click();
  await expect
    .poll(
      async () =>
        (await control<{ kind: string; subject: string }[]>(server, "ews-meetings")).some(
          (m) => m.kind === "Accept" && m.subject.includes("Board review"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  expect(
    (await control<string[]>(server, "ews-sent")).some((raw) => raw.includes("text/calendar")),
  ).toBe(false);
});
