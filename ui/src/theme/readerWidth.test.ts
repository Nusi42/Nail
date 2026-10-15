import { THREAD_VIEW_SKIN_CSS } from "../widgets/threadViewSkin";
import { p } from "./selectors";
import { SKIN_CSS } from "./skin";

/** The declarations of the rules in `css` whose selector list is exactly `selector`. */
const bodiesOf = (css: string, selector: string): string[] =>
  css
    .split("}")
    .map((block) => block.split("{"))
    .filter(([selectors = ""]) => selectors.trim() === selector)
    .map(([, body = ""]) => body);

describe("the reader's width follows the window", () => {
  it("lets a column with an HTML body fill the reader", () => {
    const column = `${p("message-view.column")}:has(> ${p("message-view.body")})`;
    expect(bodiesOf(SKIN_CSS, column).join(" ")).toMatch(/max-width: none/);
  });

  it("does not cap the card feed at the reading width", () => {
    const feed = `${p("thread-view")}[data-mode="cards"] > ${p("thread-view.header")}, ${p("thread-view.feed")}`;
    const bodies = bodiesOf(THREAD_VIEW_SKIN_CSS, feed);
    expect(bodies).toHaveLength(1);
    expect(bodies.join(" ")).not.toMatch(/max-width/);
  });
});
