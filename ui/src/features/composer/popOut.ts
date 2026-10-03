/**
 * Composers in desktop pop-out windows (M5c, ADRs 0118 / 0119): before a
 * composer pops out its draft is saved (every webview has its own UI
 * store; the draft is how the message travels) and `{ "draft": <id> }` is
 * handed to the pop-out; the pop-out page opens that draft in the window id
 * of its `#/window/<id>` route.
 */
import { useEffect, useRef } from "react";
import type { JsonValue } from "../../api/generated";
import type { ComposeContext } from "../../state/compose";
import type { UiStore } from "../../state/uiStore";
import { usePopOutContext, usePopOutTitle } from "../popout/hooks";
import { registerPopOutPreparer } from "../popout/model";
import { openRichDraft, saveRich } from "./flows";
import { COMPOSER_KIND, isComposerInstance } from "./store";

/** The draft id in a pop-out context (`{ "draft": "<id>" }`), else `null`. */
export function draftOfPopOutContext(context: JsonValue | null): string | null {
  if (context === null || typeof context !== "object" || Array.isArray(context)) return null;
  const draft = (context as { [key: string]: JsonValue | undefined }).draft;
  return typeof draft === "string" && draft !== "" ? draft : null;
}

/** Why a composer stays in the page (the pop-out button shows the reason). */
export class ComposerPopOutError extends Error {
  readonly localized: { id: string };

  constructor(id: "composer-pop-out-plain" | "composer-pop-out-not-saved") {
    super(id);
    this.localized = { id };
  }
}

/**
 * Saves the composer of `window` and returns its pop-out context; throws
 * (the window stays in the page) when the draft cannot be saved.
 */
export async function prepareComposerPopOut(
  ctx: ComposeContext,
  window: string,
): Promise<JsonValue> {
  if (!isComposerInstance(window)) throw new ComposerPopOutError("composer-pop-out-plain");
  const draft = await saveRich(ctx, { window, quiet: true });
  if (draft === null) throw new ComposerPopOutError("composer-pop-out-not-saved");
  draftsOf(ctx.store).set(window, draft);
  return { draft };
}

/**
 * The drafts of the composers this page sent to a pop-out, by window id:
 * when one comes back ("Return to main window") its window opens here
 * again without a composer, and loads the draft from this list.
 */
const popped = new WeakMap<UiStore, Map<string, string>>();
function draftsOf(store: UiStore): Map<string, string> {
  let map = popped.get(store);
  if (map === undefined) {
    map = new Map();
    popped.set(store, map);
  }
  return map;
}

/** Registers the composer's pop-out preparer while the app runs. */
export function useComposerPopOutPreparer(ctx: ComposeContext): void {
  const ref = useRef(ctx);
  useEffect(() => {
    ref.current = ctx;
  });
  useEffect(
    () =>
      registerPopOutPreparer(COMPOSER_KIND, (window) => prepareComposerPopOut(ref.current, window)),
    [],
  );
}

/**
 * In a pop-out page: opens the draft the composer was popped out with
 * (once, while the window has no composer yet), and names the OS window
 * after the subject.
 */
export function useComposerPopOut(
  ctx: ComposeContext,
  window: string,
  hasComposer: boolean,
  subject: string | null,
): void {
  const context = draftOfPopOutContext(usePopOutContext());
  const opened = useRef(false);
  useEffect(() => {
    if (hasComposer || opened.current) return;
    // A pop-out page: the draft it was opened with; the main page: the
    // draft of a composer that came back from its pop-out.
    const returned = draftsOf(ctx.store).get(window) ?? null;
    const draft = context ?? returned;
    if (draft === null) return;
    opened.current = true;
    draftsOf(ctx.store).delete(window);
    void openRichDraft(ctx, draft, { window }).catch(() => {
      opened.current = false;
    });
  }, [ctx, window, hasComposer, context]);
  usePopOutTitle(subject);
}
