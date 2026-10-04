import { render } from "@testing-library/react";
import { useEffect, useState } from "react";
import { allowConsole, consoleProblems, expectConsole, resetConsole } from "./console";

/** Updates its state from a timer, i.e. outside `act`. */
function LateUpdate() {
  const [n, setN] = useState(0);
  useEffect(() => {
    const timer = setTimeout(() => {
      setN(1);
    }, 0);
    return () => {
      clearTimeout(timer);
    };
  }, []);
  return <output>{n}</output>;
}

describe("console output in tests", () => {
  it("reports unexpected console.error and console.warn lines", () => {
    console.error("boom %s", "x");
    console.warn('nail: missing message "a"');
    expect(consoleProblems(true)).toEqual([
      expect.stringMatching(/^unexpected console\.error: boom x/),
      expect.stringMatching(/^unexpected console\.warn: nail: missing message "a"/),
    ]);
  });

  it("accepts expected and allowed lines", () => {
    expectConsole(/missing message "b"/);
    allowConsole("never logged");
    console.warn('nail: missing message "b"');
    expect(consoleProblems(true)).toEqual([]);
  });

  it("reports an expectation that nothing matched", () => {
    expectConsole(/never logged/);
    expect(consoleProblems(true)).toEqual([
      "expected console output matching /never logged/, got none",
    ]);
    resetConsole();
  });

  it("records React's warning about updates outside act", async () => {
    render(<LateUpdate />);
    // Runs after the component's timer (same delay, scheduled later).
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(consoleProblems(true)).toEqual([
      expect.stringMatching(/An update to LateUpdate inside a test was not wrapped in act/),
    ]);
  });
});
