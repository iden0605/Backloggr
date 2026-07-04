import { Outlet, useLocation } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { ErrorBoundary } from "../shared/ErrorBoundary";

export function Shell() {
  const location = useLocation();
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-bg text-text-hi">
      <Sidebar />
      <main className="flex-1 overflow-y-auto p-8">
        {/* Keyed by path so navigating away from a crashed page resets the boundary. */}
        <ErrorBoundary key={location.pathname}>
          <Outlet />
        </ErrorBoundary>
      </main>
    </div>
  );
}
