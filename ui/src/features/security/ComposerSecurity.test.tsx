import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { RecipientKeysReport, Request, SendCrypto } from "../../api/generated";
import { emptyComposer } from "../../state/compose";
import {
  createEnv,
  renderNode,
  renderWithProviders,
  settleQueries,
  widget,
  type Env,
} from "../../test/harness";
import { ComposerSecurity, composerRecipients } from "./ComposerSecurity";
import {
  composerCrypto,
  composerCryptoStore,
  composerSendBlock,
  loadComposerCrypto,
} from "./composerStore";
import { must } from "../../test/must";

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

afterEach(() => {
  // Unmount before resetting module-level stores.
  cleanup();
  composerCryptoStore.getState().reset();
});

function Harness({
  identityId = "ada@example.org",
  initial,
  onChoice,
  reports,
}: {
  identityId?: string;
  initial: string[];
  onChoice?: (c: SendCrypto | null) => void;
  reports?: (RecipientKeysReport | null)[];
}) {
  const [choice, setChoice] = useState<SendCrypto | null>(null);
  const [recipients, setRecipients] = useState(initial);
  return (
    <>
      <input
        data-testid="recipients"
        value={recipients.join(",")}
        onChange={(e) => {
          setRecipients(e.target.value.split(",").filter(Boolean));
        }}
      />
      <ComposerSecurity
        identityId={identityId}
        recipients={recipients}
        choice={choice}
        onChoice={(c) => {
          setChoice(c);
          onChoice?.(c);
        }}
        onReport={(r) => reports?.push(r)}
      />
    </>
  );
}

describe("composer security", () => {
  it("shows each recipient's key and blocks encryption with a missing key", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const choices: (SendCrypto | null)[] = [];
    renderWithProviders(
      <Harness
        initial={["grace@navy.example", "ewd@tue.example"]}
        onChoice={(c) => choices.push(c)}
      />,
      env,
    );
    const section = screen.getByRole("region", { name: "Signing and encryption" });
    const list = await within(section).findByRole("list", { name: "Recipient keys" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.getAttribute("data-state"))).toEqual(["usable", "expired"]);
    expect(within(must(rows[0])).getByText("Key available (Verified by you)")).toBeInTheDocument();
    expect(within(must(rows[1])).getByText("Key expired")).toBeInTheDocument();
    expect(within(section).queryByRole("alert")).not.toBeInTheDocument();

    await user.click(within(section).getByRole("checkbox", { name: "Encrypt" }));
    expect(choices.at(-1)).toEqual({
      sign: false,
      encrypt: true,
      technology: null,
      protect_headers: null,
    });
    const block = await within(section).findByRole("alert");
    expect(block).toHaveTextContent(
      "Cannot encrypt: no usable key for ewd@tue.example. Nail will not send it unencrypted",
    );
    expect(block).toHaveAttribute("data-reason", "security-compose-block-missing");

    // Removing the recipient without a key lifts the block.
    const input = screen.getByTestId("recipients");
    await user.clear(input);
    await user.type(input, "grace@navy.example");
    await waitFor(() => expect(within(section).queryByRole("alert")).not.toBeInTheDocument());
    expect(within(section).getByRole("checkbox", { name: "Encrypt" })).toBeChecked();

    // "Use defaults" drops the explicit choice.
    await user.click(within(section).getByRole("button", { name: "Use defaults" }));
    expect(choices.at(-1)).toBeNull();
    expect(within(section).getByRole("checkbox", { name: "Encrypt" })).not.toBeChecked();
  });

  it("debounces the key check while recipients are typed", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<Harness initial={[]} />, env);
    await waitFor(() => {
      expect(requestsOf(env, "CheckRecipientKeys")).toHaveLength(1);
    });
    await user.type(screen.getByTestId("recipients"), "alan@bletchley.example");
    await screen.findByText("alan@bletchley.example");
    const checks = requestsOf(env, "CheckRecipientKeys");
    // Not one request per keystroke.
    expect(checks.length).toBeLessThan(5);
    expect(checks.at(-1)).toEqual({
      type: "CheckRecipientKeys",
      identity_id: "ada@example.org",
      recipients: ["alan@bletchley.example"],
      lookup: false,
    });
    expect(screen.getByText("Prefers encryption")).toBeInTheDocument();
    expect(screen.getByText("Encryption is possible for every recipient.")).toBeInTheDocument();
  });

  it("follows the core's suggestion until the user decides", async () => {
    const env = createEnv();
    const identity = env.transport.data.accountsConfig.accounts[0]?.identities[0];
    if (!identity) throw new Error("no identity");
    identity.crypto = {
      technology: "auto",
      sign: true,
      encrypt: false,
      openpgp_key: null,
      smime_certificate: null,
      autocrypt: true,
      prefer_encrypt: "mutual",
      protect_headers: true,
    };
    renderWithProviders(<Harness initial={["alan@bletchley.example"]} />, env);
    expect(await screen.findByText("Every recipient prefers encrypted mail.")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Encrypt" })).toBeChecked());
    expect(screen.getByRole("checkbox", { name: "Sign" })).toBeChecked();
    expect(screen.queryByRole("button", { name: "Use defaults" })).not.toBeInTheDocument();
  });

  it("explains a missing own key", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(
      <Harness identityId="ada@work.example" initial={["grace@navy.example"]} />,
      env,
    );
    await screen.findByRole("list", { name: "Recipient keys" });
    await user.selectOptions(screen.getByRole("combobox", { name: "Technology" }), "OpenPGP");
    await user.click(screen.getByRole("checkbox", { name: "Sign" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "You have no OpenPGP key for this identity.",
    );
    // S/MIME works for the work identity.
    await user.selectOptions(screen.getByRole("combobox", { name: "Technology" }), "S/MIME");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("looks up missing keys on demand and imports a key found online", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<Harness initial={["liskov@mit.example", "nobody@x.example"]} />, env);
    const lookup = await screen.findByRole("button", { name: "Look up 2 missing keys online" });
    await user.click(lookup);
    const list = screen.getByRole("list", { name: "Recipient keys" });
    const liskov = await within(list).findByText("Key found online");
    expect(within(list).getByText("No key found online")).toBeInTheDocument();
    expect(requestsOf(env, "CheckRecipientKeys").at(-1)?.lookup).toBe(true);
    expect(screen.queryByRole("button", { name: /Look up/ })).not.toBeInTheDocument();
    const row = liskov.closest("li") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: "Use key found online" }));
    await waitFor(() =>
      expect(
        within(list)
          .getAllByRole("listitem")
          .find((li) => li.getAttribute("data-email") === "liskov@mit.example"),
      ).toHaveAttribute("data-state", "usable"),
    );
    expect(requestsOf(env, "ImportKeys")).toHaveLength(1);
  });

  it("looks up automatically when the settings say so, never when off", async () => {
    const env = createEnv();
    env.transport.data.features.security.settings.key_lookup = "automatic";
    renderWithProviders(<Harness initial={["liskov@mit.example"]} />, env);
    expect(await screen.findByText("Key found online")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Look up/ })).not.toBeInTheDocument();

    const off = createEnv();
    off.transport.data.features.security.settings.key_lookup = "off";
    const { unmount } = renderWithProviders(<Harness initial={["liskov@mit.example"]} />, off);
    await waitFor(() => {
      expect(requestsOf(off, "CheckRecipientKeys").length).toBeGreaterThan(0);
    });
    expect(requestsOf(off, "CheckRecipientKeys").every((r) => !r.lookup)).toBe(true);
    unmount();
  });

  it("reports every recipient report upwards", async () => {
    const reports: (RecipientKeysReport | null)[] = [];
    renderWithProviders(
      <Harness initial={["grace@navy.example"]} reports={reports} />,
      createEnv(),
    );
    await waitFor(() => {
      expect(reports.some((r) => r !== null)).toBe(true);
    });
  });
});

describe("security-composer widget", () => {
  it("binds to the open composer and exposes the choice for sending", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const { container } = renderNode(widget("security-composer"), env);
    expect(container.querySelector('[data-part="security-compose"]')).toBeNull();
    act(() => {
      env.store.getState().setComposer({
        ...emptyComposer(),
        identityId: "ada@example.org",
        to: "Grace <grace@navy.example>",
        cc: "ewd@tue.example",
      });
    });
    await screen.findByRole("list", { name: "Recipient keys" });
    expect(composerCrypto()).toBeUndefined();
    await user.click(screen.getByRole("checkbox", { name: "Encrypt" }));
    expect(composerCrypto()).toEqual({
      sign: false,
      encrypt: true,
      technology: null,
      protect_headers: null,
    });
    await waitFor(() => {
      expect(composerSendBlock()).toEqual({
        id: "security-compose-block-missing",
        recipients: ["ewd@tue.example"],
      });
    });
    act(() => {
      env.store.getState().updateComposer({ cc: "" });
    });
    await waitFor(() => {
      expect(composerSendBlock()).toBeNull();
    });

    // Closing the composer forgets the choice.
    act(() => {
      env.store.getState().setComposer(null);
    });
    await waitFor(() => {
      expect(composerCrypto()).toBeUndefined();
    });
  });

  it("restores a draft's saved choice", async () => {
    const env = createEnv();
    act(() => {
      env.store
        .getState()
        .setComposer({ ...emptyComposer(), identityId: "ada@example.org", to: "" });
    });
    loadComposerCrypto({
      sign: true,
      encrypt: false,
      technology: "openpgp",
      protect_headers: null,
    });
    renderNode(widget("security-composer"), env);
    expect(await screen.findByRole("checkbox", { name: "Sign" })).toBeChecked();
    expect(screen.getByRole("combobox", { name: "Technology" })).toHaveValue("openpgp");
    // The recipient-key report arrives after the toggles show.
    await settleQueries(env);
    act(() => {
      loadComposerCrypto(undefined);
    });
    expect(composerCrypto()).toBeUndefined();
  });

  it("parses recipients from the typed fields", () => {
    expect(
      composerRecipients({ to: "A <a@x.example>, b@x.example", cc: "bad", bcc: "c@x.example" }),
    ).toEqual(["a@x.example", "b@x.example", "c@x.example"]);
  });
});
