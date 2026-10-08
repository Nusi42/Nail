import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, widget } from "../../test/harness";
import { armorKey } from "./mock";
import { must } from "../../test/must";
import { requestsOf } from "../../test/env";

async function open(env = createEnv()) {
  const user = userEvent.setup({ delay: null });
  renderNode(widget("security-keys"), env);
  const list = await screen.findByRole("list", { name: "Keys" });
  return { user, env, list };
}

function keyButton(list: HTMLElement, name: RegExp) {
  return within(list).getByRole("button", { name });
}

describe("security-keys", () => {
  it("lists own keys first and shows the selected key", async () => {
    const { user, list } = await open();
    const items = within(list).getAllByRole("button");
    expect(items[0]).toHaveAttribute("data-own");
    expect(items[1]).toHaveAttribute("data-own");
    expect(items[0]).toHaveAttribute("aria-pressed", "true");
    expect(within(must(items[0])).getByText("Yours")).toBeInTheDocument();

    await user.click(keyButton(list, /Katherine Johnson/));
    const detail = screen.getByRole("region", { name: /^Key CN=Katherine Johnson/ });
    expect(within(detail).getAllByText("katherine@nasa.example")).toHaveLength(2);
    expect(within(detail).getByText("ECDSA P-256")).toBeInTheDocument();
    expect(within(detail).getByText("sign and encrypt")).toBeInTheDocument();
    expect(within(detail).getByText("Attached to a message")).toBeInTheDocument();
    expect(within(detail).getByText("The certificate chain is valid.")).toBeInTheDocument();
    expect(within(detail).getByRole("list", { name: "Certificate chain" })).toHaveTextContent(
      "CN=Demo Mail CA",
    );

    await user.click(keyButton(list, /Edsger Dijkstra/));
    expect(screen.getByText("This key has expired.")).toBeInTheDocument();
    expect(keyButton(list, /Edsger Dijkstra/)).toHaveAttribute("data-key-state", "expired");
  });

  it("filters by text, type and own keys", async () => {
    const { user, env } = await open();
    await user.type(screen.getByRole("textbox", { name: "Search" }), "navy");
    await waitFor(() => {
      expect(
        within(screen.getByRole("list", { name: "Keys" })).getAllByRole("button"),
      ).toHaveLength(1);
    });
    expect(requestsOf(env, "ListKeys").at(-1)?.filter.text).toBe("navy");
    await user.clear(screen.getByRole("textbox", { name: "Search" }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Type" }), "S/MIME");
    await waitFor(() => {
      expect(
        within(screen.getByRole("list", { name: "Keys" }))
          .getAllByRole("button")
          .every((b) => b.getAttribute("data-technology") === "smime"),
      ).toBe(true);
    });
    await user.click(screen.getByRole("checkbox", { name: "Only my own keys" }));
    await waitFor(() => {
      expect(
        within(screen.getByRole("list", { name: "Keys" })).getAllByRole("button"),
      ).toHaveLength(1);
    });
    await user.type(screen.getByRole("textbox", { name: "Search" }), "zzz");
    expect(await screen.findByText("No keys match.")).toBeInTheDocument();
  });

  it("edits trust with a stored decision", async () => {
    const { user, env, list } = await open();
    await user.click(keyButton(list, /Alan Turing/));
    const detail = screen.getByRole("region", { name: /^Key Alan Turing/ });
    expect(within(detail).getAllByText(/Learned from the sender’s mail/).length).toBeGreaterThan(0);
    const save = within(detail).getByRole("button", { name: "Save trust" });
    expect(save).toBeDisabled();
    await user.selectOptions(
      within(detail).getByRole("combobox", { name: "Your decision" }),
      "Rejected",
    );
    expect(within(detail).getByText(/will neither encrypt to this key/)).toBeInTheDocument();
    await user.click(save);
    await waitFor(() =>
      expect(keyButton(list, /Alan Turing/)).toHaveAttribute("data-key-state", "rejected"),
    );
    expect(requestsOf(env, "SetKeyTrust")).toEqual([
      { type: "SetKeyTrust", key_id: "key-4", decision: "rejected" },
    ]);
    expect(await screen.findByText(/You rejected this key/, { selector: "p" })).toBeInTheDocument();
  });

  it("offers no trust editing for own keys", async () => {
    await open();
    expect(screen.queryByRole("combobox", { name: "Your decision" })).not.toBeInTheDocument();
    expect(screen.getAllByText("Your own key").length).toBeGreaterThan(0);
  });

  it("exports a public key as text and as a download", async () => {
    const { user, list } = await open();
    await user.click(keyButton(list, /Grace Hopper/));
    await user.click(screen.getByRole("button", { name: "Export public key" }));
    const region = await screen.findByRole("region", { name: "Exported key" });
    const text = within(region).getByRole("textbox", { name: "Exported key" });
    expect((text as HTMLTextAreaElement).value).toContain("BEGIN PGP PUBLIC KEY BLOCK");
    const link = within(region).getByRole("link", { name: "Save grace@navy.example.asc" });
    expect(link).toHaveAttribute("download", "grace@navy.example.asc");
    expect(link.getAttribute("href")).toMatch(/^data:application\/pgp-keys;charset=utf-8,/);
  });

  it("deletes another person's key and needs a second confirmation for an own key", async () => {
    const { user, env, list } = await open();
    await user.click(keyButton(list, /Edsger Dijkstra/));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete key" }));
    await waitFor(() => expect(within(list).queryByText(/Edsger/)).not.toBeInTheDocument());
    expect(requestsOf(env, "DeleteKey")).toEqual([
      { type: "DeleteKey", key_id: "key-6", delete_secret: false },
    ]);

    await user.click(keyButton(list, /Ada Lovelace <ada@example.org>/));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByText(/is your own key/)).toBeInTheDocument();
    const confirm = screen.getByRole("button", { name: "Delete key" });
    expect(confirm).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("button", { name: "Delete key" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("checkbox", { name: "Also delete my secret key" }));
    await user.click(screen.getByRole("button", { name: "Delete key" }));
    await waitFor(() =>
      expect(within(list).queryByText("Ada Lovelace <ada@example.org>")).not.toBeInTheDocument(),
    );
    expect(requestsOf(env, "DeleteKey").at(-1)).toMatchObject({ delete_secret: true });
  });

  it("unlocks a passphrase-protected own key", async () => {
    const { user, list } = await open();
    await user.click(keyButton(list, /ada@work.example/));
    expect(screen.getByText("Protected by a passphrase")).toBeInTheDocument();
    const form = screen.getByRole("form", { name: "Unlock key" });
    await user.type(within(form).getByLabelText("Passphrase"), "demo");
    await user.click(within(form).getByRole("button", { name: "Unlock" }));
    expect(await screen.findByText("The key is unlocked.")).toBeInTheDocument();
  });
});

describe("security-keys tools", () => {
  it("imports pasted keys and selects the new one", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Import" }));
    const tool = screen.getByRole("region", { name: "Import" });
    const submit = within(tool).getByRole("button", { name: "Import" });
    expect(submit).toBeDisabled();
    await user.click(within(tool).getByRole("textbox", { name: "Key or certificate text" }));
    await user.paste(
      armorKey({
        fingerprint: "C".repeat(40),
        user_ids: ["Carol <carol@x.example>"],
        algorithm: "Ed25519",
      }),
    );
    await user.click(submit);
    expect(await within(tool).findByRole("status")).toHaveTextContent(
      "Imported: 1 new, 0 updated.",
    );
    expect(requestsOf(env, "ImportKeys")[0]?.encoding).toBe("text");
    const list = screen.getByRole("list", { name: "Keys" });
    await waitFor(() => expect(keyButton(list, /Carol/)).toHaveAttribute("aria-pressed", "true"));
    expect(within(tool).getByRole("textbox", { name: "Key or certificate text" })).toHaveValue("");
  });

  it("says when pasted text holds no key", async () => {
    const { user } = await open();
    await user.click(screen.getByRole("button", { name: "Import" }));
    const tool = screen.getByRole("region", { name: "Import" });
    await user.type(
      within(tool).getByRole("textbox", { name: "Key or certificate text" }),
      "hello",
    );
    await user.click(within(tool).getByRole("button", { name: "Import" }));
    expect(await within(tool).findByRole("alert")).toHaveTextContent(
      "No key or certificate was found in this data.",
    );
  });

  it("imports key files as text or binary", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Import" }));
    const tool = screen.getByRole("region", { name: "Import" });
    const input = within(tool).getByLabelText("Key file");
    const armored = armorKey({
      fingerprint: "D".repeat(40),
      user_ids: ["Dan <dan@x.example>"],
      algorithm: "Ed25519",
    });
    await user.upload(input, new File([armored], "dan.asc", { type: "text/plain" }));
    expect(await within(tool).findByRole("status")).toHaveTextContent("1 new");
    await user.upload(input, new File([new Uint8Array([0x99, 0x01, 0x02])], "x.gpg"));
    await waitFor(() => {
      expect(requestsOf(env, "ImportKeys")).toHaveLength(2);
    });
    expect(requestsOf(env, "ImportKeys")[1]).toMatchObject({ encoding: "base64", data: "mQEC" });
  });

  it("imports a PKCS#12 file with its password and a protection choice", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Import" }));
    const tool = screen.getByRole("region", { name: "Import" });
    await user.upload(
      within(tool).getByLabelText("Key file"),
      new File(["certificate of bob@x.example"], "bob.p12"),
    );
    const form = await within(tool).findByRole("form", { name: "Import bob.p12" });
    await user.type(within(form).getByLabelText("Password of the file"), "nope");
    await user.click(within(form).getByRole("button", { name: "Import certificate" }));
    expect(await within(tool).findByRole("alert")).toHaveTextContent(
      "Wrong password for this file.",
    );

    await user.clear(within(form).getByLabelText("Password of the file"));
    await user.type(within(form).getByLabelText("Password of the file"), "demo");
    await user.selectOptions(
      within(form).getByRole("combobox", { name: "Protect the secret key with" }),
      "A passphrase",
    );
    await user.type(within(form).getByLabelText("Passphrase"), "one");
    await user.type(within(form).getByLabelText("Repeat passphrase"), "two");
    await user.click(within(form).getByRole("button", { name: "Import certificate" }));
    expect(await within(tool).findByRole("alert")).toHaveTextContent(
      "The passphrases do not match.",
    );
    await user.clear(within(form).getByLabelText("Repeat passphrase"));
    await user.type(within(form).getByLabelText("Repeat passphrase"), "one");
    await user.click(within(form).getByRole("button", { name: "Import certificate" }));
    expect(await within(tool).findByRole("status")).toHaveTextContent("1 new");
    expect(requestsOf(env, "ImportPkcs12").at(-1)).toMatchObject({
      passphrase: "demo",
      protection: { kind: "passphrase", passphrase: "one" },
    });
    expect(within(tool).queryByRole("form", { name: /Import bob/ })).not.toBeInTheDocument();
  });

  it("creates a key for an identity", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Create key" }));
    const tool = screen.getByRole("region", { name: "Create key" });
    const identity = await within(tool).findByRole("combobox", { name: "For" });
    await waitFor(() => {
      expect(within(identity).getAllByRole("option").length).toBeGreaterThan(1);
    });
    await user.selectOptions(identity, "ada@work.example");
    await user.selectOptions(within(tool).getByRole("combobox", { name: "Expires" }), "Never");
    await user.click(within(tool).getByRole("button", { name: "Create key" }));
    expect(await within(tool).findByRole("status")).toHaveTextContent(/^Key created: [0-9A-F ]+$/);
    expect(requestsOf(env, "GenerateKey")).toEqual([
      {
        type: "GenerateKey",
        identity_id: "ada@work.example",
        algorithm: "curve25519",
        expires_days: null,
        protection: { kind: "keychain" },
      },
    ]);
    const detail = screen.getByRole("region", { name: "Key Ada Lovelace <ada@work.example>" });
    expect(within(detail).getByText("Created in Nail")).toBeInTheDocument();
  });

  it("finds a key online and imports it", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Find online" }));
    const tool = screen.getByRole("region", { name: "Find online" });
    await user.type(within(tool).getByRole("textbox", { name: "Address" }), "nobody@x.example");
    await user.click(within(tool).getByRole("button", { name: "Search" }));
    expect(await within(tool).findByText("No key found for nobody@x.example.")).toBeInTheDocument();

    await user.clear(within(tool).getByRole("textbox", { name: "Address" }));
    await user.type(within(tool).getByRole("textbox", { name: "Address" }), "liskov@mit.example");
    await user.click(within(tool).getByRole("button", { name: "Search" }));
    const found = await within(tool).findByRole("list", { name: "Keys found" });
    expect(within(found).getByText("Barbara Liskov <liskov@mit.example>")).toBeInTheDocument();
    expect(within(found).getByText(/^Web Key Directory · Ed25519 · created/)).toBeInTheDocument();
    expect(within(tool).getByText(/A key found online is not verified/)).toBeInTheDocument();
    await user.click(within(found).getByRole("button", { name: "Import" }));
    expect(await within(found).findByText("In your keyring")).toBeInTheDocument();
    expect(requestsOf(env, "LookupKeys").at(-1)?.sources).toEqual(["wkd", "keys-openpgp-org"]);
    const list = screen.getByRole("list", { name: "Keys" });
    await waitFor(() =>
      expect(keyButton(list, /Barbara Liskov/)).toHaveAttribute("aria-pressed", "true"),
    );

    await user.clear(within(tool).getByRole("textbox", { name: "Address" }));
    await user.type(
      within(tool).getByRole("textbox", { name: "Address" }),
      "a@unreachable.example",
    );
    await user.click(within(tool).getByRole("button", { name: "Search" }));
    expect(
      await within(tool).findByText(/^Web Key Directory could not be reached:/),
    ).toBeInTheDocument();
  });

  it("explains that online lookup is off", async () => {
    const env = createEnv();
    env.transport.data.features.security.settings.key_lookup = "off";
    const { user } = await open(env);
    await user.click(screen.getByRole("button", { name: "Find online" }));
    const tool = screen.getByRole("region", { name: "Find online" });
    expect(await within(tool).findByRole("note")).toHaveTextContent(/turned off/);
    await user.type(within(tool).getByRole("textbox", { name: "Address" }), "liskov@mit.example");
    expect(within(tool).getByRole("button", { name: "Search" })).toBeDisabled();
  });

  it("imports a certificate from the Windows store", async () => {
    const { user, env } = await open();
    await user.click(screen.getByRole("button", { name: "Windows certificates" }));
    const tool = screen.getByRole("region", { name: "Windows certificates" });
    const personal = await within(tool).findByRole("list", { name: "My certificates" });
    const items = within(personal).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(within(must(items[1])).getByText("Cannot be used for e-mail")).toBeInTheDocument();
    await user.click(within(must(items[0])).getByRole("button", { name: "Use in Nail" }));
    expect(await within(tool).findByText("Already imported")).toBeInTheDocument();
    expect(requestsOf(env, "ImportWindowsCertificate")).toHaveLength(1);

    await user.selectOptions(within(tool).getByRole("combobox", { name: "Store" }), "Other people");
    const others = await within(tool).findByRole("list", { name: "Other people" });
    expect(within(others).getByText("CN=Barbara Liskov,E=liskov@mit.example")).toBeInTheDocument();
  });

  it("says when the Windows store is not available", async () => {
    const env = createEnv();
    env.transport.data.features.security.windows = false;
    const { user } = await open(env);
    await user.click(screen.getByRole("button", { name: "Windows certificates" }));
    expect(
      await screen.findByText("The Windows certificate store is only available on Windows."),
    ).toBeInTheDocument();
  });
});
