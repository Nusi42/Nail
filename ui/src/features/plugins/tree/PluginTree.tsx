/**
 * The renderer of plug-in UI trees (`PluginUiNode`, ADR 0050) for dialogs
 * and panels. Every element carries a generic `plugin.*` part plus the
 * plug-in's id (`./parts.ts`); inputs are native, labelled controls, so
 * everything works with the keyboard and screen readers. Plug-in text is
 * always text: nothing is rendered as HTML (Markdown is drawn as elements,
 * images are `data:` URIs of raster formats only).
 */
import { useQueries } from "@tanstack/react-query";
import { useId, useState, type ReactNode } from "react";
import type {
  Address,
  Folder,
  JsonValue,
  PluginMessage,
  PluginUiNode,
} from "../../../api/generated";
import { DATE_ONLY, formatDateTime } from "../../../i18n/format";
import { useLocale, useT } from "../../../i18n/I18nProvider";
import { queryKeys, useAccounts, useClient } from "../../../state/queries";
import { flattenFolders } from "../../../widgets/FolderTree";
import { Icon } from "../../../widgets/Icon";
import { partAttrs, type PluginPart } from "../parts";
import { Markdown } from "./markdown";
import { parseAddress, type FieldIssue, type Values } from "./values";

export interface TreeProps {
  /** The plug-in the tree belongs to. */
  plugin: string;
  nodes: readonly PluginUiNode[];
  values: Values;
  onChange: (id: string, value: JsonValue) => void;
  /** Problems to show, by node id. */
  issues: Readonly<Record<string, FieldIssue>>;
  /** A `button` node was pressed. */
  onAction: (componentId: string) => void;
  /** While a call is running, buttons are disabled. */
  busy?: boolean;
}

/** Image types an `image` node may show. */
export const IMAGE_PREFIXES = [
  "data:image/png;base64,",
  "data:image/jpeg;base64,",
  "data:image/gif;base64,",
  "data:image/webp;base64,",
];

/** Whether an image source is an embedded raster image. */
export function isSafeImage(src: string): boolean {
  return IMAGE_PREFIXES.some((p) => src.startsWith(p)) && !/[\s"'<>]/.test(src);
}

/** A folder name indented by its depth (non-breaking spaces; options cannot be styled). */
export function indented(name: string, depth: number): string {
  return "  ".repeat(depth) + name;
}

function formatAddress(address: Address): string {
  return address.name ? `${address.name} <${address.email}>` : address.email;
}

interface FieldProps {
  plugin: string;
  id: string;
  label: string;
  issue: FieldIssue | undefined;
  control: (a11y: {
    id: string;
    "aria-invalid": boolean | undefined;
    "aria-describedby": string | undefined;
  }) => ReactNode;
  /** A group of controls (checkbox lists) gets a fieldset. */
  labelFor?: boolean;
}

function Field({ plugin, id, label, issue, control, labelFor = true }: FieldProps) {
  const t = useT();
  const domId = useId();
  const errorId = `${domId}-error`;
  return (
    <div {...partAttrs("plugin.field", plugin, id)} data-state={issue ? "invalid" : undefined}>
      {labelFor ? (
        <label {...partAttrs("plugin.field.label", plugin)} htmlFor={domId}>
          {label}
        </label>
      ) : (
        <span {...partAttrs("plugin.field.label", plugin)} id={`${domId}-label`}>
          {label}
        </span>
      )}
      {control({
        id: domId,
        "aria-invalid": issue ? true : undefined,
        "aria-describedby": issue ? errorId : undefined,
      })}
      {issue && (
        <span {...partAttrs("plugin.field.error", plugin)} id={errorId} role="alert">
          {t(`plugin-field-${issue}`)}
        </span>
      )}
    </div>
  );
}

function ContactPicker({
  plugin,
  node,
  value,
  issue,
  onChange,
}: {
  plugin: string;
  node: Extract<PluginUiNode, { type: "contact-picker" }>;
  value: JsonValue | undefined;
  issue: FieldIssue | undefined;
  onChange: (value: JsonValue) => void;
}) {
  const t = useT();
  const [text, setText] = useState("");
  const [invalid, setInvalid] = useState(false);
  const contacts: Address[] = Array.isArray(value)
    ? value.filter(
        (v): v is { name: string | null; email: string } =>
          typeof v === "object" && v !== null && !Array.isArray(v) && typeof v.email === "string",
      )
    : [];
  const add = () => {
    const parts = text
      .split(/[,;]/)
      .map((p) => p.trim())
      .filter((p) => p !== "");
    if (parts.length === 0) return;
    const parsed = parts.map(parseAddress);
    if (parsed.some((a) => a === null)) {
      setInvalid(true);
      return;
    }
    const added = parsed as Address[];
    const next = node.multiple
      ? [...contacts, ...added.filter((a) => !contacts.some((c) => c.email === a.email))]
      : added.slice(-1);
    setInvalid(false);
    setText("");
    onChange(next.map((a) => ({ name: a.name, email: a.email })));
  };
  const shown: FieldIssue | undefined = invalid ? "email" : issue;
  return (
    <Field
      plugin={plugin}
      id={node.id}
      label={node.label}
      issue={shown}
      control={(a11y) => (
        <div {...partAttrs("plugin.contact-picker", plugin, node.id)}>
          {contacts.length > 0 && (
            <ul {...partAttrs("plugin.contact-picker.list", plugin)}>
              {contacts.map((contact) => (
                <li key={contact.email} {...partAttrs("plugin.contact-picker.contact", plugin)}>
                  <span>{formatAddress(contact)}</span>
                  <button
                    type="button"
                    {...partAttrs("plugin.contact-picker.remove", plugin)}
                    aria-label={t("plugin-contact-remove", { address: contact.email })}
                    title={t("plugin-contact-remove", { address: contact.email })}
                    onClick={() => {
                      onChange(
                        contacts
                          .filter((c) => c.email !== contact.email)
                          .map((c) => ({ name: c.name, email: c.email })),
                      );
                    }}
                  >
                    {t("plugin-contact-remove-short")}
                  </button>
                </li>
              ))}
            </ul>
          )}
          <input
            {...a11y}
            {...partAttrs("plugin.contact-picker.input", plugin)}
            type="text"
            autoComplete="off"
            aria-required={node.required ? true : undefined}
            placeholder={t("plugin-contact-placeholder")}
            value={text}
            onChange={(event) => {
              setInvalid(false);
              setText(event.target.value);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === ",") {
                if (text.trim() === "") return;
                event.preventDefault();
                add();
              }
            }}
            onBlur={() => {
              if (text.trim() !== "") add();
            }}
          />
        </div>
      )}
    />
  );
}

function FolderPickerNode({
  plugin,
  node,
  value,
  issue,
  onChange,
}: {
  plugin: string;
  node: Extract<PluginUiNode, { type: "folder-picker" }>;
  value: JsonValue | undefined;
  issue: FieldIssue | undefined;
  onChange: (value: JsonValue) => void;
}) {
  const t = useT();
  const client = useClient();
  const accounts = useAccounts();
  const accountIds =
    node.accountId !== undefined
      ? [node.accountId]
      : (accounts.data ?? []).filter((a) => a.kind !== "virtual").map((a) => a.id);
  const folders = useQueries({
    queries: accountIds.map((accountId) => ({
      queryKey: queryKeys.folders(accountId),
      queryFn: async (): Promise<Folder[]> =>
        (await client.request({ type: "ListFolders", account_id: accountId }, "Folders")).folders,
    })),
  });
  const nameOf = (id: string) => accounts.data?.find((a) => a.id === id)?.name ?? id;
  return (
    <Field
      plugin={plugin}
      id={node.id}
      label={node.label}
      issue={issue}
      control={(a11y) => (
        <select
          {...a11y}
          {...partAttrs("plugin.folder-picker", plugin, node.id)}
          aria-required={node.required ? true : undefined}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => {
            onChange(event.target.value === "" ? null : event.target.value);
          }}
        >
          <option value="">{t("plugin-folder-none")}</option>
          {accountIds.map((accountId, index) => (
            <optgroup key={accountId} label={nameOf(accountId)}>
              {flattenFolders(folders[index]?.data ?? []).map(({ folder, depth }) => (
                <option key={folder.id} value={folder.id} data-depth={depth}>
                  {indented(folder.name, depth)}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      )}
    />
  );
}

function MessageListNode({
  plugin,
  node,
  value,
  onChange,
}: {
  plugin: string;
  node: Extract<PluginUiNode, { type: "message-list" }>;
  value: JsonValue | undefined;
  onChange: (value: JsonValue) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const name = useId();
  const rows: PluginMessage[] = node.messages ?? [];
  const selected = Array.isArray(value) ? value.filter((v) => typeof v === "string") : [];
  const selectable = node.selectable === true && node.id !== undefined;
  const toggle = (id: string, on: boolean) => {
    if (!node.multiple) onChange(on ? [id] : []);
    else onChange(on ? [...selected, id] : selected.filter((s) => s !== id));
  };
  const date = (iso: string) => formatDateTime(iso, locale, DATE_ONLY);
  return (
    <ul
      {...partAttrs("plugin.message-list", plugin, node.id)}
      aria-label={t("plugin-message-list-label", { count: rows.length })}
    >
      {rows.map((message) => {
        const content = (
          <>
            <span {...partAttrs("plugin.message-list.subject", plugin)}>
              {message.subject === "" ? t("plugin-message-no-subject") : message.subject}
            </span>
            <span {...partAttrs("plugin.message-list.from", plugin)}>
              {message.from ? formatAddress(message.from) : null}
            </span>
            <span {...partAttrs("plugin.message-list.date", plugin)}>{date(message.date)}</span>
          </>
        );
        const on = selected.includes(message.id);
        return (
          <li
            key={message.id}
            {...partAttrs("plugin.message-list.row", plugin)}
            data-state={on ? "selected" : undefined}
            data-unread={message.flags.seen ? undefined : ""}
          >
            {selectable ? (
              <label>
                <input
                  {...partAttrs("plugin.message-list.choice", plugin)}
                  type={node.multiple ? "checkbox" : "radio"}
                  name={name}
                  checked={on}
                  onChange={(event) => {
                    toggle(message.id, event.target.checked);
                  }}
                />
                {content}
              </label>
            ) : (
              content
            )}
          </li>
        );
      })}
    </ul>
  );
}

function TableNode({
  plugin,
  node,
  value,
  onChange,
}: {
  plugin: string;
  node: Extract<PluginUiNode, { type: "table" }>;
  value: JsonValue | undefined;
  onChange: (value: JsonValue) => void;
}) {
  const t = useT();
  const name = useId();
  const selectable = node.selectable === true && node.id !== undefined;
  const selected = Array.isArray(value) ? value.filter((v) => typeof v === "number") : [];
  const toggle = (row: number, on: boolean) => {
    if (!node.multiple) onChange(on ? [row] : []);
    else
      onChange(on ? [...selected, row].sort((a, b) => a - b) : selected.filter((s) => s !== row));
  };
  return (
    <table {...partAttrs("plugin.table", plugin, node.id)} aria-label={t("plugin-table-label")}>
      <thead>
        <tr {...partAttrs("plugin.table.row", plugin)}>
          {selectable && (
            <th {...partAttrs("plugin.table.header", plugin)} scope="col">
              {t("plugin-table-select")}
            </th>
          )}
          {node.columns.map((column, index) => (
            <th key={index} {...partAttrs("plugin.table.header", plugin)} scope="col">
              {column.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {node.rows.map((row, rowIndex) => {
          const on = selected.includes(rowIndex);
          return (
            <tr
              key={rowIndex}
              {...partAttrs("plugin.table.row", plugin)}
              data-state={on ? "selected" : undefined}
            >
              {selectable && (
                <td {...partAttrs("plugin.table.cell", plugin)}>
                  <input
                    {...partAttrs("plugin.table.choice", plugin)}
                    type={node.multiple ? "checkbox" : "radio"}
                    name={name}
                    aria-label={t("plugin-table-select-row", { row: rowIndex + 1 })}
                    checked={on}
                    onChange={(event) => {
                      toggle(rowIndex, event.target.checked);
                    }}
                  />
                </td>
              )}
              {row.map((cell, cellIndex) => (
                <td key={cellIndex} {...partAttrs("plugin.table.cell", plugin)}>
                  {cell}
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function NodeView(props: TreeProps & { node: PluginUiNode }) {
  const { plugin, node, values, onChange, issues, onAction, busy } = props;
  const t = useT();
  switch (node.type) {
    case "stack":
      return (
        <div {...partAttrs("plugin.stack", plugin)} data-direction={node.direction ?? "vertical"}>
          {node.children.map((child, index) => (
            <NodeView key={index} {...props} node={child} />
          ))}
        </div>
      );
    case "text":
      return node.style === "heading" ? (
        <h3 {...partAttrs("plugin.text", plugin)} data-appearance="heading">
          {node.value}
        </h3>
      ) : (
        <p {...partAttrs("plugin.text", plugin)} data-appearance={node.style ?? "body"}>
          {node.value}
        </p>
      );
    case "markdown":
      return <Markdown plugin={plugin} value={node.value} />;
    case "input": {
      const value = values[node.id];
      return (
        <Field
          plugin={plugin}
          id={node.id}
          label={node.label}
          issue={issues[node.id]}
          control={(a11y) => (
            <input
              {...a11y}
              {...partAttrs("plugin.input", plugin, node.id)}
              type={node.kind === "number" ? "text" : (node.kind ?? "text")}
              inputMode={node.kind === "number" ? "decimal" : undefined}
              aria-required={node.required ? true : undefined}
              maxLength={node.maxLength}
              placeholder={node.placeholder}
              value={typeof value === "string" ? value : ""}
              onChange={(event) => {
                onChange(node.id, event.target.value);
              }}
            />
          )}
        />
      );
    }
    case "textarea": {
      const value = values[node.id];
      return (
        <Field
          plugin={plugin}
          id={node.id}
          label={node.label}
          issue={issues[node.id]}
          control={(a11y) => (
            <textarea
              {...a11y}
              {...partAttrs("plugin.textarea", plugin, node.id)}
              aria-required={node.required ? true : undefined}
              rows={node.rows}
              maxLength={node.maxLength}
              placeholder={node.placeholder}
              value={typeof value === "string" ? value : ""}
              onChange={(event) => {
                onChange(node.id, event.target.value);
              }}
            />
          )}
        />
      );
    }
    case "select": {
      const value = values[node.id];
      return (
        <Field
          plugin={plugin}
          id={node.id}
          label={node.label}
          issue={issues[node.id]}
          control={(a11y) =>
            node.multiple ? (
              <select
                {...a11y}
                {...partAttrs("plugin.select", plugin, node.id)}
                multiple
                aria-required={node.required ? true : undefined}
                value={Array.isArray(value) ? value.filter((v) => typeof v === "string") : []}
                onChange={(event) => {
                  onChange(
                    node.id,
                    [...event.target.selectedOptions].map((o) => o.value),
                  );
                }}
              >
                {node.options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            ) : (
              <select
                {...a11y}
                {...partAttrs("plugin.select", plugin, node.id)}
                aria-required={node.required ? true : undefined}
                value={typeof value === "string" ? value : ""}
                onChange={(event) => {
                  onChange(node.id, event.target.value);
                }}
              >
                {(node.default === undefined || node.default.length === 0) && (
                  <option value="">{t("plugin-select-none")}</option>
                )}
                {node.options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            )
          }
        />
      );
    }
    case "checkbox": {
      const value = values[node.id];
      return (
        <div {...partAttrs("plugin.field", plugin, node.id)}>
          <label {...partAttrs("plugin.field.label", plugin)}>
            <input
              {...partAttrs("plugin.checkbox", plugin, node.id)}
              type="checkbox"
              checked={value === true}
              onChange={(event) => {
                onChange(node.id, event.target.checked);
              }}
            />
            {node.label}
          </label>
        </div>
      );
    }
    case "date": {
      const value = values[node.id];
      return (
        <Field
          plugin={plugin}
          id={node.id}
          label={node.label}
          issue={issues[node.id]}
          control={(a11y) => (
            <input
              {...a11y}
              {...partAttrs("plugin.date", plugin, node.id)}
              type={node.time ? "datetime-local" : "date"}
              aria-required={node.required ? true : undefined}
              min={node.min === undefined ? undefined : node.time ? `${node.min}T00:00` : node.min}
              max={node.max === undefined ? undefined : node.time ? `${node.max}T23:59` : node.max}
              value={typeof value === "string" ? value : ""}
              onChange={(event) => {
                onChange(node.id, event.target.value === "" ? null : event.target.value);
              }}
            />
          )}
        />
      );
    }
    case "contact-picker":
      return (
        <ContactPicker
          plugin={plugin}
          node={node}
          value={values[node.id]}
          issue={issues[node.id]}
          onChange={(value) => {
            onChange(node.id, value);
          }}
        />
      );
    case "folder-picker":
      return (
        <FolderPickerNode
          plugin={plugin}
          node={node}
          value={values[node.id]}
          issue={issues[node.id]}
          onChange={(value) => {
            onChange(node.id, value);
          }}
        />
      );
    case "message-list":
      return (
        <MessageListNode
          plugin={plugin}
          node={node}
          value={node.id === undefined ? undefined : values[node.id]}
          onChange={(value) => {
            if (node.id !== undefined) onChange(node.id, value);
          }}
        />
      );
    case "table":
      return (
        <TableNode
          plugin={plugin}
          node={node}
          value={node.id === undefined ? undefined : values[node.id]}
          onChange={(value) => {
            if (node.id !== undefined) onChange(node.id, value);
          }}
        />
      );
    case "button":
      return (
        <button
          type="button"
          {...partAttrs("plugin.button", plugin, node.id)}
          data-variant={node.variant ?? "default"}
          disabled={node.disabled === true || busy === true}
          onClick={() => {
            onAction(node.id);
          }}
        >
          <Icon name={node.icon} part={"plugin.button.icon" satisfies PluginPart} />
          <span {...partAttrs("plugin.button.label", plugin)}>{node.label}</span>
        </button>
      );
    case "progress": {
      const indeterminate = node.value === undefined;
      return (
        <div
          {...partAttrs("plugin.progress", plugin)}
          data-state={indeterminate ? "indeterminate" : undefined}
        >
          {node.label !== undefined && (
            <span {...partAttrs("plugin.progress.label", plugin)}>{node.label}</span>
          )}
          <progress
            max={100}
            value={indeterminate ? undefined : Math.min(Math.max(node.value ?? 0, 0), 100)}
            aria-label={node.label ?? t("plugin-progress-label")}
          />
        </div>
      );
    }
    case "image":
      return isSafeImage(node.src) ? (
        <img
          {...partAttrs("plugin.image", plugin)}
          src={node.src}
          alt={node.alt}
          width={node.width}
          height={node.height}
        />
      ) : null;
  }
}

/** A plug-in UI tree. */
export function PluginTree(props: TreeProps) {
  return (
    <div {...partAttrs("plugin.tree", props.plugin)}>
      {props.nodes.map((node, index) => (
        <NodeView key={index} {...props} node={node} />
      ))}
    </div>
  );
}
