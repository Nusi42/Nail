/**
 * The small shared widgets Q2 extracted from the features (SSOT): the
 * common buttons, the empty / loading dialog states, the labelled field,
 * the save row, server options and the roving focus of button groups.
 */
import { fireEvent, screen } from "@testing-library/react";
import type { KeyboardEvent } from "react";
import { describe, expect, it, vi } from "vitest";
import type { IncomingServer } from "../api/generated";
import { createEnv, renderWithProviders } from "../test/harness";
import { must } from "../test/must";
import { CloseButton } from "./CloseButton";
import { EmptyDialog, LoadingDialog } from "./DialogStates";
import { LabelledField, SaveRow, withOptions } from "./forms";
import { RefreshButton } from "./RefreshButton";
import { RemoveButton } from "./RemoveButton";
import { HORIZONTAL, moveRovingFocus, VERTICAL } from "./rovingFocus";

const REMOVE_LABEL = "Remove reminder 2";
const HOST = "Host";
const HOST_HINT = "The server's name";

describe("common buttons", () => {
  it("close, refresh and remove with their shared labels", () => {
    const onClose = vi.fn();
    const refetch = vi.fn(() => Promise.resolve());
    const onRemove = vi.fn();
    renderWithProviders(
      <>
        <CloseButton onClose={onClose} action="done" />
        <RefreshButton refetch={refetch} />
        <RemoveButton label={REMOVE_LABEL} onRemove={onRemove} />
      </>,
      createEnv(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    const remove = screen.getByRole("button", { name: "Remove reminder 2" });
    expect(remove).toHaveTextContent("Remove");
    fireEvent.click(remove);
    expect(onClose).toHaveBeenCalledOnce();
    expect(refetch).toHaveBeenCalledOnce();
    expect(onRemove).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Close" })).toHaveAttribute("data-action", "done");
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveAttribute(
      "data-action",
      "refresh",
    );
    expect(remove).toHaveAttribute("data-action", "remove");
  });
});

describe("dialog states", () => {
  it("shows the empty state's hint with a Close button, and the loading state", () => {
    const close = vi.fn();
    const { container } = renderWithProviders(
      <>
        <EmptyDialog part="x.dialog" textPart="x.hint" hintId="common-loading" close={close} />
        <LoadingDialog part="y.dialog" textPart="y.hint" />
      </>,
      createEnv(),
    );
    expect(container.querySelector('[data-part="x.dialog"]')).toHaveAttribute(
      "data-state",
      "empty",
    );
    const loading = container.querySelector('[data-part="y.dialog"]');
    expect(loading).toHaveAttribute("data-state", "loading");
    expect(loading).toHaveAttribute("aria-busy", "true");
    expect(container.querySelectorAll('[data-part$=".hint"]')).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(close).toHaveBeenCalledOnce();
  });
});

describe("forms", () => {
  it("labels a control and shows its hint", () => {
    renderWithProviders(
      <LabelledField name="host" id="f-host" label={HOST} hint={HOST_HINT}>
        <input id="f-host" data-part="form.input" aria-describedby="f-host-hint" />
      </LabelledField>,
      createEnv(),
    );
    const input = screen.getByRole("textbox", { name: "Host" });
    expect(input).toHaveAccessibleDescription("The server's name");
    expect(input.closest('[data-part="form.field"]')).toHaveAttribute("data-field", "host");
  });

  it("leaves the hint out when there is none", () => {
    const { container } = renderWithProviders(
      <LabelledField name="host" id="f-host" label={HOST} hint={null}>
        <input id="f-host" />
      </LabelledField>,
      createEnv(),
    );
    expect(container.querySelector('[data-part="form.hint"]')).toBeNull();
  });

  it("saves from the primary Save button of its row", () => {
    const onSave = vi.fn();
    const { container } = renderWithProviders(
      <SaveRow part="x.actions" onSave={onSave} />,
      createEnv(),
    );
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toHaveAttribute("data-variant", "primary");
    expect(save.parentElement).toBe(container.querySelector('[data-part="x.actions"]'));
    fireEvent.click(save);
    expect(onSave).toHaveBeenCalledOnce();
  });

  it("patches a server's options and keeps the rest", () => {
    const server: IncomingServer = {
      protocol: "imap",
      server: { host: "mail.example.org", port: 993, tls: "implicit", auth: { method: "none" } },
      options: { poll_interval_secs: 60, compress: false, max_connections: null },
    };
    const patched = withOptions(server, { compress: true });
    expect(patched.options).toEqual({
      poll_interval_secs: 60,
      compress: true,
      max_connections: null,
    });
    expect(patched.server).toBe(server.server);
    expect(server.options.compress).toBe(false);
  });
});

describe("roving focus", () => {
  function group(count: number): HTMLElement {
    const root = document.createElement("div");
    for (let i = 0; i < count; i++) {
      const button = document.createElement("button");
      button.dataset.part = "item";
      button.textContent = String(i);
      root.append(button);
    }
    document.body.append(root);
    return root;
  }

  function press(
    root: HTMLElement,
    target: Element,
    key: string,
    mods: Partial<KeyboardEvent> = {},
  ) {
    const preventDefault = vi.fn();
    const event = {
      key,
      altKey: false,
      ctrlKey: false,
      metaKey: false,
      ...mods,
      currentTarget: root,
      target,
      preventDefault,
    } as unknown as KeyboardEvent<HTMLElement>;
    moveRovingFocus(event, '[data-part="item"]', HORIZONTAL);
    return preventDefault;
  }

  it("moves to the neighbour with wrapping, and to the ends with Home / End", () => {
    const root = group(3);
    const a = must(root.children[0]);
    const b = must(root.children[1]);
    const c = must(root.children[2]);
    press(root, a, "ArrowRight");
    expect(document.activeElement).toBe(b);
    press(root, a, "ArrowLeft");
    expect(document.activeElement).toBe(c);
    press(root, b, "End");
    expect(document.activeElement).toBe(c);
    press(root, c, "Home");
    expect(document.activeElement).toBe(a);
    root.remove();
  });

  it("leaves other and modified keys alone", () => {
    const root = group(2);
    const a = must(root.children[0]);
    expect(press(root, a, "ArrowDown")).not.toHaveBeenCalled();
    expect(press(root, a, "ArrowRight", { ctrlKey: true })).not.toHaveBeenCalled();
    expect(VERTICAL).toEqual({ previous: "ArrowUp", next: "ArrowDown" });
    root.remove();
  });
});
