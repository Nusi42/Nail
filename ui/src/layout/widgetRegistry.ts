/**
 * The widget registry: layout `widget` nodes are rendered by the component
 * registered for their `kind`. Plug-ins will register further kinds.
 */
import { createContext, useContext, type ComponentType } from "react";
import type { LayoutNode } from "../api/generated";

export type WidgetNode = Extract<LayoutNode, { type: "widget" }>;

export interface WidgetProps {
  /** The layout node (options, rows, label). */
  node: WidgetNode;
  /** Stable key of the node for UI state (`node.id` or its tree path). */
  nodeKey: string;
}

export type WidgetRegistry = Readonly<Record<string, ComponentType<WidgetProps>>>;

export const WidgetRegistryContext = createContext<WidgetRegistry>({});

export function useWidgetRegistry(): WidgetRegistry {
  return useContext(WidgetRegistryContext);
}

/** Typed option accessors (options are kebab-case keys as in KDL). */
export function optionBool(node: WidgetNode, key: string, fallback: boolean): boolean {
  const value = node.options[key];
  return typeof value === "boolean" ? value : fallback;
}

export function optionNumber(node: WidgetNode, key: string, fallback: number): number {
  const value = node.options[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function optionString(node: WidgetNode, key: string, fallback: string): string {
  const value = node.options[key];
  return typeof value === "string" ? value : fallback;
}

/** A whitespace-separated list option (`headers="from to cc date"`). */
export function optionList(node: WidgetNode, key: string, fallback: readonly string[]): string[] {
  const value = node.options[key];
  if (typeof value === "string") return value.split(/\s+/).filter(Boolean);
  if (Array.isArray(value)) return value.filter((v): v is string => typeof v === "string");
  return [...fallback];
}
