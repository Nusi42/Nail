/**
 * Recent mail on the contact card (R8, ADR 0251): the newest messages from
 * or to the contact's primary address, through the mail search every
 * account shares (`Search`, through `NailClient`); a click opens the
 * message in the mail view. A person without mail shows no section.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { must } from "../../test/must";
import { queryKeys } from "../../state/queries";
import { RECENT_MAIL_COUNT, pimKeys, recentMailQuery } from "./queries";
import { pimStoreFor } from "./store";

function showCard(env: Env, contactId: string) {
  pimStoreFor(env.store).getState().showContact(contactId);
  renderWithProviders(<NodeView node={widget("contact-card")} path="t/0" />, env);
}

describe("recent mail on the contact card (R8)", () => {
  it("asks the mail search for messages from or to the address", () => {
    expect(recentMailQuery("grace@navy.example")).toBe(
      'from:"grace@navy.example" OR to:"grace@navy.example"',
    );
    // Quotes cannot end the value early.
    expect(recentMailQuery('a"b@example.org')).toBe('from:"ab@example.org" OR to:"ab@example.org"');
  });

  it("keeps its own cache entry: the mail lists treat every search entry as theirs", () => {
    expect(pimKeys.recentMail("grace@navy.example")[0]).not.toBe(queryKeys.allSearches[0]);
  });

  it("lists the newest messages with the person and opens one in the mail view", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const grace = must(env.transport.data.features.pim.contacts.find((c) => c.id === "ct-grace"));
    const email = must(grace.fields.emails[0]).value;
    showCard(env, "ct-grace");
    const section = await screen.findByRole("heading", { name: "Recent mail" });
    const list = within(must(section.parentElement)).getByRole("list");
    const rows = within(list).getAllByRole("button");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThanOrEqual(RECENT_MAIL_COUNT);
    expect(requestsOf(env, "Search").at(-1)).toMatchObject({
      query: { text: recentMailQuery(email), account_id: null, folder_id: null, sort: "date" },
    });
    const first = must(rows[0]);
    expect(first).toHaveAccessibleName(/^Open “.+”, /);
    await user.click(first);
    await waitFor(() => {
      expect(env.store.getState().selectedMessageId).not.toBeNull();
    });
    const message = must(
      env.transport.data.messages.find((m) => m.id === env.store.getState().selectedMessageId),
    );
    expect(message.from?.email === email || message.to.some((a) => a.email === email)).toBe(true);
    expect(env.store.getState().activeFolderId).toBe(message.folder_id);
    expect(env.store.getState().activeView).toBe("mail");
  });

  it("shows no section for a person without mail", async () => {
    const env = createEnv();
    const katherine = must(
      env.transport.data.features.pim.contacts.find((c) => c.id === "ct-katherine"),
    );
    must(katherine.fields.emails[0]).value = "nobody@example.net";
    showCard(env, "ct-katherine");
    await screen.findByRole("heading", { name: /Katherine/ });
    await waitFor(() => {
      expect(requestsOf(env, "Search").length).toBeGreaterThan(0);
    });
    expect(screen.queryByRole("heading", { name: "Recent mail" })).toBeNull();
  });
});
