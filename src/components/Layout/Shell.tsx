import { Outlet } from "react-router-dom";
import { Sidebar } from "./Sidebar";

export function Shell() {
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-bg text-text-hi">
      <Sidebar />
      <main className="flex-1 overflow-y-auto p-8">
        <Outlet />
      </main>
    </div>
  );
}
