import { render, screen, waitFor } from "@testing-library/react";
import { I18nProvider } from "../i18n/I18nProvider";
import { BootTheme, bootTheme } from "./BootTheme";

describe("boot theme", () => {
  it("styles the pre-configuration screens with the bundled default theme", async () => {
    render(
      <I18nProvider locale="en">
        <BootTheme modeOverride="dark">
          <p>login</p>
        </BootTheme>
      </I18nProvider>,
    );
    // Styled at once: no loading step.
    expect(screen.getByText("login")).toBeInTheDocument();
    expect(bootTheme().name).toBe("default");
    // The compiled theme is in the document, and the mode is applied.
    await waitFor(() => {
      expect(document.documentElement.dataset.nailMode).toBe("dark");
    });
    const css = [...document.querySelectorAll("style")].map((s) => s.textContent).join("\n");
    expect(css).toContain("--nail-color-bg");
    expect(css).toContain('[data-part="remote-login"]');
  });
});
