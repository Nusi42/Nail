import { Buffer } from "node:buffer";

import { describe, expect, it } from "vitest";

import {
  decodeLatin1,
  decodeText,
  decodeUtf8,
  encodeUtf8,
  fromBase64,
  isUtf8,
  toBase64,
  toBytes,
} from "./bytes";

const samples = [
  "",
  "plain ASCII",
  "Grüße, 東京",
  "emoji 😀 and 𝄞",
  "a\u0000b",
  "x".repeat(20_000),
];

describe("UTF-8", () => {
  it("round-trips like Node's codec", () => {
    for (const text of samples) {
      const bytes = encodeUtf8(text);
      expect(Buffer.from(bytes).equals(Buffer.from(text, "utf8")), text.slice(0, 20)).toBe(true);
      expect(decodeUtf8(bytes)).toBe(text);
      expect(isUtf8(bytes)).toBe(true);
    }
  });

  it("drops a byte order mark and replaces malformed sequences", () => {
    expect(decodeUtf8(Uint8Array.from([0xef, 0xbb, 0xbf, 0x41]))).toBe("A");
    // A lone continuation byte, an overlong "/", a truncated sequence, a surrogate.
    const bad = Uint8Array.from([0x80, 0xc0, 0xaf, 0x41, 0xe2, 0x82, 0xed, 0xa0, 0x80]);
    expect(decodeUtf8(bad)).toBe(new TextDecoder().decode(bad));
    expect(isUtf8(bad)).toBe(false);
    expect(isUtf8(Uint8Array.from([0xef, 0xbf, 0xbd]))).toBe(true); // an encoded U+FFFD is valid
  });

  it("agrees with TextDecoder on random bytes", () => {
    let seed = 42;
    const rand = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed;
    };
    const decoder = new TextDecoder("utf-8", { ignoreBOM: false });
    for (let n = 0; n < 2000; n += 1) {
      const bytes = Uint8Array.from({ length: rand() % 12 }, () => {
        const r = rand() % 6;
        // Bias towards the interesting lead and continuation bytes.
        return r === 0 ? 0x80 + (rand() % 64) : r === 1 ? 0xc0 + (rand() % 64) : rand() % 256;
      });
      const expected = decoder.decode(bytes);
      expect(decodeUtf8(bytes), Array.from(bytes).join(",")).toBe(expected);
      expect(isUtf8(bytes), Array.from(bytes).join(",")).toBe(
        !expected.includes("�") || Buffer.from(expected, "utf8").equals(Buffer.from(bytes)),
      );
    }
  });

  it("encodes lone surrogates as U+FFFD", () => {
    expect(decodeUtf8(encodeUtf8("a\ud800b"))).toBe("a�b");
    expect(decodeUtf8(encodeUtf8("a\udc00"))).toBe("a�");
  });

  it("falls back to Latin-1 for text that is not UTF-8", () => {
    const latin1 = Uint8Array.from([0x47, 0x72, 0xfc, 0xdf, 0x65]); // "Grüße" in Windows-1252
    expect(isUtf8(latin1)).toBe(false);
    expect(decodeText(latin1)).toBe("Grüße");
    expect(decodeLatin1(latin1)).toBe("Grüße");
    expect(decodeText(encodeUtf8("Grüße"))).toBe("Grüße");
  });
});

describe("base64", () => {
  it("matches Node's encoder and decodes both alphabets", () => {
    for (const length of [0, 1, 2, 3, 4, 5, 100, 10_000]) {
      const bytes = Uint8Array.from({ length }, (_, i) => (i * 37 + 11) & 255);
      const b64 = toBase64(bytes);
      expect(b64).toBe(Buffer.from(bytes).toString("base64"));
      expect(Buffer.from(fromBase64(b64)).equals(Buffer.from(bytes))).toBe(true);
      const url = Buffer.from(bytes).toString("base64url");
      expect(Buffer.from(fromBase64(url)).equals(Buffer.from(bytes))).toBe(true);
    }
    expect(fromBase64(" aGVs\nbG8= ")).toEqual(encodeUtf8("hello"));
    expect(() => fromBase64("a")).toThrow(/base64/);
    expect(() => fromBase64("a$b=")).toThrow(/base64/);
  });

  it("views buffers and typed arrays as bytes", () => {
    const bytes = Uint8Array.from([1, 2, 3, 4]);
    expect(toBytes(bytes)).toBe(bytes);
    expect(toBytes(bytes.buffer)).toEqual(bytes);
    expect(toBytes(new DataView(bytes.buffer, 1, 2))).toEqual(Uint8Array.from([2, 3]));
  });
});
