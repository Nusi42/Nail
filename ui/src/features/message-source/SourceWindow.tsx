/**
 * The source window (`message-source:<n>`, ADR 0194): a message as it was
 * received — every header, the MIME structure, the content as encoded — or
 * decoded for reading (each part's content in its characters, parts that
 * are not text named in one line). Wrap lines, Copy all; text can also be
 * selected and copied in part. Nothing is loaded from the network.
 */
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { MessageId } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { formatBytes } from "../portability/model";
import { isTruncated, sourceText, type OmittedLine } from "./model";
import { SOURCE_PART, SOURCE_VIEWS, type SourceView } from "./names";
import { useSourceState } from "./store";

/** Query keys of this feature. */
export const sourceKeys = {
  message: (id: MessageId) => ["message-source", id] as const,
};

function useMessageSource(id: MessageId) {
  const client = useClient();
  return useQuery({
    queryKey: sourceKeys.message(id),
    queryFn: async () =>
      (await client.request({ type: "GetMessageSource", message_id: id }, "MessageSource")).source,
    // A message's source does not change.
    staleTime: Infinity,
    retry: false,
  });
}

function Source({ id, close }: { id: MessageId; close: () => void }) {
  const t = useT();
  const locale = useLocale();
  const source = useMessageSource(id);
  const [view, setView] = useState<SourceView>("original");
  const [wrap, setWrap] = useState(true);
  const [status, setStatus] = useState<string | null>(null);
  const omitted: OmittedLine = (chunk) => {
    const args = { mime: chunk.mime, size: formatBytes(chunk.size, locale) };
    return chunk.filename === null
      ? t("source-omitted", args)
      : t("source-omitted-named", { ...args, name: chunk.filename });
  };
  const data = source.data;
  const text = data ? sourceText(data, view, omitted) : "";
  const copy = () => {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setStatus("source-copied");
      })
      .catch((error: unknown) => {
        setStatus(errorMessageIdOf(error));
      });
  };
  return (
    <div data-part={SOURCE_PART} data-view={view} data-state={wrap ? "wrapped" : undefined}>
      <div data-part="message-source.controls">
        <div data-part="message-source.views" role="group" aria-label={t("source-views")}>
          {SOURCE_VIEWS.map((v) => (
            <button
              key={v}
              type="button"
              data-part="message-source.view"
              data-view={v}
              aria-pressed={view === v}
              onClick={() => {
                setView(v);
                setStatus(null);
              }}
            >
              {t(`source-view-${v}`)}
            </button>
          ))}
        </div>
        <label data-part="form.check">
          <input
            type="checkbox"
            data-part="form.checkbox"
            checked={wrap}
            onChange={(e) => {
              setWrap(e.target.checked);
            }}
          />
          {t("source-wrap")}
        </label>
      </div>
      {data && (
        <p data-part="message-source.note">
          {view === "decoded" ? t("source-decoded-hint") : t("source-original-hint")}{" "}
          {data.encrypted && t("source-encrypted")}
        </p>
      )}
      {data && isTruncated(data, view) && (
        <p data-part="form.hint" data-tone="warning" role="status">
          {t("source-truncated", { size: formatBytes(data.size, locale) })}
        </p>
      )}
      {source.isPending && <p data-part="message-source.note">{t("source-loading")}</p>}
      {source.isError && (
        <p data-part="form.hint" data-tone="danger" role="alert">
          {t(errorMessageIdOf(source.error))}
        </p>
      )}
      {data && (
        <pre
          data-part="message-source.text"
          aria-label={t("source-text-label")}
          // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the text scrolls: keyboard users must be able to reach it (axe scrollable-region-focusable).
          tabIndex={0}
        >
          {text}
        </pre>
      )}
      <p data-part="message-source.status" role="status">
        {status ? t(status) : ""}
      </p>
      <div data-part="message-source.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="copy"
          disabled={!data}
          onClick={copy}
        >
          {t("source-copy")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="close"
          onClick={close}
        >
          {t("common-close")}
        </button>
      </div>
    </div>
  );
}

/** The source window: its message comes from the store. */
export function SourceWindow({ name, close }: DialogProps) {
  const t = useT();
  const id = useSourceState((s) => s.messages[name]);
  if (id === undefined) {
    return (
      <div data-part={SOURCE_PART}>
        <p data-part="message-source.note">{t("source-gone")}</p>
        <div data-part="message-source.actions">
          <button type="button" data-part="form.button" data-action="close" onClick={close}>
            {t("common-close")}
          </button>
        </div>
      </div>
    );
  }
  return <Source id={id} close={close} />;
}
