/**
 * Playwright global setup: builds the example plug-ins once (outside the
 * repository) and hands their paths to the workers.
 */
import { buildExample, exampleEnv, exampleNames } from "./plugins";

export default async function globalSetup(): Promise<void> {
  for (const name of exampleNames()) {
    process.env[exampleEnv(name)] ??= await buildExample(name);
  }
}
