/**
 * Command-line arguments of the Node scripts, read one way (wave Q2, DRY):
 * `forEachArg` walks them and hands each one a `value()` that takes the next
 * argument as its value, and a `rest()` that takes every remaining one.
 * Tests in scripts/test/args.test.mjs.
 */

/**
 * @typedef {object} ArgReader
 * @property {() => string} value the next argument, as this option's value;
 *   throws "<option> needs a value" when it is missing or another option
 * @property {() => string[]} rest every remaining argument (the walk ends)
 */

/**
 * Calls `handle` with each argument of `argv`.
 *
 * @param {readonly string[]} argv
 * @param {(arg: string, reader: ArgReader) => void} handle
 */
export function forEachArg(argv, handle) {
  let i = 0;
  /** @type {ArgReader} */
  const reader = {
    value: () => {
      const next = argv[i + 1];
      if (next === undefined || next.startsWith("--")) {
        throw new Error(`${argv[i] ?? ""} needs a value`);
      }
      i += 1;
      return next;
    },
    rest: () => {
      const rest = argv.slice(i + 1);
      i = argv.length;
      return rest;
    },
  };
  for (; i < argv.length; i += 1) handle(argv[i] ?? "", reader);
}
