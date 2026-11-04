/**
 * The frame an HTML mail body renders in, sized to its content (P3
 * WP-card-height, ADR 0164; ADR 0212; `frameHeight.ts` has the design).
 * Both frames are sandboxed with scripts and an opaque origin
 * (`MAIL_FRAME_SANDBOX`) and run only the frame script, which posts the
 * height, the hovered or clicked link and the context menu; the reader
 * accepts them only from the frame's own window ({@link useFrameMessages}):
 *
 * - {@link LocalMailFrame}: `srcdoc` (set through the Trusted Types policy;
 *   `readerDocument` adds the script and names this window as its parent).
 * - {@link RemoteMailFrame}: the remote UI's sandbox origin
 *   (`render_url?parent=<app origin>`).
 *
 * Links (ADR 0206, `mailLinks.ts`) never navigate a frame; they reach
 * `onLink`. A context menu becomes a bubbling `FRAME_MENU_EVENT` on the
 * `<iframe>` element, which the menu host turns into the text menu.
 *
 * The measured height reaches the frame as a geometry rule (`useGeometry`),
 * never an inline style; `data-state~="measured"` lets the skin drop the
 * placeholder height the frame has until then. Heights pass a
 * {@link FrameHeightGuard} (no oscillation, no runaway growth); a document
 * taller than `MAX_FRAME_HEIGHT` scrolls inside its frame.
 */
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  FRAME_MENU_EVENT,
  FrameHeightGuard,
  frameHeightOf,
  frameMenuOf,
  MAIL_FRAME_SANDBOX,
  OPAQUE_ORIGIN,
  RENDER_PARENT_PARAM,
} from "../theme/frameHeight";
import { useGeometry } from "../theme/ThemeProvider";
import { setSrcdoc } from "../theme/trustedHtml";
import { frameLinkOf, type MailLinkReport } from "./mailLinks";

/** What both frames share. */
interface FrameProps {
  /** The frame's accessible name. */
  title: string;
  /** `designed` for mail with its own design (the skin widens the column). */
  designed: boolean;
  /** A link was hovered, focused, left or clicked (ADR 0206). */
  onLink: (report: MailLinkReport) => void;
}

/** The latest `onLink`, for listeners that outlive a render. */
function useLinkReporter(onLink: (report: MailLinkReport) => void) {
  const latest = useRef(onLink);
  useLayoutEffect(() => {
    latest.current = onLink;
  }, [onLink]);
  return useMemo(
    () => (report: MailLinkReport) => {
      latest.current(report);
    },
    [],
  );
}

/**
 * A frame's measured height: state plus the guard, reset when the frame's
 * width changes (the content reflows, so a smaller height is legitimate).
 */
function useMeasuredHeight(frame: React.RefObject<HTMLIFrameElement | null>) {
  const [height, setHeight] = useState<number | null>(null);
  const guard = useMemo(() => new FrameHeightGuard(), []);
  const width = useRef(-1);
  const apply = useMemo(
    () => (measured: number) => {
      const el = frame.current;
      if (el !== null && el.clientWidth !== width.current) {
        width.current = el.clientWidth;
        guard.reset();
      }
      const next = guard.accept(measured);
      if (next !== null) setHeight(next);
    },
    [frame, guard],
  );
  return { height, guard, apply };
}

/** The geometry rule and `data-state` of a measured frame. */
function useFrameGeometry(height: number | null, designed: boolean) {
  const geom = `mail-frame-${useId()}`;
  useGeometry(geom, height === null ? {} : { height: `${String(height)}px` });
  const states = [designed ? "designed" : null, height === null ? null : "measured"].filter(
    (s) => s !== null,
  );
  return { geom, state: states.length > 0 ? states.join(" ") : undefined };
}

/**
 * Listens to what `frame`'s document posts from `origin` (the sandbox
 * origin, or the opaque origin of a `srcdoc` frame): heights go to
 * `apply`, links to `report`, a context menu becomes a
 * {@link FRAME_MENU_EVENT} on the `<iframe>`.
 */
function useFrameMessages(
  frame: React.RefObject<HTMLIFrameElement | null>,
  origin: string,
  apply: (height: number) => void,
  report: (link: MailLinkReport) => void,
) {
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const el = frame.current;
      const source = el?.contentWindow ?? null;
      const posted = frameHeightOf(event, source, origin);
      if (posted !== null) apply(posted);
      const link = frameLinkOf(event, source, origin);
      if (link !== null) report(link);
      const menu = frameMenuOf(event, source, origin);
      if (menu !== null && el) {
        el.dispatchEvent(new CustomEvent(FRAME_MENU_EVENT, { bubbles: true, detail: menu }));
      }
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [frame, origin, apply, report]);
}

/** A local (`srcdoc`) mail frame; its document's script posts the height and links. */
export function LocalMailFrame({
  srcDoc,
  title,
  designed,
  onLink,
}: FrameProps & { srcDoc: string }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const { height, guard, apply } = useMeasuredHeight(frame);
  const report = useLinkReporter(onLink);
  useLayoutEffect(() => {
    // A new document (the first, a theme switch, remote content allowed) starts over.
    guard.reset();
    if (frame.current) setSrcdoc(frame.current, srcDoc);
  }, [srcDoc, guard]);
  useFrameMessages(frame, OPAQUE_ORIGIN, apply, report);
  const { geom, state } = useFrameGeometry(height, designed);
  return (
    <iframe
      ref={frame}
      data-part="message-view.body"
      data-state={state}
      data-geom={geom}
      sandbox={MAIL_FRAME_SANDBOX}
      referrerPolicy="no-referrer"
      title={title}
    />
  );
}

/** The address of the message document on the remote UI's sandbox origin. */
export function renderSrc(renderUrl: string, css: string, parent: string): string {
  const url = new URL(renderUrl);
  url.searchParams.set("css", css);
  url.searchParams.set(RENDER_PARENT_PARAM, parent);
  return url.toString();
}

/** A remote UI mail frame on the sandbox origin; its document's script posts the height and links. */
export function RemoteMailFrame({
  renderUrl,
  css,
  title,
  designed,
  onLink,
}: FrameProps & { renderUrl: string; css: string }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const { height, guard, apply } = useMeasuredHeight(frame);
  const report = useLinkReporter(onLink);
  const src = renderSrc(renderUrl, css, window.location.origin);
  const sandboxOrigin = new URL(renderUrl).origin;
  useFrameMessages(frame, sandboxOrigin, apply, report);
  // A new document starts over.
  useEffect(() => {
    guard.reset();
  }, [src, guard]);
  const { geom, state } = useFrameGeometry(height, designed);
  return (
    <iframe
      ref={frame}
      data-part="message-view.body"
      data-state={state}
      data-geom={geom}
      sandbox={MAIL_FRAME_SANDBOX}
      src={src}
      referrerPolicy="no-referrer"
      title={title}
    />
  );
}
