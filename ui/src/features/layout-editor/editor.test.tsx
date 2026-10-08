/**
 * Customize-layout mode end to end against the mock core (M5d WP181): the
 * banner, pane headers, the eye, moving panes with the keyboard and the
 * pointer, the palette (presets, Add to layout), Undo / Redo with "Undo
 * anyway" after a change on disk, Reset to default, Done, Escape, the
 * settings card and the `layout.customize` action; axe on the editor.
 */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { useEffect, useState, type ReactNode } from "react";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import { simulateExternalEdit } from "../../api/editHistoryMock";
import { WindowView } from "../../layout/LayoutRenderer";
import { registeredWindowChrome } from "../../layout/LayoutChrome";
import { applyEditHistory } from "../../state/editHistory";
import { violations } from "../../test/axe";
import {
  Providers,
  createEnv,
  setWindowWidth,
  settleQueries,
  stubElementSizes,
  widget,
  type Env,
} from "../../test/harness";
import { render } from "@testing-library/react";
import { must } from "../../test/must";
import { LAYOUT_EDITOR_SKIN_CSS, actions, widgets } from "./index";
import { resetEditor, startEditing, editorStore } from "./store";
import { CustomizeViewWatcher } from "./ViewWatcher";
import { actionContext, staleLayoutRevision } from "../../test/env";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  setWindowWidth(1200);
});
afterEach(() => {
  cleanup();
  resetEditor();
  restoreSizes();
  setWindowWidth(1024);
});

/** Renders `children(layout)` with the layout the mock core announces. */
function Live({ env, children }: { env: Env; children: (layout: ResolvedLayout) => ReactNode }) {
  const [layout, setLayout] = useState(env.transport.data.layout);
  useEffect(
    () =>
      env.client.subscribe((event) => {
        if (event.type === "ConfigChanged") setLayout(event.layout);
        if (event.type === "EditHistoryChanged") applyEditHistory(env.queryClient, event.history);
      }),
    [env],
  );
  return (
    <Providers env={env} layout={layout}>
      {children(layout)}
    </Providers>
  );
}

function renderMail(env: Env, extra?: ReactNode) {
  return render(
    <Live env={env}>
      {(layout) => (
        <>
          <WindowView layout={layout} name="mail" />
          {extra}
        </>
      )}
    </Live>,
  );
}

const mailRoot = (env: Env): LayoutNode => must(env.transport.data.layout.windows.mail?.[0]).root;
const paneIds = (env: Env): (string | null)[] => {
  const root = mailRoot(env);
  return root.type === "widget" ? [root.id] : root.children.map((c) => c.id);
};
const editor = () => screen.getByRole("region", { name: /Customizing layout/ });
const pane = (name: string) => {
  const found = screen
    .getAllByRole("region", { name })
    .find((el) => el.getAttribute("data-part") === "layout-editor.pane");
  if (!found) throw new Error(`no pane ${name}`);
  return found;
};

async function customize(env: Env) {
  renderMail(env);
  await settleQueries(env);
  act(() => {
    startEditing("mail");
  });
  await screen.findByRole("region", { name: "Customizing layout: Mail" });
  await settleQueries(env);
}

/** Hides the Folders pane with its eye button. */
function hideFolders(): void {
  fireEvent.click(within(pane("Folders")).getByRole("button", { name: "Hide Folders" }));
}

/** Runs `step` and checks that it ended customize mode. */
async function endedBy(env: Env, step: () => unknown): Promise<void> {
  await act(async () => {
    await step();
  });
  expect(editorStore.getState().window).toBeNull();
  await settleQueries(env);
}

describe("customize-layout mode", () => {
  it("frames the window with the banner, pane headers and the palette", async () => {
    const env = createEnv();
    await customize(env);
    const region = editor();
    expect(screen.getByRole("heading", { name: "Customizing layout: Mail" })).toHaveFocus();
    expect(registeredWindowChrome("mail")).not.toBeNull();
    for (const name of ["Folders", "Messages", "Message"]) {
      const p = pane(name);
      expect(within(p).getByRole("button", { name: `Move ${name}` })).toBeInTheDocument();
      expect(within(p).getByRole("button", { name: `Hide ${name}` })).toBeInTheDocument();
      expect(
        within(p).getByRole("button", { name: `Remove ${name} from the layout` }),
      ).toBeInTheDocument();
      // The real pane is there, but not usable while customizing.
      expect(p.querySelector('[data-part="layout-editor.pane-body"]')).toHaveAttribute("inert");
    }
    // The widgets inside a pane get no header of their own.
    expect(region.querySelectorAll('[data-part="layout-editor.pane"]')).toHaveLength(3);
    const palette = screen.getByRole("complementary", { name: "Layout palette" });
    expect(within(palette).getByRole("button", { name: /Three panes/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(palette).getByRole("button", { name: /Reader below/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(within(palette).getByRole("button", { name: "Move Mini month" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Undo" })).toBeDisabled();
    expect(await violations(region)).toEqual([]);
  });

  it("hides and shows a pane with the eye", async () => {
    const env = createEnv();
    await customize(env);
    hideFolders();
    await waitFor(() => {
      expect(pane("Folders")).toHaveAttribute("data-state", "hidden");
    });
    expect(screen.getByRole("status")).toHaveTextContent("Folders is hidden.");
    const root = mailRoot(env);
    expect(root.type === "split" && root.children[0]?.hidden).toBe(true);
    // Focus stays on the pane that changed.
    await waitFor(() => {
      expect(within(pane("Folders")).getByRole("button", { name: "Move Folders" })).toHaveFocus();
    });
    fireEvent.click(within(pane("Folders")).getByRole("button", { name: "Show Folders" }));
    await waitFor(() => {
      expect(pane("Folders")).not.toHaveAttribute("data-state");
    });
    await settleQueries(env);
  });

  it("keeps moving mode accessible and styled", async () => {
    const env = createEnv();
    await customize(env);
    fireEvent.click(within(pane("Folders")).getByRole("button", { name: "Move Folders" }));
    await screen.findByRole("button", { name: "Swap Folders with Message" });
    expect(await violations(editor())).toEqual([]);
    // Every part the editor renders is styled by its skin.
    const parts = new Set(
      [...editor().querySelectorAll("[data-part^='layout-editor']")].map((el) =>
        must(el.getAttribute("data-part")),
      ),
    );
    parts.add(must(editor().getAttribute("data-part")));
    for (const part of parts) expect(LAYOUT_EDITOR_SKIN_CSS).toContain(`[data-part="${part}"]`);
    await settleQueries(env);
  });

  it("swaps panes with the keyboard: pick up, choose a target, undo, redo", async () => {
    const env = createEnv();
    await customize(env);
    const handle = within(pane("Folders")).getByRole("button", { name: "Move Folders" });
    fireEvent.click(handle);
    // The first drop target takes the focus; the moved pane offers none.
    const swap = await screen.findByRole("button", { name: "Swap Folders with Message" });
    expect(document.activeElement?.getAttribute("data-part")).toBe("layout-editor.target");
    expect(pane("Folders").querySelector('[data-part="layout-editor.targets"]')).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Choose where it goes");
    fireEvent.click(swap);
    await waitFor(() => {
      expect(paneIds(env)).toEqual(["reader", "list-pane", "sidebar"]);
    });
    expect(await screen.findByText("Folders and Message swapped places.")).toBeInTheDocument();
    const undo = await screen.findByRole("button", { name: "Undo" });
    await waitFor(() => {
      expect(undo).toBeEnabled();
    });
    expect(undo).toHaveAttribute("title", "Undo: Swap panes");
    fireEvent.click(undo);
    await waitFor(() => {
      expect(paneIds(env)).toEqual(["sidebar", "list-pane", "reader"]);
    });
    const redo = screen.getByRole("button", { name: "Redo" });
    await waitFor(() => {
      expect(redo).toBeEnabled();
    });
    fireEvent.click(redo);
    await waitFor(() => {
      expect(paneIds(env)).toEqual(["reader", "list-pane", "sidebar"]);
    });
    await settleQueries(env);
  });

  // Review finding (ADR 0138): a double-click on Undo undoes one entry.
  it("undoes one entry per double-click, and with Ctrl+Z / Ctrl+Y", async () => {
    const env = createEnv();
    await customize(env);
    const hidden = () =>
      mailRoot(env).type === "widget"
        ? 0
        : (mailRoot(env) as Extract<LayoutNode, { children: unknown }>).children.filter(
            (c) => c.hidden === true,
          ).length;
    hideFolders();
    await waitFor(() => {
      expect(hidden()).toBe(1);
    });
    fireEvent.click(within(pane("Messages")).getByRole("button", { name: "Hide Messages" }));
    await waitFor(() => {
      expect(hidden()).toBe(2);
    });
    const undo = screen.getByRole("button", { name: "Undo" });
    await waitFor(() => {
      expect(undo).toBeEnabled();
    });
    fireEvent.click(undo);
    fireEvent.click(undo);
    await waitFor(() => {
      expect(hidden()).toBe(1);
    });
    await settleQueries(env);
    expect(hidden()).toBe(1);
    // The keyboard: Ctrl+Z undoes the other one, Ctrl+Y redoes it.
    await waitFor(() => {
      expect(undo).toBeEnabled();
    });
    fireEvent.keyDown(editor(), { key: "z", ctrlKey: true });
    await waitFor(() => {
      expect(hidden()).toBe(0);
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Redo" })).toBeEnabled();
    });
    fireEvent.keyDown(editor(), { key: "y", ctrlKey: true });
    await waitFor(() => {
      expect(hidden()).toBe(1);
    });
    await settleQueries(env);
  });

  it("docks a pane below another and cancels a pick-up with Escape", async () => {
    const env = createEnv();
    await customize(env);
    const handle = within(pane("Message")).getByRole("button", { name: "Move Message" });
    handle.focus();
    fireEvent.click(handle);
    await screen.findByRole("button", { name: "Put Message below Messages" });
    fireEvent.keyDown(document.activeElement ?? editor(), { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Put Message below Messages" })).toBeNull();
    });
    expect(screen.getByRole("status")).toHaveTextContent("Nothing was moved.");
    expect(within(pane("Message")).getByRole("button", { name: "Move Message" })).toHaveFocus();
    // Still customizing; a second Escape ends it.
    expect(editor()).toBeInTheDocument();
    fireEvent.click(within(pane("Message")).getByRole("button", { name: "Move Message" }));
    fireEvent.click(await screen.findByRole("button", { name: "Put Message below Messages" }));
    await waitFor(() => {
      const root = mailRoot(env);
      expect(root.type === "split" && root.children.map((c) => c.type)).toEqual(["stack", "split"]);
    });
    await screen.findByText("Message is now below Messages.");
    fireEvent.keyDown(editor(), { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: /Customizing layout/ })).toBeNull();
    });
    await settleQueries(env);
  });

  it("drags a pane onto a target with the pointer", async () => {
    const env = createEnv();
    await customize(env);
    const handle = within(pane("Folders")).getByRole("button", { name: "Move Folders" });
    fireEvent.pointerDown(handle, { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
    // Below the threshold nothing happens.
    act(() => {
      firePointer("pointermove", 1, 11, 11);
    });
    expect(editorStore.getState().dragging).toBe(false);
    const original = document.elementFromPoint.bind(document);
    let under: Element | null = null;
    document.elementFromPoint = () => under;
    try {
      act(() => {
        firePointer("pointermove", 1, 60, 60);
      });
      expect(editorStore.getState().dragging).toBe(true);
      const target = await screen.findByRole("button", { name: "Put Folders right of Message" });
      under = target;
      act(() => {
        firePointer("pointermove", 1, 400, 200);
      });
      expect(target).toHaveAttribute("data-state", "hover");
      act(() => {
        firePointer("pointerup", 1, 400, 200);
      });
      await waitFor(() => {
        expect(paneIds(env)).toEqual(["list-pane", "reader", "sidebar"]);
      });
      // A drag released over nothing puts the pane back.
      under = null;
      fireEvent.pointerDown(within(pane("Folders")).getByRole("button", { name: "Move Folders" }), {
        button: 0,
        pointerId: 2,
        clientX: 0,
        clientY: 0,
      });
      act(() => {
        firePointer("pointermove", 2, 50, 50);
      });
      expect(editorStore.getState().moving).not.toBeNull();
      act(() => {
        firePointer("pointerup", 2, 50, 50);
      });
      expect(editorStore.getState().moving).toBeNull();
    } finally {
      document.elementFromPoint = original;
    }
    await settleQueries(env);
  });

  it("applies presets and greys out those the window cannot take", async () => {
    const env = createEnv();
    await customize(env);
    const palette = screen.getByRole("complementary", { name: "Layout palette" });
    fireEvent.click(within(palette).getByRole("button", { name: /Focus/ }));
    await waitFor(() => {
      expect(within(palette).getByRole("button", { name: /Focus/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    expect(screen.getByRole("status")).toHaveTextContent("Layout preset “Focus” applied.");
    // The other panes stay, hidden, and can be shown again.
    expect(pane("Folders")).toHaveAttribute("data-state", "hidden");
    expect(pane("Messages")).toHaveAttribute("data-state", "hidden");
    fireEvent.click(within(palette).getByRole("button", { name: /Reader below/ }));
    await waitFor(() => {
      expect(within(palette).getByRole("button", { name: /Reader below/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    // Without the reader, no preset fits.
    fireEvent.click(
      within(pane("Message")).getByRole("button", { name: "Remove Message from the layout" }),
    );
    await waitFor(() => {
      expect(within(palette).getByRole("button", { name: /Three panes/ })).toBeDisabled();
    });
    expect(within(palette).getByText(/unavailable/)).toBeInTheDocument();
    // Reset to default brings the window back.
    fireEvent.click(screen.getByRole("button", { name: "Reset to default" }));
    await waitFor(() => {
      expect(within(palette).getByRole("button", { name: /Three panes/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    expect(screen.getByRole("status")).toHaveTextContent("Mail is back to the default layout.");
    await settleQueries(env);
  });

  it("adds a widget from the palette with the keyboard", async () => {
    const env = createEnv();
    await customize(env);
    fireEvent.click(screen.getByRole("button", { name: "Move Mini month" }));
    expect(screen.getByRole("button", { name: "Stop moving Mini month" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    fireEvent.click(await screen.findByRole("button", { name: "Add Mini month to Folders" }));
    await waitFor(() => {
      const root = mailRoot(env);
      const sidebar = root.type === "split" ? root.children[0] : undefined;
      expect(sidebar?.type === "stack" && sidebar.children.at(-1)).toMatchObject({
        kind: "mini-month",
        id: "mini-month",
      });
    });
    await screen.findByText("Mini month was added to Folders.");
    // The drop target went away: the focus is on the editor's title.
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: /Customizing layout/ })).toHaveFocus();
    });
    await settleQueries(env);
  });

  it("shows panes that view.toggle hid for now", async () => {
    const env = createEnv({ state: { hiddenNodes: ["sidebar"] } });
    renderMail(env);
    await settleQueries(env);
    expect(document.querySelector('[data-node-id="sidebar"]')).toBeNull();
    act(() => {
      startEditing("mail");
    });
    await screen.findByRole("region", { name: /Customizing layout/ });
    expect(pane("Folders")).toHaveAttribute("data-node-id", "sidebar");
    await settleQueries(env);
  });

  it("offers Undo anyway when layout.kdl changed on disk", async () => {
    const env = createEnv();
    await customize(env);
    hideFolders();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();
    });
    act(() => {
      simulateExternalEdit(env.transport, "layout");
    });
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    const force = await screen.findByRole("button", { name: "Undo anyway" });
    expect(screen.getByRole("status")).toHaveTextContent("layout.kdl was changed outside Nail");
    fireEvent.click(force);
    await waitFor(() => {
      expect(pane("Folders")).not.toHaveAttribute("data-state");
    });
    expect(screen.queryByRole("button", { name: "Undo anyway" })).toBeNull();
    await settleQueries(env);
  });

  it("shows an edit refused by the core", async () => {
    const env = createEnv();
    await customize(env);
    // The history in the cache is stale now: the edit is a conflict.
    staleLayoutRevision(env);
    hideFolders();
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveAttribute("data-state", "error");
    });
    expect(screen.getByRole("status")).toHaveTextContent("changed outside Nail");
    expect(paneIds(env)).toEqual(["sidebar", "list-pane", "reader"]);
    await settleQueries(env);
  });

  it("ends with Done and gives the focus back", async () => {
    const env = createEnv();
    renderMail(env, <button type="button">Before</button>);
    await settleQueries(env);
    const before = screen.getByRole("button", { name: "Before" });
    before.focus();
    act(() => {
      startEditing("mail");
    });
    await screen.findByRole("region", { name: /Customizing layout/ });
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => {
      expect(screen.queryByRole("region", { name: /Customizing layout/ })).toBeNull();
    });
    expect(registeredWindowChrome("mail")).toBeNull();
    expect(before).toHaveFocus();
    // Hidden panes are skipped again outside customize mode.
    await settleQueries(env);
  });
});

describe("layout.customize and the settings card", () => {
  it("customizes the shown view's window and toggles off again", async () => {
    const env = createEnv();
    renderMail(env);
    await settleQueries(env);
    const layout = env.transport.data.layout;
    const action = must(actions["layout.customize"]);
    const ctx = { ...actionContext(env), layout };
    expect(action.enabled?.(ctx, [])).toBe(true);
    await act(async () => {
      await action.run(ctx, []);
    });
    expect(editorStore.getState().window).toBe("mail");
    expect(env.store.getState().activeView).toBe("mail");
    await screen.findByRole("region", { name: /Customizing layout/ });
    await endedBy(env, () => action.run(ctx, []));
  });

  // Review finding (ADR 0138): a session of a view the user left must not
  // swallow the next "Customize layout".
  it("customizes the shown view even while another view's session runs", async () => {
    const env = createEnv();
    renderMail(env);
    await settleQueries(env);
    const action = must(actions["layout.customize"]);
    const ctx = actionContext(env);
    act(() => {
      env.store.getState().setActiveView("mail");
      startEditing("mail");
    });
    act(() => {
      env.store.getState().setActiveView("calendar");
    });
    await act(async () => {
      await action.run(ctx, []);
    });
    expect(editorStore.getState().window).toBe("calendar");
    expect(env.store.getState().activeView).toBe("calendar");
    // Again on the same view: Done.
    await endedBy(env, () => action.run(ctx, []));
  });

  it("ends customize mode when the user switches to another view", async () => {
    const env = createEnv({ state: { activeView: "mail" } });
    render(
      <Live env={env}>
        {(layout) => (
          <>
            <CustomizeViewWatcher />
            <WindowView layout={layout} name="mail" />
          </>
        )}
      </Live>,
    );
    await settleQueries(env);
    act(() => {
      startEditing("mail");
    });
    await screen.findByRole("region", { name: "Customizing layout: Mail" });
    act(() => {
      env.store.getState().setActiveView("calendar");
    });
    await waitFor(() => {
      expect(editorStore.getState().window).toBeNull();
    });
    expect(screen.queryByRole("region", { name: /Customizing layout/ })).toBeNull();
    await settleQueries(env);
  });

  it("starts customizing the chosen view and applies its presets", async () => {
    const env = createEnv({ state: { activeView: "settings" } });
    const Card = must(widgets["layout-editor-settings"]);
    render(
      <Live env={env}>
        {(layout) => (
          <>
            <Card node={widget("layout-editor-settings")} nodeKey="card" />
            <WindowView layout={layout} name="mail" />
          </>
        )}
      </Live>,
    );
    await settleQueries(env);
    const card = screen.getByRole("region", { name: "Window layout" });
    const select = within(card).getByRole("combobox", { name: "View" });
    expect(
      within(select)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Mail", "Calendar", "Contacts", "Tasks", "Settings"]);
    expect(await violations(card)).toEqual([]);
    fireEvent.click(within(card).getByRole("button", { name: /Wide reader/ }));
    await waitFor(() => {
      expect(within(card).getByRole("button", { name: /Wide reader/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    expect(within(card).getByRole("status")).toHaveTextContent("Wide reader");
    fireEvent.click(within(card).getByRole("button", { name: "Reset to default" }));
    await waitFor(() => {
      expect(within(card).getByRole("button", { name: /Three panes/ })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    // The calendar has no presets.
    fireEvent.change(select, { target: { value: "calendar" } });
    expect(within(card).queryByRole("group", { name: "Presets" })).toBeNull();
    fireEvent.change(select, { target: { value: "mail" } });
    fireEvent.click(within(card).getByRole("button", { name: "Customize layout" }));
    expect(env.store.getState().activeView).toBe("mail");
    await screen.findByRole("region", { name: "Customizing layout: Mail" });
    await settleQueries(env);
  });
});

function firePointer(type: string, pointerId: number, clientX: number, clientY: number): void {
  const event = new MouseEvent(type, { clientX, clientY, bubbles: true });
  Object.defineProperty(event, "pointerId", { value: pointerId });
  window.dispatchEvent(event);
}
