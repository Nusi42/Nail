/**
 * Accessibility of the Gmail UI (axe-core in jsdom, like
 * `src/a11y.test.tsx`): the label list (with hidden labels and a pending
 * one), the category tabs, the message chips, and every dialog with real
 * content (editor for a new and an existing label, delete confirmation,
 * apply labels). Contrast needs a real layout engine (Playwright).
 */
import type { InfiniteData } from "@tanstack/react-query";
import { act, screen, waitFor } from "@testing-library/react";
import { violations } from "../../test/axe";
import type { ComponentType } from "react";
import type { Response } from "../../api/generated";
import type { DialogProps } from "../../layout/dialogRegistry";
import { NodeView } from "../../layout/LayoutRenderer";
import { queryKeys } from "../../state/queries";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { dialogs, MessageLabels } from ".";
import { GMAIL_ACCOUNT, GMAIL_FOLDERS, seedGmailDemo } from "./mockSeed";
import { APPLY_LABELS_DIALOG, DIALOGS, LABEL_DELETE_DIALOG, LABEL_EDITOR_DIALOG } from "./names";
import { gmailStoreFor } from "./store";

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

function setup(): Env {
  const env = createEnv();
  seedGmailDemo(env.transport.data);
  env.store.getState().selectAccount(GMAIL_ACCOUNT);
  env.store.getState().selectFolder(GMAIL_FOLDERS.inbox);
  return env;
}

describe("axe: Gmail widgets", () => {
  it("the label list, the category tabs and message chips have no violations", async () => {
    const env = setup();
    const travel = env.transport.data.messages.find((m) => m.subject === "Hotel confirmation");
    if (!travel) throw new Error("demo message missing");
    // A label whose creation is still queued.
    env.transport.data.behaviour.workMs = 600_000;
    await env.client.request(
      {
        type: "CreateGmailLabel",
        account_id: GMAIL_ACCOUNT,
        settings: {
          name: "Queued",
          color: null,
          list_visibility: "show",
          message_visibility: "show",
        },
      },
      "GmailLabelSaved",
    );
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("label-list", { options: { "show-hidden": true } })} path="t/0" />
        <NodeView node={widget("category-tabs")} path="t/1" />
        <MessageLabels message={travel} />
      </>,
      env,
    );
    await settle(env);
    expect(screen.getByRole("navigation", { name: "Inbox categories" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Queued, not synced yet/ })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("axe: Gmail dialogs", () => {
  function prepare(env: Env, name: string, existing: boolean): void {
    const gmail = gmailStoreFor(env.store).getState();
    if (name === LABEL_EDITOR_DIALOG) {
      gmail.editLabel({
        accountId: GMAIL_ACCOUNT,
        labelId: existing ? "Label_4" : null,
        parent: existing ? null : "Work",
      });
    }
    if (name === LABEL_DELETE_DIALOG) {
      const label = env.transport.data.features.gmail.labels[GMAIL_ACCOUNT]?.find(
        (l) => l.id === "Label_1",
      );
      gmail.deleteLabel(label ?? null);
    }
    if (name === APPLY_LABELS_DIALOG) {
      const summary = env.transport.data.messages.find((m) => m.account_id === GMAIL_ACCOUNT);
      if (!summary) throw new Error("demo message missing");
      env.queryClient.setQueryData<InfiniteData<Extract<Response, { type: "Messages" }>>>(
        queryKeys.messages(summary.folder_id, 50),
        {
          pages: [
            {
              type: "Messages",
              folder_id: summary.folder_id,
              offset: 0,
              total: 1,
              messages: [summary],
            },
          ],
          pageParams: [0],
        },
      );
      env.store.getState().selectMessage(summary.id);
    }
  }

  const cases = [...DIALOGS.map((d) => [d, false] as const), [LABEL_EDITOR_DIALOG, true] as const];
  it.each(cases)(
    "dialog %s (existing label: %s) has no violations",
    async (name, existing) => {
      const env = setup();
      prepare(env, name, existing);
      const Dialog = dialogs[name] as ComponentType<DialogProps>;
      const { container } = renderWithProviders(
        <div role="dialog" aria-label={name}>
          <Dialog name={name} close={() => undefined} />
        </div>,
        env,
      );
      await settle(env);
      expect(container.querySelector('[data-part="gmail.dialog"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    30_000,
  );
});
