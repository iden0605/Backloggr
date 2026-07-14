import { useEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import { TopNav } from "./TopNav";
import { ErrorBoundary } from "../shared/ErrorBoundary";
import { QuickOpen } from "../shared/QuickOpen";
import { maybeUnlock } from "../shared/unlocks";
import { useAppStore } from "../../store/useAppStore";
import type { LibraryGame } from "../Library/Library";
import type { Clip } from "../Clips/Clips";

export function Shell() {
  const location = useLocation();
  const mainRef = useRef<HTMLElement>(null);
  const [scrolled, setScrolled] = useState(false);
  const unlockNotice = useAppStore((s) => s.unlockNotice);
  const setUnlockNotice = useAppStore((s) => s.setUnlockNotice);

  // Load the configured clip hotkey once — every page's hotkey copy reads it from the store.
  useEffect(() => {
    invoke<string>("get_clip_hotkey")
      .then((hotkey) => useAppStore.getState().setClipHotkey(hotkey))
      .catch(() => {});
  }, []);

  // First-of-kind unlock moments (first session ever / first clip) — Shell is the one
  // always-mounted component, so the listeners live here. Each fires at most once per
  // install; the checks confirm the event really is the first before showing the banner.
  useEffect(() => {
    const unlistenSession = listen("session-started", () => {
      void maybeUnlock("first-session", async () => {
        const games = await invoke<LibraryGame[]>("get_library");
        return games.reduce((sum, g) => sum + g.sessionCount, 0) <= 1;
      });
    });
    const unlistenClip = listen("clip-saved", () => {
      void maybeUnlock("first-clip", async () => {
        const clips = await invoke<Clip[]>("get_clips");
        return clips.length === 1;
      });
    });
    return () => {
      unlistenSession.then((f) => f());
      unlistenClip.then((f) => f());
    };
  }, []);

  // The unlock banner dismisses itself — it's a moment, not a persistent notice.
  useEffect(() => {
    if (!unlockNotice) return;
    const timer = setTimeout(() => setUnlockNotice(null), 7000);
    return () => clearTimeout(timer);
  }, [unlockNotice, setUnlockNotice]);

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
      {/* Barely-there ambient wash (same language as the chat page's radial) so the app
          isn't a flat near-black field: warm rust breathing at the top, a whisper of
          verdigris low-right. Static layer outside the scroll container — paints once,
          never re-rasters on scroll (WebView2-safe). */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_75%_60%_at_50%_-8%,rgba(185,106,85,0.2),transparent_68%),radial-gradient(ellipse_55%_45%_at_100%_105%,rgba(127,160,140,0.12),transparent_60%),linear-gradient(180deg,#211C19_0%,#181615_45%,#151414_100%)]"
      />
      <TopNav solid={scrolled || !isDashboard} />

      {/* First-ever unlock banner — quiet, success-tinted, self-dismissing. */}
      {unlockNotice && (
        <div className="pointer-events-none absolute inset-x-0 top-16 z-40 flex justify-center px-6">
          <div className="pointer-events-auto flex animate-fade-up items-center gap-3 rounded-xl border border-success/25 bg-surface px-4 py-2.5 shadow-[0_16px_40px_-12px_rgba(0,0,0,0.7)]">
            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-success" />
            <p className="text-[12.5px] font-medium text-text-hi">{unlockNotice}</p>
            <button
              onClick={() => setUnlockNotice(null)}
              className="rounded-full p-0.5 text-text-lo transition-colors hover:text-text-hi"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      )}

      <main
        ref={mainRef}
        onScroll={() => setScrolled((mainRef.current?.scrollTop ?? 0) > 16)}
        // `relative` lifts the content above the positioned ambient-wash layer behind it.
        className="relative flex-1 overflow-y-auto"
      >
        <div className={wrapperClass}>
          {/* Keyed by path so navigating away from a crashed page resets the boundary. */}
          <ErrorBoundary key={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </div>
      </main>

      <QuickOpen />
    </div>
  );
}
