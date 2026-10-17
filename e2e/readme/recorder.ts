/**
 * Records a README animation (ADR 0197): screenshots of the page as frames,
 * with a drawn mouse pointer that glides to what is clicked next and a
 * ripple where it clicks. The pointer is an overlay of the recording only
 * (pointer-events: none), never part of the app.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import type { Locator, Page } from "@playwright/test";
import { encodeGif, type Frame } from "./lib/gif.ts";

/** Frames of a pointer move, and how long each shows. */
const MOVE_STEPS = 9;
const STEP_MS = 30;
/** How long a frame shows after an action by default. */
export const HOLD_MS = 900;
const POINTER_ID = "readme-pointer";
const RIPPLE_ID = "readme-ripple";

/** Ease in and out (cubic), 0 → 1. */
export function ease(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - (-2 * t + 2) ** 3 / 2;
}

/** The points a pointer passes on its way, without the start, ending at `to`. */
export function glide(
  from: { x: number; y: number },
  to: { x: number; y: number },
  steps = MOVE_STEPS,
): { x: number; y: number }[] {
  return Array.from({ length: steps }, (_, i) => {
    const t = ease((i + 1) / steps);
    return {
      x: Math.round(from.x + (to.x - from.x) * t),
      y: Math.round(from.y + (to.y - from.y) * t),
    };
  });
}

export class Recorder {
  readonly frames: Frame[] = [];
  private at = { x: 0, y: 0 };

  constructor(private readonly page: Page) {
    const size = page.viewportSize();
    this.at = {
      x: Math.round((size?.width ?? 0) * 0.6),
      y: Math.round((size?.height ?? 0) * 0.55),
    };
  }

  /** Draws the pointer (again after a reload) at its current place. */
  async showPointer(): Promise<void> {
    await this.page.evaluate(
      ({ id, ripple, at }) => {
        document.getElementById(id)?.remove();
        document.getElementById(ripple)?.remove();
        const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        svg.id = id;
        svg.setAttribute("width", "22");
        svg.setAttribute("height", "26");
        svg.setAttribute("viewBox", "0 0 22 26");
        const arrow = document.createElementNS("http://www.w3.org/2000/svg", "path");
        arrow.setAttribute("d", "M2 2 L2 21 L7 16.5 L10.5 24 L14 22.5 L10.5 15 L17 15 Z");
        arrow.setAttribute("fill", "#111");
        arrow.setAttribute("stroke", "#fff");
        arrow.setAttribute("stroke-width", "1.6");
        arrow.setAttribute("stroke-linejoin", "round");
        svg.append(arrow);
        const dot = document.createElement("div");
        dot.id = ripple;
        for (const el of [svg, dot]) {
          el.style.position = "fixed";
          el.style.pointerEvents = "none";
          el.style.zIndex = "2147483647";
        }
        svg.style.left = `${String(at.x - 2)}px`;
        svg.style.top = `${String(at.y - 2)}px`;
        dot.style.width = "34px";
        dot.style.height = "34px";
        dot.style.borderRadius = "50%";
        dot.style.background = "rgba(255, 170, 0, 0.45)";
        dot.style.border = "2px solid rgba(255, 140, 0, 0.9)";
        dot.style.display = "none";
        document.body.append(dot, svg);
      },
      { id: POINTER_ID, ripple: RIPPLE_ID, at: this.at },
    );
  }

  private async placePointer(x: number, y: number, ripple: boolean): Promise<void> {
    await this.page.evaluate(
      ({ id, rippleId, x, y, ripple }) => {
        const pointer = document.getElementById(id);
        const dot = document.getElementById(rippleId);
        if (pointer) {
          pointer.style.left = `${String(x - 2)}px`;
          pointer.style.top = `${String(y - 2)}px`;
        }
        if (dot) {
          dot.style.left = `${String(x - 17)}px`;
          dot.style.top = `${String(y - 17)}px`;
          dot.style.display = ripple ? "block" : "none";
        }
      },
      { id: POINTER_ID, rippleId: RIPPLE_ID, x, y, ripple },
    );
    this.at = { x, y };
  }

  /** A frame of the page as it is now, shown for `delay` ms. */
  async shot(delay = HOLD_MS): Promise<void> {
    this.frames.push({ png: await this.page.screenshot({ animations: "disabled" }), delay });
  }

  /** Glides the pointer to the middle of `target`, a frame per step. */
  async moveTo(target: Locator): Promise<void> {
    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    if (!box) throw new Error("the target is not visible");
    const to = { x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) };
    for (const point of glide(this.at, to)) {
      await this.placePointer(point.x, point.y, false);
      await this.shot(STEP_MS);
    }
  }

  /** Moves to `target`, shows the ripple, clicks, and holds the result. */
  async click(
    target: Locator,
    options: { button?: "left" | "right"; hold?: number; settle?: () => Promise<void> } = {},
  ): Promise<void> {
    await this.moveTo(target);
    await this.placePointer(this.at.x, this.at.y, true);
    await this.shot(160);
    await target.click({ button: options.button ?? "left" });
    await this.placePointer(this.at.x, this.at.y, false);
    await options.settle?.();
    await this.shot(options.hold ?? HOLD_MS);
  }

  /** Types `text` a few characters per frame, then holds. */
  async type(text: string, hold = HOLD_MS): Promise<void> {
    const perFrame = 2;
    for (let i = 0; i < text.length; i += perFrame) {
      await this.page.keyboard.type(text.slice(i, i + perFrame));
      await this.shot(70);
    }
    await this.shot(hold);
  }

  /** Presses a key (or a chord like Control+k) and holds the result. */
  async press(key: string, options: { hold?: number; settle?: () => Promise<void> } = {}) {
    await this.page.keyboard.press(key);
    await options.settle?.();
    await this.shot(options.hold ?? HOLD_MS);
  }

  /**
   * Writes the frames as `<dir>/<name>.gif`; answers the file and its size.
   * With `NAIL_README_FRAMES` set, also every frame as a PNG below that
   * folder (`<name>/<n>.png`), for reviewing a scene.
   */
  save(dir: string, name: string): { file: string; bytes: number } {
    mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${name}.gif`);
    const gif = encodeGif(this.frames);
    writeFileSync(file, gif);
    const frames = process.env.NAIL_README_FRAMES;
    if (frames) {
      const sceneDir = path.join(frames, name);
      mkdirSync(sceneDir, { recursive: true });
      this.frames.forEach((frame, i) => {
        writeFileSync(path.join(sceneDir, `${String(i).padStart(3, "0")}.png`), frame.png);
      });
    }
    return { file, bytes: gif.length };
  }
}
