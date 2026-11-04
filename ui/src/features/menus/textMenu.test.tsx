import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { createEnv, renderWithProviders, stubElementSizes, widget } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { must } from "../../test/must";
import { ContextMenuHost } from "./ContextMenuHost";
import { EDIT_SHORTCUTS } from "./editing";
import { FRAME_SELECT_ALL_MESSAGE } from "../../api/generated";
import { FRAME_MENU_EVENT, type FrameMenu } from "../../theme/frameHeight";

const NOTES = "notes";
let restoreSizes: () => void;
let commands: string[] = [];
beforeEach(() => {
  restoreSizes = stubElementSizes();
  commands = [];
  // jsdom has no editing commands: record what the menu asks for.
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    value: (command: string) => {
      commands.push(command);
      return true;
    },
  });
});
afterEach(() => {
  restoreSizes();
  document.getSelection()?.removeAllRanges();
  Reflect.deleteProperty(document, "execCommand");
});

function render() {
  const env = createEnv({
    state: { activeAccountId: "acc-personal", activeFolderId: "acc-personal/Inbox" },
  });
  const view = renderWithProviders(
    <>
      <NodeView node={widget("folder-tree")} path="t/0" />
      <header data-part="message-view.header">
        <span data-part="message-view.header.label">From</span>
        <span data-part="message-view.header.value">ada@example.org</span>
      </header>
      <article data-part="thread-view.card" data-message="m1">
        <pre data-part="message-view.text">Hello from a card</pre>
      </article>
      <textarea aria-label={NOTES} defaultValue="hello world" />
      <iframe data-part="message-view.body" data-message="m2" title={NOTES} />
      <ContextMenuHost />
    </>,
    env,
  );
  return { env, ...view };
}

function rightClick(target: Element): MouseEvent {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    button: 2,
    clientX: 4,
    clientY: 4,
  });
  act(() => {
    target.dispatchEvent(event);
  });
  return event;
}

const items = (menu: HTMLElement) =>
  within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.getAttribute("data-action"));

/** What a mail frame's `<iframe>` dispatches when its document asks for a menu (ADR 0212). */
function frameMenu(frame: HTMLIFrameElement, detail: FrameMenu): void {
  act(() => {
    frame.dispatchEvent(new CustomEvent(FRAME_MENU_EVENT, { bubbles: true, detail }));
  });
}

describe("the text menu (ADR 0184)", () => {
  it("opens for a mail frame with its text: Copy writes it, Select all asks the frame", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    try {
      const { container } = render();
      const frame = must(container.querySelector("iframe"));
      const view = must(frame.contentWindow);
      const postMessage = vi.spyOn(view, "postMessage").mockImplementation(() => undefined);
      frameMenu(frame, { x: 4, y: 6, button: 2, text: "from the mail" });
      let menu = await screen.findByRole("menu", { name: "Edit" });
      expect(items(menu)).toEqual(["edit.copy", "edit.select-all"]);
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Copy/ }));
      await waitFor(() => {
        expect(writeText).toHaveBeenCalledWith("from the mail");
      });
      // Never the page's own editing commands for a frame.
      expect(commands).toEqual([]);

      // Nothing selected: no Copy; Select all goes to the frame's script.
      frameMenu(frame, { x: 0, y: 0, button: 0, text: "" });
      menu = await screen.findByRole("menu", { name: "Edit" });
      expect(items(menu)).toEqual(["edit.select-all"]);
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Select all/ }));
      expect(postMessage).toHaveBeenCalledWith({ type: FRAME_SELECT_ALL_MESSAGE }, "*");
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("offers Copy and Select all on selected text and copies it", async () => {
    const { container } = render();
    const value = must(
      container.querySelector<HTMLElement>('[data-part="message-view.header.value"]'),
    );
    const label = must(
      container.querySelector<HTMLElement>('[data-part="message-view.header.label"]'),
    );
    // A label (chrome) has no menu: nothing opens.
    expect(rightClick(label).defaultPrevented).toBe(true);
    expect(screen.queryByRole("menu")).toBeNull();
    // Selected text: Copy, with the platform's key shown.
    const range = document.createRange();
    range.selectNodeContents(value);
    must(document.getSelection()).addRange(range);
    rightClick(value);
    const menu = await screen.findByRole("menu", { name: "Edit" });
    expect(items(menu)).toEqual(["edit.copy", "edit.select-all"]);
    expect(within(menu).getByRole("menuitem", { name: /Copy/ })).toHaveTextContent("Ctrl+C");
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Copy/ }));
    await waitFor(() => {
      expect(commands).toEqual(["copy"]);
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(EDIT_SHORTCUTS["edit.copy"]).toBe("ctrl+c");

    // Selected text in a reader card: the text menu with its keys too (not the card's menu).
    const text = must(container.querySelector<HTMLElement>('[data-part="message-view.text"]'));
    const inCard = document.createRange();
    inCard.selectNodeContents(text);
    must(document.getSelection()).removeAllRanges();
    must(document.getSelection()).addRange(inCard);
    rightClick(text);
    const cardMenu = await screen.findByRole("menu", { name: "Edit" });
    expect(within(cardMenu).getByRole("menuitem", { name: /Copy/ })).toHaveTextContent("Ctrl+C");
  });

  it("cuts, pastes and selects all in a field", async () => {
    const readText = vi.fn(() => Promise.resolve("pasted"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { readText },
    });
    try {
      render();
      const field = screen.getByRole("textbox", { name: NOTES });
      if (!(field instanceof HTMLTextAreaElement)) throw new Error("not a textarea");
      field.setSelectionRange(0, 5);
      rightClick(field);
      let menu = await screen.findByRole("menu", { name: "Edit" });
      expect(items(menu)).toEqual(["edit.cut", "edit.copy", "edit.paste", "edit.select-all"]);
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Cut/ }));
      await waitFor(() => {
        expect(commands).toEqual(["cut"]);
      });
      // The selection the menu opened on came back first.
      expect([field.selectionStart, field.selectionEnd]).toEqual([0, 5]);

      rightClick(field);
      menu = await screen.findByRole("menu", { name: "Edit" });
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Paste/ }));
      await waitFor(() => {
        expect(commands).toEqual(["cut", "insertText"]);
      });
      expect(readText).toHaveBeenCalledTimes(1);

      rightClick(field);
      menu = await screen.findByRole("menu", { name: "Edit" });
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Select all/ }));
      await waitFor(() => {
        expect([field.selectionStart, field.selectionEnd]).toEqual([0, field.value.length]);
      });
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("gives the folder pane around its folders its own menu", async () => {
    const { container } = render();
    await screen.findByRole("treeitem", { name: /Archive/ });
    const pane = must(container.querySelector<HTMLElement>('[data-part="folder-tree"]'));
    expect(rightClick(pane).defaultPrevented).toBe(true);
    const menu = await screen.findByRole("menu", { name: "Mail" });
    expect(items(menu)).toEqual(["sync.now", "mail.compose", "account.add"]);
  });
});
