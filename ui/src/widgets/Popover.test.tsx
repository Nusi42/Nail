import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { defaultTheme } from "../api/mockData";
import { ThemeProvider } from "../theme/ThemeProvider";
import { geometry } from "../test/env";
import { Popover, placePopover, type AnchorRect } from "./Popover";

const VIEWPORT = { width: 1000, height: 800 };
const LABEL = "Quick look";
const SIZE = { width: 300, height: 200 };
const at = (left: number, top: number, width = 100, height = 40): AnchorRect => ({
  left,
  top,
  width,
  height,
});

describe("placePopover", () => {
  it("goes below the anchor, or above it when there is no room", () => {
    expect(placePopover(at(100, 100), SIZE, VIEWPORT, "below")).toEqual({ x: 100, y: 144 });
    expect(placePopover(at(100, 700), SIZE, VIEWPORT, "below")).toEqual({ x: 100, y: 496 });
    // Pulled back inside on the right, never past the top-left corner.
    expect(placePopover(at(900, 100), SIZE, VIEWPORT, "below")).toEqual({ x: 700, y: 144 });
    expect(placePopover(at(0, 100), SIZE, { width: 200, height: 250 }, "below")).toEqual({
      x: 0,
      y: 0,
    });
  });

  it("goes beside the anchor, or to its left when there is no room", () => {
    expect(placePopover(at(100, 100), SIZE, VIEWPORT, "beside")).toEqual({ x: 204, y: 100 });
    expect(placePopover(at(650, 100), SIZE, VIEWPORT, "beside")).toEqual({ x: 346, y: 100 });
    expect(placePopover(at(100, 700), SIZE, VIEWPORT, "beside")).toEqual({ x: 204, y: 600 });
  });
});

function Harness({ onClosed }: { onClosed: () => void }) {
  const [open, setOpen] = useState(true);
  return (
    <ThemeProvider theme={defaultTheme()}>
      <button type="button">Opener</button>
      <p>Outside</p>
      {open && (
        <Popover
          anchor={at(10, 20)}
          placement="below"
          label={LABEL}
          onClose={() => {
            setOpen(false);
            onClosed();
          }}
        >
          <button type="button">First</button>
          <button type="button" data-autofocus="">
            Focus me
          </button>
        </Popover>
      )}
    </ThemeProvider>
  );
}

describe("Popover", () => {
  it("is a named dialog in the body, positioned by a geometry rule, focusing data-autofocus", () => {
    let closed = 0;
    render(<Harness onClosed={() => (closed += 1)} />);
    const dialog = screen.getByRole("dialog", { name: "Quick look" });
    expect(dialog.parentElement).toBe(document.body);
    expect(dialog).toHaveAttribute("data-part", "popover");
    expect(dialog).toHaveAttribute("data-placement", "below");
    expect(dialog).not.toHaveAttribute("style");
    expect(geometry(dialog)).toMatchObject({ left: "10px", top: "64px" });
    expect(screen.getByRole("button", { name: "Focus me" })).toHaveFocus();
    expect(closed).toBe(0);
  });

  it("closes on Escape inside it and gives the focus back", async () => {
    const user = userEvent.setup();
    let closed = 0;
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    render(<Harness onClosed={() => (closed += 1)} />);
    await user.keyboard("{Escape}");
    expect(closed).toBe(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(opener).toHaveFocus();
    opener.remove();
  });

  it("closes on a press outside, not inside", async () => {
    const user = userEvent.setup();
    let closed = 0;
    render(<Harness onClosed={() => (closed += 1)} />);
    await user.click(screen.getByRole("button", { name: "First" }));
    expect(closed).toBe(0);
    await user.click(screen.getByText("Outside"));
    expect(closed).toBe(1);
  });
});
