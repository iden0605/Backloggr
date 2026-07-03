import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  Library,
  Search,
  Sparkles,
  Clapperboard,
  Settings,
  Gamepad2,
} from "lucide-react";

const navItems = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/backlog", label: "Backlog", icon: Library },
  { to: "/search", label: "Search", icon: Search },
  { to: "/recommendations", label: "Recommendations", icon: Sparkles },
  { to: "/clips", label: "Clips", icon: Clapperboard },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function Sidebar() {
  return (
    <aside className="flex h-screen w-60 shrink-0 flex-col border-r border-border bg-surface">
      <div className="flex items-center gap-2.5 px-5 py-6">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent/10 text-accent">
          <Gamepad2 className="h-4.5 w-4.5" />
        </div>
        <span className="page-title text-[17px]">
          Backloggr
        </span>
      </div>

      <nav className="flex flex-1 flex-col gap-0.5 px-3">
        {navItems.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              `group relative flex items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] font-medium transition-all duration-150 ${
                isActive
                  ? "bg-accent/10 text-accent"
                  : "text-text-lo hover:bg-surface-alt hover:text-text-hi"
              }`
            }
          >
            {({ isActive }) => (
              <>
                <span
                  className={`absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-accent transition-opacity duration-150 ${
                    isActive ? "opacity-100" : "opacity-0"
                  }`}
                />
                <Icon className="h-4 w-4 shrink-0" strokeWidth={2} />
                {label}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      <div className="border-t border-border px-5 py-4">
        <p className="font-mono text-[10px] uppercase tracking-wider text-text-lo/70">
          local-first · v0.1
        </p>
      </div>
    </aside>
  );
}
