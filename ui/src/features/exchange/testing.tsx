/** Test helpers of the Exchange feature (imported by the tests only). */
import type { ComponentType } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import { createEnv, type Env } from "../../test/harness";
import { dialogs } from ".";
import { EXCHANGE_ACCOUNT, seedExchangeDemo } from "./mockSeed";

export function setup(account: string | null = EXCHANGE_ACCOUNT): Env {
  const env = createEnv();
  seedExchangeDemo(env.transport.data);
  env.transport.data.behaviour.workMs = 0;
  if (account !== null) env.store.getState().selectAccount(account);
  return env;
}

export function dialog(name: string): ComponentType<DialogProps> {
  const found = dialogs[name];
  if (!found) throw new Error(`no dialog ${name}`);
  return found;
}
