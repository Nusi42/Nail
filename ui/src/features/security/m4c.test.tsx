/**
 * The M4c security UI against the mock core: the recovery key (create →
 * shown once with Copy / Print → typed back; check; remove; restoring a lost
 * keychain entry, opened by `RecoveryGate`), smartcards (modules, cards,
 * using a key) and the PIN dialog driven by `PinRequested` /
 * `PinPromptClosed`, plus axe.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { PrintHost } from "../print";
import { handlePinEvent } from "./PinPrompt";
import { RECOVERY_CREATE_DIALOG, RecoveryGate } from "./RecoveryKey";
import { formatRecoveryKey, parseRecoveryKey } from "./recoveryKeyText";
import { handleSecurityEvent } from ".";
import { closePinPrompt, simulateLostVaultKey, simulatePinRequest } from "./vaultMock";
import { must } from "../../test/must";

const MODULE = "C:\\Windows\\System32\\opensc-pkcs11.dll";

function listen(env: Env): () => void {
  return env.client.subscribe((event) => {
    const ctx = { client: env.client, queryClient: env.queryClient, store: env.store };
    handleSecurityEvent(event, ctx);
    handlePinEvent(event, ctx);
  });
}

function render(env: Env, nodes: React.ReactNode) {
  return renderWithProviders(
    <>
      {nodes}
      <WindowLayer layout={env.transport.data.layout} />
      <PrintHost />
    </>,
    env,
  );
}

let print: ReturnType<typeof vi.fn>;
beforeEach(() => {
  print = vi.fn(() => window.dispatchEvent(new Event("afterprint")));
  window.print = print as unknown as typeof window.print;
});
afterEach(() => {
  delete document.documentElement.dataset.nailMode;
});

describe("recovery key", () => {
  it("is offered only for encrypted data", async () => {
    const env = createEnv();
    render(env, <NodeView node={widget("security-recovery-key")} path="t" />);
    expect(await screen.findByText(/Turn on encryption at rest first; then/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Create recovery key" })).toBeNull();
  });

  it("is created, shown once with copy and print, and typed back", async () => {
    const env = createEnv();
    env.transport.data.features.security.vault.state = "encrypted";
    env.transport.data.features.security.vault.key_in_keychain = true;
    const stop = listen(env);
    const user = userEvent.setup();
    const { container } = render(env, <NodeView node={widget("security-recovery-key")} path="t" />);
    expect(await screen.findByText(/There is no recovery key yet/)).toBeVisible();
    expect(await violations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Create recovery key" }));
    const dialog = await screen.findByRole("dialog", { name: "Recovery key" });
    await user.click(within(dialog).getByRole("button", { name: "Create recovery key" }));
    const shown = await within(dialog).findByRole("figure", { name: "Recovery key" });
    const key = env.transport.data.features.vault.recoveryKey ?? "";
    expect(parseRecoveryKey(key).ok).toBe(true);
    expect(within(shown).getAllByText(/^[0-9A-Z]{4}$/)).toHaveLength(14);
    expect(await violations(container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    // user-event provides the clipboard.
    expect(await navigator.clipboard.readText()).toBe(key);
    expect(within(dialog).getByRole("button", { name: "Copied" })).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Print" }));
    await waitFor(() => {
      expect(print).toHaveBeenCalled();
    });
    expect(within(dialog).getByRole("button", { name: "Next" })).toBeDisabled();
    await user.click(within(dialog).getByRole("checkbox", { name: /I have written down/ }));
    await user.click(within(dialog).getByRole("button", { name: "Next" }));
    const input = within(dialog).getByLabelText("Recovery key");
    await user.type(input, "ABCD-");
    expect(within(dialog).getByRole("button", { name: "Check" })).toBeDisabled();
    await user.clear(input);
    await user.type(input, formatRecoveryKey(new Uint8Array(32)));
    await user.click(within(dialog).getByRole("button", { name: "Check" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This is not the key that was just shown.",
    );
    await user.clear(input);
    await user.type(input, key.toLowerCase().replaceAll("-", " "));
    await user.click(within(dialog).getByRole("button", { name: "Check" }));
    expect(await within(dialog).findByText("Your recovery key works. Keep it safe.")).toBeVisible();
    // The key is forgotten once it was typed back.
    expect(within(dialog).queryByRole("figure")).toBeNull();
    await user.click(must(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)));
    expect(await screen.findByText("Last checked")).toBeVisible();
    expect(screen.queryByText(/Never — check/)).toBeNull();
    stop();
  });

  it("lets the Save dialog confirm replacing the kit file", async () => {
    const env = createEnv();
    env.transport.data.features.security.vault.state = "encrypted";
    env.transport.data.features.security.vault.key_in_keychain = true;
    env.transport.data.features.portability.desktop = true;
    env.transport.data.features.portability.pickAnswers = ["C:\\Users\\Ada\\kit.txt"];
    const stop = listen(env);
    const user = userEvent.setup();
    render(env, <NodeView node={widget("security-recovery-key")} path="t" />);
    await user.click(await screen.findByRole("button", { name: "Create recovery key" }));
    const dialog = await screen.findByRole("dialog", { name: "Recovery key" });
    await user.click(
      within(dialog).getByRole("checkbox", {
        name: "Also save it in a text file (a recovery kit)",
      }),
    );
    await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
    const field = within(dialog).getByLabelText("Recovery kit file");
    expect(field).toHaveValue("C:\\Users\\Ada\\kit.txt");
    await user.click(within(dialog).getByRole("button", { name: "Create recovery key" }));
    await within(dialog).findByRole("figure", { name: "Recovery key" });
    expect(env.transport.requests.find((r) => r.type === "CreateRecoveryKey")).toMatchObject({
      save_to: "C:\\Users\\Ada\\kit.txt",
      overwrite: true,
    });
    stop();
  });

  it("reports typos while typing and checks or removes a stored key", async () => {
    const env = createEnv();
    env.transport.data.features.security.vault.state = "encrypted";
    const created = await env.transport.send({ type: "CreateRecoveryKey", save_to: null });
    if (created.type !== "RecoveryKeyCreated") throw new Error(created.type);
    const user = userEvent.setup();
    render(env, <NodeView node={widget("security-recovery-key")} path="t" />);
    await user.click(await screen.findByRole("button", { name: "Check recovery key" }));
    const dialog = await screen.findByRole("dialog", { name: "Check the recovery key" });
    const input = within(dialog).getByLabelText("Recovery key");
    await user.type(input, "UUUU");
    expect(
      within(dialog).getByText("The recovery key contains a character it cannot have."),
    ).toBeVisible();
    await user.clear(input);
    const typo = (created.key.startsWith("A") ? "B" : "A") + created.key.slice(1);
    await user.type(input, typo);
    expect(within(dialog).getByText("The recovery key has a typo.")).toBeVisible();
    await user.clear(input);
    await user.type(input, created.key);
    await user.click(within(dialog).getByRole("button", { name: "Check" }));
    expect(await within(dialog).findByText("This recovery key works.")).toBeVisible();
    await user.click(must(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)));
    await user.click(screen.getByRole("button", { name: "Remove recovery key" }));
    expect(screen.getByText(/your data is lost if the key/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Remove it" }));
    expect(await screen.findByText(/There is no recovery key yet/)).toBeVisible();
    expect(env.transport.data.features.vault.recoveryKey).toBeNull();
  });

  it("restores a lost keychain entry from the dialog the gate opens", async () => {
    const env = createEnv();
    const key = simulateLostVaultKey(env.transport);
    const stop = listen(env);
    const user = userEvent.setup();
    const { container } = render(
      env,
      <>
        <RecoveryGate />
        <NodeView node={widget("security-recovery-key")} path="t" />
      </>,
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Open your data with the recovery key",
    });
    expect(within(dialog).getByText(/its key is missing from this computer/)).toBeVisible();
    expect(await violations(container)).toEqual([]);
    const input = within(dialog).getByLabelText("Recovery key");
    await user.type(input, formatRecoveryKey(new Uint8Array(32)));
    await user.click(within(dialog).getByRole("button", { name: "Open with recovery key" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This recovery key does not open your data.",
    );
    await user.clear(input);
    await user.type(input, key);
    await user.click(within(dialog).getByRole("button", { name: "Open with recovery key" }));
    expect(await within(dialog).findByText(/Your data is open again/)).toBeVisible();
    expect(env.transport.data.features.security.vault.needs_recovery).toBe(false);
    stop();
  });

  it("asks for a restart when the locked data directory gets its key back", async () => {
    const env = createEnv();
    const key = simulateLostVaultKey(env.transport);
    env.transport.data.features.vault.restartAfterRestore = true;
    const stop = listen(env);
    const user = userEvent.setup();
    render(env, <RecoveryGate />);
    const dialog = await screen.findByRole("dialog", {
      name: "Open your data with the recovery key",
    });
    await user.type(within(dialog).getByLabelText("Recovery key"), key);
    await user.click(within(dialog).getByRole("button", { name: "Open with recovery key" }));
    expect(await within(dialog).findByText(/Restart Nail to open your data/)).toBeVisible();
    expect(within(dialog).queryByText(/Your data is open again/)).toBeNull();
    stop();
  });

  it("lets the create dialog explain the vault state when it is plain", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    render(env, null);
    act(() => {
      env.store.getState().openWindow(RECOVERY_CREATE_DIALOG);
    });
    const dialog = await screen.findByRole("dialog", { name: "Recovery key" });
    await user.click(within(dialog).getByRole("button", { name: "Create recovery key" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Turn on encryption at rest first.",
    );
  });
});

describe("smartcards", () => {
  it("configures modules, lists cards and uses a key", async () => {
    const env = createEnv();
    env.transport.data.features.portability.desktop = true;
    env.transport.data.features.portability.pickAnswers = [MODULE];
    const stop = listen(env);
    const user = userEvent.setup();
    const { container } = render(env, <NodeView node={widget("security-smartcards")} path="t" />);
    expect(await screen.findByText("No module yet.")).toBeVisible();
    await user.type(screen.getByLabelText("Module (DLL)"), "opensc.dll");
    await user.click(screen.getByRole("button", { name: "Add module" }));
    expect(screen.getByText("Give the full path of the PKCS#11 module.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(screen.getByRole("button", { name: "Browse…" }));
    await user.click(screen.getByRole("button", { name: "Add module" }));
    const cache = screen.getByLabelText("Keep a card unlocked for (minutes)");
    await user.clear(cache);
    await user.type(cache, "600");
    expect(screen.getByText("Keep the card unlocked for at most 8 hours.")).toBeVisible();
    await user.clear(cache);
    await user.type(cache, "15");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Ada's YubiKey")).toBeVisible();
    expect(env.transport.data.features.vault.smartcards).toEqual({
      modules: [{ path: MODULE, name: null, enabled: true }],
      pin_cache_minutes: 15,
    });
    expect(await violations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Use for S/MIME" }));
    expect(await screen.findByText("In your keys")).toBeVisible();
    expect(
      env.transport.data.features.security.keys.some(
        (k) => k.key.secret_storage === "smartcard" && k.key.technology === "smime",
      ),
    ).toBe(true);
    stop();
  });

  it("reports modules that cannot be loaded and the web mode", async () => {
    const env = createEnv();
    env.transport.data.features.vault.smartcards = {
      modules: [{ path: "C:\\missing\\vendor.dll", name: "Vendor", enabled: true }],
      pin_cache_minutes: 0,
    };
    render(env, <NodeView node={widget("security-smartcards")} path="t" />);
    expect(
      await screen.findByText("C:\\missing\\vendor.dll: The PKCS#11 module was not found."),
    ).toBeVisible();
    expect(screen.getByText("No card found. Insert your card or token.")).toBeVisible();
  });
});

describe("PIN dialog", () => {
  it("asks for the PIN, asks again after a wrong one and closes when right", async () => {
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    const { container } = render(env, null);
    act(() => {
      simulatePinRequest(env.transport, { purpose: "decrypt", attempts_left: 3 });
    });
    const dialog = await screen.findByRole("dialog", { name: "Smartcard PIN" });
    expect(within(dialog).getByText("Decrypt the message")).toBeVisible();
    expect(within(dialog).getByText(/Card: Ada's YubiKey/)).toBeVisible();
    expect(within(dialog).getByText("3 attempts left.")).toBeVisible();
    expect(await violations(container)).toEqual([]);
    const pin = within(dialog).getByLabelText("PIN");
    await waitFor(() => expect(pin).toHaveFocus());
    await user.type(pin, "12");
    expect(within(dialog).getByRole("button", { name: "OK" })).toBeDisabled();
    await user.type(pin, "34{Enter}");
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "AnswerPinPrompt", pin: "1234" }),
      );
    });
    const retry = await screen.findByText("The PIN is wrong.");
    const again = retry.closest("form") as HTMLElement;
    expect(within(again).getByText("2 attempts left.")).toBeVisible();
    expect(within(again).getByLabelText("PIN")).toHaveValue("");
    await user.type(within(again).getByLabelText("PIN"), "123456");
    await user.click(within(again).getByRole("button", { name: "OK" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(env.transport.data.features.vault.prompts).toEqual({});
    stop();
  });

  it("cancels and says why the core stopped waiting", async () => {
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    render(env, null);
    let prompt = { prompt_id: "" };
    act(() => {
      prompt = simulatePinRequest(env.transport, { attempts_left: 1 });
    });
    const dialog = await screen.findByRole("dialog", { name: "Smartcard PIN" });
    expect(within(dialog).getByText("Last attempt: a wrong PIN locks the card.")).toBeVisible();
    act(() => {
      closePinPrompt(env.transport, prompt.prompt_id, "token-removed");
    });
    expect(await within(dialog).findByText("The card was removed.")).toBeVisible();
    await user.click(must(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    act(() => {
      prompt = simulatePinRequest(env.transport);
    });
    const next = await screen.findByRole("dialog", { name: "Smartcard PIN" });
    await user.click(within(next).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    expect(env.transport.requests).toContainEqual({
      type: "AnswerPinPrompt",
      prompt_id: prompt.prompt_id,
      pin: null,
    });
    stop();
  });

  it("cancels a waiting prompt when the dialog is closed", async () => {
    const env = createEnv();
    const stop = listen(env);
    const user = userEvent.setup();
    render(env, null);
    let prompt = { prompt_id: "" };
    act(() => {
      prompt = simulatePinRequest(env.transport);
    });
    const dialog = await screen.findByRole("dialog", { name: "Smartcard PIN" });
    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "AnswerPinPrompt",
        prompt_id: prompt.prompt_id,
        pin: null,
      });
    });
    stop();
  });
});
