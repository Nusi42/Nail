import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator } from "@playwright/test";
import { lengthenAutosave, newMessage } from "../support/compose";
import { nailPerTest } from "../support/nail";
import { composerWindows, messageList, messageRows, openMail } from "../support/views";

/**
 * R8 (mail and windows UI) against `nail serve --seed demo`: the Markdown
 * preview grows with its content (up to a limit), and several unsaved
 * composers that go at once — a view switch that closes every window —
 * are asked about in one prompt that lists them.
 */

const server = nailPerTest({ seed: "demo" });

/** The preview frame's height in pixels. */
async function frameHeight(composer: Locator): Promise<number> {
  const box = await composer.locator('[data-part="composer.preview.frame"]').boundingBox();
  return box?.height ?? 0;
}

test("the Markdown preview grows with its content, up to a limit", async ({ page }) => {
  await openMail(page, server.url);
  const composer = await newMessage(page, { subject: "Preview height" });
  await composer.getByRole("radio", { name: "Markdown" }).click();
  const editor = composer.getByRole("textbox", { name: "Message" });
  await editor.click();
  await page.keyboard.type("One line");
  const frame = composer.locator('[data-part="composer.preview.frame"]');
  await expect(frame).toHaveAttribute("data-state", "measured");
  const short = await frameHeight(composer);

  // Twenty paragraphs: taller, until the limit.
  for (let i = 0; i < 20; i += 1) await page.keyboard.type(`\n\nParagraph ${String(i)}`);
  await expect.poll(() => frameHeight(composer)).toBeGreaterThan(short + 100);
  // Many more: it stops growing (the preview scrolls inside its frame).
  for (let i = 0; i < 40; i += 1) await page.keyboard.type(`\n\nMore ${String(i)}`);
  await page.waitForTimeout(1_000);
  const capped = await frameHeight(composer);
  const max = await frame.evaluate((el) => Number.parseFloat(getComputedStyle(el).maxHeight));
  expect(capped).toBeLessThanOrEqual(max + 1);
});

/** A layout of the user's whose Mail view is the main window (no view host). */
const MAIN_WINDOW_LAYOUT = [
  'layout "Mine" extends="default" {',
  '    window "main" {',
  '        split "horizontal" id="panes" sizes="240px 1fr 1.4fr" {',
  '            folder-tree id="folders" show-unread=#true',
  '            message-list id="list"',
  '            message-view id="message"',
  "        }",
  "    }",
  '    views default="mail" {',
  '        view "mail" label="@view.mail" icon="mail" window="main"',
  '        view "calendar" label="@view.calendar" icon="calendar" window="calendar"',
  '        view "contacts" label="@view.contacts" icon="users" window="contacts"',
  '        view "tasks" label="@view.tasks" icon="square-check-big" window="tasks"',
  '        view "settings" label="@view.settings" icon="settings" window="settings"',
  "    }",
  "}",
  "",
].join("\n");

test.describe("several unsaved composers", () => {
  const mainWindow = nailPerTest({ seed: "demo" }, (s) => {
    writeFileSync(path.join(s.configDir, "layout.kdl"), MAIN_WINDOW_LAYOUT);
  });

  test("one prompt lists every unsaved composer when the view changes", async ({ page }) => {
    await lengthenAutosave(page);
    await page.goto(mainWindow.url);
    await expect(messageRows(page).first()).toBeVisible({ timeout: 30_000 });
    await messageRows(page).first().click();
    await newMessage(page, { subject: "First draft" });
    await messageList(page).focus();
    await newMessage(page, { subject: "Second draft" });
    await expect(composerWindows(page)).toHaveCount(2);

    // Calendar opens as a window; Mail is the main window, so every window goes.
    await messageList(page).focus();
    await page.keyboard.press("Control+2");
    await expect(page.getByRole("dialog", { name: "Calendar", exact: true })).toBeVisible();
    await page.keyboard.press("Control+1");
    const prompts = page.getByRole("dialog", { name: "Unsaved changes" });
    await expect(prompts).toHaveCount(1);
    const prompt = prompts.first();
    await expect(prompt).toContainText("Save changes to 2 items?");
    await expect(prompt.getByRole("listitem")).toHaveText(["First draft", "Second draft"]);
    // Don't save: both go.
    await prompt.getByRole("button", { name: "Don't save" }).click();
    await expect(composerWindows(page)).toHaveCount(0);
    await expect(prompts).toHaveCount(0);
  });
});
