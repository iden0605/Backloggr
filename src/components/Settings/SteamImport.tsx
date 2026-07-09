import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Search, X } from "lucide-react";

interface SteamLibraryGame {
  appId: number;
  name: string;
  playtimeMinutes: number;
  inBacklog: boolean;
}

interface SteamLibrary {
  steamId: string;
  games: SteamLibraryGame[];
}

interface SteamImportProgress {
  done: number;
  total: number;
}

interface SteamImportSummary {
  imported: number;
  linked: number;
  skipped: number;
}

function formatPlaytime(minutes: number): string {
  if (minutes <= 0) return "Never played";
  if (minutes < 60) return `${minutes} min`;
  return `${(minutes / 60).toFixed(minutes < 600 ? 1 : 0)} hrs`;
}

/** Settings card: paste a Steam profile → review the owned games → import to the library. */
export function SteamImportSection() {
  const [profile, setProfile] = useState("");
  const [fetching, setFetching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [library, setLibrary] = useState<SteamLibrary | null>(null);

  useEffect(() => {
    invoke<string | null>("get_steam_profile")
      .then((saved) => saved && setProfile(saved))
      .catch(() => {});
  }, []);

  async function fetchLibrary() {
    if (!profile.trim() || fetching) return;
    setFetching(true);
    setError(null);
    try {
      const lib = await invoke<SteamLibrary>("fetch_steam_library", { profile });
      setLibrary(lib);
    } catch (err) {
      setError(String(err));
    } finally {
      setFetching(false);
    }
  }

  return (
    <div className="mt-5 max-w-xl rounded-xl border border-border bg-surface p-5">
      <h2 className="shelf-label">Steam</h2>
      <p className="mt-3 text-[13.5px] font-medium text-text-hi">Import your Steam library</p>
      <p className="mt-0.5 text-xs text-text-lo">
        Pulls your entire owned library — including games you've never launched — and adds the
        ones you pick to your library. Your profile's "Game details" must be set to Public.
      </p>
      <div className="mt-3 flex gap-2">
        <input
          type="text"
          value={profile}
          onChange={(e) => setProfile(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && fetchLibrary()}
          placeholder="Profile URL, vanity name, or steamID64"
          className="min-w-0 flex-1 rounded-md border border-border bg-surface-alt px-3 py-1.5 text-[13px] text-text-hi outline-none transition-colors placeholder:text-text-lo/60 focus:border-accent/40"
        />
        <button
          onClick={fetchLibrary}
          disabled={fetching || !profile.trim()}
          className="shrink-0 rounded-md bg-text-hi px-3.5 py-1.5 text-[13px] font-medium text-bg transition-all duration-150 hover:opacity-85 enabled:active:scale-[0.98] disabled:opacity-40"
        >
          {fetching ? "Fetching…" : "Fetch library"}
        </button>
      </div>
      {error && <p className="mt-2 text-xs text-danger">{error}</p>}

      {library && (
        <SteamImportModal library={library} onClose={() => setLibrary(null)} />
      )}
    </div>
  );
}

function SteamImportModal({
  library,
  onClose,
}: {
  library: SteamLibrary;
  onClose: () => void;
}) {
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(library.games.filter((g) => !g.inBacklog).map((g) => g.appId))
  );
  const [filter, setFilter] = useState("");
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<SteamImportProgress | null>(null);
  const [summary, setSummary] = useState<SteamImportSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The import keeps running server-side once started — don't let a stray backdrop click
  // tear down the progress UI mid-run.
  const busyRef = useRef(false);
  busyRef.current = importing;

  useEffect(() => {
    const unlisten = listen<SteamImportProgress>("steam-import-progress", (event) => {
      setProgress(event.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const importable = useMemo(
    () => library.games.filter((g) => !g.inBacklog),
    [library.games]
  );
  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return library.games;
    return library.games.filter((g) => g.name.toLowerCase().includes(q));
  }, [library.games, filter]);

  function toggle(appId: number) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(appId)) next.delete(appId);
      else next.add(appId);
      return next;
    });
  }

  async function runImport() {
    const picks = importable
      .filter((g) => selected.has(g.appId))
      .map((g) => ({ appId: g.appId, name: g.name }));
    if (picks.length === 0 || importing) return;
    setImporting(true);
    setError(null);
    setProgress({ done: 0, total: picks.length });
    try {
      const result = await invoke<SteamImportSummary>("import_steam_games", { games: picks });
      setSummary(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setImporting(false);
    }
  }

  const pct = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex animate-fade-in items-center justify-center bg-bg/80 p-4 backdrop-blur-sm"
      onClick={() => !busyRef.current && onClose()}
    >
      <div
        className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-border bg-surface"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-5 py-4">
          <div>
            <h2 className="page-title text-[17px]">Import from Steam</h2>
            <p className="mt-0.5 text-xs text-text-lo">
              {library.games.length} games found · {importable.length} not in your library yet
            </p>
          </div>
          <button
            onClick={onClose}
            disabled={importing}
            className="rounded-md p-1.5 text-text-lo transition-colors hover:text-text-hi disabled:opacity-40"
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>

        {summary ? (
          <div className="flex flex-col items-center gap-3 px-5 py-10 text-center">
            <span className="flex h-10 w-10 items-center justify-center rounded-full bg-success/15 text-success">
              <Check size={20} />
            </span>
            <p className="text-[13.5px] font-medium text-text-hi">
              Imported {summary.imported} game{summary.imported === 1 ? "" : "s"}
            </p>
            <p className="text-xs text-text-lo">
              {summary.linked > 0 && `${summary.linked} linked to existing library entries. `}
              {summary.skipped > 0 && `${summary.skipped} already imported. `}
              Covers and genres came from RAWG where a match was found.
            </p>
            <button
              onClick={onClose}
              className="mt-2 rounded-md bg-text-hi px-4 py-1.5 text-[13px] font-medium text-bg transition-all duration-150 hover:opacity-85 active:scale-[0.98]"
            >
              Done
            </button>
          </div>
        ) : importing ? (
          <div className="flex flex-col gap-3 px-5 py-10">
            <p className="text-center text-[13.5px] text-text-hi">
              Importing {progress?.done ?? 0} / {progress?.total ?? 0}…
            </p>
            <div className="h-1.5 overflow-hidden rounded-full bg-surface-alt">
              <div
                className="h-full rounded-full bg-text-hi transition-all duration-300"
                style={{ width: `${pct}%` }}
              />
            </div>
            <p className="text-center text-xs text-text-lo">
              Looking up covers and genres — this can take a moment for large libraries.
            </p>
          </div>
        ) : (
          <>
            <div className="flex items-center gap-2 border-b border-border px-5 py-3">
              <div className="relative min-w-0 flex-1">
                <Search
                  size={14}
                  className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-text-lo/60"
                />
                <input
                  type="text"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder="Filter games"
                  className="w-full rounded-md border border-border bg-surface-alt py-1.5 pl-8 pr-3 text-[13px] text-text-hi outline-none transition-colors placeholder:text-text-lo/60 focus:border-accent/40"
                />
              </div>
              <button
                onClick={() => setSelected(new Set(importable.map((g) => g.appId)))}
                className="shrink-0 text-xs text-text-lo transition-colors hover:text-text-hi"
              >
                Select all
              </button>
              <span className="text-text-lo/40">·</span>
              <button
                onClick={() => setSelected(new Set())}
                className="shrink-0 text-xs text-text-lo transition-colors hover:text-text-hi"
              >
                Clear
              </button>
            </div>

            <ul className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
              {visible.map((game) => (
                <li key={game.appId}>
                  <button
                    onClick={() => !game.inBacklog && toggle(game.appId)}
                    disabled={game.inBacklog}
                    className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors ${
                      game.inBacklog ? "opacity-45" : "hover:bg-surface-alt"
                    }`}
                  >
                    <span
                      className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors ${
                        !game.inBacklog && selected.has(game.appId)
                          ? "border-text-hi bg-text-hi text-bg"
                          : "border-border-strong bg-transparent"
                      }`}
                    >
                      {!game.inBacklog && selected.has(game.appId) && <Check size={11} />}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[13px] text-text-hi">
                      {game.name}
                    </span>
                    {game.inBacklog ? (
                      <span className="shrink-0 text-[11px] text-text-lo">In library</span>
                    ) : (
                      <span className="shrink-0 font-mono text-[11px] text-text-lo">
                        {formatPlaytime(game.playtimeMinutes)}
                      </span>
                    )}
                  </button>
                </li>
              ))}
              {visible.length === 0 && (
                <li className="px-3 py-6 text-center text-xs text-text-lo">
                  No games match "{filter}".
                </li>
              )}
            </ul>

            <div className="flex items-center justify-between gap-3 border-t border-border px-5 py-3.5">
              {error ? (
                <p className="min-w-0 flex-1 truncate text-xs text-danger">{error}</p>
              ) : (
                <p className="text-xs text-text-lo">
                  {selected.size} of {importable.length} selected
                </p>
              )}
              <button
                onClick={runImport}
                disabled={selected.size === 0}
                className="shrink-0 rounded-md bg-text-hi px-4 py-1.5 text-[13px] font-medium text-bg transition-all duration-150 hover:opacity-85 enabled:active:scale-[0.98] disabled:opacity-40"
              >
                Import {selected.size} game{selected.size === 1 ? "" : "s"}
              </button>
            </div>
          </>
        )}
      </div>
    </div>,
    document.body
  );
}
