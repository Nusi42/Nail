/**
 * Trusted Types for the two documents the UI hands the browser as HTML:
 * the reader's and the composer preview's `srcdoc` (both sanitized by the
 * core, both rendered in `sandbox=""` iframes without scripts).
 *
 * The remote UI enforces Trusted Types (`require-trusted-types-for
 * 'script'; trusted-types nail nail-reader`, ADR 0038): assigning a plain
 * string to `srcdoc` throws there. The `nail-reader` policy is the one
 * place such HTML becomes a `TrustedHTML`. Browsers without Trusted Types
 * (and tests) get the string itself.
 */

interface TrustedTypePolicyLike {
  createHTML: (input: string) => unknown;
}

interface TrustedTypesLike {
  createPolicy: (
    name: string,
    rules: { createHTML: (input: string) => string },
  ) => TrustedTypePolicyLike;
}

/** The policy name the server's CSP allows for mail HTML. */
export const READER_POLICY = "nail-reader";

let policy: TrustedTypePolicyLike | null | undefined;

function readerPolicy(): TrustedTypePolicyLike | null {
  if (policy !== undefined) return policy;
  const tt = (globalThis as { trustedTypes?: TrustedTypesLike }).trustedTypes;
  try {
    // The input is a whole document the core sanitized; it runs in a
    // sandboxed frame without scripts. Nothing else goes through here.
    policy = tt ? tt.createPolicy(READER_POLICY, { createHTML: (input) => input }) : null;
  } catch (error) {
    console.error("nail: cannot create the reader's Trusted Types policy", error);
    policy = null;
  }
  return policy;
}

/** `html` as a value `srcdoc` accepts under Trusted Types. */
export function trustedHtml(html: string): string {
  const reader = readerPolicy();
  // A TrustedHTML object; typed as string for the DOM attribute setter.
  return reader ? (reader.createHTML(html) as string) : html;
}

/** Sets an iframe's `srcdoc` (a Trusted Types sink) from sanitized HTML. */
export function setSrcdoc(frame: HTMLIFrameElement, html: string): void {
  frame.setAttribute("srcdoc", trustedHtml(html));
}
