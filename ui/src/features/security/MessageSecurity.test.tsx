import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { MessageBody, SecurityStatus } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { AuthBadge, SecurityBadge } from "./MessageSecurity";
import { securityDemoBySender } from "./mockSeed";
import { must } from "../../test/must";
import { requestsOf } from "../../test/env";

const demo = securityDemoBySender();

function statusOf(sender: string): SecurityStatus {
  const s = demo[sender]?.security;
  if (!s) throw new Error(sender);
  return structuredClone(s);
}

describe("security badge", () => {
  it("shows a verified, encrypted message and its details", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(
      <SecurityBadge status={statusOf("grace@navy.example")} messageFrom="grace@navy.example" />,
      env,
    );
    const badge = screen.getByRole("button", {
      name: "Security: Encrypted · Signed, verified. Show details",
    });
    expect(badge).toHaveAttribute("data-tone", "good");
    expect(badge).toHaveAttribute("aria-expanded", "false");
    await user.click(badge);
    const details = screen.getByRole("dialog", { name: /Security: Encrypted/ });
    expect(badge).toHaveAttribute("aria-expanded", "true");
    expect(within(details).getByText(/The signature is valid/)).toBeInTheDocument();
    expect(within(details).getByText("grace@navy.example")).toBeInTheDocument();
    expect(within(details).getByText("Verified by you")).toBeInTheDocument();
    expect(within(details).getByText(/Nail decrypted it for display/)).toBeInTheDocument();
    expect(within(details).getByText("AES-256")).toBeInTheDocument();
    expect(within(details).getByText("2 keys")).toBeInTheDocument();
    expect(within(details).getByText(/The subject and headers are protected/)).toBeInTheDocument();
    const fp = within(details).getByText(/^[0-9A-F]{4}( [0-9A-F]{4}){9}$/);
    expect(fp).toHaveAttribute("data-part", "security-fingerprint");
    // Verified keys offer no "mark verified".
    expect(within(details).queryByRole("button", { name: /Mark key/ })).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(badge).toHaveFocus();
  });

  it("closes on a click outside", async () => {
    const user = userEvent.setup();
    renderWithProviders(<SecurityBadge status={statusOf("alan@bletchley.example")} />, createEnv());
    await user.click(screen.getByRole("button", { name: /Security:/ }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("warns about a signature by someone else and a partly protected message", async () => {
    const user = userEvent.setup();
    const status = statusOf("alan@bletchley.example");
    status.partial = true;
    const first = status.signatures[0];
    if (first) first.signer_matches_from = false;
    renderWithProviders(
      <SecurityBadge status={status} messageFrom="boss@corp.example" />,
      createEnv(),
    );
    const badge = screen.getByRole("button", {
      name: /Signed by someone else · Only partly protected/,
    });
    expect(badge).toHaveAttribute("data-tone", "warning");
    await user.click(badge);
    expect(
      screen.getByText(
        "The signature is by alan@bletchley.example, but the message says it is from boss@corp.example.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/Only a part of this message is signed/)).toBeInTheDocument();
  });

  it("shows an invalid signature as danger", () => {
    renderWithProviders(<SecurityBadge status={statusOf("ewd@tue.example")} />, createEnv());
    expect(screen.getByRole("button", { name: /Invalid signature/ })).toHaveAttribute(
      "data-tone",
      "danger",
    );
  });

  it("shows the S/MIME certificate chain and that revocation was not checked", async () => {
    const user = userEvent.setup();
    renderWithProviders(<SecurityBadge status={statusOf("katherine@nasa.example")} />, createEnv());
    await user.click(screen.getByRole("button", { name: /Signed, verified/ }));
    expect(screen.getByText("S/MIME")).toBeInTheDocument();
    expect(screen.getByText("The certificate chain is valid.")).toBeInTheDocument();
    expect(screen.getByText(/Revocation was not checked/)).toBeInTheDocument();
    const chain = screen.getByRole("list", { name: "Certificate chain" });
    expect(within(chain).getAllByRole("listitem")).toHaveLength(3);
    expect(within(chain).getByText("CN=Demo Root CA,O=Demo Trust")).toBeInTheDocument();
  });

  it("marks an Autocrypt key as verified from the details", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<SecurityBadge status={statusOf("alan@bletchley.example")} />, env);
    await user.click(screen.getByRole("button", { name: /Security:/ }));
    await user.click(screen.getByRole("button", { name: "Mark key as verified" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Key marked as verified.");
    expect(requestsOf(env, "SetKeyTrust")).toEqual([
      { type: "SetKeyTrust", key_id: "key-4", decision: "verified" },
    ]);
  });

  it("unlocks a locked key with its passphrase", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const status: SecurityStatus = {
      technology: "smime",
      signatures: [],
      encryption: { state: "locked", key_id: "key-2", recipients: [], algorithm: null },
      header_protection: "none",
      partial: false,
      inline_pgp: false,
    };
    renderWithProviders(<SecurityBadge status={status} />, env);
    const badge = screen.getByRole("button", { name: /Encrypted, key locked/ });
    expect(badge).toHaveAttribute("data-tone", "warning");
    await user.click(badge);
    expect(screen.getByText("hidden recipients")).toBeInTheDocument();
    expect(screen.getByText(/The message is not signed/)).toBeInTheDocument();
    const form = screen.getByRole("form", { name: "Unlock key" });
    const submit = within(form).getByRole("button", { name: "Unlock" });
    expect(submit).toBeDisabled();
    await user.type(within(form).getByLabelText("Passphrase"), "wrong");
    await user.click(submit);
    expect(await within(form).findByRole("alert")).toHaveTextContent("Wrong passphrase.");
    await user.selectOptions(within(form).getByLabelText("Keep unlocked"), "Until Nail closes");
    await user.clear(within(form).getByLabelText("Passphrase"));
    await user.type(within(form).getByLabelText("Passphrase"), "demo");
    await user.click(submit);
    await waitFor(() => expect(within(form).queryByRole("alert")).not.toBeInTheDocument());
    expect(requestsOf(env, "UnlockKey").at(-1)).toEqual({
      type: "UnlockKey",
      key_id: "key-2",
      passphrase: "demo",
      remember_minutes: null,
    });
    expect(requestsOf(env, "UnlockKey")[0]?.remember_minutes).toBe(15);
  });
});

describe("auth badge", () => {
  it("shows a failed sender check and every method", async () => {
    const user = userEvent.setup();
    const results = demo["noreply@shop.example"]?.auth_results;
    if (!results) throw new Error("demo");
    renderWithProviders(<AuthBadge results={results} />, createEnv());
    const badge = screen.getByRole("button", {
      name: "Sender check: Sender check failed. Show details",
    });
    expect(badge).toHaveAttribute("data-tone", "danger");
    expect(badge).toHaveAttribute("data-verdict", "fail");
    await user.click(badge);
    const details = screen.getByRole("dialog");
    expect(within(details).getByText(/probably does not come from/)).toBeInTheDocument();
    const rows = within(details).getAllByRole("listitem");
    expect(rows.map((r) => r.getAttribute("data-method"))).toEqual(["dmarc", "dkim", "spf"]);
    expect(within(must(rows[1])).getByText("shop.example (selector mail)")).toBeInTheDocument();
    expect(within(must(rows[1])).getByText("signature did not verify")).toBeInTheDocument();
    expect(within(must(rows[2])).getByText("soft fail")).toBeInTheDocument();
    expect(within(details).getByText("Checked by imap.example.org")).toBeInTheDocument();
  });

  it("shows a pass and an empty result", async () => {
    const user = userEvent.setup();
    const { unmount } = renderWithProviders(
      <AuthBadge results={demo["grace@navy.example"]?.auth_results as never} />,
      createEnv(),
    );
    expect(screen.getByRole("button", { name: /Sender confirmed/ })).toHaveAttribute(
      "data-tone",
      "good",
    );
    unmount();
    renderWithProviders(
      <AuthBadge results={{ authserv_id: "mx", dkim: [], spf: null, dmarc: null, arc: null }} />,
      createEnv(),
    );
    await user.click(screen.getByRole("button", { name: /Sender not checked/ }));
    expect(screen.getByText("No checks were reported.")).toBeInTheDocument();
  });
});

describe("security-message widget", () => {
  function withFindings(env: Env, sender: string): MessageBody {
    const body = [...env.transport.data.bodies.values()][0];
    if (!body) throw new Error("no body");
    Object.assign(body, structuredClone(demo[sender]));
    return body;
  }

  it("shows both badges of the selected message", async () => {
    const env = createEnv();
    const body = withFindings(env, "grace@navy.example");
    renderNode(widget("security-message"), env);
    expect(screen.queryByRole("group")).not.toBeInTheDocument();
    act(() => {
      env.store.getState().selectMessage(body.id);
    });
    const group = await screen.findByRole("group", { name: "Message security" });
    expect(within(group).getByRole("button", { name: /Security:/ })).toBeInTheDocument();
    expect(within(group).getByRole("button", { name: /Sender check:/ })).toBeInTheDocument();
  });

  it("renders nothing for a message without findings", async () => {
    const env = createEnv();
    const body = [...env.transport.data.bodies.values()][1];
    if (!body) throw new Error("no body");
    const { container } = renderNode(widget("security-message"), env);
    act(() => {
      env.store.getState().selectMessage(body.id);
    });
    await waitFor(() => {
      expect(env.transport.requests.some((r) => r.type === "GetMessage")).toBe(true);
    });
    expect(container.querySelector('[data-part="security-message"]')).toBeNull();
  });
});
