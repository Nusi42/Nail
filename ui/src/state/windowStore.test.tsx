/**
 * The UI store's window model (M5c, ADR 0116): window ids with instances in
 * z-order, focus, per-instance composers, the instance context of a window
 * and the pop-out route.
 */
import { act, screen } from "@testing-library/react";
import type { DialogProps } from "../layout/dialogRegistry";
import { DialogRegistryContext } from "../layout/dialogRegistry";
import { WindowLayer } from "../layout/WindowLayer";
import { PopOutHost } from "../features/popout";
import { createEnv, renderWithProviders } from "../test/harness";
import { emptyComposer } from "./compose";
import { createUiStore } from "./uiStore";
import { useWindowInstance } from "./windows";

function Probe(props: DialogProps) {
  const instance = useWindowInstance();
  return (
    <p data-part="text">
      {props.name}|{instance?.kind}|{instance?.instance ?? "-"}
    </p>
  );
}

describe("window store (M5c)", () => {
  it("keeps window ids in z-order and focuses by raising", () => {
    const store = createUiStore();
    const s = () => store.getState();
    s().openWindow("composer:d1");
    s().openWindow("settings");
    s().openWindow("composer:d2");
    expect(s().windows).toEqual(["composer:d1", "settings", "composer:d2"]);
    s().focusWindow("composer:d1");
    expect(s().windows).toEqual(["settings", "composer:d2", "composer:d1"]);
    const before = s().windows;
    s().focusWindow("composer:d1");
    s().focusWindow("not-open");
    expect(s().windows).toBe(before);
    s().closeWindow("settings");
    expect(s().windows).toEqual(["composer:d2", "composer:d1"]);
  });

  it("keeps a composer per instance next to the single composer", () => {
    const store = createUiStore();
    const s = () => store.getState();
    s().setComposerOf("composer:d1", { ...emptyComposer(), subject: "One" });
    s().setComposerOf("composer:d2", { ...emptyComposer(), subject: "Two" });
    s().updateComposerOf("composer:d1", { subject: "One!" });
    s().updateComposerOf("composer:none", { subject: "ignored" });
    expect(s().composers["composer:d1"]?.subject).toBe("One!");
    expect(s().composers["composer:d2"]?.subject).toBe("Two");
    expect(Object.keys(s().composers)).toEqual(["composer:d1", "composer:d2"]);
    expect(s().composer).toBeNull();
    s().setComposerOf("composer:d1", null);
    const after = s().composers;
    s().setComposerOf("composer:d1", null);
    expect(s().composers).toBe(after);
    expect(Object.keys(after)).toEqual(["composer:d2"]);
  });

  it("gives every window its instance and title by kind", async () => {
    const env = createEnv();
    const layout = env.transport.data.layout;
    const { container } = renderWithProviders(
      <DialogRegistryContext.Provider value={{ "about-nail": Probe }}>
        <WindowLayer layout={layout} />
      </DialogRegistryContext.Provider>,
      env,
    );
    act(() => {
      env.store.getState().openWindow("about-nail:w1");
      env.store.getState().openWindow("about-nail");
    });
    expect(await screen.findByText("about-nail:w1|about-nail|w1")).toBeInTheDocument();
    expect(screen.getByText("about-nail|about-nail|-")).toBeInTheDocument();
    // Both are open; the lower one is hidden from assistive technology
    // while the modal frame of the contract stage covers it.
    const frames = [...container.querySelectorAll('[data-part="window"]')];
    expect(frames.map((d) => d.getAttribute("data-window"))).toEqual([
      "about-nail:w1",
      "about-nail",
    ]);
  });

  it("renders one window filling a pop-out", async () => {
    const env = createEnv();
    const { container } = renderWithProviders(
      <DialogRegistryContext.Provider value={{ "about-nail": Probe }}>
        <PopOutHost layout={env.transport.data.layout} window="about-nail:w2" />
      </DialogRegistryContext.Provider>,
      env,
    );
    expect(await screen.findByText("about-nail:w2|about-nail|w2")).toBeInTheDocument();
    expect(container.querySelector('[data-part="pop-out"]')).toHaveAttribute(
      "data-window",
      "about-nail:w2",
    );
  });
});
