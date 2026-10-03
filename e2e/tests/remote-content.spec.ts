import { expect, test } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import { openSettingsTab, showView, viewRegion } from "../support/views";

/**
 * Remote content (M2b, ADR 0041) against `--seed demo-multi`: its proxy
 * reaches only `images.nail.example` (the mock HTTP server) and resolves
 * `private.nail.example` to a private address. Images are blocked until
 * the user allows the sender; then they load through the core's proxy,
 * never from the reader, and private targets stay refused.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

interface RemoteHosts {
  images: string;
  private: string;
  requests: string[];
}

test("remote images load through the proxy after allowing the sender; private targets are refused", async ({
  page,
}) => {
  const hosts = await control<RemoteHosts>(server, "remote-content");
  const raw = [
    "From: Garden Shop <news@shop.example>",
    "To: ada@work.example",
    "Subject: Autumn bulbs are here",
    "Message-ID: <bulbs@shop.example>",
    "Date: Tue, 29 Sep 2026 09:00:00 +0000",
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    `<p>Our catalogue</p><img alt="logo" src="${hosts.images}/logo.png">` +
      `<img alt="inside" src="${hosts.private}/pixel.png">` +
      `<img alt="loopback" src="http://127.0.0.1:9/secret.png">`,
    "",
  ].join("\n");
  await control(server, "imap-deliver", { raw });

  await page.goto(server.url);
  const row = page.locator('[data-part="message-list.row"]', { hasText: "Autumn bulbs" });
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();

  // Blocked: the bar says so, the document has no remote address at all.
  const bar = page.getByRole("region", { name: "Remote content" });
  await expect(bar).toContainText("Nail blocked 3");
  const frame = page.locator('iframe[data-part="message-view.body"]');
  await expect(frame).toHaveAttribute("sandbox", "");
  const blocked = (await frame.getAttribute("srcdoc")) ?? "";
  expect(blocked).toContain("Our catalogue");
  expect(blocked).not.toContain("images.nail.example");
  expect(blocked).not.toContain("/api/proxy/");

  await bar.getByRole("button", { name: "Always load from news@shop.example" }).click();
  await expect(bar).toContainText("because you allowed news@shop.example");
  await expect(frame).toHaveAttribute("srcdoc", /\/api\/proxy\//);
  const srcdoc = (await frame.getAttribute("srcdoc")) ?? "";
  const proxied = [...srcdoc.matchAll(/src="([^"]*\/api\/proxy\/[^"]+)"/g)].map((m) => m[1] ?? "");
  // Two URLs are signed; the literal loopback address never is.
  expect(proxied).toHaveLength(2);
  expect(srcdoc).not.toContain("127.0.0.1:9");
  expect(srcdoc).not.toContain(hosts.images);

  // The image arrives through the proxy (the reader never contacts the host).
  const logo = page.frameLocator('iframe[data-part="message-view.body"]').getByAltText("logo");
  await expect
    .poll(() => logo.evaluate((img: HTMLImageElement) => img.naturalWidth), { timeout: 15_000 })
    .toBe(1);
  const requested = await control<RemoteHosts>(server, "remote-content");
  expect(requested.requests).toContain("/logo.png");

  // The private host's URL is signed, but the proxy refuses the address.
  const privateUrl = proxied.find((url) => url !== proxied[0]) ?? "";
  const statuses = await Promise.all(proxied.map((url) => page.request.get(url)));
  expect(statuses.map((r) => r.status()).sort()).toEqual([200, 403]);
  expect((await page.request.get(privateUrl)).headers()["x-content-type-options"]).toBe("nosniff");
  // A made-up token is a 404; the proxy is never open.
  expect((await page.request.get(`${server.url}api/proxy/forged.token`)).status()).toBe(404);
  expect(requested.requests).not.toContain("/pixel.png");
});

test("remote content can be loaded once without remembering the sender", async ({ page }) => {
  const hosts = await control<RemoteHosts>(server, "remote-content");
  const raw = [
    "From: Tea Club <hello@tea.example>",
    "To: ada@work.example",
    "Subject: Our autumn blends",
    "Message-ID: <blends@tea.example>",
    "Date: Tue, 29 Sep 2026 09:30:00 +0000",
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    `<p>Blends</p><img alt="logo" src="${hosts.images}/logo.png">`,
    "",
  ].join("\n");
  await control(server, "imap-deliver", { raw });
  await page.goto(server.url);
  const row = page.locator('[data-part="message-list.row"]', { hasText: "Our autumn blends" });
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();
  const bar = page.getByRole("region", { name: "Remote content" });
  await bar.getByRole("button", { name: "Load once" }).click();
  await expect(bar).toContainText("Remote content is loaded this time only.");
  const logo = page.frameLocator('iframe[data-part="message-view.body"]').getByAltText("logo");
  await expect
    .poll(() => logo.evaluate((img: HTMLImageElement) => img.naturalWidth), { timeout: 15_000 })
    .toBe(1);
  // Nothing was added to the allow-lists (Settings → Privacy).
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Privacy");
  await expect(settings.getByText("hello@tea.example")).toHaveCount(0);
});
