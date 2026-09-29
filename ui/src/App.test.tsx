import { render } from "@testing-library/react";
import { App } from "./App.tsx";

describe("App", () => {
  it("renders the themeable root part", () => {
    const { container } = render(<App />);
    expect(container.querySelector('[data-part="app"]')).toBeInTheDocument();
  });
});
