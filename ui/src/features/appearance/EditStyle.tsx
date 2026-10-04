/**
 * The inspector's "Edit style" (M5d WP183, design.md "Inspector"): opens
 * the theme editor on the active theme's Parts tab at the picked element's
 * part, with the rules that style it — or, for a built-in theme, with
 * "Duplicate as new theme" first. The inspector closes so the editor can
 * be used.
 */
import { useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { openThemeEditor } from "./editorStore";

/** The part an element is styled as (`data-part`), `null` when it has none. */
export function stylePartOf(element: Element): string | null {
  const part = element.getAttribute("data-part");
  return part === null || part === "" ? null : part;
}

export function EditStyleButton({
  element,
  onOpen,
}: {
  element: Element;
  /** Called after the editor opened (the inspector stops picking). */
  onOpen?: () => void;
}) {
  const t = useT();
  const ui = useUiStoreApi();
  const part = stylePartOf(element);
  if (part === null) return null;
  return (
    <button
      type="button"
      data-part="inspector.button"
      data-action="edit-style"
      onClick={() => {
        openThemeEditor(ui, null, part);
        onOpen?.();
      }}
    >
      <Icon name="palette" part="inspector.button.icon" />
      {t("appearance-edit-style")}
    </button>
  );
}
