/**
 * Small shared controls of the M5a look (WP153). Like every component they
 * only emit `data-part`s; the look is the skin's (`theme/skin.ts`) and any
 * theme's. Texts are passed in already localized.
 */
import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { colorSlot } from "../theme/skin";
import { Icon } from "./Icon";
import { chars } from "../text/chars";
import { nonEmpty } from "../text/empty";

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Localized label (also the accessible name). */
  label: string;
  /** Localized hint under the label. */
  hint?: string | undefined;
  disabled?: boolean;
  /** `data-field` of the row (tests, theme rules). */
  name?: string;
}

/** A labelled on / off switch (`role="switch"`). */
export function Switch({ checked, onChange, label, hint, disabled = false, name }: SwitchProps) {
  const id = useId();
  return (
    <div data-part="switch-field" data-field={name}>
      <span data-part="switch-field.text">
        <span data-part="switch-field.label" id={`${id}-label`}>
          {label}
        </span>
        {hint && (
          <span data-part="switch-field.hint" id={`${id}-hint`}>
            {hint}
          </span>
        )}
      </span>
      <button
        type="button"
        role="switch"
        data-part="switch"
        aria-checked={checked}
        aria-labelledby={`${id}-label`}
        aria-describedby={hint ? `${id}-hint` : undefined}
        disabled={disabled}
        onClick={() => {
          onChange(!checked);
        }}
      >
        <span data-part="switch.thumb" aria-hidden="true" />
      </button>
    </div>
  );
}

export interface SegmentedOption {
  value: string;
  /** Localized label. */
  label: string;
}

export interface SegmentedProps {
  value: string;
  options: readonly SegmentedOption[];
  onChange: (value: string) => void;
  /** Localized name of the group. */
  label: string;
  name?: string;
  disabled?: boolean;
}

/**
 * A segmented control: a radio group of buttons. Arrow keys move and select
 * (roving tab index), like native radios.
 */
export function Segmented({ value, options, onChange, label, name, disabled }: SegmentedProps) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  );
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = (index + step + options.length) % options.length;
    const option = options[next];
    if (!option) return;
    onChange(option.value);
    refs.current[next]?.focus();
  };
  return (
    <div data-part="segmented" data-field={name} role="radiogroup" aria-label={label}>
      {options.map((option, index) => {
        const selected = index === current;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            role="radio"
            data-part="segmented.option"
            data-value={option.value}
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            disabled={disabled}
            onClick={() => {
              onChange(option.value);
            }}
            onKeyDown={(event) => {
              onKeyDown(event, index);
            }}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

export interface TagProps {
  /** Localized or data text. */
  text: string;
  /** Lucide icon name. */
  icon?: string;
  /** Palette slot of a colour dot (account colour). */
  dot?: number;
  tone?: "warning" | "danger" | "accent";
  /** `data-kind` (what the tag says: account, attachments, …). */
  kind?: string;
  /** Accessible text when the visible text is not enough. */
  title?: string;
}

/** A small tag (account, attachment count, invitation, warning, label). */
export function Tag({ text, icon, dot, tone, kind, title }: TagProps) {
  return (
    <span data-part="tag" data-kind={kind} data-tone={tone} title={title}>
      {dot !== undefined && <span data-part="tag.dot" data-color={dot} aria-hidden="true" />}
      {icon && <Icon name={icon} part="tag.icon" />}
      <span data-part="tag.text">{text}</span>
    </span>
  );
}

/** Up to two initials of a display name or address (`Ada Lovelace` → `AL`). */
export function initialsOf(name: string | null | undefined, email = ""): string {
  const source = nonEmpty((name ?? "").trim()) ?? email.split("@")[0] ?? "";
  const words = source
    .replace(/[<>"()[\]]/g, " ")
    .split(/[\s._-]+/)
    .filter((w) => w.length > 0 && /\p{L}|\p{N}/u.test(w));
  const letters = (w: string) => chars(w).find((c) => /\p{L}|\p{N}/u.test(c)) ?? "";
  if (words.length === 0) return "";
  const first = letters(words[0] ?? "");
  const last = words.length > 1 ? letters(words[words.length - 1] ?? "") : "";
  return (first + last).toLocaleUpperCase();
}

export interface AvatarProps {
  name: string | null | undefined;
  email: string;
  part?: string;
}

/** Initials on a stable colour per address (decorative: the name is shown next to it). */
export function Avatar({ name, email, part = "avatar" }: AvatarProps): ReactNode {
  return (
    <span data-part={part} data-color={colorSlot(nonEmpty(email) ?? name ?? "")} aria-hidden="true">
      {initialsOf(name, email)}
    </span>
  );
}
