/**
 * The config error list (M1b: all errors at once): every error of every
 * configuration file, grouped by file, each with "open file at line". The
 * last valid configuration stays active while a file is invalid.
 *
 * Available as the layout widget `config-errors` and as the dialog
 * `config-errors` (`window.open "config-errors"`, `customize.errors`).
 */
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { SourceLink } from "./SourceLink";
import { allConfigErrors, useCustomization } from "./state";

interface ListProps {
  /** Hide the whole list when there are no errors. */
  hideWhenValid: boolean;
  nodeId?: string | null;
  label?: string | null;
}

function ConfigErrorList({ hideWhenValid, nodeId, label: nodeLabel }: ListProps) {
  const t = useT();
  const label = useLabel();
  const errors = useCustomization((s) => s.configErrors);
  const files = allConfigErrors(errors);
  const total = files.reduce((n, f) => n + f.issues.length, 0);
  if (hideWhenValid && total === 0) return null;
  return (
    <section
      data-part="config-errors"
      data-node-id={nodeId ?? undefined}
      data-state={total > 0 ? "invalid" : "valid"}
      aria-label={nodeLabel ? label(nodeLabel) : t("config-error-list-label")}
    >
      <p data-part="config-errors.summary" role={total > 0 ? "alert" : "status"}>
        {total > 0
          ? t("config-error-summary", { count: total, files: files.length })
          : t("config-error-none")}
      </p>
      {total > 0 && <p data-part="config-errors.note">{t("config-error-last-valid")}</p>}
      {files.map(({ file, issues }) => (
        <section key={file} data-part="config-errors.file" data-file={file}>
          <h3 data-part="config-errors.file.name">
            {t("config-error-file", { file, count: issues.length })}
          </h3>
          <ol data-part="config-errors.list">
            {issues.map((issue, index) => (
              <li key={index} data-part="config-errors.item">
                <span data-part="config-errors.item.message">
                  {issue.localized ? t(issue.localized.id, issue.localized.args) : issue.message}
                </span>
                <SourceLink
                  part="config-errors.item.source"
                  file={file}
                  line={issue.line}
                  column={issue.column}
                />
              </li>
            ))}
          </ol>
        </section>
      ))}
    </section>
  );
}

/** Layout widget `config-errors` (option `hide-when-valid`, default off). */
export function ConfigErrors({ node }: WidgetProps) {
  return (
    <ConfigErrorList
      hideWhenValid={optionBool(node, "hide-when-valid", false)}
      nodeId={node.id}
      label={node.label}
    />
  );
}

/** Dialog `config-errors`. */
export function ConfigErrorsDialog() {
  return <ConfigErrorList hideWhenValid={false} />;
}
