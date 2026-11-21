/**
 * "Values from this mail" (R2, ADR 0222): the menu button beside a
 * condition's or an action's value field (through the mock core's
 * `GetMessageFacts`), and "Add to rule…", which opens the rules window on a
 * saved rule with the mail as its origin.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Condition, MessageSummary, RuleAction } from "../../../api/generated";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../../test/harness";
import { actionContext } from "../../../test/env";
import { must } from "../../../test/must";
import { queryKeys } from "../../../state/queries";
import { literalMenuLabel } from "../../menus/sources";
import { listRules, saveRule } from "../api";
import { newRule } from "../model";
import { rulesStore } from "../store";
import {
  ADD_TO_RULE_ACTION,
  ADD_TO_RULE_PICK_ACTION,
  ActionValuesFromMessage,
  ValuesFromMessage,
  addToRuleEntries,
  domainOf,
  fromMessageActions,
  fromMessageMenuSources,
  valuesMessageOf,
} from "./index";

const FROM: Condition = {
  type: "address",
  field: "from",
  matches: { op: "contains", value: "", case_sensitive: false },
};

/** A mock message with a sender, listed (so its summary is known). */
function messageOf(env: Env): MessageSummary {
  const message = must(env.transport.data.messages.find((m) => m.from !== null));
  env.queryClient.setQueryData(queryKeys.messages(message.folder_id, 50), {
    pages: [{ offset: 0, total: 1, messages: [message] }],
    pageParams: [0],
  });
  return message;
}

function senderDomain(message: MessageSummary): string {
  return must(domainOf(must(message.from).email));
}

describe("valuesMessageOf", () => {
  it("prefers the window's origin, else the selected message", () => {
    expect(valuesMessageOf({ messageId: "m1" }, "m2")).toEqual({ messageId: "m1", from: "origin" });
    expect(valuesMessageOf(null, "m2")).toEqual({ messageId: "m2", from: "selection" });
    expect(valuesMessageOf(null, null)).toBeNull();
  });
});

describe("ValuesFromMessage", () => {
  it("offers the origin mail's values and puts the picked one into the condition", async () => {
    const env = createEnv();
    const message = messageOf(env);
    rulesStore(env.client)
      .getState()
      .setOrigin({ messageId: message.id, folderId: message.folder_id });
    const onPick = vi.fn();
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<ValuesFromMessage condition={FROM} onPick={onPick} />, env);
    const button = await screen.findByRole("button", { name: "Values from this mail" });
    expect(button.closest('[data-part="rules.from-message"]')).toHaveAttribute(
      "data-from",
      "origin",
    );
    await user.click(button);
    const menu = screen.getByRole("menu", { name: "Values from this mail" });
    expect(within(menu).getByText("From")).toBeVisible();
    const domain = senderDomain(message);
    await user.click(within(menu).getByRole("menuitem", { name: `Domain: ${domain}` }));
    expect(onPick).toHaveBeenCalledWith(`@${domain}`, {
      ...FROM,
      matches: { op: "ends-with", value: `@${domain}`, case_sensitive: false },
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("offers the selected message, labelled so, when the window has no origin", async () => {
    const env = createEnv();
    const message = messageOf(env);
    env.store.getState().selectMessage(message.id);
    renderWithProviders(<ValuesFromMessage condition={FROM} onPick={vi.fn()} />, env);
    const button = await screen.findByRole("button", { name: "Values from the selected mail" });
    expect(button.closest('[data-part="rules.from-message"]')).toHaveAttribute(
      "data-from",
      "selection",
    );
  });

  it("renders nothing without a message or without values for the field", async () => {
    const env = createEnv();
    const { container, unmount } = renderWithProviders(
      <ValuesFromMessage condition={FROM} onPick={vi.fn()} />,
      env,
    );
    await settleQueries(env, 20);
    expect(container).toBeEmptyDOMElement();
    unmount();

    const message = messageOf(env);
    env.store.getState().selectMessage(message.id);
    const body: Condition = { type: "body", matches: FROM.matches };
    const other = renderWithProviders(<ValuesFromMessage condition={body} onPick={vi.fn()} />, env);
    await settleQueries(env, 20);
    expect(other.container).toBeEmptyDOMElement();
  });

  it("offers the mail's folder for a move action", async () => {
    const env = createEnv();
    const message = messageOf(env);
    env.store.getState().selectMessage(message.id);
    const folder = must(env.transport.data.folders.find((f) => f.id === message.folder_id));
    const move: RuleAction = { type: "move", folder: { type: "path", path: "", account_id: null } };
    const onPick = vi.fn();
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<ActionValuesFromMessage action={move} onPick={onPick} />, env);
    await user.click(await screen.findByRole("button", { name: "Values from the selected mail" }));
    await user.click(screen.getByRole("menuitem", { name: folder.path }));
    expect(onPick).toHaveBeenCalledWith(folder.path, {
      type: "move",
      folder: { type: "path", path: folder.path, account_id: null },
    });
  });
});

describe("Add to rule…", () => {
  const actions = fromMessageActions("rules");

  it("lists the saved rules, named as written", async () => {
    const env = createEnv();
    const rule = await saveRule(env.client, { ...newRule(), name: "@home" });
    const entries = await must(fromMessageMenuSources[ADD_TO_RULE_ACTION]).load(actionContext(env));
    expect(entries).toEqual(addToRuleEntries(await listRules(env.client)));
    expect(
      entries.find((e) => e.type === "action" && e.label === literalMenuLabel("@home")),
    ).toMatchObject({
      type: "action",
      action: ADD_TO_RULE_PICK_ACTION,
      args: [rule.id],
    });
  });

  it("opens the rules window on the picked rule with the mail as origin", async () => {
    const env = createEnv();
    const ctx = actionContext(env);
    const pickRule = must(actions[ADD_TO_RULE_PICK_ACTION]);
    expect(pickRule.enabled?.(ctx, ["r"])).toBe(false);
    await pickRule.run(ctx, ["r"]);
    expect(env.store.getState().windows).toEqual([]);

    const message = messageOf(env);
    env.store.getState().selectMessage(message.id);
    expect(pickRule.enabled?.(ctx, ["r"])).toBe(true);
    await pickRule.run(ctx, ["r"]);
    const rules = rulesStore(env.client).getState();
    expect(rules.origin).toEqual({ messageId: message.id, folderId: message.folder_id });
    expect(rules.editing).toMatchObject({ kind: "saved", id: "r" });
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual(["rules"]);
    });
  });

  it("without a rule opens the window with the mail as origin", async () => {
    const env = createEnv();
    const message = messageOf(env);
    env.store.getState().selectMessage(message.id);
    await must(actions[ADD_TO_RULE_ACTION]).run(actionContext(env), []);
    const rules = rulesStore(env.client).getState();
    expect(rules.origin?.messageId).toBe(message.id);
    expect(rules.editing).toBeNull();
    expect(env.store.getState().windows).toEqual(["rules"]);
  });
});
