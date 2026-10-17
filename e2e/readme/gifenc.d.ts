/** The part of `gifenc` (MIT, no types of its own) the README media use. */
declare module "gifenc" {
  export type Palette = number[][];
  export type Format = "rgb565" | "rgb444" | "rgba4444";

  export interface FrameOptions {
    palette?: Palette;
    delay?: number;
    transparent?: boolean;
    transparentIndex?: number;
    repeat?: number;
    dispose?: number;
  }

  export interface Encoder {
    writeFrame(index: Uint8Array, width: number, height: number, options?: FrameOptions): void;
    finish(): void;
    bytes(): Uint8Array;
  }

  interface Gifenc {
    GIFEncoder: () => Encoder;
    quantize: (rgba: Uint8Array, maxColors: number, options?: { format?: Format }) => Palette;
    applyPalette: (rgba: Uint8Array, palette: Palette, format?: Format) => Uint8Array;
  }

  const gifenc: Gifenc;
  export default gifenc;
}
