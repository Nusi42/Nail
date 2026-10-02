import { readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { startNail, tempDir, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";

/**
 * M4c WP125 (ADRs 0095, 0099): the recovery key opens encrypted data after
 * the key in the keychain is lost. A seeded `nail serve` keeps its demo
 * keychain in `<data-dir>/demo-keychain.json`; the test encrypts the data,
 * creates a recovery key in the Settings, stops Nail, removes the data
 * key's entry from that file (what a new Windows profile does to the
 * Credential Manager), starts Nail again on the same directories, restores
 * the key in the dialog Nail opens by itself, and restarts once more.
 */

let server: NailServer | undefined;
const dirs = { dataDir: "", configDir: "" };

test.afterEach(async () => {
  await server?.stop();
  for (const dir of [dirs.dataDir, dirs.configDir]) {
    if (dir) rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
});

async function vaultState(nail: NailServer) {
  const answer = await rpcOk(nail, { type: "GetVaultStatus" }, "VaultStatus");
  return answer.status as {
    state: string;
    needs_recovery: boolean;
    localized?: { id: string } | null;
  };
}

test("a recovery key opens the data after the keychain entry was lost", async ({ page }) => {
  dirs.dataDir = tempDir("vault-data");
  dirs.configDir = tempDir("vault-config");
  server = await startNail({ dirs });
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  const welcome = page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Welcome to Nail" });
  await expect(welcome).toBeVisible();

  // Encryption at rest on, then a recovery key from the Settings.
  await rpcOk(server, { type: "SetVaultEncryption", enabled: true }, "VaultStatus");
  await expect
    .poll(async () => (await vaultState(server!)).state, { timeout: 60_000 })
    .toBe("encrypted");
  await page
    .getByRole("toolbar", { name: "Main" })
    .getByRole("button", { name: "Settings" })
    .click();
  const settings = page.locator('[data-part="window"][data-window="settings"]');
  await settings
    .getByRole("tablist")
    .first()
    .getByRole("tab", { name: "Recovery key", exact: true })
    .click();
  await settings.getByRole("button", { name: "Create recovery key" }).click();
  const create = page.locator('[data-part="window"][data-window="security-recovery-create"]');
  await create.getByRole("button", { name: "Create recovery key" }).click();
  const shown = create.locator('[data-part="recovery-key.value"]');
  await expect(shown).toBeVisible({ timeout: 30_000 });
  const key = ((await shown.textContent()) ?? "").replace(/\s+/g, "");
  expect(key.length).toBeGreaterThanOrEqual(56);
  await create
    .getByRole("checkbox", { name: "I have written down, printed or saved the key" })
    .check();
  await create.getByRole("button", { name: "Next" }).click();
  await create.getByLabel("Recovery key").fill(key);
  await create.getByRole("button", { name: "Check", exact: true }).click();
  await expect(create.getByText("Your recovery key works. Keep it safe.")).toBeVisible();
  await server.stop();

  // The data key's entry disappears from the (demo) keychain.
  const vaultFile = JSON.parse(readFileSync(path.join(dirs.dataDir, "vault.json"), "utf8")) as {
    key_ref: string;
  };
  const keychainFile = path.join(dirs.dataDir, "demo-keychain.json");
  const keychain = JSON.parse(readFileSync(keychainFile, "utf8")) as Record<string, string>;
  expect(keychain[vaultFile.key_ref]).toBeDefined();
  delete keychain[vaultFile.key_ref];
  writeFileSync(keychainFile, JSON.stringify(keychain));

  // Nail starts locked and asks for the recovery key by itself.
  server = await startNail({ dirs });
  expect((await vaultState(server)).needs_recovery).toBe(true);
  await page.goto(server.url);
  const restore = page.getByRole("dialog", { name: "Open your data with the recovery key" });
  await expect(restore).toBeVisible({ timeout: 30_000 });
  const scan = await new AxeBuilder({ page }).include('[data-part="window"]').analyze();
  expect(scan.violations.map((v) => v.id)).toEqual([]);
  await restore.getByLabel("Recovery key").fill(key);
  await restore.getByRole("button", { name: "Open with recovery key" }).click();
  await expect(restore.getByText(/Restart Nail to open your data/)).toBeVisible();
  expect((await vaultState(server)).localized?.id).toBe("vault-restart-needed");
  await server.stop();

  // After the restart the data is there again.
  server = await startNail({ dirs });
  const status = await vaultState(server);
  expect(status.needs_recovery).toBe(false);
  expect(status.state).toBe("encrypted");
  await page.goto(server.url);
  await expect(
    page.locator('[data-part="message-list.row"]').filter({ hasText: "Welcome to Nail" }),
  ).toBeVisible();
  await expect(
    page.getByRole("dialog", { name: "Open your data with the recovery key" }),
  ).toHaveCount(0);
});
