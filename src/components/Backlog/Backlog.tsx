import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
      <h1 className="text-2xl font-semibold">Backlog</h1>
      <p className="mt-2 text-neutral-400">Your game library, grouped by what you're doing with it.</p>

      {autoAddedNotice && (
        <div className="mt-4 flex items-center justify-between rounded-lg border border-emerald-800 bg-emerald-950/40 px-4 py-2 text-sm text-emerald-300">
          <span>{autoAddedNotice}</span>
          <button
            onClick={() => setAutoAddedNotice(null)}
            className="text-emerald-400 hover:text-emerald-200"
          >
            Dismiss
          </button>
        </div>
      )}

      {error && <p className="mt-4 text-red-400">{error}</p>}

      {games.length === 0 ? (
        <p className="mt-6 text-neutral-500">No games yet — add some from Search.</p>
      ) : (
        <div className="mt-6 space-y-8">
          {SECTIONS.map(({ status, label }) => {
            const sectionGames = grouped.get(status) ?? [];
            if (sectionGames.length === 0) return null;
            return (
              <div key={status}>
                <h2 className="text-sm font-semibold uppercase tracking-wide text-neutral-500">
                  {label} <span className="text-neutral-600">({sectionGames.length})</span>
                </h2>
                <div className="mt-2 space-y-2">
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
    <div className="flex items-center gap-3 rounded bg-neutral-800 p-3">
      {game.coverUrl ? (
        <img src={game.coverUrl} alt={game.name} className="h-14 w-14 rounded object-cover" />
      ) : (
        <div className="h-14 w-14 rounded bg-neutral-700" />
      )}
      <div className="flex-1">
        <p className="font-medium">{game.name}</p>
        <p className="text-xs text-neutral-400">
          {[game.genre, game.platform].filter(Boolean).join(" · ")}
          {totalSeconds > 0 && (
            <span className="text-neutral-500"> · {formatDuration(totalSeconds)} played</span>
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
          className="mt-1 w-72 max-w-full rounded bg-neutral-700 px-2 py-1 text-xs outline-none placeholder:text-neutral-500"
        />
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <StatusActions game={game} onChangeStatus={onChangeStatus} />
        <button
          onClick={() => onRemove(game.id)}
          className="rounded bg-neutral-700 px-2 py-1 text-sm hover:bg-red-900"
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
      className="rounded bg-neutral-700 px-2 py-1 text-sm hover:bg-neutral-600"
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
