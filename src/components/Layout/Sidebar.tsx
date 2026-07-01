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
    <aside className="flex h-screen w-56 flex-col border-r border-neutral-800 bg-neutral-950 text-neutral-200">
      <div className="flex items-center gap-2 px-4 py-4 text-lg font-semibold text-white">
        <Gamepad2 className="h-6 w-6 text-emerald-400" />
        Game Backlog
      </div>
      <nav className="flex flex-1 flex-col gap-1 px-2">
        {navItems.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              `flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors ${
                isActive
                  ? "bg-emerald-500/10 text-emerald-400"
                  : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-100"
              }`
            }
          >
            <Icon className="h-4 w-4" />
            {label}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
