/**
 * P2 review: from the list, Enter / o move the focus into the reader (the
 * card feed was out of reach for keyboard users: 12 Tab stops).
 */
import { focusReader } from "./actions";

afterEach(() => {
  document.body.innerHTML = "";
});

function feed(cards: { id: string; state?: string; current?: boolean }[]): void {
  document.body.innerHTML = `<section data-part="thread-view" data-mode="cards" data-thread="t1">
    <div role="feed" data-part="thread-view.feed">${cards
      .map(
        (c) => `<article data-part="thread-view.card" data-message="${c.id}"${
          c.state ? ` data-state="${c.state}"` : ""
        }${c.current ? ' aria-current="true"' : ""}>
          <div data-part="thread-view.card.header">
            <button type="button" data-part="thread-view.card.toggle" id="toggle-${c.id}">x</button>
          </div>
        </article>`,
      )
      .join("")}</div></section>`;
}

describe("conversation.open-in-reader", () => {
  it("focuses the selected card's header", () => {
    feed([
      { id: "m1" },
      { id: "m2", state: "expanded", current: true },
      { id: "m3", state: "expanded" },
    ]);
    expect(focusReader()).toBe(true);
    expect(document.activeElement?.id).toBe("toggle-m2");
  });

  it("falls back to the newest open card", () => {
    feed([{ id: "m1", state: "expanded" }, { id: "m2", state: "expanded unread" }, { id: "m3" }]);
    expect(focusReader()).toBe(true);
    expect(document.activeElement?.id).toBe("toggle-m2");
  });

  it("focuses a message shown alone, and reports when the reader is empty", () => {
    document.body.innerHTML =
      '<article data-part="message-view" data-message="m1" tabindex="-1" id="view"></article>';
    expect(focusReader()).toBe(true);
    expect(document.activeElement?.id).toBe("view");
    document.body.innerHTML = '<article data-part="message-view" tabindex="-1"></article>';
    expect(focusReader()).toBe(false);
  });
});
