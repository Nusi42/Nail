/**
 * Playwright global setup: builds the example plug-ins once (outside the
 * repository) and hands their paths to the workers.
 */
import { buildExample, EXAMPLES, exampleEnv } from "./plugins";

export default async function globalSetup(): Promise<void> {
  for (const name of EXAMPLES) {
    process.env[exampleEnv(name)] ??= await buildExample(name);
  }
}
