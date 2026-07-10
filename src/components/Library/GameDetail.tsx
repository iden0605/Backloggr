import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ArrowLeft, Check, ChevronRight, Film, Play } from "lucide-react";
import { Clip, ClipPlayer } from "../Clips/Clips";
import { CoverImage } from "../shared/CoverImage";
import { RawgGameDetail } from "../shared/GameCard";
import { GameStatus } from "../../store/useAppStore";
import { formatPlaytime, formatRelative, LibraryGame } from "./Library";

interface GameStats {
  totalSeconds: number;
  lastPlayedAt: string | null;
  sessionCount: number;
  avgSessionSeconds: number;
  weeklySeconds: number[];
}

interface CurrentlyPlaying {
  gameId: number;
  startedAt: string;
}

function formatClipLength(seconds: number | null): string {
  if (!seconds) return "";
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function GameDetail() {
  const { id } = useParams();
  const gameId = Number(id);
  const navigate = useNavigate();

  const [game, setGame] = useState<LibraryGame | null>(null);
  const [stats, setStats] = useState<GameStats | null>(null);
  const [clips, setClips] = useState<Clip[]>([]);
  const [live, setLive] = useState<CurrentlyPlaying | null>(null);
  const [detail, setDetail] = useState<RawgGameDetail | null>(null);
  const [playingClip, setPlayingClip] = useState<Clip | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notFound, setNotFound] = useState(false);

  async function load() {
    try {
      // The library read is one cheap local query; picking this game out of it beats adding a
      // dedicated single-game command.
      const library = await invoke<LibraryGame[]>("get_library");
      const found = library.find((g) => g.id === gameId) ?? null;
      setGame(found);
      if (!found) {
        setNotFound(true);
        return;
      }
      setStats(await invoke<GameStats>("get_game_stats", { id: gameId }));
      setClips(await invoke<Clip[]>("get_clips", { gameId }));
      const playing = await invoke<CurrentlyPlaying | null>("get_currently_playing");
      setLive(playing?.gameId === gameId ? playing : null);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    load();
  }, [gameId]);

  useEffect(() => {
    const unlistenStarted = listen("session-started", load);
    const unlistenEnded = listen("session-ended", load);
    const unlistenClip = listen("clip-saved", load);
    return () => {
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
      unlistenClip.then((f) => f());
    };
  }, [gameId]);

  async function setStatus(status: GameStatus) {
    try {
      await invoke("update_game_status", { id: gameId, status });
      await load();
    } catch (err) {
      setError(String(err));
    }
  }

  async function linkExe(exeName: string) {
    try {
      await invoke("set_game_exe_name", { id: gameId, exeName: exeName.trim() || null });
      await load();
    } catch (err) {
      setError(String(err));
    }
  }

  async function remove() {
    try {
      await invoke("delete_game", { id: gameId });
      navigate("/library");
    } catch (err) {
      setError(String(err));
    }
  }

  // Lazy-load the RAWG detail the first time the About section is opened — same on-demand
  // pattern as GameCard's expand (the search endpoint never returns these fields).
  async function loadDetail() {
    if (detail || !game?.rawgId) return;
    try {
      setDetail(await invoke<RawgGameDetail>("get_game_details", { rawgId: game.rawgId }));
    } catch {
      // About stays empty — not worth an error banner.
    }
  }

  if (notFound) {
    return (
      <div className="pt-2">
        <BackLink />
        <p className="mt-8 text-[13.5px] text-text-lo">This game isn't in your library anymore.</p>
      </div>
    );
  }

  if (!game) {
    return <p className="pt-2 text-sm text-text-lo">{error ?? "Loading…"}</p>;
  }

  const completed = game.status === "completed";
  const notForMe = game.status === "dropped";
  const maxWeekly = stats ? Math.max(...stats.weeklySeconds, 1) : 1;

  const stateLine = live
    ? "Playing now"
    : stats?.lastPlayedAt
      ? `Last played ${formatRelative(stats.lastPlayedAt)}`
      : "Never played";

  return (
    <div>
      <BackLink />
      {error && <p className="mt-3 text-sm text-danger">{error}</p>}

      <div className="mt-4 overflow-hidden rounded-2xl border border-border bg-surface">
        {/* Blurred-cover hero — same visual language as the Dashboard's Backdrop. */}
        <div className="relative">
          <div className="absolute inset-0 overflow-hidden">
            {game.coverUrl ? (
              <img
                src={game.coverUrl}
                alt=""
                aria-hidden
                decoding="async"
                className="h-full w-full scale-110 object-cover blur-2xl brightness-[0.5] saturate-[0.9]"
              />
            ) : (
              <div className="h-full w-full bg-gradient-to-br from-surface-alt to-surface" />
            )}
            <div className="absolute inset-0 bg-gradient-to-b from-surface/0 via-surface/30 to-surface" />
          </div>

          <div className="relative flex flex-wrap items-end gap-6 px-7 pb-6 pt-16">
            <CoverImage
              src={game.coverUrl}
              alt={game.name}
              eager
              className="h-48 w-32 shrink-0 rounded-[10px] border border-border-strong shadow-[0_18px_40px_rgba(0,0,0,0.5)]"
            />

            <div className="min-w-0 flex-1">
              <h1 className="page-title truncate text-[28px]">{game.name}</h1>
              <p className="mt-2 truncate font-mono text-[11px] uppercase tracking-[0.05em] text-text-lo">
                {[game.genre, game.platform].filter(Boolean).join(" · ") || "No metadata"}
              </p>
              <span className="mt-3 inline-flex items-center gap-2 rounded-md border border-border-strong bg-bg/60 px-2.5 py-1.5 font-mono text-[10px] uppercase tracking-[0.1em] text-text-hi">
                {live && <span className="h-1.5 w-1.5 animate-pulse-soft rounded-full bg-accent" />}
                {stateLine}
              </span>
            </div>

            <div className="flex gap-2.5 pb-1">
              <button
                onClick={() => setStatus(completed ? "backlog" : "completed")}
                className={`flex items-center gap-1.5 rounded-[10px] border px-3.5 py-2 text-[12.5px] font-semibold transition-all duration-150 active:scale-[0.98] ${
                  completed
                    ? "border-success/40 text-success"
                    : "border-border-strong text-text-lo hover:border-text-hi hover:text-text-hi"
                }`}
              >
                <Check className="h-3.5 w-3.5" />
                {completed ? "Completed" : "Mark completed"}
              </button>
              <button
                onClick={() => setStatus(notForMe ? "backlog" : "dropped")}
                className={`rounded-[10px] border px-3.5 py-2 text-[12.5px] font-semibold transition-all duration-150 active:scale-[0.98] ${
                  notForMe
                    ? "border-border-strong bg-surface-alt text-text-hi"
                    : "border-border-strong text-text-lo hover:border-text-hi hover:text-text-hi"
                }`}
              >
                {notForMe ? "Not for me ✓" : "Not for me"}
              </button>
              {game.status === "wishlist" && (
                <button
                  onClick={() => setStatus("backlog")}
                  className="rounded-[10px] bg-text-hi px-3.5 py-2 text-[12.5px] font-semibold text-bg transition-all duration-150 hover:opacity-85 active:scale-[0.98]"
                >
                  Move to library
                </button>
              )}
            </div>
          </div>
        </div>

        <div className="px-7 pb-7 pt-6">
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <StatTile label="Total playtime" value={stats ? formatPlaytime(stats.totalSeconds) : "—"} />
            <StatTile
              label="Last played"
              value={
                live ? "Now" : stats?.lastPlayedAt ? formatRelative(stats.lastPlayedAt) : "Never"
              }
            />
            <StatTile label="Sessions" value={stats ? String(stats.sessionCount) : "—"} />
            <StatTile
              label="Avg session"
              value={
                stats && stats.avgSessionSeconds > 0 ? formatPlaytime(stats.avgSessionSeconds) : "—"
              }
            />
          </div>

          {stats && stats.totalSeconds > 0 && (
            <div className="mt-7">
              <h2 className="shelf-label">Playtime · last 8 weeks</h2>
              <div className="mt-3 flex h-16 items-end gap-1.5">
                {stats.weeklySeconds.map((seconds, i) => (
                  <div
                    key={i}
                    title={seconds > 0 ? formatPlaytime(seconds) : "No playtime"}
                    className="min-h-[3px] flex-1 rounded-t-[3px] bg-border-strong transition-colors hover:bg-accent"
                    style={{ height: `${Math.max(4, (seconds / maxWeekly) * 100)}%`, opacity: seconds > 0 ? 1 : 0.35 }}
                  />
                ))}
              </div>
              <div className="mt-2 flex justify-between font-mono text-[9.5px] uppercase text-text-lo/70">
                <span>8 weeks ago</span>
                <span>This week</span>
              </div>
            </div>
          )}

          <div className="mt-7">
            <h2 className="shelf-label">
              Clips {clips.length > 0 && <span className="text-text-lo/50">· {clips.length}</span>}
            </h2>
            {clips.length === 0 ? (
              <p className="mt-3 text-[13px] text-text-lo">
                No clips for this game yet — hit <kbd className="kbd">Alt+F9</kbd> while playing to
                save one.
              </p>
            ) : (
              <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
                {clips.map((clip) => (
                  <button
                    key={clip.id}
                    onClick={() => setPlayingClip(clip)}
                    className="group relative aspect-video overflow-hidden rounded-[10px] border border-border bg-surface-alt transition-colors hover:border-border-strong"
                  >
                    {clip.thumbnailPath ? (
                      <img
                        src={convertFileSrc(clip.thumbnailPath)}
                        alt={clip.title ?? "Clip thumbnail"}
                        loading="lazy"
                        decoding="async"
                        className="h-full w-full object-cover"
                      />
                    ) : (
                      <span className="flex h-full items-center justify-center">
                        <Film className="h-6 w-6 text-text-lo/50" />
                      </span>
                    )}
                    <span className="absolute inset-0 flex items-center justify-center bg-bg/0 opacity-0 transition-opacity group-hover:bg-bg/40 group-hover:opacity-100">
                      <span className="flex h-9 w-9 items-center justify-center rounded-full bg-text-hi">
                        <Play className="ml-0.5 h-3.5 w-3.5 text-bg" />
                      </span>
                    </span>
                    {clip.durationSeconds != null && (
                      <span className="absolute bottom-1.5 right-1.5 rounded bg-bg/80 px-1.5 py-0.5 font-mono text-[9.5px] text-text-hi">
                        {formatClipLength(clip.durationSeconds)}
                      </span>
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>

          {game.rawgId && (
            <details className="group mt-7 border-t border-border pt-5" onToggle={loadDetail}>
              <summary className="flex cursor-pointer list-none items-center gap-2 text-[12.5px] font-semibold text-text-lo transition-colors hover:text-text-hi group-open:text-text-hi [&::-webkit-details-marker]:hidden">
                <ChevronRight className="h-3.5 w-3.5 transition-transform group-open:rotate-90" />
                About this game
              </summary>
              {detail ? (
                <div className="mt-3 max-w-2xl">
                  <p className="text-[13px] leading-relaxed text-text-lo">
                    {detail.description ?? "No description available."}
                  </p>
                  <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.05em] text-text-lo/80">
                    {[
                      detail.metacriticScore != null ? `Metacritic ${detail.metacriticScore}` : null,
                      detail.developer,
                      detail.publisher,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </p>
                </div>
              ) : (
                <p className="mt-3 text-[13px] text-text-lo">Loading…</p>
              )}
            </details>
          )}

          <div className="mt-7 flex flex-wrap items-center gap-3 border-t border-border pt-5">
            <input
              type="text"
              defaultValue={game.exeName ?? ""}
              placeholder="link launch file for auto-tracking (e.g. DaveTheDiver.exe)"
              spellCheck={false}
              onBlur={(e) => {
                if (e.target.value !== (game.exeName ?? "")) linkExe(e.target.value);
              }}
              className="w-80 max-w-full rounded-md border border-border bg-transparent px-2.5 py-1.5 font-mono text-[11px] text-text-lo outline-none transition-colors placeholder:text-text-lo/50 hover:border-border-strong focus:border-accent/40 focus:text-text-hi"
            />
            <button
              onClick={remove}
              className="ml-auto rounded-lg border border-border-strong/60 px-3 py-1.5 text-xs font-medium text-text-lo transition-all duration-150 hover:border-danger/40 hover:bg-danger/10 hover:text-danger active:scale-[0.98]"
            >
              Remove from library
            </button>
          </div>
        </div>
      </div>

      {playingClip && <ClipPlayer clip={playingClip} onClose={() => setPlayingClip(null)} />}
    </div>
  );
}

function BackLink() {
  return (
    <Link
      to="/library"
      className="inline-flex items-center gap-1.5 text-[12.5px] font-semibold text-text-lo transition-colors hover:text-text-hi"
    >
      <ArrowLeft className="h-3.5 w-3.5" /> Library
    </Link>
  );
}

function StatTile({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-surface-alt px-4 py-3.5">
      <p className="shelf-label">{label}</p>
      <p className="mt-1.5 text-[20px] font-bold tracking-tight text-text-hi">{value}</p>
    </div>
  );
}
