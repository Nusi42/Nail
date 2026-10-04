import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { defaultTheme } from "../api/mockData";
import { ThemeProvider } from "../theme/ThemeProvider";
import { colorSlot } from "../theme/skin";
import { Avatar, Segmented, Switch, Tag, initialsOf } from "./primitives";
import { must } from "../test/must";

function withTheme(ui: React.ReactElement) {
  return render(<ThemeProvider theme={defaultTheme()}>{ui}</ThemeProvider>);
}

const DEMO = "Demo";
const CHOICE = "Choice";

describe("Switch", () => {
  function Harness() {
    const [on, setOn] = useState(false);
    return <Switch name="demo" checked={on} onChange={setOn} label={DEMO} hint="A hint" />;
  }

  it("is a labelled switch that toggles", () => {
    withTheme(<Harness />);
    const sw = screen.getByRole("switch", { name: "Demo" });
    expect(sw).toHaveAttribute("data-part", "switch");
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).toHaveAccessibleDescription("A hint");
    fireEvent.click(sw);
    expect(sw).toHaveAttribute("aria-checked", "true");
    expect(sw.closest('[data-part="switch-field"]')).toHaveAttribute("data-field", "demo");
  });
});

describe("Segmented", () => {
  function Harness() {
    const [value, setValue] = useState("a");
    return (
      <Segmented
        label={CHOICE}
        value={value}
        onChange={setValue}
        options={[
          { value: "a", label: "A" },
          { value: "b", label: "B" },
          { value: "c", label: "C" },
        ]}
      />
    );
  }

  it("is a radio group with one tab stop and arrow-key selection", () => {
    withTheme(<Harness />);
    expect(screen.getByRole("radiogroup", { name: "Choice" })).toHaveAttribute(
      "data-part",
      "segmented",
    );
    const [a, b, c] = screen.getAllByRole("radio");
    expect(a).toHaveAttribute("aria-checked", "true");
    expect(a).toHaveAttribute("tabindex", "0");
    expect(b).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(must(a), { key: "ArrowRight" });
    expect(b).toHaveAttribute("aria-checked", "true");
    expect(b).toHaveFocus();
    fireEvent.keyDown(must(b), { key: "ArrowLeft" });
    fireEvent.keyDown(must(a), { key: "ArrowLeft" });
    expect(c).toHaveAttribute("aria-checked", "true");
    fireEvent.click(must(a));
    expect(a).toHaveAttribute("aria-checked", "true");
  });
});

describe("Tag and Avatar", () => {
  it("renders a tag with dot, icon and text parts", () => {
    const { container } = withTheme(
      <Tag kind="account" dot={3} icon="paperclip" tone="warning" text="Work" />,
    );
    const tag = container.querySelector('[data-part="tag"]');
    expect(tag).toHaveAttribute("data-kind", "account");
    expect(tag).toHaveAttribute("data-tone", "warning");
    expect(container.querySelector('[data-part="tag.dot"]')).toHaveAttribute("data-color", "3");
    expect(container.querySelector('[data-part="tag.icon"]')).toHaveAttribute(
      "data-icon",
      "paperclip",
    );
    expect(container.querySelector('[data-part="tag.text"]')).toHaveTextContent("Work");
  });

  it("derives initials from names and addresses", () => {
    expect(initialsOf("Ada Lovelace")).toBe("AL");
    expect(initialsOf("ada")).toBe("A");
    expect(initialsOf("Grace B. Hopper")).toBe("GH");
    expect(initialsOf(null, "charles.babbage@example.org")).toBe("CB");
    expect(initialsOf("  ", "x@example.org")).toBe("X");
    expect(initialsOf("Émile Zola")).toBe("ÉZ");
    expect(initialsOf(null, "")).toBe("");
  });

  it("gives an address a stable colour", () => {
    const { container } = withTheme(<Avatar name="Ada" email="Ada@Example.org" />);
    const avatar = container.querySelector('[data-part="avatar"]');
    expect(avatar).toHaveAttribute("aria-hidden", "true");
    expect(avatar).toHaveAttribute("data-color", String(colorSlot("ada@example.org")));
    expect(avatar).toHaveTextContent("A");
  });
});
