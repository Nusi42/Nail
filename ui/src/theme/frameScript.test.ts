/**
 * The frame script every mail frame runs (ADR 0212,
 * `crates/nail-api/src/mail_frame.rs`): run against a jsdom document with a
 * recording `parent`, so its link, menu and select-all behaviour is tested
 * here, where the reader's side of the contract lives.
 */
import { afterEach, describe, expect, it } from "vitest";
import {
  FRAME_LINK_MESSAGE,
  FRAME_MENU_MESSAGE,
  FRAME_PARENT_ATTRIBUTE,
  FRAME_SCRIPT,
  FRAME_SELECT_ALL_MESSAGE,
} from "../api/generated";
import { must } from "../test/must";

const APP = "https://app.example";

const LINKS =
  '<p><a href="https://track.example.net/e/1/click"><span>I am fully satisfied.</span></a>' +
  '<a name="x">no target</a><a href="https://example.org/pic"><img alt="Picture"></a></p>';

type Listener = (event: { source: unknown; origin: string; data: unknown }) => void;

/** Runs the frame script in a frame document holding `body`, with `parent` recorded. */
function runScript(body: string, parentOrigin: string | null = APP) {
  const frame = document.createElement("iframe");
  document.body.append(frame);
  const doc = must(frame.contentDocument);
  const view = must(doc.defaultView);
  if (parentOrigin !== null) doc.documentElement.setAttribute(FRAME_PARENT_ATTRIBUTE, parentOrigin);
  doc.body.innerHTML = body;
  const posted: { message: Record<string, unknown>; target: string }[] = [];
  const parent = {
    postMessage: (message: Record<string, unknown>, target: string) => {
      posted.push({ message, target });
    },
  };
  const windowListeners: Record<string, Listener[]> = {};
  const addEventListener = (type: string, listener: Listener) => {
    (windowListeners[type] ??= []).push(listener);
  };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval -- runs the contract's script text against a test document; there is no other way to execute it in jsdom
  const script = new Function(
    "document",
    "parent",
    "addEventListener",
    "getSelection",
    "innerHeight",
    "ResizeObserver",
    FRAME_SCRIPT,
  ) as (...args: unknown[]) => void;
  script(doc, parent, addEventListener, () => doc.getSelection(), 0, undefined);
  const links = (): Record<string, unknown>[] =>
    posted.map((p) => p.message).filter((m) => m.type === FRAME_LINK_MESSAGE);
  return { doc, view, posted, links, parent, windowListeners };
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("the frame script", () => {
  it("reports hovers, focus and clicks of links and never lets them navigate", () => {
    const { doc, view, posted, links } = runScript(LINKS);
    const span = must(doc.querySelector("span"));
    const img = must(doc.querySelector("img"));

    span.dispatchEvent(new view.MouseEvent("mouseover", { bubbles: true }));
    expect(links().at(-1)).toEqual({
      type: FRAME_LINK_MESSAGE,
      open: false,
      href: "https://track.example.net/e/1/click",
      index: 0,
    });
    // Moving inside the same link reports nothing new.
    const count = links().length;
    must(doc.querySelector("a")).dispatchEvent(new view.MouseEvent("mouseover", { bubbles: true }));
    expect(links()).toHaveLength(count);

    const click = new view.MouseEvent("click", { bubbles: true, cancelable: true });
    span.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(links().at(-1)).toMatchObject({ open: true, index: 0 });

    // The image link is the second link: `<a>` without `href` is not counted.
    const middle = new view.MouseEvent("auxclick", { bubbles: true, cancelable: true, button: 1 });
    img.dispatchEvent(middle);
    expect(middle.defaultPrevented).toBe(true);
    expect(links().at(-1)).toMatchObject({ open: true, href: "https://example.org/pic", index: 1 });
    // A right button's auxclick opens nothing (and navigates nothing).
    const before = links().length;
    const right = new view.MouseEvent("auxclick", { bubbles: true, cancelable: true, button: 2 });
    img.dispatchEvent(right);
    expect(right.defaultPrevented).toBe(true);
    expect(links()).toHaveLength(before);

    // Keyboard focus shows the link too; leaving the document clears it.
    must(doc.querySelectorAll("a")[2]).dispatchEvent(
      new view.FocusEvent("focusin", { bubbles: true }),
    );
    expect(links().at(-1)).toMatchObject({ open: false, index: 1 });
    img.dispatchEvent(new view.FocusEvent("focusout", { bubbles: true, relatedTarget: null }));
    expect(links().at(-1)).toEqual({
      type: FRAME_LINK_MESSAGE,
      open: false,
      href: null,
      index: -1,
    });

    // Clicks outside links are left alone.
    const plain = new view.MouseEvent("click", { bubbles: true, cancelable: true });
    doc.body.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(false);
    // Everything went to the named parent only.
    expect(new Set(posted.map((p) => p.target))).toEqual(new Set([APP]));
  });

  it("asks for the text menu with the point, the button and the selection", () => {
    const { doc, view, posted } = runScript("<p>Hello <b>world</b></p>");
    const selection = must(doc.getSelection());
    selection.selectAllChildren(must(doc.querySelector("b")));
    const menu = new view.MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 12,
      clientY: 30,
      button: 2,
    });
    must(doc.querySelector("p")).dispatchEvent(menu);
    // Never the browser's menu (ADR 0184).
    expect(menu.defaultPrevented).toBe(true);
    expect(posted.at(-1)).toEqual({
      message: { type: FRAME_MENU_MESSAGE, x: 12, y: 30, button: 2, text: "world" },
      target: APP,
    });
  });

  it("selects all only when its parent asks", () => {
    const { doc, parent, windowListeners } = runScript("<p>Hello <b>world</b></p>");
    const [onMessage] = must(windowListeners.message);
    const selected = () => String(doc.getSelection());
    const ask = { type: FRAME_SELECT_ALL_MESSAGE };
    must(onMessage)({ source: {}, origin: APP, data: ask });
    must(onMessage)({ source: parent, origin: "https://evil.example", data: ask });
    must(onMessage)({ source: parent, origin: APP, data: { type: "other" } });
    expect(selected()).toBe("");
    must(onMessage)({ source: parent, origin: APP, data: ask });
    expect(selected()).toBe("Hello world");
  });

  it("does nothing without a parent to post to", () => {
    const { doc, view, posted, windowListeners } = runScript(LINKS, null);
    const click = new view.MouseEvent("click", { bubbles: true, cancelable: true });
    must(doc.querySelector("span")).dispatchEvent(click);
    expect(click.defaultPrevented).toBe(false);
    expect(posted).toEqual([]);
    expect(windowListeners).toEqual({});
  });
});
