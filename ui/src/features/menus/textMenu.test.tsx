import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { createEnv, renderWithProviders, stubElementSizes, widget } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { must } from "../../test/must";
import { ContextMenuHost } from "./ContextMenuHost";
import { EDIT_SHORTCUTS } from "./editing";

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

describe("the text menu (ADR 0184)", () => {
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
