/**
 * The editor's text view: the rule in `rules.kdl` syntax, with line
 * numbers and syntax highlighting (R2 design review, item 14).
 *
 * The text is a plain `textarea` (typing, selection, undo and screen
 * readers work as everywhere); under it a highlighted copy of the same text
 * (`rules.text.highlight`, hidden from assistive technology) shows the
 * colours through the transparent text, and the gutter numbers the lines.
 * Both scroll with the textarea. A line the core reports a problem on is
 * marked in the gutter; the textarea is `aria-invalid` and names the
 * footer's problem badge, which lists the problems with their line and
 * column (item 2).
 *
 * Parts: `rules.text`, `rules.text.editor`, `rules.text.gutter`,
 * `rules.text.line` (`data-state="error"`), `rules.text.area`,
 * `rules.text.highlight`, `rules.text.token` (`data-kind`, `./kdlTokens`),
 * `rules.text.source`.
 */
import { useId, useRef } from "react";
import { useT } from "../../i18n/I18nProvider";
import { kdlTokens, lineNumbers } from "./kdlTokens";

export interface TextViewProps {
  text: string;
  onText: (text: string) => void;
  /** Lines (1-based) with a problem. */
  errorLines: readonly number[];
  /** The id of the footer's problem details (`aria-describedby`) while there are problems. */
  describedBy?: string | undefined;
}

export function TextView({ text, onText, errorLines, describedBy }: TextViewProps) {
  const t = useT();
  const id = useId();
  const gutter = useRef<HTMLOListElement>(null);
  const highlight = useRef<HTMLPreElement>(null);
  const invalid = errorLines.length > 0;
  return (
    <div data-part="rules.text">
      <label data-part="form.label" htmlFor={id}>
        {t("rules-text-label")}
      </label>
      <div data-part="rules.text.editor">
        <ol data-part="rules.text.gutter" ref={gutter} aria-hidden="true">
          {lineNumbers(text).map((n) => (
            <li
              key={n}
              data-part="rules.text.line"
              data-state={errorLines.includes(n) ? "error" : undefined}
            >
              {n}
            </li>
          ))}
        </ol>
        <div data-part="rules.text.area">
          <pre data-part="rules.text.highlight" ref={highlight} aria-hidden="true">
            {kdlTokens(text).map((token, i) => (
              <span key={i} data-part="rules.text.token" data-kind={token.kind}>
                {token.text}
              </span>
            ))}
            {/* A last empty line still takes its height. */}
            {"\n"}
          </pre>
          <textarea
            id={id}
            data-part="rules.text.source"
            name="rule-text"
            spellCheck={false}
            wrap="off"
            value={text}
            aria-invalid={invalid || undefined}
            aria-describedby={invalid ? describedBy : undefined}
            onChange={(event) => {
              onText(event.target.value);
            }}
            onScroll={(event) => {
              const { scrollTop, scrollLeft } = event.currentTarget;
              if (highlight.current) {
                highlight.current.scrollTop = scrollTop;
                highlight.current.scrollLeft = scrollLeft;
              }
              if (gutter.current) gutter.current.scrollTop = scrollTop;
            }}
          />
        </div>
      </div>
    </div>
  );
}
