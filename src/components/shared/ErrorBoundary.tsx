import { Component, ReactNode } from "react";
import { AlertTriangle } from "lucide-react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

// Catches render-time crashes in a single page so one broken view doesn't blank the whole
// app shell (sidebar/nav stay usable). Reset by remounting with a new `key` on route change —
// see Shell.tsx, which keys this by pathname.
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack: string }) {
    console.error("Render error caught by ErrorBoundary:", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex flex-col items-start gap-3 rounded-xl border border-danger/25 bg-danger/5 p-6">
          <div className="flex items-center gap-2 text-danger">
            <AlertTriangle className="h-5 w-5" />
            <p className="text-sm font-semibold">This page hit an unexpected error.</p>
          </div>
          <p className="max-w-md text-xs text-text-lo">{this.state.error.message}</p>
          <button
            onClick={() => window.location.reload()}
            className="rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-text-hi transition-all duration-150 hover:border-accent/40 hover:bg-accent/10 hover:text-accent active:scale-[0.98]"
          >
            Reload
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
