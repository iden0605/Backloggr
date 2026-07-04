import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link } from "react-router-dom";
import { LibraryBig, X } from "lucide-react";
import { useAppStore, Game, GameStatus } from "../../store/useAppStore";

interface GameAutoAdded {
  gameId: number;
  name: string;
}

interface GameTotalPlaytime {
  gameId: number;
  totalSeconds: number;
}

const SECTIONS: { status: GameStatus; label: string }[] = [
  { status: "playing", label: "Playing" },
  { status: "backlog", label: "Backlog" },
  { status: "wishlist", label: "Wishlist" },
  { status: "completed", label: "Completed" },
  { status: "dropped", label: "Dropped" },
];

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}

export function Backlog() {
  const games = useAppStore((s) => s.games);
  const setGames = useAppStore((s) => s.setGames);
  const [playtimeByGame, setPlaytimeByGame] = useState<Map<number, number>>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [autoAddedNotice, setAutoAddedNotice] = useState<string | null>(null);

  async function loadGames() {
    try {
      const backlog = await invoke<Game[]>("get_backlog");
      setGames(backlog);
    } catch (err) {
      setError(String(err));
    }
  }

  async function loadPlaytime() {
    try {
      const totals = await invoke<GameTotalPlaytime[]>("get_playtime_totals");
      setPlaytimeByGame(new Map(totals.map((t) => [t.gameId, t.totalSeconds])));
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    loadGames();
    loadPlaytime();
  }, []);

  // The tracker auto-adds games launched from a known storefront, and now also flips a game's
  // status to "playing" the moment it's launched — refresh live on both events instead of
  // requiring a revisit to this tab.
  useEffect(() => {
    const unlistenAdded = listen<GameAutoAdded>("game-auto-added", (event) => {
      setAutoAddedNotice(`${event.payload.name} was automatically added from a running game.`);
      loadGames();
    });
    const unlistenStarted = listen("session-started", () => {
      loadGames();
      loadPlaytime();
    });
    const unlistenEnded = listen("session-ended", () => {
      loadGames();
      loadPlaytime();
    });
    return () => {
      unlistenAdded.then((f) => f());
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
    };
  }, []);

  async function changeStatus(id: number, status: GameStatus) {
    try {
      await invoke("update_game_status", { id, status });
      await loadGames();
    } catch (err) {
      setError(String(err));
    }
  }

  async function removeGame(id: number) {
    try {
      await invoke("delete_game", { id });
      await loadGames();
      await loadPlaytime();
    } catch (err) {
      setError(String(err));
    }
  }

  async function linkExe(id: number, exeName: string) {
    try {
      await invoke("set_game_exe_name", { id, exeName: exeName.trim() || null });
      await loadGames();
    } catch (err) {
      setError(String(err));
    }
  }

  const grouped = useMemo(() => {
    const map = new Map<GameStatus, Game[]>();
    for (const section of SECTIONS) map.set(section.status, []);
    for (const game of games) {
      map.get(game.status)?.push(game);
    }
    // Playing games surface the ones you're most invested in first; everything else keeps the
    // backend's most-recently-added-first order.
    map.get("playing")?.sort(
      (a, b) => (playtimeByGame.get(b.id) ?? 0) - (playtimeByGame.get(a.id) ?? 0),
    );
    return map;
  }, [games, playtimeByGame]);

  return (
    <div>
      <h1 className="page-title text-[26px]">Backlog</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Your game library, grouped by what you're doing with it.
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
        <div className="mt-8 flex flex-col items-start gap-3 rounded-xl border border-dashed border-border p-8">
          <LibraryBig className="h-8 w-8 text-text-lo/50" />
          <div>
            <p className="text-sm font-semibold text-text-hi">Your backlog is empty</p>
            <p className="mt-1 text-[13.5px] text-text-lo">
              Search for a game to add it, or just launch something you own — it'll show up here
              automatically if it's installed via Steam, Epic, GOG, Battle.net, or Riot.
            </p>
          </div>
          <Link
            to="/search"
            className="rounded-lg bg-accent px-3.5 py-2 text-xs font-semibold text-bg transition-colors hover:bg-accent-hover"
          >
            Search for a game
          </Link>
        </div>
      ) : (
        <div className="mt-7 space-y-9">
          {SECTIONS.map(({ status, label }) => {
            const sectionGames = grouped.get(status) ?? [];
            if (sectionGames.length === 0) return null;
            return (
              <div key={status}>
                <h2 className="font-mono text-[11px] font-medium uppercase tracking-wider text-text-lo">
                  {label} <span className="text-text-lo/50">({sectionGames.length})</span>
                </h2>
                <div className="mt-3 space-y-2">
                  {sectionGames.map((game) => (
                    <GameRow
                      key={game.id}
                      game={game}
                      totalSeconds={playtimeByGame.get(game.id) ?? 0}
                      onChangeStatus={changeStatus}
                      onRemove={removeGame}
                      onLinkExe={linkExe}
                    />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function GameRow({
  game,
  totalSeconds,
  onChangeStatus,
  onRemove,
  onLinkExe,
}: {
  game: Game;
  totalSeconds: number;
  onChangeStatus: (id: number, status: GameStatus) => void;
  onRemove: (id: number) => void;
  onLinkExe: (id: number, exeName: string) => void;
}) {
  return (
    <div className="group flex items-center gap-4 rounded-xl border border-border bg-surface p-3 transition-colors hover:border-border/80 hover:bg-surface-alt/40">
      {game.coverUrl ? (
        <img src={game.coverUrl} alt={game.name} className="h-14 w-14 rounded-lg object-cover" />
      ) : (
        <div className="h-14 w-14 rounded-lg bg-gradient-to-br from-surface-alt to-surface" />
      )}
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13.5px] font-semibold text-text-hi">{game.name}</p>
        <p className="mt-0.5 truncate text-xs text-text-lo">
          {[game.genre, game.platform].filter(Boolean).join(" · ")}
          {totalSeconds > 0 && (
            <span className="text-text-lo/70"> · {formatDuration(totalSeconds)} played</span>
          )}
        </p>
        <input
          type="text"
          defaultValue={game.exeName ?? ""}
          placeholder="link launch file for auto-tracking (e.g. DaveTheDiver.exe)"
          onBlur={(e) => {
            if (e.target.value !== (game.exeName ?? "")) {
              onLinkExe(game.id, e.target.value);
            }
          }}
          className="mt-1.5 w-72 max-w-full rounded-md border border-transparent bg-surface-alt px-2 py-1 font-mono text-[11px] text-text-lo outline-none transition-colors placeholder:text-text-lo/50 focus:border-accent/40 focus:text-text-hi"
        />
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <StatusActions game={game} onChangeStatus={onChangeStatus} />
        <button
          onClick={() => onRemove(game.id)}
          className="rounded-lg border border-border px-2.5 py-1.5 text-xs font-medium text-text-lo transition-colors hover:border-danger/40 hover:bg-danger/10 hover:text-danger"
        >
          Remove
        </button>
      </div>
    </div>
  );
}

function ActionButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="rounded-lg border border-border px-2.5 py-1.5 text-xs font-medium text-text-lo transition-colors hover:border-accent/40 hover:bg-accent/10 hover:text-accent"
    >
      {label}
    </button>
  );
}

function StatusActions({
  game,
  onChangeStatus,
}: {
  game: Game;
  onChangeStatus: (id: number, status: GameStatus) => void;
}) {
  const set = (status: GameStatus) => onChangeStatus(game.id, status);

  switch (game.status) {
    case "playing":
      return (
        <>
          <ActionButton label="Mark Completed" onClick={() => set("completed")} />
          <ActionButton label="Drop" onClick={() => set("dropped")} />
        </>
      );
    case "backlog":
      return (
        <>
          <ActionButton label="Start Playing" onClick={() => set("playing")} />
          <ActionButton label="Move to Wishlist" onClick={() => set("wishlist")} />
        </>
      );
    case "wishlist":
      return (
        <>
          <ActionButton label="Start Playing" onClick={() => set("playing")} />
          <ActionButton label="Move to Backlog" onClick={() => set("backlog")} />
        </>
      );
    case "completed":
    case "dropped":
      return <ActionButton label="Play Again" onClick={() => set("playing")} />;
  }
}
