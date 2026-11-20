import { render, screen } from "@testing-library/react";
import { defaultTheme } from "../api/mockData";
import { ThemeProvider } from "../theme/ThemeProvider";
import { SKIN_CSS } from "../theme/skin";
import { p } from "../theme/selectors";
import { ProblemIndicator } from "./ProblemIndicator";
import { must } from "../test/must";

function withTheme(ui: React.ReactElement) {
  return render(<ThemeProvider theme={defaultTheme()}>{ui}</ThemeProvider>);
}

const DETAILS = "Name is missing.";
const ONE_ERROR = "1 error";
const TWO_MISSING = "2 missing";

describe("ProblemIndicator", () => {
  it("is a button whose popover details describe it (toggle mode)", () => {
    const { container } = withTheme(
      <ProblemIndicator severity="error" label={ONE_ERROR} title={DETAILS}>
        {DETAILS}
      </ProblemIndicator>,
    );
    const root = must(container.querySelector('[data-part="problem"]'));
    expect(root).toHaveAttribute("data-mode", "toggle");
    expect(root).toHaveAttribute("data-severity", "error");
    expect(root).toHaveAttribute("data-kind", "error");
    expect(root).toHaveAttribute("role", "alert");
    const button = screen.getByRole("button", { name: ONE_ERROR });
    expect(button).toHaveAttribute("data-part", "problem.badge");
    expect(button).toHaveAttribute("title", DETAILS);
    expect(button).toHaveAccessibleDescription(DETAILS);
    const details = must(document.getElementById(must(button.getAttribute("aria-describedby"))));
    expect(details).toHaveAttribute("data-part", "problem.details");
    expect(details).toHaveAttribute("popover", "auto");
    expect(button.getAttribute("popovertarget")).toBe(details.id);
    expect(root.querySelector('[data-part="problem.icon"]')).not.toBeNull();
  });

  it("announces only errors, not warnings", () => {
    const { container } = withTheme(
      <ProblemIndicator severity="warning" label={TWO_MISSING}>
        {DETAILS}
      </ProblemIndicator>,
    );
    const root = must(container.querySelector('[data-part="problem"]'));
    expect(root).toHaveAttribute("data-severity", "warning");
    expect(root).not.toHaveAttribute("role");
  });

  it("shows its details while the host says so (follow mode)", () => {
    const { container, rerender } = withTheme(
      <ProblemIndicator severity="error" kind="unknown-field" open={false} detailsId="tip">
        {DETAILS}
      </ProblemIndicator>,
    );
    const root = must(container.querySelector('[data-part="problem"]'));
    expect(root).toHaveAttribute("data-mode", "follow");
    expect(root).toHaveAttribute("data-kind", "unknown-field");
    // The icon is decorative: the host's field names the details.
    expect(screen.queryByRole("button")).toBeNull();
    expect(root.querySelector('[data-part="problem.badge"]')).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(root.querySelector('[data-part="problem.label"]')).toBeNull();
    const details = must(document.getElementById("tip"));
    expect(details).toHaveAttribute("role", "tooltip");
    expect(details).not.toBeVisible();
    rerender(
      <ThemeProvider theme={defaultTheme()}>
        <ProblemIndicator severity="error" kind="unknown-field" open detailsId="tip">
          {DETAILS}
        </ProblemIndicator>
      </ThemeProvider>,
    );
    expect(details).toBeVisible();
    expect(details).toHaveTextContent(DETAILS);
  });

  it("has one look in the shared skin, by part only", () => {
    for (const part of ["problem", "problem.badge", "problem.icon", "problem.details"]) {
      expect(SKIN_CSS).toContain(p(part));
    }
    expect(SKIN_CSS).toContain(`${p("problem.details")}[data-mode="follow"]`);
  });
});
