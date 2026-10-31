import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest, control } from "../support/nail";
import { openSettingsTab, showView, viewRegion, messageRows } from "../support/views";

/**
 * Remote content (M2b, ADR 0041) against `--seed demo-multi`: its proxy
 * reaches only `images.nail.example` (the mock HTTP server) and resolves
 * `private.nail.example` to a private address. Images are blocked until
 * the user allows the sender; then they load through the core's proxy,
 * never from the reader, and private targets stay refused.
 */

const server = nailPerTest({ seed: "demo-multi" });

interface RemoteHosts {
  images: string;
  private: string;
  requests: string[];
}

/** The newsletter's logo loaded in the reader (through the proxy). */
async function expectLogoLoaded(page: Page): Promise<void> {
  const logo = page.frameLocator('iframe[data-part="message-view.body"]').getByAltText("logo");
  await expect
    .poll(() => logo.evaluate((img: HTMLImageElement) => img.naturalWidth), { timeout: 15_000 })
    .toBe(1);
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
  const row = messageRows(page).filter({ hasText: "Autumn bulbs" });
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();

  // Blocked: the bar says so, the document has no remote address at all.
  const bar = page.getByRole("region", { name: "Remote content" });
  await expect(bar).toContainText("Nail blocked 3");
  const frame = page.locator('iframe[data-part="message-view.body"]');
  // P3 (ADR 0164): the app's origin for measuring, never scripts.
  await expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
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
  await expectLogoLoaded(page);
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
  const bar = await openNewsletter(page, {
    from: "Tea Club <hello@tea.example>",
    subject: "Our autumn blends",
    id: "blends@tea.example",
    body: "<p>Blends</p>",
  });
  await bar.getByRole("button", { name: "Load once" }).click();
  await expect(bar).toContainText("Remote content is loaded this time only.");
  await expectLogoLoaded(page);
  // Nothing was added to the allow-lists (Settings → Privacy).
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Privacy");
  await expect(settings.getByText("hello@tea.example")).toHaveCount(0);
});

test("when the reader is narrower than a label, the bar's buttons stay inside it and shorten their labels", async ({
  page,
}) => {
  // Longer than the reader is wide at any window size the layout shows it in.
  const sender = `${"newsletter-for-our-most-valued-customers".repeat(2)}@${"a-very-long-garden-shop-name".repeat(3)}.example`;
  const bar = await openNewsletter(page, {
    from: `Garden Shop <${sender}>`,
    subject: "Spring seeds",
    id: "seeds@a-very-long-garden-shop-name.example",
    body: "<p>Seeds</p>",
  });
  const button = bar.getByRole("button", { name: `Always load from ${sender}` });
  await expect(button).toBeVisible();
  await expect(button).toHaveAttribute("title", `Always load from ${sender}`);
  const barBox = await bar.boundingBox();
  expect(barBox).not.toBeNull();
  for (const each of await bar.getByRole("button").all()) {
    const box = await each.boundingBox();
    expect(box).not.toBeNull();
    if (box === null || barBox === null) continue;
    expect(box.x + box.width).toBeLessThanOrEqual(barBox.x + barBox.width + 0.5);
  }
  // The label is cut with an ellipsis, not the button pushed out.
  const label = button.locator('[data-part="form.button.label"]');
  expect(await label.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await label.evaluate((el) => getComputedStyle(el).textOverflow)).toBe("ellipsis");
});

/** Delivers an HTML newsletter with the remote logo, opens it and answers its remote-content bar. */
async function openNewsletter(
  page: Page,
  mail: { from: string; subject: string; id: string; body: string },
): Promise<Locator> {
  const hosts = await control<RemoteHosts>(server, "remote-content");
  const raw = [
    `From: ${mail.from}`,
    "To: ada@work.example",
    `Subject: ${mail.subject}`,
    `Message-ID: <${mail.id}>`,
    "Date: Tue, 29 Sep 2026 09:30:00 +0000",
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    `${mail.body}<img alt="logo" src="${hosts.images}/logo.png">`,
    "",
  ].join("\n");
  await control(server, "imap-deliver", { raw });
  await page.goto(server.url);
  const row = messageRows(page).filter({ hasText: mail.subject });
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();
  return page.getByRole("region", { name: "Remote content" });
}
