import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { Search as SearchIcon, Sparkles } from "lucide-react";
import { CoverImage } from "./CoverImage";
import { useAppStore } from "../../store/useAppStore";
import { formatPlaytime, type LibraryGame } from "../Library/Library";

const MAX_RESULTS = 8;

/**
 * Global quick-open palette (Ctrl/⌘+K from anywhere, or TopNav's search button): jump
 * straight to any game in the library, or hand the typed text to Discover's RAWG search
 * when it isn't in the library yet. Rendered once by Shell; visibility lives in the store.
 */
export function QuickOpen() {
  const navigate = useNavigate();
  const visible = useAppStore((s) => s.quickOpenVisible);
  const setVisible = useAppStore((s) => s.setQuickOpenVisible);
  const [games, setGames] = useState<LibraryGame[]>([]);
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // The global shortcut lives here (not in Shell) so the palette is self-contained.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        useAppStore.getState().setQuickOpenVisible(!useAppStore.getState().quickOpenVisible);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // Fresh library snapshot per open — it's one cheap local read, and stale results after
  // an import/auto-add would be worse than the refetch.
  useEffect(() => {
    if (!visible) return;
    setQuery("");
    setHighlighted(0);
    invoke<LibraryGame[]>("get_library")
      .then(setGames)
      .catch(() => setGames([]));
    // The input mounts in the same commit — focus on the next frame.
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [visible]);

  if (!visible) return null;

  const q = query.trim().toLowerCase();
  const matches = (
    q ? games.filter((g) => g.name.toLowerCase().includes(q)) : [...games]
  )
    .sort((a, b) => b.totalSeconds - a.totalSeconds || a.name.localeCompare(b.name))
    .slice(0, MAX_RESULTS);
  // Last row: hand the query to Discover's RAWG search for games not in the library.
  const rowCount = matches.length + (q ? 1 : 0);

  function close() {
    setVisible(false);
  }

  function openRow(index: number) {
    if (index < matches.length) {
      navigate(`/library/${matches[index].id}`);
    } else if (q) {
      navigate("/discover", { state: { search: query.trim() } });
    }
    close();
  }

  function onKeyDown(e: React.KeyboardEvent) {
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        close();
        break;
      case "ArrowDown":
        e.preventDefault();
        setHighlighted((h) => Math.min(h + 1, rowCount - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setHighlighted((h) => Math.max(h - 1, 0));
        break;
      case "Enter":
        e.preventDefault();
        if (rowCount > 0) openRow(Math.min(highlighted, rowCount - 1));
        break;
    }
  }

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex animate-fade-in items-start justify-center bg-bg/80 px-6 pt-[18vh]"
      onClick={close}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-lg animate-fade-up overflow-hidden rounded-2xl border border-border-strong bg-surface shadow-[0_24px_64px_-16px_rgba(0,0,0,0.8)]"
      >
        <div className="flex items-center gap-3 border-b border-border px-4 py-3">
          <SearchIcon className="h-4 w-4 shrink-0 text-text-lo" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setHighlighted(0);
            }}
            onKeyDown={onKeyDown}
            placeholder="Jump to a game…"
            spellCheck={false}
            className="w-full bg-transparent text-sm text-text-hi outline-none placeholder:text-text-lo/60"
          />
          <kbd className="kbd shrink-0">Esc</kbd>
        </div>

        <div className="max-h-80 overflow-y-auto py-1.5">
          {matches.length === 0 && !q && (
            <p className="px-4 py-3 text-[12.5px] text-text-lo">
              Your library is empty — type to search RAWG instead.
            </p>
          )}
          {matches.map((game, i) => (
            <button
              key={game.id}
              onClick={() => openRow(i)}
              onMouseEnter={() => setHighlighted(i)}
              className={`flex w-full items-center gap-3 px-3.5 py-2 text-left transition-colors ${
                i === highlighted ? "bg-surface-alt" : ""
              }`}
            >
              <CoverImage
                src={game.coverUrl}
                alt={game.name}
                className="h-10 w-7 shrink-0 rounded-md"
                resizeWidth={80}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-medium text-text-hi">
                  {game.name}
                </span>
                <span className="block font-mono text-[10.5px] text-text-lo">
                  {game.totalSeconds > 0 ? `${formatPlaytime(game.totalSeconds)} played` : "Never played"}
                </span>
              </span>
            </button>
          ))}
          {q && (
            <button
              onClick={() => openRow(matches.length)}
              onMouseEnter={() => setHighlighted(matches.length)}
              className={`flex w-full items-center gap-3 px-3.5 py-2.5 text-left transition-colors ${
                highlighted === matches.length ? "bg-surface-alt" : ""
              }`}
            >
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-alt">
                <Sparkles className="h-3.5 w-3.5 text-accent" />
              </span>
              <span className="text-[13px] text-text-hi">
                Search RAWG for &ldquo;<span className="font-semibold">{query.trim()}</span>&rdquo;
              </span>
            </button>
          )}
        </div>

        <p className="border-t border-border px-4 py-2 text-center font-mono text-[10px] uppercase tracking-wide text-text-lo/50">
          ↑↓ navigate · Enter open
        </p>
      </div>
    </div>,
    document.body,
  );
}
