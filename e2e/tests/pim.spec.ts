import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { McpSession } from "../support/mcp";
import { control, startNail, tempDir, type NailServer } from "../support/nail";

/**
 * Wave M3a (ADR 0063) against `nail serve --seed demo-multi`: calendars,
 * contacts and tasks with the core's PIM sources — a mock CalDAV / CardDAV
 * server (`work-calendars`, `work-contacts`; controls `dav-objects`,
 * `dav-put`), a read-only ICS subscription (`holidays`; `feeds`), the
 * adjustable clock (`clock-set`) — plus `--launch` for opened files and
 * the MCP server.
 */

let server: NailServer;

test.afterEach(async () => {
  await server?.stop();
});

async function openApp(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
}

const mainToolbar = (page: Page) => page.getByRole("toolbar", { name: "Main toolbar" });

/** Opens the calendar window from the main toolbar. */
async function openCalendar(page: Page): Promise<Locator> {
  await mainToolbar(page).getByRole("button", { name: "Calendar" }).click();
  const window = page.getByRole("dialog", { name: "Calendar", exact: true });
  await expect(window).toBeVisible();
  return window;
}

/** An event chip in the calendar views, by the start of its label. */
const chip = (scope: Locator, title: string) =>
  scope.getByRole("button", { name: new RegExp(`^${title},`) }).first();

/** Today + `days` at `hour`:00 UTC, as an iCalendar UTC time. */
function icalTime(days: number, hour: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + days);
  d.setUTCHours(hour, 0, 0, 0);
  return d
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d{3}/, "");
}

/** Today + `days` at `hour`:00 UTC, in Unix seconds. */
function unixAt(days: number, hour: number, minute = 0): number {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + days);
  d.setUTCHours(hour, minute, 0, 0);
  return Math.floor(d.getTime() / 1000);
}

function serverEvent(uid: string, title: string, days: number, hour: number, alarm = ""): string {
  return [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Another client//EN",
    "BEGIN:VEVENT",
    `UID:${uid}`,
    "DTSTAMP:20260101T000000Z",
    `DTSTART:${icalTime(days, hour)}`,
    `DTEND:${icalTime(days, hour + 1)}`,
    `SUMMARY:${title}`,
    ...(alarm
      ? ["BEGIN:VALARM", "ACTION:DISPLAY", `TRIGGER:${alarm}`, `DESCRIPTION:${title}`, "END:VALARM"]
      : []),
    "END:VEVENT",
    "END:VCALENDAR",
    "",
  ].join("\r\n");
}

interface DavObject {
  name: string;
  etag: string;
  data: string;
}

const davObjects = (collection: string) =>
  control<DavObject[]>(server, "dav-objects", { collection });

/** Syncs every source from the calendar window's toolbar. */
async function syncNow(page: Page) {
  await page
    .getByRole("toolbar", { name: "Calendar actions" })
    .getByRole("button", { name: "Sync calendars and contacts" })
    .click();
}

test("a recurring event is changed for this and the following occurrences", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title").fill("Book club");
  await editor.getByLabel("Repeats").selectOption("weekly");
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect(chip(calendar, "Book club")).toBeVisible();

  // Next week: change this and the following occurrences.
  await calendar.getByRole("button", { name: "Next", exact: true }).click();
  await chip(calendar, "Book club").click();
  await expect(editor).toBeVisible();
  await editor.getByRole("radio", { name: "This and following occurrences" }).check();
  await editor.getByLabel("Title").fill("Book club (new room)");
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect(chip(calendar, "Book club \\(new room\\)")).toBeVisible();
  await calendar.getByRole("button", { name: "Next", exact: true }).click();
  await expect(chip(calendar, "Book club \\(new room\\)")).toBeVisible();
  // This week keeps the original series.
  await calendar.getByRole("button", { name: "Today" }).click();
  await expect(chip(calendar, "Book club")).toBeVisible();
  await expect(chip(calendar, "Book club \\(new room\\)")).toHaveCount(0);
});

test("an event syncs to the CalDAV server and a server-side change appears", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Agenda" }).click();
  // The server's sample event.
  await expect(chip(calendar, "Team sync")).toBeVisible();

  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title").fill("Dentist");
  await editor.getByLabel("Calendar", { exact: true }).selectOption({ label: "Personal" });
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect
    .poll(async () =>
      (await davObjects("personal")).some((o) => o.data.includes("SUMMARY:Dentist")),
    )
    .toBe(true);

  // Another client adds an event on the server.
  await control(server, "dav-put", {
    collection: "personal",
    name: "lunch.ics",
    data: serverEvent("lunch@other.example", "Lunch with Charles", 1, 12),
  });
  await syncNow(page);
  await expect(chip(calendar, "Lunch with Charles")).toBeVisible();
});

test("an ICS subscription shows its events read-only", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Agenda" }).click();
  const holiday = chip(calendar, "Founders' Day");
  await expect(holiday).toBeVisible();
  await holiday.click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await expect(editor.getByText("This item is read-only.")).toBeVisible();
  await expect(editor.getByRole("button", { name: "Save" })).toHaveCount(0);
  await editor.getByRole("button", { name: "Close" }).first().click();
  await expect(editor).toBeHidden();

  // A new subscription from the sources dialog.
  const feeds = await control<{ town: string }>(server, "feeds");
  await page
    .getByRole("toolbar", { name: "Calendar actions" })
    .getByRole("button", { name: "Calendars and address books" })
    .click();
  const sources = page.getByRole("dialog", { name: "Calendars and address books", exact: true });
  await sources.getByLabel("Name").fill("Town events");
  await sources.getByLabel("Address of the calendar feed").fill(feeds.town);
  await sources.getByRole("button", { name: "Add" }).click();
  await expect(sources.getByText("Added. Nail is syncing it now.")).toBeVisible();
  await sources.getByRole("button", { name: "Close" }).first().click();
  await expect(chip(calendar, "Town fair")).toBeVisible();
});

test("a reminder fires at the controlled clock time", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Agenda" }).click();
  await control(server, "dav-put", {
    collection: "personal",
    name: "checkup.ics",
    data: serverEvent("checkup@other.example", "Checkup", 1, 10, "-PT10M"),
  });
  await syncNow(page);
  await expect(chip(calendar, "Checkup")).toBeVisible();
  const toast = page.getByRole("group", { name: "Reminder: Checkup" });
  await control(server, "clock-set", { at: unixAt(1, 9, 40) });
  await page.waitForTimeout(1500);
  await expect(toast).toHaveCount(0);
  // Ten minutes before the start (09:50 UTC tomorrow) it fires.
  await control(server, "clock-set", { at: unixAt(1, 9, 50) + 5 });
  await expect(toast).toBeVisible({ timeout: 15_000 });
  await toast.getByRole("button", { name: "Dismiss" }).click();
  await expect(toast).toHaveCount(0);
});

test("a contact syncs to CardDAV and is suggested in the composer", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  await mainToolbar(page).getByRole("button", { name: "Contacts" }).click();
  const contacts = page.getByRole("dialog", { name: "Contacts", exact: true });
  await expect(contacts.getByRole("option", { name: /Grace Hopper/ })).toBeVisible();
  await contacts.getByRole("button", { name: "New contact" }).click();
  const editor = page.getByRole("dialog", { name: "Contact", exact: true });
  await editor.getByLabel("Address book", { exact: true }).selectOption({ label: "Contacts" });
  await editor.getByLabel("First name").fill("Katherine");
  await editor.getByLabel("Last name").fill("Johnson");
  await editor.getByRole("button", { name: "Add (Email)" }).click();
  await editor.getByRole("textbox", { name: "Email 1" }).fill("katherine@nasa.example");
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect
    .poll(async () =>
      (await davObjects("contacts")).some((o) => o.data.includes("FN:Katherine Johnson")),
    )
    .toBe(true);
  await contacts.getByRole("button", { name: "Close" }).first().click();

  await page.keyboard.press("c");
  const composer = page.getByRole("dialog", { name: "New message", exact: true });
  await composer.getByRole("textbox", { name: "To" }).pressSequentially("Kath");
  const suggestions = composer.getByRole("listbox", { name: "Suggested addresses" });
  await expect(suggestions.getByRole("option", { name: /katherine@nasa\.example/ })).toBeVisible();
  await suggestions.getByRole("option", { name: /katherine@nasa\.example/ }).click();
  await expect(composer.getByRole("textbox", { name: "To" })).toHaveValue(
    /Katherine Johnson <katherine@nasa\.example>/,
  );
});

test("duplicate contacts are merged", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  await mainToolbar(page).getByRole("button", { name: "Contacts" }).click();
  const contacts = page.getByRole("dialog", { name: "Contacts", exact: true });
  await expect(contacts.getByRole("option", { name: /Grace Hopper/ })).toHaveCount(1);
  // The same person in the local address book.
  await contacts.getByRole("button", { name: "New contact" }).click();
  const editor = page.getByRole("dialog", { name: "Contact", exact: true });
  await editor.getByLabel("Address book", { exact: true }).selectOption({ label: "My contacts" });
  await editor.getByLabel("First name").fill("Grace");
  await editor.getByLabel("Last name").fill("Hopper");
  await editor.getByRole("button", { name: "Add (Email)" }).click();
  await editor.getByRole("textbox", { name: "Email 1" }).fill("grace@navy.example");
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect(contacts.getByRole("option", { name: /Grace Hopper/ })).toHaveCount(2);

  await contacts.getByRole("button", { name: "Find duplicates…" }).click();
  const merge = page.getByRole("dialog", { name: "Merge contacts", exact: true });
  const groups = merge.getByRole("list", { name: "Possible duplicates" });
  await groups.getByRole("button", { name: "Merge these" }).first().click();
  await merge.getByRole("button", { name: "Merge 2 contacts" }).click();
  await expect(merge).toBeHidden();
  // Linked across address books: the person shows once.
  await expect(contacts.getByRole("option", { name: /Grace Hopper/ })).toHaveCount(1);
});

test("a task gets subtasks", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  await mainToolbar(page).getByRole("button", { name: "Tasks" }).click();
  const window = page.getByRole("dialog", { name: "Tasks", exact: true });
  const tree = window.getByRole("tree", { name: "Tasks" });
  await window.getByLabel(/^Add a task to/).fill("Plan the trip");
  await window.getByRole("button", { name: "Add", exact: true }).click();
  const trip = tree.getByRole("treeitem", { name: /^Plan the trip/ });
  await expect(trip).toBeVisible();
  await trip.focus();
  await page.keyboard.press("Enter");
  const editor = page.getByRole("dialog", { name: "Task", exact: true });
  await editor.getByLabel("New subtask").fill("Book the flights");
  await editor.getByLabel("New subtask").press("Enter");
  await expect(editor.getByRole("checkbox", { name: "Book the flights" })).toBeVisible();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  const flights = tree.getByRole("treeitem", { name: /^Book the flights/ });
  await expect(flights).toHaveAttribute("aria-level", "2");
  await window.getByRole("checkbox", { name: "Mark “Book the flights” as done" }).click();
  await expect(flights).toHaveCount(0); // done subtasks are hidden
});

test("an .ics file given at launch is imported", async ({ page }) => {
  const dir = tempDir("launch");
  const file = path.join(dir, "launch.ics");
  writeFileSync(file, serverEvent("launch@example.org", "Launch party", 1, 18));
  server = await startNail({ seed: "demo-multi", launch: [file] });
  await page.goto(server.url);
  const dialog = page.getByRole("dialog", { name: "Import calendar or contacts", exact: true });
  await expect(
    dialog.getByText("launch.ics: events: 1, tasks: 0, contacts: 0, groups: 0."),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "Import" }).click();
  await expect(dialog.getByRole("status")).toHaveText("Imported: 1 new, 0 updated, 0 skipped.");
  await dialog.getByRole("button", { name: "Close" }).first().click();
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Agenda" }).click();
  await expect(chip(calendar, "Launch party")).toBeVisible();
});

test("an MCP client lists events after a grant", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  await openApp(page);
  await mainToolbar(page).getByRole("button", { name: "Settings" }).click();
  const settings = page.getByRole("dialog", { name: "Settings", exact: true });
  await settings.getByRole("tab", { name: "MCP clients" }).click();
  const serverLine = settings.locator('[data-part="mcp-clients.server"]');
  await expect(serverLine).toHaveAttribute("data-state", "running");
  const url = /(http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec((await serverLine.textContent()) ?? "")?.[1];
  expect(url).toBeDefined();
  await settings.getByRole("textbox", { name: "Client name" }).fill("Calendar agent");
  await settings.getByRole("button", { name: "Create client" }).click();
  const token = (await settings.locator('[data-part="mcp-token.value"]').textContent())?.trim();
  expect(token).toMatch(/^nail_mcp_/);

  const mcp = new McpSession(url!, token!);
  await mcp.open();
  const before = await mcp.request("tools/call", { name: "list_events", arguments: {} });
  expect(JSON.stringify(before)).toMatch(/denied/);

  const form = settings.getByRole("form", { name: "Access of Calendar agent" });
  await form.getByRole("checkbox", { name: "Read mail" }).check();
  await form.getByRole("checkbox", { name: /All accounts/ }).check();
  await form.getByRole("button", { name: "Save access" }).click();
  await expect(form.getByRole("status")).toHaveText("Access saved.");

  await expect
    .poll(async () =>
      JSON.stringify(await mcp.request("tools/call", { name: "list_events", arguments: {} })),
    )
    .toMatch(/Team sync/);
});
