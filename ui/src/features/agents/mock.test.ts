import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Event as NailEvent } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { AGENTS_EVENTS, agentsKeys, handleAgentsEvent } from ".";
import { runtimeIds } from "./messages";
import { AGENTS_REQUESTS, isAgentsRequest, requestConfirmation } from "./mock";
import { ACTION_KINDS, canAlwaysAllow, defaultMode, policyTable } from "./model";

const client = { kind: "mcp-client", client_id: "mcp-1" } as const;

function pending(t: MockTransport, kind: (typeof ACTION_KINDS)[number]) {
  return requestConfirmation(t, {
    requested_by: { actor: client, name: "Claude Desktop" },
    kind,
    operation: kind === "send" ? "send_message" : "delete_messages",
    summary: { id: `confirmation-summary-${kind}`, args: { actor: "Claude Desktop", count: 1 } },
    account_ids: ["acc-work"],
  });
}

describe("agents model", () => {
  it("mirrors the contract's rules", () => {
    expect(ACTION_KINDS).toHaveLength(11);
    for (const kind of ["trash", "permanent-delete", "pim-delete"] as const) {
      expect(canAlwaysAllow(kind)).toBe(false);
      expect(defaultMode(kind)).toBe("ask");
    }
    expect(defaultMode("send")).toBe("ask");
    expect(defaultMode("rule")).toBe("ask");
    expect(defaultMode("draft")).toBe("allow");
    expect(defaultMode("organize")).toBe("allow");
    const table = policyTable({ send: "allow", trash: "allow", draft: "ask" });
    expect(table.find((p) => p.kind === "send")).toEqual({
      kind: "send",
      mode: "allow",
      customized: true,
      can_allow: true,
    });
    // A stored allow of a destructive kind is never honoured.
    expect(table.find((p) => p.kind === "trash")?.mode).toBe("ask");
    expect(table.find((p) => p.kind === "trash")?.customized).toBe(false);
    expect(runtimeIds()).toContain("action-kind-permanent-delete");
  });
});

describe("agents mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(AGENTS_REQUESTS).size).toBe(AGENTS_REQUESTS.length);
    expect(
      isAgentsRequest({
        type: "ListConfirmations",
        filter: "pending",
        actor: null,
        before: null,
        limit: 10,
      }),
    ).toBe(true);
    expect(isAgentsRequest({ type: "ListMcpClients" })).toBe(false);
  });

  it("lists and decides confirmations like the core", async () => {
    const t = new MockTransport();
    const seen: NailEvent[] = [];
    t.onEvent((e) => seen.push(e));
    const send = pending(t, "send");
    const trash = pending(t, "trash");
    expect(seen.map((e) => e.type)).toEqual(["ConfirmationRequested", "ConfirmationRequested"]);

    const list = await t.send({
      type: "ListConfirmations",
      filter: "pending",
      actor: client,
      before: null,
      limit: 10,
    });
    expect(list).toMatchObject({ type: "Confirmations", confirmations: [send, trash] });

    expect(
      await t.send({
        type: "DecideConfirmation",
        confirmation_id: trash.id,
        decision: "approve",
        always_allow: true,
      }),
    ).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "action-policy-issue-destructive" },
    });
    const decided = await t.send({
      type: "DecideConfirmation",
      confirmation_id: send.id,
      decision: "approve",
      always_allow: true,
    });
    expect(decided).toMatchObject({ confirmation: { state: "approved", decided_via: "nail" } });
    expect(seen.map((e) => e.type)).toContain("ActionPoliciesChanged");
    expect(seen.map((e) => e.type)).toContain("ConfirmationResolved");
    expect(
      await t.send({
        type: "DecideConfirmation",
        confirmation_id: send.id,
        decision: "deny",
        always_allow: false,
      }),
    ).toMatchObject({ type: "Error", code: "conflict" });
    const policies = await t.send({ type: "ListActionPolicies", actor: client });
    expect(policies).toMatchObject({ type: "ActionPolicies" });
    if (policies.type !== "ActionPolicies") throw new Error("policies expected");
    expect(policies.policies.find((p) => p.kind === "send")?.mode).toBe("allow");

    const history = await t.send({
      type: "ListConfirmations",
      filter: "decided",
      actor: null,
      before: null,
      limit: 10,
    });
    expect(history).toMatchObject({ confirmations: [{ id: send.id }] });
  });

  it("validates policies and actors", async () => {
    const t = new MockTransport();
    expect(
      await t.send({
        type: "SetActionPolicy",
        actor: client,
        kind: "permanent-delete",
        mode: "allow",
      }),
    ).toMatchObject({ code: "invalid_request" });
    expect(
      await t.send({
        type: "ListActionPolicies",
        actor: { kind: "plugin", plugin_id: "ghost" },
      }),
    ).toMatchObject({ code: "not_found" });
    const set = await t.send({
      type: "SetActionPolicy",
      actor: client,
      kind: "draft",
      mode: "ask",
    });
    expect(set).toMatchObject({ type: "ActionPolicies" });
    const reset = await t.send({
      type: "SetActionPolicy",
      actor: client,
      kind: "draft",
      mode: null,
    });
    if (reset.type !== "ActionPolicies") throw new Error("policies expected");
    expect(reset.policies.find((p) => p.kind === "draft")).toMatchObject({
      mode: "allow",
      customized: false,
    });
  });

  it("refetches on its events", () => {
    const queryClient = new QueryClient();
    const spy = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleAgentsEvent({ type: "ActionPoliciesChanged", actor: client }, ctx);
    expect(spy).toHaveBeenCalledWith({ queryKey: agentsKeys.policies(client) });
    handleAgentsEvent({ type: "PluginsChanged" }, ctx);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(AGENTS_EVENTS).toContain("ConfirmationRequested");
  });
});
