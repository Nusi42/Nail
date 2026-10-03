/**
 * The navigation rail (M5b WP161): landmark, aria-current, switching,
 * badges, tooltips with shortcuts, arrow keys, options, the account avatar.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ResolvedLayout } from "../../api/generated";
import { createEnv, renderNode, settleQueries, widget, type Env } from "../../test/harness";
import { seedAgentsDemo } from "../agents";
import { hostLayout } from "./testing";

function renderRail(env: Env, layout: ResolvedLayout, options: Record<string, unknown> = {}) {
  return renderNode(widget("nav-rail", { options }), env, { layout });
}

const rail = () => screen.getByRole("navigation", { name: "Sections" });
const item = (name: RegExp) => within(rail()).getByRole("button", { name });

describe("nav-rail", () => {
  it("is a navigation landmark with the rail's views, the action and the account", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    renderRail(env, hostLayout());
    await settleQueries(env);
    const buttons = within(rail()).getAllByRole("button");
    expect(
      buttons.map((b) => b.getAttribute("data-view") ?? b.getAttribute("data-action")),
    ).toEqual(["mail", "calendar", "contacts", "tasks", "agents.open-center", "settings"]);
    // The start view is current.
    expect(item(/^Mail/)).toHaveAttribute("aria-current", "page");
    expect(item(/^Calendar/)).not.toHaveAttribute("aria-current");
    // Logo (decorative), spacer, account avatar.
    const nav = rail();
    expect(nav.querySelector('[data-part="nav-rail.logo"]')).toHaveAttribute("aria-hidden", "true");
    expect(nav.querySelector('[data-part="nav-rail.spacer"]')).not.toBeNull();
    expect(within(nav).getByRole("img", { name: /^Account: / })).toBeInTheDocument();
    expect(nav).toHaveAttribute("data-labels", "hidden");
    expect(nav).toHaveAttribute("data-key-context", "global");
  });

  it("switches views through view.show and marks the new one current", async () => {
    const env = createEnv();
    renderRail(env, hostLayout());
    await settleQueries(env);
    fireEvent.click(item(/^Tasks/));
    expect(env.store.getState().activeView).toBe("tasks");
    await waitFor(() => expect(item(/^Tasks/)).toHaveAttribute("aria-current", "page"));
    expect(item(/^Mail/)).not.toHaveAttribute("aria-current");
    expect(item(/^Tasks/)).toHaveAttribute("data-state", "active");
    // With a view host nothing opens as a window.
    expect(env.store.getState().windows).toEqual([]);
  });

  it("runs action items (the confirmation center)", async () => {
    const env = createEnv();
    renderRail(env, hostLayout());
    await settleQueries(env);
    fireEvent.click(item(/^Show confirmations/));
    expect(env.store.getState().windows).toContain("confirmation-center");
  });

  it("shows the unread badge on Mail and the confirmations badge, with accessible names", async () => {
    const env = createEnv();
    const pending = seedAgentsDemo(env.transport);
    renderRail(env, hostLayout());
    await settleQueries(env);
    const mail = item(/^Mail, \d+ unread messages$/);
    const badge = mail.querySelector('[data-part="nav-rail.badge"]');
    expect(badge).toHaveAttribute("data-badge", "unread");
    expect(badge).toHaveAttribute("aria-hidden", "true");
    expect(Number(badge?.textContent)).toBeGreaterThan(0);
    const center = within(rail())
      .getAllByRole("button")
      .find((b) => b.getAttribute("data-action") === "agents.open-center");
    expect(center?.getAttribute("aria-label")).toMatch(
      new RegExp(`, ${pending.length} changes waiting for your approval$`),
    );
    expect(center?.querySelector('[data-part="nav-rail.badge"]')?.textContent).toBe(
      String(pending.length),
    );
    // No badge where the layout names none.
    expect(item(/^Calendar$/).querySelector('[data-part="nav-rail.badge"]')).toBeNull();
  });

  it("shows no confirmations badge while nothing waits", async () => {
    const env = createEnv();
    renderRail(env, hostLayout());
    await settleQueries(env);
    const center = within(rail())
      .getAllByRole("button")
      .find((b) => b.getAttribute("data-action") === "agents.open-center");
    expect(center?.querySelector('[data-part="nav-rail.badge"]')).toBeNull();
    expect(center?.getAttribute("aria-label")).not.toMatch(/waiting/);
  });

  it("names the shortcut in the tooltip and aria-keyshortcuts", async () => {
    const env = createEnv();
    renderRail(env, hostLayout());
    await settleQueries(env);
    const calendar = item(/^Calendar/);
    expect(calendar).toHaveAttribute("title", "Calendar (Ctrl+2)");
    expect(calendar).toHaveAttribute("aria-keyshortcuts", "Control+2");
    // Settings shows through `settings.open` (Ctrl+,), an alias of view.show.
    expect(item(/^Settings/)).toHaveAttribute("title", "Settings (Ctrl+,)");
  });

  it("moves focus with Up / Down / Home / End", async () => {
    const env = createEnv();
    renderRail(env, hostLayout());
    await settleQueries(env);
    const mail = item(/^Mail/);
    act(() => mail.focus());
    fireEvent.keyDown(mail, { key: "ArrowDown" });
    expect(document.activeElement).toBe(item(/^Calendar/));
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    expect(document.activeElement).toBe(item(/^Settings/));
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    expect(document.activeElement).toBe(mail);
    fireEvent.keyDown(mail, { key: "ArrowUp" });
    expect(document.activeElement).toBe(item(/^Settings/));
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    expect(document.activeElement).toBe(mail);
    // Other keys are left alone.
    fireEvent.keyDown(mail, { key: "ArrowRight" });
    expect(document.activeElement).toBe(mail);
  });

  it("follows show-labels and show-logo", async () => {
    const env = createEnv();
    renderRail(env, hostLayout(), { "show-labels": true, "show-logo": false });
    await settleQueries(env);
    const nav = rail();
    expect(nav).toHaveAttribute("data-labels", "shown");
    expect(nav.querySelector('[data-part="nav-rail.logo"]')).toBeNull();
    expect(within(item(/^Contacts/)).getByText("Contacts")).toHaveAttribute(
      "data-part",
      "nav-rail.label",
    );
  });

  it("shows the selected real account, else the first one", async () => {
    const env = createEnv({ state: { activeAccountId: "all" } });
    renderRail(env, hostLayout());
    await settleQueries(env);
    const accounts = env.transport.data.accounts.filter((a) => a.kind !== "virtual");
    const first = accounts[0];
    const second = accounts[1];
    if (!first || !second) throw new Error("the mock needs two accounts");
    expect(within(rail()).getByRole("img")).toHaveAccessibleName(
      `Account: ${first.name || first.email}`,
    );
    act(() => env.store.getState().selectAccount(second.id));
    expect(within(rail()).getByRole("img")).toHaveAccessibleName(
      `Account: ${second.name || second.email}`,
    );
  });

  it("drops separators that separate nothing and renders the rest", async () => {
    const env = createEnv();
    const layout: ResolvedLayout = {
      ...hostLayout(),
      rail: [
        { type: "view", view: "mail" },
        { type: "separator" },
        { type: "view", view: "calendar" },
        { type: "separator" },
      ],
    };
    renderRail(env, layout);
    await settleQueries(env);
    expect(within(rail()).getAllByRole("separator")).toHaveLength(1);
    expect(within(rail()).getAllByRole("button")).toHaveLength(2);
  });
});
