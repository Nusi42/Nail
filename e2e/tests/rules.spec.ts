import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest, control, serverMessages } from "../support/nail";
import {
  readerAction,
  runFromPalette,
  messageRows,
  DEMO_MULTI_CONVERSATIONS,
  openMail,
} from "../support/views";

/**
 * Filter rules (ADR 0024) against the `demo-multi` mock servers: an arrival
 * rule moves new mail through the operation queue and its run is undone as
 * one unit; the test-rule preview explains its matches; a rule is created
 * from a message.
 */

const server = nailPerTest({ seed: "demo-multi" });

const rulesFile = () => readFileSync(path.join(server.configDir, "rules.kdl"), "utf8");

async function openRules(page: Page): Promise<Locator> {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await runFromPalette(page, "filter rules", "Filter rules");
  const window = page.getByRole("dialog", { name: "Filter rules" });
  await expect(window).toBeVisible();
  return window;
}

/** Starts a new rule and enters it in the text view. */
async function newRuleFromText(window: Locator, text: string): Promise<Locator> {
  await window.getByRole("button", { name: "New rule" }).click();
  const editor = window.getByRole("form", { name: "Rule" });
  await editor.getByRole("button", { name: "Text" }).click();
  await editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" }).fill(text);
  await editor.getByRole("button", { name: "Editor" }).click();
  return editor;
}

const INVOICE_RULE = `rule "Invoices to Archive" trigger="arrival" {
    when {
        subject contains="invoice 77"
    }
    then {
        archive
        flag
    }
}
`;

test("an arrival rule moves new mail and its run can be undone", async ({ page }) => {
  const window = await openRules(page);
  const editor = await newRuleFromText(window, INVOICE_RULE);
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("Invoices to Archive");
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor.getByText("The rule is saved.")).toBeVisible();
  expect(rulesFile()).toContain('rule "Invoices to Archive"');

  // New mail arrives (IDLE) and the rule files it in Archive, flagged.
  await control(server, "imap-deliver", {
    raw: "From: Shop <shop@example.org>\r\nTo: ada@work.example\r\nSubject: Your invoice 77\r\nMessage-ID: <inv77@example.org>\r\n\r\nPlease pay.\r\n",
  });
  await expect
    .poll(async () => (await serverMessages(server, "Archive")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("Your invoice 77");
  const archived = (await serverMessages(server, "Archive")).find(
    (m) => m.subject === "Your invoice 77",
  );
  await expect
    .poll(async () =>
      (await serverMessages(server, "Archive"))
        .find((m) => m.subject === "Your invoice 77")
        ?.flags.includes("\\Flagged"),
    )
    .toBe(true);
  expect(archived).toBeDefined();

  // The run is in the history and undone as one unit.
  const runs = editor.getByRole("region", { name: "Recent runs" });
  await expect(runs.getByText(/^New mail: one message, done$/)).toBeVisible({ timeout: 15_000 });
  await runs.getByRole("button", { name: "Undo this run" }).click();
  await expect(editor.getByText("The run was undone.")).toBeVisible();
  await expect(runs.getByText(/^New mail: one message, undone$/)).toBeVisible();
  await expect
    .poll(async () => (await serverMessages(server, "INBOX")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("Your invoice 77");
  await expect
    .poll(async () =>
      (await serverMessages(server, "INBOX"))
        .find((m) => m.subject === "Your invoice 77")
        ?.flags.includes("\\Flagged"),
    )
    .toBe(false);
  expect((await serverMessages(server, "Archive")).map((m) => m.subject)).not.toContain(
    "Your invoice 77",
  );
});

test("the test-rule preview lists matches and explains them", async ({ page }) => {
  const window = await openRules(page);
  const editor = await newRuleFromText(
    window,
    `rule "Nightingale" {
    when {
        subject contains="nightingale"
    }
    then {
        tag "project"
    }
}
`,
  );
  await editor.getByRole("button", { name: "Test rule" }).click();
  const results = editor.getByRole("region", { name: "Test results" });
  await expect(results.getByRole("status")).toHaveText(/^\d+ messages of \d+ match/);
  const kickoff = results
    .locator('[data-part="rules.test.match"]')
    .filter({ hasText: "Project Nightingale kickoff" })
    .first();
  await expect(kickoff.getByRole("list", { name: "What would happen" })).toContainText("Add tag");
  await expect(kickoff.getByRole("list", { name: "Why it matches" })).toContainText(
    "Subject matched:",
  );
  // A dry run saves nothing.
  expect(rulesFile).toThrow();
});

test("a rule is created from a message", async ({ page }) => {
  await page.goto(server.url);
  const row = messageRows(page).filter({ hasText: "Release notes for M1" });
  await row.click();
  await readerAction(page, "Create rule from message");
  const window = page.getByRole("dialog", { name: "Filter rules" });
  const editor = window.getByRole("form", { name: "Rule" });
  await expect(editor.locator('[data-part="rules.condition"]').first()).toBeVisible();
  // The rule matches the sender and the list's subject tag; the user adds
  // where it goes.
  await editor.getByRole("button", { name: "Add action" }).click();
  await editor.getByRole("textbox", { name: "Folder path" }).fill("Lists");
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor.getByText("The rule is saved.")).toBeVisible();
  const text = rulesFile();
  expect(text).toContain('from equals="users@lists.nail.example"');
  expect(text).toContain('subject starts-with="[nail-users]"');
  expect(text).toContain('move "Lists"');
  await expect(window.getByRole("list", { name: "Rules, in the order they run" })).toContainText(
    "when mail arrives",
  );
});
