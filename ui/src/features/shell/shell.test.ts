/**
 * The shell slot's contract-stage actions (M5b): `view.show` switches the
 * active view and, while the main window has no `view-host`, opens the
 * view's window; `view.next` / `view.previous` step through the rail.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { ActionContext } from "../../actions/registry";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";
import { createEnv } from "../../test/harness";
import { actions, showView } from ".";
import { fallbackLayout } from "./testing";

function context(layout: ResolvedLayout): ActionContext {
  const env = createEnv();
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    layout,
    t: (id) => id,
    locale: "en",
  };
}

const run = (ctx: ActionContext, id: string, ...args: string[]) => actions[id]?.run(ctx, args);

describe("shell actions (M5b contract stage)", () => {
  it("opens a view's window while the layout has no view host", () => {
    const ctx = context(fallbackLayout());
    expect(actions["view.show"]?.enabled?.(ctx, ["calendar"])).toBe(true);
    expect(actions["view.show"]?.enabled?.(ctx, ["ghost"])).toBe(false);
    expect(actions["view.show"]?.enabled?.(ctx, [])).toBe(false);
    void run(ctx, "view.show", "calendar");
    expect(ctx.store.getState().activeView).toBe("calendar");
    expect(ctx.store.getState().windows).toEqual(["calendar"]);
    void run(ctx, "view.next");
    expect(ctx.store.getState().activeView).toBe("contacts");
    expect(ctx.store.getState().windows).toEqual(["calendar", "contacts"]);
    // Mail shows the main window: what covers it closes.
    void run(ctx, "view.show", "mail");
    expect(ctx.store.getState().windows).toEqual([]);
    void run(ctx, "view.previous");
    expect(ctx.store.getState().activeView).toBe("settings");
    // Unknown views do nothing.
    showView(ctx, "ghost");
    expect(ctx.store.getState().activeView).toBe("settings");
  });

  it("only switches the active view when the main window hosts views", () => {
    const host: LayoutNode = {
      type: "widget",
      id: null,
      label: null,
      kind: "view-host",
      options: {},
      rows: [],
      source: null,
    };
    const base = normalizeLayout(defaultLayoutJson);
    const ctx = context({
      ...base,
      windows: { ...base.windows, main: [{ min_width: null, max_width: null, root: host }] },
    });
    void run(ctx, "view.show", "tasks");
    expect(ctx.store.getState().activeView).toBe("tasks");
    expect(ctx.store.getState().windows).toEqual([]);
  });

  it("the default layout hosts the views (WP165)", () => {
    const ctx = context(normalizeLayout(defaultLayoutJson));
    void run(ctx, "view.show", "calendar");
    expect(ctx.store.getState().activeView).toBe("calendar");
    expect(ctx.store.getState().windows).toEqual([]);
    // The old window aliases show the view, too (no dialog copy).
    expect(ctx.layout.actions["pim.contacts.open"]?.alias_of).toBe("view.show");
    expect(ctx.layout.actions["pim.contacts.open"]?.args).toEqual(["contacts"]);
  });
});
