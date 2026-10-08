/**
 * The Gmail label dialogs: the label editor (`gmail-label-editor`: name,
 * nesting under another label, a colour from Gmail's palette, visibility
 * in the label list and on messages), the delete confirmation
 * (`gmail-label-delete`) and "apply labels" for the selected message
 * (`gmail-apply-labels`; user labels are Nail tags, so applying is a tag
 * change through the operation queue).
 */
import {
  GMAIL_LABEL_LIST_VISIBILITY_VALUES,
  GMAIL_MESSAGE_LIST_VISIBILITY_VALUES,
} from "../../api/generated";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState, type SubmitEvent } from "react";
import type {
  GmailLabel,
  GmailLabelColor,
  GmailLabelListVisibility,
  GmailLabelSettings,
  GmailMessageListVisibility,
  LocalizedMessage,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { TagCheckbox } from "../mail-actions/TagCheckbox";
import { useTagWindow } from "../mail-actions/tags";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { useGeometry } from "../../theme/ThemeProvider";
import { isWithin, labelIssues, leafOf, parentOf } from "./model";
import { LABEL_EDITOR_DIALOG } from "./names";
import { LABEL_COLORS, paletteName } from "./palette";
import { deleteLabel, saveLabel, useGmailLabels } from "./queries";
import { gmailStoreFor, useGmail } from "./store";
import { LabelSwatch } from "./Labels";
import { EmptyDialog, LoadingDialog } from "../../widgets/DialogStates";
import { TextField } from "../../widgets/forms";
import { useBusyAction } from "../../state/busyAction";

const LIST_VISIBILITIES: readonly GmailLabelListVisibility[] = GMAIL_LABEL_LIST_VISIBILITY_VALUES;
const MESSAGE_VISIBILITIES: readonly GmailMessageListVisibility[] =
  GMAIL_MESSAGE_LIST_VISIBILITY_VALUES;

function Problem({ message }: { message: LocalizedMessage | null }) {
  const t = useT();
  if (message === null) return null;
  return (
    <p data-part="gmail.problem" role="alert">
      {t(message.id, message.args)}
    </p>
  );
}

/** One colour of the palette as a radio button. */
function ColorOption({
  name,
  color,
  group,
  checked,
  onPick,
}: {
  name: string;
  color: GmailLabelColor | null;
  group: string;
  checked: boolean;
  onPick: () => void;
}) {
  const t = useT();
  const geom = `gmail-color-${useId().replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  useGeometry(geom, color ? { "background-color": color.background, color: color.text } : {});
  return (
    <label data-part="gmail.color" data-color={name} data-state={checked ? "selected" : undefined}>
      <input type="radio" name={group} checked={checked} onChange={onPick} />
      <span data-part="gmail.color.sample" data-geom={geom} aria-hidden="true">
        {t("gmail-color-sample")}
      </span>
      <span data-part="gmail.color.name">{t(`gmail-color-${name}`)}</span>
    </label>
  );
}

/** Dialog `gmail-label-editor`. */
export function LabelEditorDialog({ close }: DialogProps) {
  const target = useGmail((s) => s.editor);
  const labels = useGmailLabels(target?.accountId ?? null);
  if (target === null) {
    return (
      <EmptyDialog
        part="gmail.dialog"
        textPart="gmail.hint"
        hintId="gmail-editor-nothing"
        close={close}
      />
    );
  }
  if (!labels.data) {
    return <LoadingDialog part="gmail.dialog" textPart="gmail.hint" />;
  }
  const label = target.labelId ? (labels.data.find((l) => l.id === target.labelId) ?? null) : null;
  return (
    <LabelEditor
      key={label?.id ?? "new"}
      accountId={target.accountId}
      label={label}
      parent={target.parent}
      labels={labels.data}
      close={close}
    />
  );
}

function LabelEditor({
  accountId,
  label,
  parent: initialParent,
  labels,
  close,
}: {
  accountId: string;
  label: GmailLabel | null;
  parent: string | null;
  labels: readonly GmailLabel[];
  close: () => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const ids = { parent: useId(), list: useId(), message: useId(), colors: useId() };
  const [name, setName] = useState(label ? leafOf(label.name) : "");
  const [parent, setParent] = useState<string>(
    (label ? parentOf(label.name) : initialParent) ?? "",
  );
  const [color, setColor] = useState<GmailLabelColor | null>(label?.color ?? null);
  const [list, setList] = useState<GmailLabelListVisibility>(label?.list_visibility ?? "show");
  const [message, setMessage] = useState<GmailMessageListVisibility>(
    label?.message_visibility ?? "show",
  );
  const { busy, problem, setProblem, run } = useBusyAction();
  // A colour Gmail would refuse (set elsewhere) stays selectable as "current".
  const current = paletteName(color);
  const parents = labels
    .filter((l) => l.kind === "user" && !(label && isWithin(l.name, label.name)))
    .map((l) => l.name)
    .sort((a, b) => a.localeCompare(b));
  const fullName = parent ? `${parent}/${name.trim()}` : name.trim();
  const settings: GmailLabelSettings = {
    name: fullName,
    color,
    list_visibility: list,
    message_visibility: message,
  };

  const finish = () => {
    gmailStoreFor(store).getState().editLabel(null);
    close();
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const issue = labelIssues(
      { ...settings, name: name.trim() === "" ? "" : fullName },
      labels.filter((l) => l.id !== label?.id),
    )[0];
    if (issue) {
      setProblem({ id: `gmail-label-issue-${issue}` });
      return;
    }
    await run(async () => {
      await saveLabel({ client, queryClient }, accountId, label?.id ?? null, settings);
      finish();
    });
  };

  return (
    <form
      data-part="gmail.dialog"
      data-dialog="label-editor"
      aria-busy={busy}
      onSubmit={(e) => void submit(e)}
    >
      <TextField
        labelId="common-name"
        name="name"
        value={name}
        maxLength={225}
        invalid={problem !== null}
        onChange={setName}
      />
      <div data-part="form.field" data-field="parent">
        <label data-part="form.label" htmlFor={ids.parent}>
          {t("gmail-label-parent")}
        </label>
        <select
          id={ids.parent}
          data-part="form.select"
          value={parent}
          onChange={(e) => {
            setParent(e.target.value);
          }}
        >
          <option value="">{t("gmail-label-parent-none")}</option>
          {parents.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
      </div>
      <fieldset data-part="gmail.colors">
        <legend data-part="form.section.title">{t("common-colour")}</legend>
        <ColorOption
          name="none"
          color={null}
          group={ids.colors}
          checked={color === null}
          onPick={() => {
            setColor(null);
          }}
        />
        {color !== null && current === null && (
          <ColorOption
            name="current"
            color={color}
            group={ids.colors}
            checked
            onPick={() => undefined}
          />
        )}
        {LABEL_COLORS.map((p) => (
          <ColorOption
            key={p.name}
            name={p.name}
            color={p.color}
            group={ids.colors}
            checked={current === p.name}
            onPick={() => {
              setColor(p.color);
            }}
          />
        ))}
      </fieldset>
      <div data-part="form.field" data-field="list-visibility">
        <label data-part="form.label" htmlFor={ids.list}>
          {t("gmail-label-list-visibility")}
        </label>
        <select
          id={ids.list}
          data-part="form.select"
          value={list}
          onChange={(e) => {
            setList(e.target.value as GmailLabelListVisibility);
          }}
        >
          {LIST_VISIBILITIES.map((v) => (
            <option key={v} value={v}>
              {t(`gmail-list-visibility-${v}`)}
            </option>
          ))}
        </select>
      </div>
      <div data-part="form.field" data-field="message-visibility">
        <label data-part="form.label" htmlFor={ids.message}>
          {t("gmail-label-message-visibility")}
        </label>
        <select
          id={ids.message}
          data-part="form.select"
          value={message}
          onChange={(e) => {
            setMessage(e.target.value as GmailMessageListVisibility);
          }}
        >
          {MESSAGE_VISIBILITIES.map((v) => (
            <option key={v} value={v}>
              {t(`gmail-message-visibility-${v}`)}
            </option>
          ))}
        </select>
      </div>
      {label && parentOf(label.name) !== (parent || null) && (
        <p data-part="gmail.hint">{t("gmail-label-move-hint")}</p>
      )}
      <Problem message={problem} />
      <div data-part="gmail.dialog.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {label ? t("common-save") : t("common-create")}
        </button>
        <button type="button" data-part="form.button" onClick={finish}>
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}

/** Dialog `gmail-label-delete`. */
export function LabelDeleteDialog({ close }: DialogProps) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const label = useGmail((s) => s.deleting);
  const { busy, problem, run } = useBusyAction();
  const finish = () => {
    gmailStoreFor(store).getState().deleteLabel(null);
    close();
  };
  if (label === null) {
    return (
      <EmptyDialog
        part="gmail.dialog"
        textPart="gmail.hint"
        hintId="gmail-delete-nothing"
        close={finish}
      />
    );
  }
  const remove = async () => {
    await run(async () => {
      await deleteLabel({ client, queryClient }, label.account_id, label.id);
      const search = store.getState().search;
      if (search?.text.includes(label.tag ?? label.name)) store.getState().setSearch(null);
      finish();
    });
  };
  return (
    <div data-part="gmail.dialog" data-dialog="label-delete" aria-busy={busy}>
      <p data-part="gmail.dialog.intro">
        {t("gmail-delete-intro", { name: label.name, count: label.total })}
      </p>
      <Problem message={problem} />
      <div data-part="gmail.dialog.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          disabled={busy}
          onClick={() => void remove()}
        >
          {t("gmail-delete-confirm")}
        </button>
        <button type="button" data-part="form.button" onClick={finish}>
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}

/**
 * Dialog `gmail-apply-labels`: the user labels as check boxes over what the
 * window opened for (the conversation, the picked rows or a card): checked
 * when every message has the label, mixed when some have it (P3 review).
 */
export function ApplyLabelsDialog({ close }: DialogProps) {
  const t = useT();
  const tags = useTagWindow("label", close);
  const { summary, store } = tags;
  const labels = useGmailLabels(summary?.gmail ? summary.account_id : null);
  const ids = { filter: useId() };
  const [filter, setFilter] = useState("");
  if (!summary?.gmail) {
    return (
      <EmptyDialog
        part="gmail.dialog"
        textPart="gmail.hint"
        hintId="gmail-apply-nothing"
        close={close}
      />
    );
  }
  const user = (labels.data ?? []).filter((l) => l.kind === "user");
  const tagOf = (l: GmailLabel) => l.tag ?? l.name;
  const shown = user.filter((l) => l.name.toLowerCase().includes(filter.trim().toLowerCase()));

  return (
    <form
      data-part="gmail.dialog"
      data-dialog="apply-labels"
      aria-busy={tags.busy}
      onSubmit={(e) => {
        e.preventDefault();
        void tags.apply(user.map(tagOf));
      }}
    >
      <p data-part="gmail.dialog.intro">
        {t("gmail-apply-intro", { subject: summary.subject || t("gmail-no-subject") })}
      </p>
      <div data-part="form.field" data-field="filter">
        <label data-part="form.label" htmlFor={ids.filter}>
          {t("gmail-apply-filter")}
        </label>
        <input
          id={ids.filter}
          data-part="form.input"
          type="search"
          value={filter}
          onChange={(e) => {
            setFilter(e.target.value);
          }}
        />
      </div>
      <fieldset data-part="gmail.apply.labels">
        <legend data-part="form.section.title">{t("gmail-apply-legend")}</legend>
        {shown.length === 0 && <p data-part="gmail.hint">{t("gmail-apply-none")}</p>}
        {shown.map((l) => (
          <label key={l.id} data-part="form.check" data-label={l.id}>
            <TagCheckbox
              mark={tags.mark(tagOf(l))}
              decided={tags.decision(tagOf(l))}
              onDecide={(checked) => {
                tags.decide(tagOf(l), checked);
              }}
            />
            <LabelSwatch label={l} />
            <span>{l.name}</span>
          </label>
        ))}
      </fieldset>
      <Problem message={tags.problem} />
      <div data-part="gmail.dialog.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={tags.busy}>
          {t("common-apply")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            gmailStoreFor(store)
              .getState()
              .editLabel({ accountId: summary.account_id, labelId: null, parent: null });
            store.getState().openWindow(LABEL_EDITOR_DIALOG);
          }}
        >
          {t("gmail-action-new-label")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}
