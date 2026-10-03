import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import {
  accountSectionNamed,
  openSettingsTab,
  readerAction,
  runFromPalette,
  sectionInbox,
  showView,
  viewRegion,
} from "../support/views";

/**
 * Wave M3c (ADR 0077) against `nail serve --seed demo-multi`: Exchange
 * Online through the mock Graph server (the "Contoso" account, switched off
 * at first; signed in through the mock OAuth2 server) and an on-premises
 * Exchange through the mock EWS server, both found by the mock Autodiscover
 * (`autodiscover.contoso.example`, `autodiscover.corp.example`). Controls:
 * `graph-*` (the mock Graph tenant), `ews-*` (the mock EWS server).
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

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

/** Settings → Accounts → `name` → "Synchronize this account"; waits for
 * `messages` rows in its inbox. */
async function enableAccount(page: Page, name: string, messages: number) {
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Accounts");
  await settings.getByRole("button", { name: new RegExp(`^${name}`) }).click();
  // An Exchange account shows its engine and options.
  await expect(settings.getByRole("combobox", { name: "Protocol" })).toHaveValue("exchange");
  await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  await showView(page, "Mail");
  const account = accountSectionNamed(page, name);
  await expect(account).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await sectionInbox(account).click();
  await expect(rows(page)).toHaveCount(messages, { timeout: 30_000 });
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
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17);
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
  await page.request.get(authorize!);
  await finishSetup(page, dialog);
  const grace = accountSectionNamed(page, "grace@contoso.example");
  await expect(grace).toHaveAttribute("data-account-kind", "exchange");
  await expect(grace).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await expect(rows(page).filter({ hasText: "Engine review on Monday" })).toHaveCount(1, {
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
  await expect(rows(page).filter({ hasText: "Quarterly numbers" })).toHaveCount(1, {
    timeout: 30_000,
  });
  // Server-side mail arrives through streaming notifications.
  await control(server, "ews-deliver", {
    raw: "From: Grace <grace@corp.example>\nTo: ada@corp.example\nSubject: Pushed by EWS\nMessage-ID: <push@corp.example>\n\nHello\n",
  });
  await expect(rows(page).filter({ hasText: "Pushed by EWS" })).toHaveCount(1, {
    timeout: 30_000,
  });
});

test("Exchange mail syncs, takes flags, moves, categories as tags and sends", async ({ page }) => {
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17);
  await enableContoso(page);

  // Focused Inbox tabs filter the inbox.
  const tabs = page.locator('[data-part="focused-tabs"]');
  await tabs.getByRole("button", { name: "Other", exact: true }).click();
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("Fabrikam newsletter, October");
  await tabs.getByRole("button", { name: "All", exact: true }).click();
  await expect(rows(page)).toHaveCount(2);

  // Flag: replayed to Graph.
  await rows(page).filter({ hasText: "Fabrikam newsletter" }).click();
  await readerAction(page, "Flag");
  await expect
    .poll(async () => (await graphMessage("Fabrikam newsletter, October"))?.flag)
    .toBe("flagged");

  // A category on the server shows as a tag with its colour.
  await rows(page).filter({ hasText: "Engine review on Monday" }).click();
  await expect(
    page.locator('[data-part="exchange.categories"]').getByText("Red category"),
  ).toBeVisible();

  // Move to Inbox/Receipts: the server moves it too.
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("v");
  const move = page.getByRole("dialog", { name: "Move to folder" });
  await move.getByRole("button", { name: "Receipts", exact: true }).click();
  await expect(rows(page)).toHaveCount(1);
  const receipts = (await graphFolders()).find((f) => f.name === "Receipts")!;
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
  await rows(page).filter({ hasText: "Fabrikam newsletter" }).click();
  await readerAction(page, "Categories…");
  const apply = page.getByRole("dialog", { name: "Categories of this message" });
  await apply.getByRole("checkbox", { name: "Urgent" }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
  await expect
    .poll(async () => (await graphMessage("Fabrikam newsletter, October"))?.categories)
    .toContain("Urgent");

  // Sending goes through Graph.
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("c");
  const composer = page.getByRole("dialog", { name: "New message" });
  await composer.getByRole("textbox", { name: "To" }).fill("bob@contoso.example");
  await composer.getByRole("textbox", { name: "Subject" }).fill("Sent through Microsoft Graph");
  await composer.getByRole("textbox", { name: "Message" }).fill("Hello Bob");
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expect
    .poll(
      async () =>
        (await control<string[]>(server, "graph-sent")).some((raw) =>
          raw.includes("Sent through Microsoft Graph"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
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
  await expect(rows(page).filter({ hasText: "Ticket 4711: login fails" })).toHaveCount(1, {
    timeout: 30_000,
  });

  // Automatic replies: on, with a text, on the server.
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Vacation reply");
  const vacation = settings.locator('[data-part="vacation-editor"]');
  await vacation.getByRole("combobox", { name: "Account" }).selectOption({ label: "Contoso" });
  await vacation.getByRole("checkbox", { name: /Send a vacation reply/ }).check();
  await vacation.getByRole("textbox", { name: "Message" }).fill("Back on Monday.");
  await vacation.getByRole("button", { name: "Save", exact: true }).click();
  // The demo's replies were scheduled: the dates stay, the text is new.
  await expect
    .poll(async () => String((await graphSettings()).automatic_replies.internalReplyMessage), {
      timeout: 30_000,
    })
    .toContain("Back on Monday.");
  // Read back through the Exchange editor.
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
  await rows(page).filter({ hasText: "Fabrikam newsletter" }).click();
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
  await editor.getByLabel("Title").fill("Design review");
  await editor.getByLabel("Calendar", { exact: true }).selectOption({ label: "Calendar" });
  await editor.getByRole("button", { name: "Find a room…" }).click();
  const room = editor.getByRole("button", { name: "Add room Conf Room Baker" });
  await expect(room).toBeVisible({ timeout: 20_000 });
  await room.click();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect
    .poll(
      async () =>
        (await graphPim()).events.some(
          (e) =>
            e.subject === "Design review" && JSON.stringify(e).includes("baker@contoso.example"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);

  // An invitation in the Exchange calendar is answered through Graph's
  // meeting API (no iMIP mail).
  await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  await calendar
    .getByRole("button", { name: /^Architecture review,/ })
    .first()
    .click();
  await expect(editor).toBeVisible();
  await editor.getByRole("button", { name: "Accept", exact: true }).click();
  await expect
    .poll(
      async () =>
        (await control<{ action: string }[]>(server, "graph-meetings")).map((a) => a.action),
      { timeout: 30_000 },
    )
    .toContain("accept");
  expect(await control<string[]>(server, "graph-sent")).toEqual([]);
});

test("Exchange Server (EWS) mail moves, takes categories, sends, sets automatic replies and answers a meeting", async ({
  page,
}) => {
  await page.goto(server.url);
  await enableAccount(page, "Corp", 3);

  // Move to Archive: replayed through EWS (MoveItem).
  await rows(page).filter({ hasText: "Quarterly numbers" }).click();
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("v");
  const move = page.getByRole("dialog", { name: "Move to folder" });
  await move.getByRole("button", { name: "Archive", exact: true }).click();
  await expect(rows(page)).toHaveCount(2);
  await expect
    .poll(async () => (await ewsItems("Archive")).map((i) => i.subject), { timeout: 30_000 })
    .toContain("Quarterly numbers");

  // A category of the server's master list, set as a tag.
  await rows(page).filter({ hasText: "Password expires in 7 days" }).click();
  await readerAction(page, "Categories…");
  const apply = page.getByRole("dialog", { name: "Categories of this message" });
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
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("c");
  const composer = page.getByRole("dialog", { name: "New message" });
  await composer.getByRole("textbox", { name: "To" }).fill("bob@corp.example");
  await composer.getByRole("textbox", { name: "Subject" }).fill("Sent through EWS");
  await composer.getByRole("textbox", { name: "Message" }).fill("Hello Bob");
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expect
    .poll(
      async () =>
        (await control<string[]>(server, "ews-sent")).some((raw) =>
          raw.includes("Sent through EWS"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);

  // Automatic replies through the vacation editor (SetUserOofSettings).
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Vacation reply");
  const vacation = settings.locator('[data-part="vacation-editor"]');
  await vacation.getByRole("combobox", { name: "Account" }).selectOption({ label: "Corp" });
  await vacation.getByRole("checkbox", { name: /Send a vacation reply/ }).check();
  await vacation.getByRole("textbox", { name: "Message" }).fill("Away until Monday.");
  await vacation.getByRole("button", { name: "Save", exact: true }).click();
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
