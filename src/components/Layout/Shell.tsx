import { useEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { TopNav } from "./TopNav";
import { ErrorBoundary } from "../shared/ErrorBoundary";
import { useAppStore } from "../../store/useAppStore";

export function Shell() {
  const location = useLocation();
  const mainRef = useRef<HTMLElement>(null);
  const [scrolled, setScrolled] = useState(false);

  // Load the configured clip hotkey once — every page's hotkey copy reads it from the store.
  useEffect(() => {
    invoke<string>("get_clip_hotkey")
      .then((hotkey) => useAppStore.getState().setClipHotkey(hotkey))
      .catch(() => {});
  }, []);

  // The scroll container is shared across routes — reset position (and the nav's
  // solid state) when navigating so pages never open mid-scroll.
  useEffect(() => {
    mainRef.current?.scrollTo(0, 0);
    setScrolled(false);
  }, [location.pathname]);

  // The For You set only regenerates when the user LEAVES the Discover section — hopping
  // Discover ↔ Shelby keeps the same grid (the store copy survives), while landing on any
  // other page drops it so the next Discover visit fetches fresh.
  useEffect(() => {
    if (!location.pathname.startsWith("/discover")) {
      const store = useAppStore.getState();
      if (store.forYouRecs) store.setForYouRecs(null);
    }
  }, [location.pathname]);

  // Dashboard renders its own full-bleed hero backdrop underneath the floating nav;
  // every other page gets a standard centered content column below it. The chat page
  // needs a viewport-height column (its transcript scrolls internally, composer pinned),
  // everything else flows naturally and scrolls in <main>.
  const isDashboard = location.pathname === "/";
  const isChatPage = location.pathname === "/discover/chat";
  const wrapperClass = isDashboard
    ? ""
    : isChatPage
      ? "mx-auto h-full w-full max-w-5xl px-8 pb-6 pt-24"
      : "mx-auto w-full max-w-5xl px-8 pb-12 pt-24";

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-bg text-text-hi">
      <TopNav solid={scrolled || !isDashboard} />
      <main
        ref={mainRef}
        onScroll={() => setScrolled((mainRef.current?.scrollTop ?? 0) > 16)}
        className="flex-1 overflow-y-auto"
      >
        <div className={wrapperClass}>
          {/* Keyed by path so navigating away from a crashed page resets the boundary. */}
          <ErrorBoundary key={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </div>
      </main>
    </div>
  );
}
