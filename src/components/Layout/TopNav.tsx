import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const navItems = [
  { to: "/", label: "Dashboard", end: true },
  { to: "/backlog", label: "Backlog" },
  { to: "/search", label: "Search" },
  { to: "/recommendations", label: "Discover" },
  { to: "/clips", label: "Clips" },
  { to: "/settings", label: "Settings" },
];

interface CurrentlyPlaying {
  gameId: number;
  name: string;
}

/**
 * Floating top navigation ("Backdrop" layout). Rendered over the Dashboard's hero backdrop,
 * so it starts transparent with a readability gradient and gains a solid blurred backing
 * once the page scrolls (`solid`) — the streaming-app pattern.
 */
export function TopNav({ solid }: { solid: boolean }) {
  const [nowPlaying, setNowPlaying] = useState<CurrentlyPlaying | null>(null);

  useEffect(() => {
    const load = () =>
      invoke<CurrentlyPlaying | null>("get_currently_playing")
        .then(setNowPlaying)
        .catch(() => setNowPlaying(null));
    load();
    const unlistenStarted = listen("session-started", load);
    const unlistenEnded = listen("session-ended", () => setNowPlaying(null));
    return () => {
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
    };
  }, []);

  return (
    <header
      className={`absolute inset-x-0 top-0 z-40 transition-colors duration-300 ${
        solid ? "border-b border-border bg-bg/85 backdrop-blur-md" : "border-b border-transparent"
      }`}
    >
      {/* Readability gradient for when the nav floats over hero art. */}
      {!solid && (
        <div className="pointer-events-none absolute inset-0 -bottom-6 bg-gradient-to-b from-bg/70 to-transparent" />
      )}

      <div className="relative flex h-14 items-center gap-8 px-7">
        <NavLink to="/" className="page-title select-none text-[15px] leading-none">
          Back<span className="text-accent">loggr</span>
        </NavLink>

        <nav className="flex h-full items-center gap-1">
          {navItems.map(({ to, label, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                `relative flex h-full items-center px-3 text-[13px] font-medium transition-colors duration-150 ${
                  isActive ? "text-text-hi" : "text-text-lo hover:text-text-hi"
                }`
              }
            >
              {({ isActive }) => (
                <>
                  {label}
                  <span
                    className={`absolute inset-x-3 bottom-0 h-0.5 rounded-full bg-accent transition-opacity duration-150 ${
                      isActive ? "opacity-100" : "opacity-0"
                    }`}
                  />
                </>
              )}
            </NavLink>
          ))}
        </nav>

        {nowPlaying && (
          <div className="ml-auto flex min-w-0 items-center gap-2.5">
            <span className="h-1.5 w-1.5 shrink-0 animate-pulse-soft rounded-full bg-accent" />
            <span className="truncate text-[12.5px] font-medium text-text-hi/90">
              {nowPlaying.name}
            </span>
          </div>
        )}
      </div>
    </header>
  );
}
