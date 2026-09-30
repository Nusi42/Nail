/**
 * Playwright global setup: builds the "Follow up" example plug-in once
 * (outside the repository) and hands its path to the workers.
 */
import { buildFollowUp, FOLLOW_UP_ENV } from "./plugins";

export default async function globalSetup(): Promise<void> {
  process.env[FOLLOW_UP_ENV] ??= await buildFollowUp();
}
