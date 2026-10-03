/**
 * The view host (M5b WP161): the active view's window, views kept mounted,
 * the remembered view, focus and keyboard contexts on switching, and the
 * PIM views as full views with the existing PIM widgets.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ResolvedLayout } from "../../api/generated";
import { WindowView } from "../../layout/LayoutRenderer";
import { UI_STATE_VIEW } from "../../state/uiState";
import {
  createEnv,
  renderWithProviders,
  settleQueries,
  stubElementSizes,
  type Env,
} from "../../test/harness";
import { hostLayout } from "./testing";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

function renderShell(env: Env, layout: ResolvedLayout = hostLayout()) {
  return renderWithProviders(<WindowView layout={layout} name="main" />, env, { layout });
}

/** The host's region of a view (widgets inside may use the same name). */
const region = (name: string) => {
  const found = screen
    .getAllByRole("region", { name })
    .find((el) => el.getAttribute("data-part") === "view-host.view");
  if (!found) throw new Error(`no view region "${name}"`);
  return found;
};
const host = (container: HTMLElement) =>
  container.querySelector<HTMLElement>('[data-part="view-host"]');
const viewEl = (container: HTMLElement, id: string) =>
  container.querySelector<HTMLElement>(`[data-part="view-host.view"][data-view="${id}"]`);
const railButton = (name: RegExp) =>
  within(screen.getByRole("navigation", { name: "Sections" })).getByRole("button", { name });

async function storedView(env: Env): Promise<unknown> {
  const response = await env.client.call({ type: "GetUiState" });
  return response.type === "UiState" ? (response.state[UI_STATE_VIEW] ?? null) : undefined;
}

describe("view-host", () => {
  it("shows the start view's window in a labelled region", async () => {
    const env = createEnv();
    const { container } = renderShell(env);
    await settleQueries(env);
    expect(host(container)).toHaveAttribute("data-view", "mail");
    const mail = region("Mail view");
    expect(mail).toHaveAttribute("data-window", "mail");
    expect(mail).toHaveAttribute("data-key-context", "global");
    expect(mail).toHaveAttribute("tabindex", "-1");
    expect(mail.querySelector('[data-part="message-list"]')).not.toBeNull();
    // Nothing is written until the user switches.
    expect(await storedView(env)).toBeNull();
  });

  it("hosts Calendar, Contacts and Tasks with the existing PIM widgets and keeps views mounted", async () => {
    const env = createEnv();
    const { container } = renderShell(env);
    await settleQueries(env);

    fireEvent.click(railButton(/^Calendar/));
    await settleQueries(env);
    const calendar = region("Calendar view");
    expect(calendar).toHaveAttribute("data-key-context", "calendar");
    expect(calendar.querySelector('[data-key-context="calendar"]')).not.toBeNull();
    expect(calendar.querySelector('[data-part="calendar-list"]')).not.toBeNull();
    // The mail view stays mounted, hidden.
    expect(viewEl(container, "mail")).toHaveAttribute("hidden");
    expect(viewEl(container, "mail")?.querySelector('[data-part="message-list"]')).not.toBeNull();

    fireEvent.click(railButton(/^Contacts/));
    await settleQueries(env);
    expect(region("Contacts view").querySelector('[data-part="contact-list"]')).not.toBeNull();

    fireEvent.click(railButton(/^Tasks/));
    await settleQueries(env);
    expect(region("Tasks view").querySelector('[data-part="task-list"]')).not.toBeNull();
    expect(viewEl(container, "calendar")).toHaveAttribute("hidden");
    // Never as a modal window.
    expect(env.store.getState().windows).toEqual([]);
    expect(container.querySelector('[data-part="window"]')).toBeNull();
  });

  it("remembers the last view in the UI state", async () => {
    const env = createEnv();
    renderShell(env);
    await settleQueries(env);
    fireEvent.click(railButton(/^Tasks/));
    await settleQueries(env);
    expect(await storedView(env)).toBe("tasks");
    fireEvent.click(railButton(/^Mail/));
    await settleQueries(env);
    expect(await storedView(env)).toBe("mail");
  });

  it("starts with the remembered view without showing the start view first", async () => {
    const env = createEnv();
    await env.client.call({ type: "SetUiState", key: UI_STATE_VIEW, value: "contacts" });
    const { container } = renderShell(env);
    await settleQueries(env);
    expect(host(container)).toHaveAttribute("data-view", "contacts");
    expect(env.store.getState().activeView).toBe("contacts");
    expect(railButton(/^Contacts/)).toHaveAttribute("aria-current", "page");
    // The mail view was never mounted.
    expect(viewEl(container, "mail")).toBeNull();
  });

  it("ignores a remembered view the layout no longer defines", async () => {
    const env = createEnv();
    await env.client.call({ type: "SetUiState", key: UI_STATE_VIEW, value: "ghost" });
    const { container } = renderShell(env);
    await settleQueries(env);
    expect(host(container)).toHaveAttribute("data-view", "mail");
    expect(env.store.getState().activeView).toBeNull();
  });

  it("moves focus into the new view on a shortcut and sets its keyboard context", async () => {
    const env = createEnv();
    renderShell(env);
    await settleQueries(env);
    fireEvent.keyDown(document.body, { key: "2", ctrlKey: true });
    await settleQueries(env);
    expect(env.store.getState().activeView).toBe("calendar");
    await waitFor(() => expect(document.activeElement).toBe(region("Calendar view")));
    expect(env.store.getState().keyContext).toBe("calendar");

    fireEvent.keyDown(document.activeElement as Element, { key: "1", ctrlKey: true });
    await settleQueries(env);
    await waitFor(() => expect(document.activeElement).toBe(region("Mail view")));
    expect(env.store.getState().keyContext).toBe("global");
  });

  it("keeps focus on the rail button after a click", async () => {
    const env = createEnv();
    renderShell(env);
    await settleQueries(env);
    const tasks = railButton(/^Tasks/);
    act(() => tasks.focus());
    fireEvent.click(tasks);
    await settleQueries(env);
    expect(region("Tasks view")).toBeVisible();
    expect(document.activeElement).toBe(tasks);
  });

  it("does not steal focus when it starts", async () => {
    const env = createEnv();
    await env.client.call({ type: "SetUiState", key: UI_STATE_VIEW, value: "tasks" });
    renderShell(env);
    await settleQueries(env);
    expect(document.activeElement).toBe(document.body);
  });

  it("closes a copy of the view's window that covers the host", async () => {
    const env = createEnv();
    renderShell(env);
    await settleQueries(env);
    act(() => env.store.getState().openWindow("calendar"));
    expect(env.store.getState().windows).toEqual(["calendar"]);
    // The rail is covered by the window; the palette / an action shows the view.
    fireEvent.click(railButton(/^Calendar/));
    expect(env.store.getState().windows).toEqual([]);
    expect(env.store.getState().activeView).toBe("calendar");
  });

  it("explains a view it cannot show", async () => {
    const env = createEnv();
    const base = hostLayout();
    const layout: ResolvedLayout = {
      ...base,
      views: { ...base.views, tasks: { label: null, icon: null, window: "nowhere", badge: null } },
    };
    renderShell(env, layout);
    await settleQueries(env);
    act(() => env.store.getState().setActiveView("tasks"));
    await settleQueries(env);
    expect(within(region("Tasks view")).getByRole("status")).toHaveTextContent(
      /Tasks cannot be shown/,
    );
  });

  it("renders nothing without views", async () => {
    const env = createEnv();
    const layout: ResolvedLayout = { ...hostLayout(), views: {}, rail: [] };
    const { container } = renderShell(env, layout);
    await settleQueries(env);
    expect(host(container)).toBeNull();
  });
});
