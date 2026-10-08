/**
 * Runs the UI's ports of contract rules over the test vectors nail-api
 * generates from the Rust functions (`crates/nail-api/tests/ts_vectors.rs`
 * writes `crates/nail-api/fixtures/ts-vectors/<area>.json`): each case is a
 * function name, its arguments and the Rust answer. The Rust code is the
 * single source; a port that answers differently fails here, and a Rust
 * change regenerates the vectors (wave Q2, ADR 0171).
 */
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import type { ResolvedLayout } from "../api/generated";
import { normalizeLayout } from "../api/normalize";

/** One vector: the function (by its TypeScript name), its arguments, the answer. */
export interface VectorCase {
  fn: string;
  args: readonly unknown[];
  out: unknown;
}

/** A generated vector file. */
export interface VectorFile {
  cases: readonly VectorCase[];
}

/**
 * The implementations under test by name. An adaptor turns the JSON
 * arguments into what the port takes (`Uint8Array`, a layout, …) and its
 * answer into the JSON shape Rust wrote.
 */
export type VectorImpls = Readonly<Record<string, (...args: never[]) => unknown>>;

/** `"default-layout"` as an argument stands for the default layout fixture. */
export const DEFAULT_LAYOUT_ARG = "default-layout";

/** The default layout (`crates/nail-api/fixtures/default-layout.json`), normalized. */
export function defaultLayout(): ResolvedLayout {
  return normalizeLayout(defaultLayoutJson);
}

/** The functions a vector file covers, in first-appearance order. */
export function vectorFunctions(file: VectorFile): string[] {
  return [...new Set(file.cases.map((c) => c.fn))];
}

/**
 * Registers one test per function of `file`: every case's answer must equal
 * the Rust answer, and `impls` must cover exactly the file's functions.
 */
export function describeVectors(area: string, file: VectorFile, impls: VectorImpls): void {
  describe(`${area}: the Rust rules' vectors`, () => {
    it("covers every function of the vectors", () => {
      expect(Object.keys(impls).sort()).toEqual(vectorFunctions(file).sort());
    });
    for (const name of vectorFunctions(file)) {
      it(name, () => {
        const run = impls[name] as ((...args: unknown[]) => unknown) | undefined;
        expect(run, name).toBeDefined();
        for (const c of file.cases.filter((x) => x.fn === name)) {
          expect(run?.(...c.args), `${name}(${JSON.stringify(c.args)})`).toEqual(c.out);
        }
      });
    }
  });
}
