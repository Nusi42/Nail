/**
 * The state helpers Q2 extracted from the features (SSOT): reply-all
 * recipients, a draft's composer fields, deleting a saved draft, the
 * active Inbox and the hooks for the active mail view and UI context.
 */
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";
import type { Draft } from "../api/generated";
import { createEnv, Providers } from "../test/harness";
import { INBOX } from "../test/mockIds";
import { replyAllCc } from "./addresses";
import { deleteSavedDraft, draftFields } from "./compose";
import { useActiveMailView, useUiContextIds } from "./hooks";
import { activeInbox } from "./mailOps";
import { queryKeys } from "./queries";

const ada = { name: "Ada", email: "ada@example.org" };
const grace = { name: "Grace", email: "grace@navy.example" };
const alan = { name: null, email: "alan@example.org" };

const draft = (patch: Partial<Draft> = {}): Draft => ({
  id: null,
  identity_id: "ada@example.org",
  to: [grace],
  cc: [],
  bcc: [],
  subject: "Hello",
  mode: "plain",
  text: "Hi Grace",
  html: null,
  markdown: null,
  attachments: [],
  context: null,
  signature: "identity",
  markdown_part: false,
  ...patch,
});

describe("replyAllCc", () => {
  it("copies the original's other recipients once, without the answered and own addresses", () => {
    const cc = replyAllCc(
      [grace],
      { to: [ada, { ...grace, email: "GRACE@navy.example" }], cc: [alan, alan] },
      new Set(["ada@example.org"]),
    );
    expect(cc).toEqual([alan]);
  });
});

describe("draftFields", () => {
  it("turns a draft's recipients into the composer's text fields", () => {
    const fields = draftFields(draft({ cc: [alan], subject: "Re: Hello" }));
    expect(fields).toEqual({
      to: "Grace <grace@navy.example>",
      cc: "alan@example.org",
      bcc: "",
      showCc: true,
      showBcc: false,
      subject: "Re: Hello",
      context: null,
    });
  });
});

describe("deleteSavedDraft", () => {
  it("deletes a saved draft and takes one that is already gone as done", async () => {
    const env = createEnv();
    const { draft_id } = await env.client.request(
      { type: "SaveDraft", draft: draft() },
      "DraftSaved",
    );
    await deleteSavedDraft(env, draft_id);
    expect(env.transport.data.drafts.has(draft_id)).toBe(false);
    await expect(deleteSavedDraft(env, draft_id)).resolves.toBeUndefined();
  });
});

describe("activeInbox", () => {
  it("is the active account's Inbox once its folders are known", () => {
    const env = createEnv();
    expect(activeInbox(env)).toBeNull();
    act(() => {
      env.store.getState().selectAccount("acc-personal");
    });
    expect(activeInbox(env)).toBeNull();
    env.queryClient.setQueryData(queryKeys.folders("acc-personal"), [
      { id: "acc-personal/Sent", role: "sent" },
      { id: INBOX, role: "inbox" },
    ]);
    expect(activeInbox(env)).toEqual({ accountId: "acc-personal", inboxId: INBOX });
  });
});

describe("mail view hooks", () => {
  it("read the active account, folder, search and selection from the UI store", () => {
    const env = createEnv();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <Providers env={env}>{children}</Providers>
    );
    const view = renderHook(() => useActiveMailView(), { wrapper });
    const ids = renderHook(() => useUiContextIds(), { wrapper });
    act(() => {
      env.store.getState().selectAccount("acc-personal");
      env.store.getState().selectFolder(INBOX);
    });
    expect(view.result.current.store).toBe(env.store);
    expect(view.result.current).toMatchObject({ accountId: "acc-personal", activeFolderId: INBOX });
    expect(ids.result.current).toMatchObject({
      accountId: "acc-personal",
      folderId: INBOX,
      messageId: null,
      draftId: null,
    });
  });
});
