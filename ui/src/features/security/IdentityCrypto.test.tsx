import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Identity, Request } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { IdentityCryptoFields, identityText } from "./IdentityCrypto";
import { makeKey } from "./mockSeed";
import { must } from "../../test/must";

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

describe("identity crypto fields", () => {
  const identity: Identity = {
    id: "ada@example.org",
    account_id: "acc-personal",
    email: "ada@example.org",
    name: "Ada Lovelace",
    reply_to: null,
    bcc: [],
    signature: null,
    composer_mode: null,
    posting: null,
    mail_style: null,
  };
  const own = [
    makeKey("key-1", "openpgp", ["Ada Lovelace <ada@example.org>"], { has_secret: true }),
    makeKey("key-9", "openpgp", ["Other <other@example.org>"], { has_secret: true }),
  ];

  function Harness({ onSave }: { onSave: (i: Identity) => void }) {
    const [value, setValue] = useState(identity);
    return (
      <IdentityCryptoFields
        identity={value}
        ownKeys={own}
        onChange={(crypto) => {
          const next = { ...value, crypto };
          setValue(next);
          onSave(next);
        }}
      />
    );
  }

  it("starts from the defaults and edits every setting", async () => {
    const user = userEvent.setup();
    const saved: Identity[] = [];
    renderWithProviders(<Harness onSave={(i) => saved.push(i)} />, createEnv());
    const group = screen.getByRole("group", {
      name: "Signing and encryption for Ada Lovelace <ada@example.org>",
    });
    expect(within(group).getByRole("checkbox", { name: "Sign new messages" })).not.toBeChecked();
    expect(
      within(group).getByRole("checkbox", { name: "Exchange keys automatically (Autocrypt)" }),
    ).toBeChecked();
    expect(
      within(group).getByRole("checkbox", { name: "Protect the subject of encrypted mail" }),
    ).toBeChecked();
    // Only the identity's own keys are offered.
    const pgp = within(group).getByRole("combobox", { name: "OpenPGP key" });
    expect(within(pgp).getAllByRole("option")).toHaveLength(2);
    expect(within(group).getByText(/No S\/MIME certificate for this address/)).toBeInTheDocument();

    await user.click(within(group).getByRole("checkbox", { name: "Sign new messages" }));
    await user.click(
      within(group).getByRole("checkbox", { name: "Encrypt new messages when possible" }),
    );
    await user.selectOptions(
      within(group).getByRole("combobox", { name: "Technology" }),
      "Prefer OpenPGP",
    );
    await user.selectOptions(pgp, own[0]?.fingerprint ?? "");
    await user.click(
      within(group).getByRole("checkbox", { name: "Tell others I prefer encrypted mail" }),
    );
    await user.click(
      within(group).getByRole("checkbox", { name: "Protect the subject of encrypted mail" }),
    );
    expect(saved.at(-1)?.crypto).toEqual({
      technology: "openpgp",
      sign: true,
      encrypt: true,
      openpgp_key: own[0]?.fingerprint,
      smime_certificate: null,
      autocrypt: true,
      prefer_encrypt: "mutual",
      protect_headers: false,
    });
    // Without Autocrypt the preference is hidden.
    await user.click(
      within(group).getByRole("checkbox", { name: "Exchange keys automatically (Autocrypt)" }),
    );
    expect(
      within(group).queryByRole("checkbox", { name: "Tell others I prefer encrypted mail" }),
    ).not.toBeInTheDocument();
    // Back to automatic key choice.
    await user.selectOptions(pgp, "");
    expect(saved.at(-1)?.crypto?.openpgp_key).toBeNull();
  });

  it("keeps a configured key that is gone visible", () => {
    renderWithProviders(
      <IdentityCryptoFields
        identity={{
          ...identity,
          crypto: {
            technology: "auto",
            sign: false,
            encrypt: false,
            openpgp_key: "AB".repeat(20),
            smime_certificate: null,
            autocrypt: true,
            prefer_encrypt: "nopreference",
            protect_headers: true,
          },
        }}
        ownKeys={[]}
        onChange={vi.fn()}
      />,
      createEnv(),
    );
    const pgp = screen.getByRole("combobox", { name: "OpenPGP key" });
    expect(pgp).toHaveValue("AB".repeat(20));
    expect(screen.getByText(/No OpenPGP key for this address yet/)).toBeInTheDocument();
  });

  it("labels identities", () => {
    expect(identityText({ name: null, email: "a@x.example" })).toBe("a@x.example");
  });
});

describe("security-identities widget", () => {
  it("saves one account's identities through UpdateAccount", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("security-identities"), env);
    const work = await screen.findByRole("region", { name: "Work" });
    const save = within(work).getByRole("button", { name: "Save Work" });
    expect(save).toBeDisabled();
    const group = within(work).getByRole("group", { name: /ada@work.example/ });
    await user.click(within(group).getByRole("checkbox", { name: "Sign new messages" }));
    await waitFor(() => {
      expect(
        within(within(group).getByRole("combobox", { name: "S/MIME certificate" })).getAllByRole(
          "option",
        ),
      ).toHaveLength(2);
    });
    expect(save).toBeEnabled();
    await user.click(save);
    expect(await within(work).findByRole("status")).toHaveTextContent("Saved.");
    const update = requestsOf(env, "UpdateAccount")[0];
    expect(update?.config.id).toBe("acc-work");
    expect(update?.secrets).toEqual([]);
    expect(update?.config.identities[0]?.crypto).toMatchObject({ sign: true, encrypt: false });
    expect(env.transport.data.accountsConfig.accounts[1]?.identities[0]?.crypto?.sign).toBe(true);
    await waitFor(() =>
      expect(
        within(screen.getByRole("region", { name: "Work" })).getByRole("button", {
          name: "Save Work",
        }),
      ).toBeDisabled(),
    );
    // Other accounts are untouched.
    expect(requestsOf(env, "UpdateAccount")).toHaveLength(1);
  });

  it("shows save errors", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("security-identities"), env);
    const personal = await screen.findByRole("region", { name: "Personal" });
    const group = must(within(personal).getAllByRole("group")[0]);
    await user.click(within(group).getByRole("checkbox", { name: "Sign new messages" }));
    env.transport.data.accountsConfig.accounts = env.transport.data.accountsConfig.accounts.filter(
      (a) => a.id !== "acc-personal",
    );
    await user.click(within(personal).getByRole("button", { name: "Save Personal" }));
    expect(await within(personal).findByRole("alert")).toBeInTheDocument();
  });
});
