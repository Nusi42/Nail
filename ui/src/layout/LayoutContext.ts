import { createContext, useContext } from "react";
import type { ResolvedLayout } from "../api/generated";

export const LayoutContext = createContext<ResolvedLayout | null>(null);

/** The resolved layout (actions, toolbars, menus) for widgets. */
export function useLayoutConfig(): ResolvedLayout {
  const layout = useContext(LayoutContext);
  if (!layout) {
    throw new Error("useLayoutConfig() must be used inside <LayoutContext.Provider>");
  }
  return layout;
}
