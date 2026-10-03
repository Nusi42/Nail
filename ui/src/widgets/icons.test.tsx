import { render } from "@testing-library/react";
import iconsJson from "../../../crates/nail-api/fixtures/icons.json";
import type { ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { ThemeProvider } from "../theme/ThemeProvider";
import { Icon, lucideName } from "./Icon";
import { LEGACY_ICON_NAMES, LUCIDE_ICONS } from "./lucideIcons";

function renderIcon(name: string | null, theme: ResolvedTheme = { ...defaultTheme(), icons: {} }) {
  return render(
    <ThemeProvider theme={theme}>
      <Icon name={name} part="test.icon" />
    </ThemeProvider>,
  );
}

describe("Lucide registry (M5a, ADR 0102)", () => {
  it("bundles exactly nail_api::icons::BUNDLED_ICONS", () => {
    expect(Object.keys(LUCIDE_ICONS)).toEqual(iconsJson.bundled);
  });

  it("maps every legacy name like nail_api::icons::LEGACY_ICON_NAMES, onto a bundled icon", () => {
    expect(LEGACY_ICON_NAMES).toEqual(iconsJson.legacy);
    for (const target of Object.values(LEGACY_ICON_NAMES)) {
      expect(Object.hasOwn(LUCIDE_ICONS, target)).toBe(true);
    }
  });

  it("resolves Lucide and legacy names", () => {
    expect(lucideName("square-pen")).toBe("square-pen");
    expect(lucideName("compose")).toBe("square-pen");
    expect(lucideName("delete")).toBe("trash");
    expect(lucideName("not-an-icon")).toBeNull();
    // Inherited object keys are not icons.
    expect(lucideName("toString")).toBeNull();
  });
});

describe("Icon", () => {
  it("renders a Lucide icon with the part, the name and currentColor", () => {
    const { container } = renderIcon("reply-all");
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("data-part", "test.icon");
    expect(svg).toHaveAttribute("data-icon", "reply-all");
    expect(svg).toHaveAttribute("data-icon-set", "lucide");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("stroke", "currentColor");
    expect(svg?.querySelectorAll("path").length).toBeGreaterThan(0);
    // Styling goes by data-part only: no class names in the DOM.
    expect(svg).not.toHaveAttribute("class");
  });

  it("renders the Lucide icon of a pre-M5 name", () => {
    const { container } = renderIcon("compose");
    expect(container.querySelector("svg")).toHaveAttribute("data-icon", "square-pen");
  });

  it("prefers the theme's own icon of that name", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      icons: { reply: { view_box: "0 0 10 10", path: "M0 0h10v10z" } },
    };
    const { container } = renderIcon("reply", theme);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("viewBox", "0 0 10 10");
    expect(svg).not.toHaveAttribute("data-icon-set");
    expect(container.querySelector("path")).toHaveAttribute("d", "M0 0h10v10z");
  });

  it("renders nothing for unknown or missing names", () => {
    expect(renderIcon("not-an-icon").container.querySelector("svg")).toBeNull();
    expect(renderIcon(null).container.querySelector("svg")).toBeNull();
  });
});
