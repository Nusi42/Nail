/**
 * The frame an HTML mail body renders in, sized to its content (P3
 * WP-card-height, ADR 0164; `frameHeight.ts` has the design):
 *
 * - {@link LocalMailFrame}: `srcdoc` (set through the Trusted Types policy)
 *   with `sandbox="allow-same-origin"` and no scripts; the reader reads the
 *   document's size itself — after `load`, after each image's `load` /
 *   `error`, on `<details>` toggles, when fonts arrive, when the frame
 *   resizes and whenever the document's root or body resizes (a width
 *   change reflows it). It only
 *   reads sizes and listens; it never writes into the document.
 * - {@link RemoteMailFrame}: the remote UI's sandbox origin
 *   (`render_url?parent=<app origin>`), `sandbox="allow-scripts"` and an
 *   opaque origin; the route's hash-pinned script posts the height, which
 *   the reader accepts only from this frame's window (`frameHeightOf`).
 *
 * Links (ADR 0206, `mailLinks.ts`): neither frame ever navigates. The
 * local frame's links are watched by the reader ({@link watchLinks}); the
 * remote frame's script posts the hovered and clicked link, accepted only
 * from that frame's window ({@link frameLinkOf}). Both report to `onLink`.
 *
 * The measured height reaches the frame as a geometry rule (`useGeometry`),
 * never an inline style; `data-state~="measured"` lets the skin drop the
 * placeholder height the frame has until then. Heights pass a
 * {@link FrameHeightGuard} (no oscillation, no runaway growth); a document
 * taller than `MAX_FRAME_HEIGHT` scrolls inside its frame.
 */
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  contentHeight,
  documentMetrics,
  FrameHeightGuard,
  frameHeightOf,
  REMOTE_SANDBOX,
  RENDER_PARENT_PARAM,
  SRCDOC_SANDBOX,
} from "../theme/frameHeight";
import { useGeometry } from "../theme/ThemeProvider";
import { setSrcdoc } from "../theme/trustedHtml";
import { frameLinkOf, watchLinks, type MailLinkReport } from "./mailLinks";

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

/** Events inside a local mail document after which its height may differ. */
const REMEASURE_EVENTS = ["load", "error", "toggle"] as const;

/** The about: URL of a `srcdoc` document (the initial `about:blank` is not measured). */
const SRCDOC_URL = "about:srcdoc";

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

/** A local (`srcdoc`) mail frame that measures its document. */
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
    if (frame.current) setSrcdoc(frame.current, srcDoc);
  }, [srcDoc]);
  useEffect(() => {
    const el = frame.current;
    if (el === null) return;
    let detach: () => void = () => undefined;
    const attach = () => {
      detach();
      const doc = el.contentDocument;
      if (doc?.URL !== SRCDOC_URL) return;
      // A new document (the first, a theme switch, remote content allowed).
      guard.reset();
      let live = true;
      const measure = () => {
        if (!live) return;
        const metrics = documentMetrics(doc);
        if (metrics !== null) apply(contentHeight(metrics));
      };
      for (const type of REMEASURE_EVENTS) doc.addEventListener(type, measure, true);
      // The frame's own size changed (content overflowing the root shows only then).
      const view = doc.defaultView;
      view?.addEventListener("resize", measure);
      const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
      observer?.observe(doc.documentElement);
      // `body` is null in a document without one (typed as present).
      const body = doc.body as HTMLElement | null;
      if (body !== null) observer?.observe(body);
      // Web fonts change line heights after the first layout.
      if ("fonts" in doc) void doc.fonts.ready.then(measure);
      measure();
      // Links never navigate the frame; the reader follows them.
      const unwatch = watchLinks(doc, report);
      detach = () => {
        live = false;
        unwatch();
        for (const type of REMEASURE_EVENTS) doc.removeEventListener(type, measure, true);
        view?.removeEventListener("resize", measure);
        observer?.disconnect();
      };
    };
    el.addEventListener("load", attach);
    attach();
    return () => {
      el.removeEventListener("load", attach);
      detach();
    };
  }, [guard, apply, report]);
  const { geom, state } = useFrameGeometry(height, designed);
  return (
    <iframe
      ref={frame}
      data-part="message-view.body"
      data-state={state}
      data-geom={geom}
      sandbox={SRCDOC_SANDBOX}
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

/** A remote UI mail frame on the sandbox origin; its route's script posts the height and links. */
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
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const source = frame.current?.contentWindow ?? null;
      const posted = frameHeightOf(event, source, sandboxOrigin);
      if (posted !== null) apply(posted);
      const link = frameLinkOf(event, source, sandboxOrigin);
      if (link !== null) report(link);
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
    };
  }, [sandboxOrigin, apply, report]);
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
      sandbox={REMOTE_SANDBOX}
      src={src}
      referrerPolicy="no-referrer"
      title={title}
    />
  );
}
