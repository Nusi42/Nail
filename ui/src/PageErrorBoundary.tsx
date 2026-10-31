/**
 * The page's error boundary (ADR 0207): a component that fails to render
 * no longer unmounts the whole page (React's default, a white window). The
 * error goes to the core's `app.log` and the page shows the fallback (a
 * message and "Reload").
 */
import { Component, type ErrorInfo, type ReactNode } from "react";

export interface PageErrorBoundaryProps {
  /** What to show once a render failed. */
  fallback: ReactNode;
  /** Told about the error (reported to the log). */
  onError: (error: unknown, info: ErrorInfo) => void;
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class PageErrorBoundary extends Component<PageErrorBoundaryProps, State> {
  override state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo): void {
    this.props.onError(error, info);
  }

  override render(): ReactNode {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}
