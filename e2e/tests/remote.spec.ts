import { expect, test, type Page } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { axeViolations } from "../support/axe";
import {
  createUser,
  enableRemoteUi,
  pairBrowser,
  pairingCode,
  remoteContext,
  rpcOk,
  totp,
  trustingBrowser,
  virtualAuthenticator,
  type RemoteUi,
} from "../support/remote";
import { openSettingsTab, showView, viewRegion, messageRows } from "../support/views";

/**
 * The remote UI (M2b, ADR 0041): the desktop turns it on (TLS with a
 * self-signed certificate, host name `localhost` so passkeys work), a
 * second browser context plays the other device. The local UI of
 * `nail serve` stays the desktop (loopback, no login).
 */

const server = nailPerTest({ seed: "demo" });

/** The app of the remote UI is up (the demo inbox is listed). */
async function expectApp(page: Page) {
  // The UI state (the last view) is shared with the desktop (M5b): show Mail.
  await showView(page, "Mail");
  await expect(messageRows(page).first()).toBeVisible({
    timeout: 20_000,
  });
}

/** Signs this browser out through the login protocol and reloads. */
async function logout(page: Page) {
  await page.evaluate(async () => {
    const post = (body: object, csrf?: string) =>
      fetch("/api/auth", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          ...(csrf ? { "x-nail-csrf": csrf } : {}),
        },
        body: JSON.stringify(body),
      }).then((r) => r.json() as Promise<{ status: { csrf_token?: string } }>);
    const status = await post({ type: "Status" });
    await post({ type: "Logout" }, status.status.csrf_token);
  });
  await page.reload();
  await expect(page.getByRole("main", { name: "Sign in to Nail" })).toBeVisible();
}

async function pairAndSkipPasskey(page: Page, remote: RemoteUi, code: string) {
  await pairBrowser(page, remote, code);
  await page.getByRole("button", { name: "Not now" }).click();
  await expectApp(page);
}

/** A browser paired for `user` (no passkey), signed out again, at the password login. */
async function pairedAtPasswordLogin(page: Page, remote: RemoteUi, user: string) {
  const other = await remoteContext(page);
  const phone = await other.newPage();
  await pairAndSkipPasskey(phone, remote, await pairingCode(server, user));
  await logout(phone);
  const usePassword = phone.getByRole("button", { name: "Use a password instead" });
  if (await usePassword.isVisible()) await usePassword.click();
  return { other, phone };
}

test("a second browser pairs with a code from the desktop", async ({ page }) => {
  const remote = await enableRemoteUi(server);
  await createUser(server, "ada");

  // Desktop: Settings → Devices → Pair a device.
  await page.goto(server.url);
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Devices");
  await settings.getByRole("button", { name: "Pair a device" }).first().click();
  await page.getByRole("button", { name: "Create pairing code" }).click();
  const code = (await page.locator('[data-part="remote-pair.code"]').textContent())?.trim() ?? "";
  expect(code).toMatch(/^[A-Z0-9]{4}-[A-Z0-9]{4}$/);
  await expect(page.locator('[data-part="remote-pair.code-panel"]')).toContainText(remote.url);

  // The other device: the login page, then the app.
  const other = await remoteContext(page);
  const phone = await other.newPage();
  await phone.goto(remote.url);
  await expect(phone.getByRole("main", { name: "Sign in to Nail" })).toBeVisible();
  // The themed login page (M2c) passes axe, colour contrast included.
  expect(await axeViolations(phone)).toEqual([]);
  // The fingerprint to compare is the one the desktop shows.
  const status = await rpcOk(server, { type: "GetRemoteUiSettings" }, "RemoteUi");
  const fingerprint = (status.status as { certificate: { fingerprint_sha256: string } }).certificate
    .fingerprint_sha256;
  await expect(phone.locator('[data-part="remote-fingerprint"]').first()).toContainText(
    fingerprint.slice(0, 4),
  );
  await pairAndSkipPasskey(phone, remote, code);
  // The desktop sees the code used.
  await expect(page.getByText("Paired. The new device is signed in.")).toBeVisible();

  // Strict headers in the remote UI: Trusted Types are enforced, so an
  // injected script cannot even be created from a string.
  const injected = await phone.evaluate(() => {
    try {
      const script = document.createElement("script");
      script.textContent = "window.__injected = 1";
      document.body.append(script);
      return "ran";
    } catch (error) {
      return (error as Error).name;
    }
  });
  expect(injected).toBe("TypeError");
  expect(await phone.evaluate(() => "__injected" in window)).toBe(false);

  // A used code cannot pair another browser.
  const third = await (await remoteContext(page)).newPage();
  await pairBrowser(third, remote, code, "Tablet");
  await expect(third.getByRole("alert")).toBeVisible();
  await other.close();
});

test("a passkey from a virtual authenticator signs the browser in", async () => {
  const remote = await enableRemoteUi(server);
  const user = await createUser(server, "ada");
  const browser = await trustingBrowser(remote);
  try {
    const phone = await (await browser.newContext()).newPage();
    await virtualAuthenticator(phone);

    await pairBrowser(phone, remote, await pairingCode(server, user));
    await phone.getByRole("button", { name: "Add a passkey" }).click();
    await expect(phone.getByText(/The passkey .* is ready/)).toBeVisible();
    await phone.getByRole("button", { name: "Continue" }).click();
    await expectApp(phone);
    const users = await rpcOk(server, { type: "ListRemoteUsers" }, "RemoteUsers");
    expect((users.users as { passkeys: unknown[] }[])[0]?.passkeys).toHaveLength(1);

    await logout(phone);
    await phone.getByRole("button", { name: "Sign in with a passkey" }).click();
    await expectApp(phone);
  } finally {
    await browser.close();
  }
});

test("password and two-step verification sign a paired browser in", async ({ page }) => {
  const remote = await enableRemoteUi(server);
  const user = await createUser(server, "ada");
  const password = "correct horse battery staple";
  await rpcOk(server, { type: "SetRemotePassword", user_id: user, password }, "RemoteUserSaved");
  const enrollment = await rpcOk(
    server,
    { type: "StartTotpEnrollment", user_id: user },
    "TotpEnrollment",
  );
  const secret = (enrollment.enrollment as { secret: string }).secret;
  const now = Date.now() / 1000;
  await rpcOk(
    server,
    { type: "ConfirmTotpEnrollment", user_id: user, code: totp(secret, now) },
    "RemoteUserSaved",
  );

  const { other, phone } = await pairedAtPasswordLogin(page, remote, user);
  await phone.getByRole("textbox", { name: "User name" }).fill("ada");
  await phone.getByLabel("Password", { exact: true }).fill(password);
  await phone.getByRole("button", { name: "Sign in" }).click();
  // A code used once is refused; the next step's code (one step of clock
  // skew is allowed) signs in.
  const code = phone.getByRole("textbox", { name: "Verification code" });
  await code.fill(totp(secret, now));
  await phone.getByRole("button", { name: "Verify" }).click();
  // Failures stay coarse (no hint whether the code or the password was wrong).
  await expect(phone.getByRole("alert")).toContainText("wrong");
  await code.fill(totp(secret, now + 30));
  await phone.getByRole("button", { name: "Verify" }).click();
  await expectApp(phone);
  await other.close();
});

test("guessing passwords is slowed down", async ({ page }) => {
  const remote = await enableRemoteUi(server);
  const user = await createUser(server, "ada");
  await rpcOk(
    server,
    { type: "SetRemotePassword", user_id: user, password: "correct horse battery staple" },
    "RemoteUserSaved",
  );
  const { other, phone } = await pairedAtPasswordLogin(page, remote, user);

  const signIn = phone.getByRole("button", { name: "Sign in" });
  await phone.getByRole("textbox", { name: "User name" }).fill("ada");
  for (let attempt = 1; attempt <= 3; attempt++) {
    await phone.getByLabel("Password", { exact: true }).fill(`wrong guess ${String(attempt)}`);
    await signIn.click();
    await expect(phone.getByText("The user name or password is wrong.")).toBeVisible();
  }
  // From now on every attempt must wait (1, 2, 4 … seconds).
  await phone.getByLabel("Password", { exact: true }).fill("wrong guess 4");
  await signIn.click();
  await expect(phone.locator('[data-part="remote-login.wait"]')).toBeVisible();
  await expect(signIn).toBeDisabled();
  // Every failure is in the desktop's security log.
  const audit = await rpcOk(
    server,
    { type: "ListRemoteAudit", user_id: null, before: null, limit: 50 },
    "RemoteAudit",
  );
  const failures = (audit.entries as { outcome: string }[]).filter((e) => e.outcome === "failure");
  expect(failures.length).toBeGreaterThanOrEqual(3);
  await other.close();
});

test("revoking a session on the desktop signs the browser out", async ({ page }) => {
  const remote = await enableRemoteUi(server);
  const user = await createUser(server, "ada");
  const other = await remoteContext(page);
  const phone = await other.newPage();
  await pairAndSkipPasskey(phone, remote, await pairingCode(server, user));

  await page.goto(server.url);
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Devices");
  await settings.getByRole("button", { name: "Sign out ada on Phone" }).click();

  await expect(phone.getByRole("main", { name: "Sign in to Nail" })).toBeVisible({
    timeout: 20_000,
  });
  await other.close();
});

test("internet mode cannot be enabled", async ({ page }) => {
  await page.goto(server.url);
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Remote access");
  const internet = settings.getByRole("checkbox", { name: "Allow access from the internet" });
  await expect(internet).toBeDisabled();
  await expect(internet).toHaveAccessibleDescription(/penetration test/);
  // The core refuses it too.
  const settingsNow = await rpcOk(server, { type: "GetRemoteUiSettings" }, "RemoteUi");
  const refused = await rpcOk(
    server,
    {
      type: "UpdateRemoteUiSettings",
      settings: { ...(settingsNow.settings as object), internet_mode: true },
    },
    "Error",
  );
  expect(refused.code).toBe("forbidden");
});

test("the local UI's CSP blocks an injected inline script", async ({ page }) => {
  await page.goto(server.url);
  await expectApp(page);
  const violations = await page.evaluate(
    () =>
      new Promise<string[]>((resolve) => {
        const seen: string[] = [];
        document.addEventListener("securitypolicyviolation", (event) => {
          seen.push(event.effectiveDirective);
        });
        const script = document.createElement("script");
        script.textContent = "window.__injected = 1";
        document.body.append(script);
        setTimeout(() => {
          resolve(seen);
        }, 1500);
      }),
  );
  expect(violations).toContain("script-src-elem");
  expect(await page.evaluate(() => "__injected" in window)).toBe(false);
});
