/**
 * The text of a source view (ADR 0194): the original as the core sent it,
 * the decoded view's chunks joined with one line for each part left out.
 */
import type { MessageSource, SourceChunk } from "../../api/generated";
import type { SourceView } from "./names";

/** The line naming a part that is not shown (localized by the caller). */
export type OmittedLine = (chunk: Extract<SourceChunk, { type: "omitted" }>) => string;

/** The text of `view`. */
export function sourceText(source: MessageSource, view: SourceView, omitted: OmittedLine): string {
  if (view === "original") return source.original;
  return source.decoded
    .map((chunk) => (chunk.type === "text" ? chunk.text : `${omitted(chunk)}\n`))
    .join("");
}

/** Whether `view` was cut by the core's size limit. */
export function isTruncated(source: MessageSource, view: SourceView): boolean {
  return view === "original" ? source.original_truncated : source.decoded_truncated;
}
