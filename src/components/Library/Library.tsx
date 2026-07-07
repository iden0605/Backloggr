import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link, useNavigate } from "react-router-dom";
import { Check, LibraryBig, Search as SearchIcon, X } from "lucide-react";
import { EmptyState } from "../shared/EmptyState";
import { Select } from "../shared/Select";
import { Game } from "../../store/useAppStore";

/** `get_library` row: the game plus its session aggregates (Game fields are flattened in). */
export interface LibraryGame extends Game {
  totalSeconds: number;
  lastPlayedAt: string | null;
  sessionCount: number;
}

interface GameAutoAdded {
  gameId: number;
  name: string;
}

interface CurrentlyPlaying {
  gameId: number;
}

/**
 * Derived activity + the two manual marks. Status is no longer a lifecycle: 'backlog' just
 * means "in the library", playing/played/never-played come from tracking data, and
 * completed / dropped ("Not for me") are opt-in badges set from the detail page.
 */
type Filter = "all" | "playing" | "played" | "never" | "completed" | "notForMe";

type Sort = "lastPlayed" | "mostPlayed" | "name" | "added";

const SORTS: { value: Sort; label: string }[] = [
  { value: "lastPlayed", label: "Last played" },
  { value: "mostPlayed", label: "Most played" },
  { value: "name", label: "Name" },
  { value: "added", label: "Recently added" },
];

export function formatPlaytime(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}

// SQLite timestamps are UTC without a timezone marker — append "Z" or JS reads them as local.
export function parseUtc(sqliteTimestamp: string): Date {
  return new Date(sqliteTimestamp.replace(" ", "T") + "Z");
}

export function formatRelative(sqliteTimestamp: string): string {
  const days = Math.floor((Date.now() - parseUtc(sqliteTimestamp).getTime()) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 14) return `${days}d ago`;
  if (days < 60) return `${Math.floor(days / 7)}w ago`;
  if (days < 365) return `${Math.floor(days / 30)}mo ago`;
  return `${Math.floor(days / 365)}y ago`;
}

export function Library() {
  const navigate = useNavigate();
  const [games, setGames] = useState<LibraryGame[]>([]);
  const [playingId, setPlayingId] = useState<number | null>(null);
  const [tab, setTab] = useState<"library" | "wishlist">("library");
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>("lastPlayed");
  const [error, setError] = useState<string | null>(null);
  const [autoAddedNotice, setAutoAddedNotice] = useState<string | null>(null);

  async function load() {
    try {
      setGames(await invoke<LibraryGame[]>("get_library"));
      const playing = await invoke<CurrentlyPlaying | null>("get_currently_playing");
      setPlayingId(playing?.gameId ?? null);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    const unlistenAdded = listen<GameAutoAdded>("game-auto-added", (event) => {
      setAutoAddedNotice(`${event.payload.name} was automatically added from a running game.`);
      load();
    });
    const unlistenStarted = listen("session-started", load);
    const unlistenEnded = listen("session-ended", load);
    // One event for the whole Steam import batch (fired from Settings) — per-game notices
    // would spam hundreds of banners for a big library.
    const unlistenImported = listen("steam-import-done", load);
    return () => {
      unlistenAdded.then((f) => f());
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
      unlistenImported.then((f) => f());
    };
  }, []);

  const owned = useMemo(() => games.filter((g) => g.status !== "wishlist"), [games]);
  const wishlist = useMemo(() => games.filter((g) => g.status === "wishlist"), [games]);

  const counts = useMemo(
    () => ({
      all: owned.length,
      playing: playingId !== null && owned.some((g) => g.id === playingId) ? 1 : 0,
      played: owned.filter((g) => g.totalSeconds > 0).length,
      never: owned.filter((g) => g.totalSeconds === 0).length,
      completed: owned.filter((g) => g.status === "completed").length,
      notForMe: owned.filter((g) => g.status === "dropped").length,
    }),
    [owned, playingId],
  );

  const FILTERS: { value: Filter; label: string; live?: boolean }[] = [
    { value: "all", label: `All · ${counts.all}` },
    ...(counts.playing > 0 ? [{ value: "playing" as Filter, label: "Playing now", live: true }] : []),
    { value: "played", label: "Played" },
    { value: "never", label: "Never played" },
    ...(counts.completed > 0 ? [{ value: "completed" as Filter, label: "Completed" }] : []),
    ...(counts.notForMe > 0 ? [{ value: "notForMe" as Filter, label: "Not for me" }] : []),
  ];

  const visible = useMemo(() => {
    let list = tab === "wishlist" ? wishlist : owned;
    if (tab === "library") {
      switch (filter) {
        case "playing":
          list = list.filter((g) => g.id === playingId);
          break;
        case "played":
          list = list.filter((g) => g.totalSeconds > 0);
          break;
        case "never":
          list = list.filter((g) => g.totalSeconds === 0);
          break;
        case "completed":
          list = list.filter((g) => g.status === "completed");
          break;
        case "notForMe":
          list = list.filter((g) => g.status === "dropped");
          break;
      }
    }
    const q = query.trim().toLowerCase();
    if (q) list = list.filter((g) => g.name.toLowerCase().includes(q));

    const byLastPlayed = (g: LibraryGame) =>
      g.id === playingId ? Infinity : g.lastPlayedAt ? parseUtc(g.lastPlayedAt).getTime() : 0;
    return [...list].sort((a, b) => {
      switch (sort) {
        case "lastPlayed":
          return byLastPlayed(b) - byLastPlayed(a) || a.name.localeCompare(b.name);
        case "mostPlayed":
          return b.totalSeconds - a.totalSeconds || a.name.localeCompare(b.name);
        case "name":
          return a.name.localeCompare(b.name);
        case "added":
          return parseUtc(b.addedAt).getTime() - parseUtc(a.addedAt).getTime();
      }
    });
  }, [tab, owned, wishlist, filter, query, sort, playingId]);

  return (
    <div>
      <h1 className="page-title text-[26px]">Library</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Every game you own, with what you've actually played.
      </p>

      {autoAddedNotice && (
        <div className="mt-5 flex animate-fade-up items-center justify-between rounded-xl border border-success/25 bg-success/10 px-4 py-2.5 text-sm text-success">
          <span>{autoAddedNotice}</span>
          <button
            onClick={() => setAutoAddedNotice(null)}
            className="rounded-full p-0.5 text-success/80 transition-colors hover:bg-success/15 hover:text-success"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      {games.length === 0 ? (
        <div className="mt-8">
          <EmptyState
            icon={LibraryBig}
            title="Your library is empty"
            cta={
              <Link
                to="/discover"
                className="rounded-lg bg-text-hi px-3.5 py-2 text-xs font-semibold text-bg transition-all duration-150 hover:opacity-85 active:scale-[0.98]"
              >
                Find a game
              </Link>
            }
          >
            Search for a game to add it, launch something you own and it'll show up
            automatically, or import your whole Steam library from Settings.
          </EmptyState>
        </div>
      ) : (
        <>
          <div className="mt-6 flex flex-wrap items-center gap-2.5">
            <div className="flex rounded-[10px] border border-border bg-surface p-[3px]">
              {(["library", "wishlist"] as const).map((t) => (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className={`rounded-lg px-3.5 py-1.5 text-xs font-semibold capitalize transition-all duration-150 active:scale-[0.97] ${
                    tab === t ? "bg-surface-alt text-text-hi" : "text-text-lo hover:text-text-hi"
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>

            <div className="flex min-w-[210px] items-center gap-2 rounded-[10px] border border-border bg-surface px-3 py-2">
              <SearchIcon className="h-3.5 w-3.5 shrink-0 text-text-lo/60" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search your library…"
                className="w-full bg-transparent text-[13px] text-text-hi outline-none placeholder:text-text-lo/60"
              />
            </div>

            {tab === "library" &&
              FILTERS.map(({ value, label, live }) => (
                <button
                  key={value}
                  onClick={() => setFilter(value)}
                  className={`rounded-full border px-3 py-1.5 text-xs font-medium transition-all duration-150 active:scale-[0.97] ${
                    filter === value
                      ? "border-text-hi bg-text-hi font-semibold text-bg"
                      : "border-border-strong text-text-lo hover:border-text-hi hover:text-text-hi"
                  }`}
                >
                  {live && (
                    <span className="mr-1.5 inline-block h-1.5 w-1.5 animate-pulse-soft rounded-full bg-accent align-[1px]" />
                  )}
                  {label}
                </button>
              ))}

            <div className="ml-auto">
              <Select value={sort} options={SORTS} onChange={setSort} prefix="Sort:" />
            </div>
          </div>

          {visible.length === 0 ? (
            <p className="mt-10 text-center text-[13px] text-text-lo">
              {tab === "wishlist" && wishlist.length === 0
                ? "Nothing wishlisted — games you don't own yet can live here."
                : "No games match."}
            </p>
          ) : (
            <div className="mt-6 grid grid-cols-[repeat(auto-fill,minmax(138px,1fr))] gap-4">
              {visible.map((game) => (
                <LibraryCard
                  key={game.id}
                  game={game}
                  live={game.id === playingId}
                  onOpen={() => navigate(`/library/${game.id}`)}
                />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

function LibraryCard({
  game,
  live,
  onOpen,
}: {
  game: LibraryGame;
  live: boolean;
  onOpen: () => void;
}) {
  const notForMe = game.status === "dropped";
  const playLine =
    game.totalSeconds > 0 ? `${formatPlaytime(game.totalSeconds)} played` : "Never played";
  const subLine = live
    ? "playing now"
    : game.lastPlayedAt
      ? `last played ${formatRelative(game.lastPlayedAt)}`
      : `added ${formatRelative(game.addedAt)}`;

  return (
    <button
      onClick={onOpen}
      className="group relative aspect-[2/3] overflow-hidden rounded-xl border border-border text-left transition-all duration-200 hover:-translate-y-0.5 hover:border-border-strong"
    >
      {game.coverUrl ? (
        <img
          src={game.coverUrl}
          alt={game.name}
          className={`absolute inset-0 h-full w-full object-cover ${
            notForMe ? "brightness-75 grayscale-[0.55]" : ""
          }`}
        />
      ) : (
        <div className="absolute inset-0 bg-gradient-to-br from-surface-alt to-surface" />
      )}

      {/* Name stays readable over any art; the veil swaps in stats on hover. */}
      <div className="absolute inset-0 flex items-end bg-gradient-to-t from-bg/85 via-bg/20 to-transparent p-2.5 opacity-100 transition-opacity duration-150 group-hover:opacity-0">
        <p className="text-[13px] font-bold leading-tight [text-shadow:0_1px_8px_rgba(0,0,0,0.55)]">
          {game.name}
        </p>
      </div>
      <div className="absolute inset-0 flex flex-col justify-end bg-gradient-to-t from-bg/95 via-bg/40 to-transparent p-2.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100">
        <p className="truncate text-[12px] font-bold leading-tight">{game.name}</p>
        <p className="mt-1 font-mono text-[10.5px] text-text-hi/90">{playLine}</p>
        <p className="mt-0.5 truncate font-mono text-[10px] text-text-lo">{subLine}</p>
      </div>

      <div className="absolute left-2 top-2 flex gap-1.5">
        {live && (
          <span className="flex items-center gap-1.5 rounded-md bg-bg/75 px-1.5 py-1 font-mono text-[9px] uppercase tracking-[0.08em] text-text-hi backdrop-blur-sm">
            <span className="h-[5px] w-[5px] animate-pulse-soft rounded-full bg-accent" />
            Playing
          </span>
        )}
        {game.status === "completed" && (
          <span className="flex items-center gap-1 rounded-md bg-bg/75 px-1.5 py-1 font-mono text-[9px] uppercase tracking-[0.08em] text-success backdrop-blur-sm">
            <Check className="h-2.5 w-2.5" /> Completed
          </span>
        )}
        {notForMe && (
          <span className="rounded-md bg-bg/75 px-1.5 py-1 font-mono text-[9px] uppercase tracking-[0.08em] text-text-lo backdrop-blur-sm">
            Not for me
          </span>
        )}
      </div>
    </button>
  );
}
