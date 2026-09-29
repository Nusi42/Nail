/**
 * A position in a configuration file ("layout.kdl:12:5") with an "open"
 * button that asks the core to open the file at that line in the user's
 * editor (`OpenConfigFile`). The shipped defaults cannot be opened.
 */
import { useState } from "react";
import { errorMessageId, type NailClient } from "../../api/client";
import type { ConfigFile } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { configFileOf } from "./inspect";

/**
 * Opens a file at a position; resolves with `null` on success, else the
 * Fluent id of the problem.
 */
export async function openConfigFile(
  client: NailClient,
  file: ConfigFile,
  line: number,
  column: number,
): Promise<string | null> {
  try {
    const response = await client.call({ type: "OpenConfigFile", file, line, column });
    if (response.type !== "Error") return null;
    return response.code === "unsupported"
      ? "customize-open-unsupported"
      : errorMessageId(response.code);
  } catch {
    return errorMessageId("unavailable");
  }
}

export interface SourceLinkProps {
  file: string;
  line: number;
  column: number;
  /** Part name prefix, e.g. `inspector.source`. */
  part: string;
}

export function SourceLink({ file, line, column, part }: SourceLinkProps) {
  const t = useT();
  const client = useClient();
  const [problem, setProblem] = useState<string | null>(null);
  const target = configFileOf(file);
  return (
    <span data-part={part}>
      <span data-part={`${part}.position`}>{t("customize-source", { file, line, column })}</span>
      {target && (
        <button
          type="button"
          data-part={`${part}.open`}
          data-action="open-file"
          onClick={() => {
            setProblem(null);
            void openConfigFile(client, target, line, column).then(setProblem);
          }}
        >
          {t("customize-open-file")}
        </button>
      )}
      {problem && (
        <span data-part={`${part}.error`} role="alert">
          {t(problem)}
        </span>
      )}
    </span>
  );
}
