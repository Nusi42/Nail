/**
 * Sender, link and attachment warnings in the reader (`MessageBody.warnings`,
 * ADRs 0029, 0195): a banner that lists a disguised sender (look-alike
 * domain or display name), every flagged link with its real target and
 * every risky attachment, plus a per-attachment marker the reader's
 * attachment list can mount (`AttachmentWarningMark`).
 */
import type { MessageBody, MessageWarning } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useMessage } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { attachmentWarning, attachmentWarnings, linkWarnings, senderWarnings } from "./model";

/** The sentences explaining a disguised sender. */
function senderText(
  t: ReturnType<typeof useT>,
  w: Extract<MessageWarning, { kind: "lookalike-sender" }>,
): string[] {
  const lines: string[] = [];
  if (w.unicode_domain !== null) {
    lines.push(
      w.resembles !== null
        ? t("security-warning-sender-domain-resembles", {
            address: w.address,
            domain: w.unicode_domain,
            resembles: w.resembles,
          })
        : t("security-warning-sender-domain", { address: w.address, domain: w.unicode_domain }),
    );
  }
  if (w.name !== null) lines.push(t("security-warning-sender-name", { name: w.name }));
  return lines;
}

function linkText(
  t: ReturnType<typeof useT>,
  w: Exclude<MessageWarning, { kind: "dangerous-attachment" | "lookalike-sender" }>,
) {
  switch (w.kind) {
    case "link-mismatch":
      return t("security-warning-link-mismatch", {
        shown: w.shown_host,
        target: w.target_host,
      });
    case "lookalike-domain":
      return w.resembles !== null
        ? t("security-warning-lookalike-resembles", {
            host: w.unicode_host,
            ascii: w.ascii_host,
            resembles: w.resembles,
          })
        : t("security-warning-lookalike", { host: w.unicode_host, ascii: w.ascii_host });
    case "suspicious-link":
      return t("security-warning-suspicious-link");
  }
}

/** The warnings banner of one message; nothing when it has none. */
export function MessageWarnings({ message }: { message: MessageBody }) {
  const t = useT();
  const warnings = message.warnings ?? [];
  if (warnings.length === 0) return null;
  const senders = senderWarnings(warnings);
  const links = linkWarnings(warnings);
  const files = attachmentWarnings(warnings);
  return (
    <section
      data-part="security-warnings"
      aria-label={t("security-warnings-label")}
      data-count={warnings.length}
    >
      <p data-part="security-warnings.title">
        {t("security-warnings-title", { count: warnings.length })}
      </p>
      <ul data-part="security-warnings.list">
        {senders.map((w) => (
          <li key={`sender-${w.address}`} data-part="security-warnings.item" data-kind={w.kind}>
            <span data-part="security-warnings.text">{senderText(t, w).join(" ")}</span>
            <span data-part="security-warnings.hint">{t("security-warning-sender-hint")}</span>
          </li>
        ))}
        {links.map((w, i) => (
          <li
            key={`link-${String(w.link)}-${w.kind}-${String(i)}`}
            data-part="security-warnings.item"
            data-kind={w.kind}
            data-link={w.link}
          >
            <span data-part="security-warnings.text">{linkText(t, w)}</span>
            <span data-part="security-warnings.target">
              {t("security-warning-real-target", { href: w.href })}
            </span>
          </li>
        ))}
        {files.map((w) => (
          <li
            key={`file-${String(w.index)}`}
            data-part="security-warnings.item"
            data-kind={w.kind}
            data-risk={w.risk}
            data-attachment={w.index}
          >
            <span data-part="security-warnings.text">
              {t(`security-risk-${w.risk}`, { name: w.filename })}
            </span>
          </li>
        ))}
      </ul>
      {links.length > 0 && (
        <p data-part="security-warnings.hint">{t("security-warnings-link-hint")}</p>
      )}
    </section>
  );
}

/**
 * Marker for one attachment in the reader's attachment list (mount it
 * next to the file name); nothing when the attachment is not flagged.
 */
export function AttachmentWarningMark({
  warnings,
  index,
}: {
  warnings: readonly MessageWarning[] | undefined;
  index: number;
}) {
  const t = useT();
  const w = attachmentWarning(warnings, index);
  if (!w) return null;
  return (
    <span data-part="security-attachment-warning" data-risk={w.risk} role="note">
      {t("security-attachment-flag", { risk: t(`security-risk-short-${w.risk}`) })}
    </span>
  );
}

/** Layout widget `security-warnings`: the warnings of the selected message. */
export function SecurityWarningsWidget({ node }: WidgetProps) {
  const label = useLabel();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId).data;
  if (message?.id !== selectedId || (message.warnings ?? []).length === 0) return null;
  return (
    <div
      data-part="security-warnings.slot"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : undefined}
    >
      <MessageWarnings message={message} />
    </div>
  );
}
