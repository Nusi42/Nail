import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { VaultStatus } from "../../api/generated";
import { createEnv } from "../../test/harness";
import { handleSecurityEvent } from "./index";
import { vaultPercent } from "./SecuritySettings";
import { requestsOf, wireEvents } from "../../test/env";
import { chipsOf, pickOption } from "../../widgets/pickers/testing";
import {
  confirmSettingsQuestion,
  renderSettings,
  saveSettings,
  saveSettingsDone,
  settingsQuestion,
} from "../settings/testing";

async function open() {
  const user = userEvent.setup();
  const env = createEnv();
  env.transport.data.behaviour.workMs = 5;
  wireEvents(env, handleSecurityEvent);
  renderSettings(env, "security-settings");
  const vault = await screen.findByRole("region", { name: "Encryption at rest" });
  return { user, env, vault };
}

describe("security-settings: encryption at rest", () => {
  it("recommends turning it on and shows the migration's progress", async () => {
    const { user, env, vault } = await open();
    expect(within(vault).getByRole("status")).toHaveTextContent(
      "Off: your mail is stored unencrypted on this computer.",
    );
    expect(within(vault).getByRole("note")).toHaveTextContent("Recommended: turn this on.");
    await user.click(within(vault).getByRole("button", { name: "Turn on encryption at rest" }));
    // R6: a normal question: its verb has the focus.
    const ask = await settingsQuestion();
    expect(within(ask).getByText(/Nail encrypts your existing data now/)).toBeInTheDocument();
    await waitFor(() => {
      expect(within(ask).getByRole("button", { name: "Encrypt now" })).toHaveFocus();
    });
    await confirmSettingsQuestion(user, "Encrypt now");

    expect(await within(vault).findByText("Encrypting your data…")).toBeInTheDocument();
    const bar = within(vault).getByRole("progressbar", { name: "Progress" });
    expect(bar).toHaveAttribute("max", "100");
    expect(within(vault).queryByRole("note")).not.toBeInTheDocument();
    expect(within(vault).queryByRole("button", { name: /Turn/ })).not.toBeInTheDocument();
    await within(vault).findByText(/^Stored messages: \d of 4/);

    expect(
      await within(vault).findByText("On: your mail is stored encrypted.", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(
      within(vault).getByText("The key is kept in the Windows Credential Manager."),
    ).toBeInTheDocument();
    expect(requestsOf(env, "SetVaultEncryption")).toEqual([
      { type: "SetVaultEncryption", enabled: true },
    ]);

    // Turning it off asks with a warning (destructive: Cancel has the focus).
    await user.click(within(vault).getByRole("button", { name: "Turn off" }));
    const off = await settingsQuestion();
    expect(within(off).getByText(/stored unencrypted again/)).toBeInTheDocument();
    await user.click(within(off).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(requestsOf(env, "SetVaultEncryption")).toHaveLength(1);
    await user.click(within(vault).getByRole("button", { name: "Turn off" }));
    await confirmSettingsQuestion(user, "Turn off and decrypt");
    expect(
      await within(vault).findByText(/^Off: your mail/, {}, { timeout: 3000 }),
    ).toBeInTheDocument();
  });

  it("shows a stopped migration and retries it", async () => {
    const { user, env, vault } = await open();
    env.transport.data.features.security.vaultError = "internal";
    await user.click(within(vault).getByRole("button", { name: "Turn on encryption at rest" }));
    await confirmSettingsQuestion(user, "Encrypt now");
    const failed = await within(vault).findByRole("alert", {}, { timeout: 3000 });
    expect(failed).toHaveTextContent("The migration stopped:");
    await user.click(within(failed).getByRole("button", { name: "Try again" }));
    expect(
      await within(vault).findByText("On: your mail is stored encrypted.", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
  });

  it("computes the overall progress", () => {
    const base: VaultStatus = {
      state: "encrypting",
      phase: "database",
      done: 0,
      total: 0,
      key_in_keychain: true,
      needs_recovery: false,
      recovery: { configured: false, created_at: null, verified_at: null },
      error: null,
      message: null,
    };
    expect(vaultPercent(base)).toBe(0);
    expect(vaultPercent({ ...base, phase: "blobs", done: 2, total: 4 })).toBe(50);
    expect(vaultPercent({ ...base, phase: "index", done: 2, total: 2 })).toBe(100);
    expect(vaultPercent({ ...base, state: "encrypted", phase: null })).toBe(100);
    expect(vaultPercent({ ...base, phase: null })).toBe(0);
  });
});

describe("security-settings: lookup and indexing", () => {
  it("saves the lookup policy and sources with the page's Save", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Finding keys online" });
    const policy = within(section).getByRole("combobox", { name: "Look up recipients’ keys" });
    expect(policy).toHaveValue("on-demand");
    await user.click(within(section).getByRole("checkbox", { name: "Web Key Directory" }));
    expect(requestsOf(env, "UpdateSecuritySettings")).toEqual([]);
    await saveSettingsDone(user);
    await waitFor(() => {
      expect(requestsOf(env, "UpdateSecuritySettings").at(-1)?.settings.lookup_sources).toEqual([
        "keys-openpgp-org",
      ]);
    });
    await user.selectOptions(policy, "Never");
    expect(
      await within(section).findByText("Nail never asks a server for keys."),
    ).toBeInTheDocument();
    expect(within(section).queryByRole("checkbox")).not.toBeInTheDocument();
    await saveSettingsDone(user);
    expect(env.transport.data.features.security.settings.key_lookup).toBe("off");
  });

  it("opts accounts into indexing decrypted mail and warns while the disk is plain", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Searching encrypted mail" });
    const label = "Accounts with searchable decrypted mail";
    expect(screen.queryByText(/Decrypted text is indexed/)).not.toBeInTheDocument();
    await pickOption(user, section, label, /^Work/);
    await saveSettingsDone(user);
    expect(await screen.findByText(/Decrypted text is indexed/)).toBeInTheDocument();
    expect(env.transport.data.features.security.settings.index_decrypted).toEqual(["acc-work"]);
    await waitFor(() => {
      expect(chipsOf(section, label)).toEqual(["Work"]);
    });
    await user.click(within(section).getByRole("button", { name: "Remove Work" }));
    await saveSettingsDone(user);
    await waitFor(() => {
      expect(env.transport.data.features.security.settings.index_decrypted).toEqual([]);
    });
    expect(chipsOf(section, label)).toEqual([]);
  });

  it("shows when saving fails and keeps the changes", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Searching encrypted mail" });
    const label = "Accounts with searchable decrypted mail";
    await user.click(within(section).getByRole("button", { name: `Show choices for ${label}` }));
    const work = await within(section).findByRole("option", { name: /^Work/ });
    // The mock rejects accounts it does not know.
    env.transport.data.accounts = env.transport.data.accounts.filter((a) => a.id !== "acc-work");
    await user.click(work);
    await saveSettings(user);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    // The change stays (with the bar) until it is saved or discarded.
    expect(chipsOf(section, label)).toEqual(["Work"]);
  });
});
