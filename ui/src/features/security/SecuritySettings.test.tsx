import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { VaultStatus } from "../../api/generated";
import { createEnv, renderNode, widget } from "../../test/harness";
import { handleSecurityEvent } from "./index";
import { vaultPercent } from "./SecuritySettings";
import { requestsOf, wireEvents } from "../../test/env";

async function open() {
  const user = userEvent.setup();
  const env = createEnv();
  env.transport.data.behaviour.workMs = 5;
  wireEvents(env, handleSecurityEvent);
  renderNode(widget("security-settings"), env);
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
    expect(within(vault).getByText(/Nail encrypts your existing data now/)).toBeInTheDocument();
    await user.click(within(vault).getByRole("button", { name: "Encrypt now" }));

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

    // Turning it off asks with a warning.
    await user.click(within(vault).getByRole("button", { name: "Turn off" }));
    expect(within(vault).getByText(/stored unencrypted again/)).toBeInTheDocument();
    await user.click(within(vault).getByRole("button", { name: "Cancel" }));
    expect(within(vault).getByRole("button", { name: "Turn off" })).toBeInTheDocument();
    await user.click(within(vault).getByRole("button", { name: "Turn off" }));
    await user.click(within(vault).getByRole("button", { name: "Turn off and decrypt" }));
    expect(
      await within(vault).findByText(/^Off: your mail/, {}, { timeout: 3000 }),
    ).toBeInTheDocument();
  });

  it("shows a stopped migration and retries it", async () => {
    const { user, env, vault } = await open();
    env.transport.data.features.security.vaultError = "internal";
    await user.click(within(vault).getByRole("button", { name: "Turn on encryption at rest" }));
    await user.click(within(vault).getByRole("button", { name: "Encrypt now" }));
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
  it("saves the lookup policy and sources at once", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Finding keys online" });
    const policy = within(section).getByRole("combobox", { name: "Look up recipients’ keys" });
    expect(policy).toHaveValue("on-demand");
    await user.click(within(section).getByRole("checkbox", { name: "Web Key Directory" }));
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
    expect(env.transport.data.features.security.settings.key_lookup).toBe("off");
  });

  it("opts accounts into indexing decrypted mail and warns while the disk is plain", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Searching encrypted mail" });
    const work = await within(section).findByRole("checkbox", { name: "Work (ada@work.example)" });
    expect(screen.queryByText(/Decrypted text is indexed/)).not.toBeInTheDocument();
    await user.click(work);
    expect(await screen.findByText(/Decrypted text is indexed/)).toBeInTheDocument();
    expect(env.transport.data.features.security.settings.index_decrypted).toEqual(["acc-work"]);
    await user.click(work);
    await waitFor(() => {
      expect(env.transport.data.features.security.settings.index_decrypted).toEqual([]);
    });
    expect(
      within(section).queryByRole("checkbox", { name: /All accounts/ }),
    ).not.toBeInTheDocument();
  });

  it("shows when saving fails and restores the stored value", async () => {
    const { user, env } = await open();
    const section = await screen.findByRole("region", { name: "Searching encrypted mail" });
    const work = await within(section).findByRole("checkbox", { name: "Work (ada@work.example)" });
    // The mock rejects accounts it does not know.
    env.transport.data.accounts = env.transport.data.accounts.filter((a) => a.id !== "acc-work");
    await user.click(work);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    await waitFor(() => expect(work).not.toBeChecked());
  });
});
