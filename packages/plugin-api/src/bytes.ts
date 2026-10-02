/**
 * Bytes helpers for the sandbox. Nail's plug-in runtime (QuickJS) has
 * typed arrays but no `TextDecoder`, `TextEncoder`, `atob` or `btoa`, so
 * attachment bytes (previews, attachment actions), binary
 * `nail.http.fetch` bodies and preview images go through these.
 */

/** The replacement character for malformed UTF-8. */
const REPLACEMENT = 0xfffd;

/**
 * Decodes UTF-8 (a leading byte order mark is dropped; malformed
 * sequences become U+FFFD, as `TextDecoder` does).
 */
export function decodeUtf8(bytes: Uint8Array): string {
  // The WHATWG decoder: one U+FFFD per maximal invalid subpart.
  const parts: string[] = [];
  let codes: number[] = [];
  const emit = (cp: number) => {
    if (cp > 0xffff) {
      const v = cp - 0x10000;
      codes.push(0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
    } else {
      codes.push(cp);
    }
    if (codes.length >= 8192) {
      parts.push(String.fromCharCode(...codes));
      codes = [];
    }
  };
  let needed = 0;
  let seen = 0;
  let cp = 0;
  let lower = 0x80;
  let upper = 0xbf;
  const start =
    bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf ? 3 : 0;
  for (let i = start; i < bytes.length; i += 1) {
    const b = bytes[i] ?? 0;
    if (needed === 0) {
      if (b <= 0x7f) emit(b);
      else if (b >= 0xc2 && b <= 0xdf) {
        needed = 1;
        cp = b & 0x1f;
      } else if (b >= 0xe0 && b <= 0xef) {
        if (b === 0xe0) lower = 0xa0;
        if (b === 0xed) upper = 0x9f;
        needed = 2;
        cp = b & 0x0f;
      } else if (b >= 0xf0 && b <= 0xf4) {
        if (b === 0xf0) lower = 0x90;
        if (b === 0xf4) upper = 0x8f;
        needed = 3;
        cp = b & 0x07;
      } else {
        emit(REPLACEMENT);
      }
      continue;
    }
    if (b < lower || b > upper) {
      needed = 0;
      seen = 0;
      cp = 0;
      lower = 0x80;
      upper = 0xbf;
      emit(REPLACEMENT);
      i -= 1; // this byte starts over
      continue;
    }
    lower = 0x80;
    upper = 0xbf;
    cp = (cp << 6) | (b & 0x3f);
    seen += 1;
    if (seen === needed) {
      emit(cp);
      needed = 0;
      seen = 0;
      cp = 0;
    }
  }
  if (needed !== 0) emit(REPLACEMENT);
  parts.push(String.fromCharCode(...codes));
  return parts.join("");
}

/** Whether `bytes` are valid UTF-8 (a BOM is allowed). */
export function isUtf8(bytes: Uint8Array): boolean {
  const at = (k: number) => bytes[k] ?? 0;
  const cont = (k: number) => k < bytes.length && (at(k) & 0xc0) === 0x80;
  let i = 0;
  while (i < bytes.length) {
    const b = at(i);
    if (b < 0x80) {
      i += 1;
    } else if (b >= 0xc2 && b <= 0xdf && cont(i + 1)) {
      i += 2;
    } else if (b >= 0xe0 && b <= 0xef && cont(i + 1) && cont(i + 2)) {
      const cp = ((b & 0x0f) << 12) | ((at(i + 1) & 0x3f) << 6);
      if (cp < 0x800 || (cp >= 0xd800 && cp <= 0xdfff)) return false;
      i += 3;
    } else if (b >= 0xf0 && b <= 0xf4 && cont(i + 1) && cont(i + 2) && cont(i + 3)) {
      const cp = ((b & 0x07) << 18) | ((at(i + 1) & 0x3f) << 12);
      if (cp < 0x10000 || cp > 0x10ffff) return false;
      i += 4;
    } else {
      return false;
    }
  }
  return true;
}

/**
 * Decodes Windows-1252 / Latin-1 text (for files that are not UTF-8,
 * like many CSV exports).
 */
export function decodeLatin1(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += 8192) {
    parts.push(String.fromCharCode(...bytes.subarray(i, i + 8192)));
  }
  return parts.join("");
}

/** Decodes text: UTF-8 when the bytes are valid UTF-8, else Latin-1. */
export function decodeText(bytes: Uint8Array): string {
  return isUtf8(bytes) ? decodeUtf8(bytes) : decodeLatin1(bytes);
}

/** Encodes a string as UTF-8 (lone surrogates become U+FFFD). */
export function encodeUtf8(text: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < text.length; i += 1) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff) {
      const next = text.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        cp = 0x10000 + ((cp - 0xd800) << 10) + (next - 0xdc00);
        i += 1;
      } else {
        cp = REPLACEMENT;
      }
    } else if (cp >= 0xdc00 && cp <= 0xdfff) {
      cp = REPLACEMENT;
    }
    if (cp < 0x80) out.push(cp);
    else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
    else if (cp < 0x10000) {
      out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      out.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f),
      );
    }
  }
  return Uint8Array.from(out);
}

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** Base64 (standard alphabet, padded), e.g. for preview images. */
export function toBase64(bytes: Uint8Array): string {
  const parts: string[] = [];
  let chunk = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i] ?? 0;
    const b = bytes[i + 1] ?? 0;
    const c = bytes[i + 2] ?? 0;
    const n = (a << 16) | (b << 8) | c;
    chunk +=
      (ALPHABET[(n >> 18) & 63] ?? "") +
      (ALPHABET[(n >> 12) & 63] ?? "") +
      (i + 1 < bytes.length ? (ALPHABET[(n >> 6) & 63] ?? "") : "=") +
      (i + 2 < bytes.length ? (ALPHABET[n & 63] ?? "") : "=");
    if (chunk.length >= 8192) {
      parts.push(chunk);
      chunk = "";
    }
  }
  parts.push(chunk);
  return parts.join("");
}

/** Decodes base64 (standard or URL alphabet; whitespace and padding optional). */
export function fromBase64(text: string): Uint8Array {
  const clean = text.replace(/[\s=]/g, "").replace(/-/g, "+").replace(/_/g, "/");
  if (!/^[A-Za-z0-9+/]*$/.test(clean) || clean.length % 4 === 1) {
    throw new TypeError("not base64");
  }
  const out = new Uint8Array(Math.floor((clean.length * 3) / 4));
  let o = 0;
  for (let i = 0; i < clean.length; i += 4) {
    const n =
      (ALPHABET.indexOf(clean[i] ?? "A") << 18) |
      (ALPHABET.indexOf(clean[i + 1] ?? "A") << 12) |
      ((i + 2 < clean.length ? ALPHABET.indexOf(clean[i + 2] ?? "A") : 0) << 6) |
      (i + 3 < clean.length ? ALPHABET.indexOf(clean[i + 3] ?? "A") : 0);
    out[o++] = (n >> 16) & 255;
    if (i + 2 < clean.length) out[o++] = (n >> 8) & 255;
    if (i + 3 < clean.length) out[o++] = n & 255;
  }
  return out.subarray(0, o);
}

/** A `Uint8Array` view of bytes given as `Uint8Array`, `ArrayBuffer` or a typed array. */
export function toBytes(data: Uint8Array | ArrayBuffer | ArrayBufferView): Uint8Array {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
}
