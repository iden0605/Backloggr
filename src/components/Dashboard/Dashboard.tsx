import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link } from "react-router-dom";
import { Clapperboard, Sparkles, Timer } from "lucide-react";
import { CoverImage, resizedCover } from "../shared/CoverImage";
import { Game, useAppStore } from "../../store/useAppStore";

type Period = "day" | "week" | "month" | "all";

const PERIODS: { value: Period; label: string }[] = [
  { value: "day", label: "Today" },
  { value: "week", label: "This Week" },
  { value: "month", label: "This Month" },
  { value: "all", label: "All Time" },
];

interface GamePlaytime {
  gameId: number;
  name: string;
  coverUrl: string | null;
  totalSeconds: number;
}

interface DailyPlaytime {
  date: string;
  totalSeconds: number;
}

interface DashboardStats {
  totalPlaytimeSeconds: number;
  gamesCompleted: number;
  gamesInLibrary: number;
  gamesPlayedCount: number;
  gamesPlayed: GamePlaytime[];
  playtimeLast7Days: DailyPlaytime[];
  prevWeekPlaytimeSeconds: number;
  weekGames: GamePlaytime[];
}

interface CurrentlyPlaying {
  gameId: number;
  name: string;
  coverUrl: string | null;
  startedAt: string;
  /** Other games that also have an open session — the hero shows the most recent + this count. */
  alsoPlaying: number;
}

interface LibraryGame extends Game {
  totalSeconds: number;
  lastPlayedAt: string | null;
  sessionCount: number;
}

// SQLite's CURRENT_TIMESTAMP is UTC but formatted without a timezone marker
// ("YYYY-MM-DD HH:MM:SS"); without the "Z", JS parses it as local time and skews the elapsed time.
function parseUtc(sqliteTimestamp: string): Date {
  return new Date(sqliteTimestamp.replace(" ", "T") + "Z");
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}

function formatElapsed(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);
  const mm = String(minutes).padStart(2, "0");
  const ss = String(secs).padStart(2, "0");
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mm}:${ss}`;
}

function dayLabel(date: string, style: "short" | "narrow"): string {
  const d = new Date(`${date}T00:00:00`);
  return d.toLocaleDateString(undefined, { weekday: style });
}

function SessionTimer({ startedAt }: { startedAt: string }) {
  const [elapsedSeconds, setElapsedSeconds] = useState(() =>
    Math.max(0, (Date.now() - parseUtc(startedAt).getTime()) / 1000),
  );

  useEffect(() => {
    const startedMs = parseUtc(startedAt).getTime();
    const tick = () => setElapsedSeconds(Math.max(0, (Date.now() - startedMs) / 1000));
    tick();
    const interval = setInterval(tick, 1000);
    return () => clearInterval(interval);
  }, [startedAt]);

  return (
    <span className="font-mono text-xl tabular-nums text-accent">
      {formatElapsed(elapsedSeconds)}
    </span>
  );
}

/**
 * The "Backdrop" hero: the featured game's own cover art fills the top of the app —
 * blurred and darkened, melting into the page background — with a sharp poster and the
 * session state at its base. The floating TopNav renders over the top of this.
 */
function Hero({
  label,
  live,
  title,
  coverUrl,
  children,
}: {
  label: string;
  live?: boolean;
  title: string;
  coverUrl: string | null;
  children?: React.ReactNode;
}) {
  return (
    <section className="relative">
      <div className="absolute inset-0 overflow-hidden">
        {coverUrl ? (
          // The backdrop is heavily blurred anyway — a 640px CDN rendition rasterizes far
          // cheaper than the full-size cover (blur cost scales with source pixels, and this
          // was the most expensive paint on the page during Windows scroll testing).
          <img
            src={resizedCover(coverUrl, 640)}
            alt=""
            aria-hidden
            decoding="async"
            className="h-full w-full scale-110 object-cover blur-2xl brightness-[0.45] saturate-[0.85]"
          />
        ) : (
          <div className="h-full w-full bg-gradient-to-br from-surface-alt to-bg" />
        )}
        {/* Melt the art into the page background so the hero has no hard bottom edge. */}
        <div className="absolute inset-0 bg-gradient-to-b from-bg/40 via-bg/50 to-bg" />
      </div>

      <div className="relative mx-auto flex min-h-[320px] w-full max-w-5xl items-end gap-7 px-8 pb-10 pt-28">
        {coverUrl && (
          <CoverImage
            src={coverUrl}
            alt={title}
            eager
            className="h-40 w-64 shrink-0 rounded-xl shadow-[0_24px_48px_-16px_rgba(0,0,0,0.8)] ring-1 ring-white/10"
          />
        )}
        <div className="min-w-0 flex-1 animate-fade-up">
          <p className="flex items-center gap-2.5 font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-accent">
            {live && <span className="h-1.5 w-1.5 animate-pulse-soft rounded-full bg-accent" />}
            {label}
          </p>
          <h1 className="page-title mt-2.5 truncate text-4xl">{title}</h1>
          {children}
        </div>
      </div>
    </section>
  );
}

/** Axis-free 7-day sparkline: quiet rust bars, day initials, exact value on hover only. */
function WeekSparkline({ days }: { days: DailyPlaytime[] }) {
  const max = Math.max(...days.map((d) => d.totalSeconds));
  return (
    <div>
      <div className="mt-5 flex h-14 items-end gap-1.5">
        {days.map((d) => {
          const barHeight =
            d.totalSeconds === 0 ? 3 : Math.max(5, (d.totalSeconds / max) * 52);
          return (
            <div
              key={d.date}
              className="group relative flex h-full flex-1 flex-col items-center justify-end"
            >
              <span className="pointer-events-none absolute -top-6 whitespace-nowrap rounded-md border border-border-strong bg-surface-alt px-1.5 py-0.5 font-mono text-[10px] text-text-hi opacity-0 transition-opacity group-hover:opacity-100">
                {dayLabel(d.date, "short")} ·{" "}
                {d.totalSeconds > 0 ? formatDuration(d.totalSeconds) : "0m"}
              </span>
              <div
                className="w-full rounded-t bg-accent/80 transition-colors group-hover:bg-accent"
                style={{ height: barHeight }}
              />
            </div>
          );
        })}
      </div>
      <div className="mt-1.5 flex gap-1.5">
        {days.map((d) => (
          <span key={d.date} className="flex-1 text-center font-mono text-[10px] text-text-lo">
            {dayLabel(d.date, "narrow")}
          </span>
        ))}
      </div>
    </div>
  );
}

/** "This week" card: one hero number, a vs-last-week delta, and the sparkline. */
function WeekCard({ stats }: { stats: DashboardStats }) {
  const weekTotal = stats.playtimeLast7Days.reduce((sum, d) => sum + d.totalSeconds, 0);
  const delta = weekTotal - stats.prevWeekPlaytimeSeconds;
  const gamesCount = stats.weekGames.length;

  let deltaLine: React.ReactNode;
  if (weekTotal === 0 && stats.prevWeekPlaytimeSeconds === 0) {
    deltaLine = <span>No sessions yet — playtime will chart here.</span>;
  } else {
    deltaLine = (
      <>
        {delta === 0 ? (
          <span>Same as last week</span>
        ) : delta > 0 ? (
          <span className="font-semibold text-success">▲ {formatDuration(delta)}</span>
        ) : (
          <span>▼ {formatDuration(-delta)}</span>
        )}
        {delta !== 0 && <span> vs last week</span>}
        {gamesCount > 0 && (
          <span>
            {" "}
            · {gamesCount} {gamesCount === 1 ? "game" : "games"}
          </span>
        )}
      </>
    );
  }

  return (
    <div className="rounded-2xl border border-border bg-surface px-6 py-5">
      <h2 className="shelf-label">This week</h2>
      <p className="mt-3 font-mono text-[34px] font-semibold tabular-nums tracking-tight text-text-hi">
        {formatDuration(weekTotal)}
      </p>
      <p className="mt-1.5 text-[12.5px] text-text-lo">{deltaLine}</p>
      <WeekSparkline days={stats.playtimeLast7Days} />
    </div>
  );
}

/** "Most played this week" card: the top game with cover, plus proportional bars for the top 3. */
function MostPlayedCard({ weekGames }: { weekGames: GamePlaytime[] }) {
  const top = weekGames[0];
  return (
    <div className="rounded-2xl border border-border bg-surface px-6 py-5">
      <h2 className="shelf-label">Most played this week</h2>
      {!top ? (
        <p className="mt-4 text-[12.5px] leading-relaxed text-text-lo">
          Nothing played in the last 7 days — your most-played game will show up here.
        </p>
      ) : (
        <>
          <div className="mt-3.5 flex items-center gap-3.5">
            <CoverImage
              src={top.coverUrl}
              alt={top.name}
              className="aspect-video w-24 shrink-0 rounded-lg"
            />
            <div className="min-w-0">
              <p className="truncate text-[14.5px] font-semibold text-text-hi">{top.name}</p>
              <p className="mt-0.5 font-mono text-[11.5px] text-text-lo">
                {formatDuration(top.totalSeconds)} this week
              </p>
            </div>
          </div>
          <div className="mt-4 flex flex-col gap-2.5">
            {weekGames.slice(0, 3).map((g, i) => (
              <div key={g.gameId} className="grid grid-cols-[110px_1fr_56px] items-center gap-3">
                <span className="truncate text-[12.5px] text-text-lo">{g.name}</span>
                <div className="h-[5px] overflow-hidden rounded-full bg-surface-alt">
                  <div
                    className={`h-full rounded-full ${i === 0 ? "bg-accent" : "bg-text-lo/40"}`}
                    style={{ width: `${Math.max(4, (g.totalSeconds / top.totalSeconds) * 100)}%` }}
                  />
                </div>
                <span className="text-right font-mono text-[11px] text-text-lo">
                  {formatDuration(g.totalSeconds)}
                </span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function GameShelfCard({ game }: { game: GamePlaytime }) {
  return (
    <Link to={`/library/${game.gameId}`} className="group w-44 shrink-0 rounded-lg">
      <CoverImage
        src={game.coverUrl}
        alt={game.name}
        className="aspect-video w-full rounded-lg transition-all duration-200 group-hover:-translate-y-1 group-hover:shadow-[0_16px_30px_-12px_rgba(0,0,0,0.7)]"
      />
      <p className="mt-2.5 truncate text-[13px] font-medium text-text-hi">{game.name}</p>
      <p className="mt-0.5 font-mono text-[11px] text-text-lo">{formatDuration(game.totalSeconds)}</p>
    </Link>
  );
}

export function Dashboard() {
  const clipHotkey = useAppStore((s) => s.clipHotkey);
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [currentlyPlaying, setCurrentlyPlaying] = useState<CurrentlyPlaying | null>(null);
  const [libraryGames, setLibraryGames] = useState<LibraryGame[]>([]);
  const [period, setPeriod] = useState<Period>("week");
  const [error, setError] = useState<string | null>(null);

  const loadStats = useCallback((p: Period) => {
    invoke<DashboardStats>("get_dashboard_stats", { period: p })
      .then(setStats)
      .catch((e) => setError(String(e)));
  }, []);

  const loadLiveState = useCallback(() => {
    invoke<CurrentlyPlaying | null>("get_currently_playing")
      .then(setCurrentlyPlaying)
      .catch((e) => setError(String(e)));
    // Library aggregates feed the idle hero's "jump back in" pick and the live hero's
    // lifetime line — one cheap local read.
    invoke<LibraryGame[]>("get_library")
      .then(setLibraryGames)
      .catch(() => setLibraryGames([]));
  }, []);

  useEffect(() => {
    loadStats(period);
  }, [period, loadStats]);

  useEffect(() => {
    loadLiveState();
  }, [loadLiveState]);

  // Refresh live state the moment a session starts/ends, instead of requiring a tab switch.
  useEffect(() => {
    const refresh = () => {
      loadLiveState();
      loadStats(period);
    };
    const unlistenStarted = listen("session-started", refresh);
    const unlistenEnded = listen("session-ended", refresh);
    return () => {
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
    };
  }, [period, loadLiveState, loadStats]);

  if (error) {
    return (
      <p className="mx-auto max-w-5xl px-8 pt-24 text-sm text-danger">
        Failed to load dashboard: {error}
      </p>
    );
  }

  if (!stats) {
    return <p className="mx-auto max-w-5xl px-8 pt-24 text-sm text-text-lo">Loading dashboard…</p>;
  }

  // Heuristic first-run check: nothing tracked or completed and no active/backlog games —
  // a brand-new install with an empty library.
  const isFirstRun =
    !currentlyPlaying &&
    stats.totalPlaytimeSeconds === 0 &&
    stats.gamesInLibrary === 0 &&
    stats.gamesCompleted === 0 &&
    stats.gamesPlayed.length === 0;

  if (isFirstRun) {
    const features = [
      {
        icon: Timer,
        title: "Playtime tracks itself",
        copy: "Launch a game from Steam, Epic, GOG, Battle.net, or Riot and sessions log automatically — no timers to start.",
      },
      {
        icon: Clapperboard,
        title: "Clip the last 30 seconds",
        copy: `A rolling buffer records while you play. Hit ${clipHotkey} after a great moment and it's saved.`,
      },
      {
        icon: Sparkles,
        title: "Know what to play next",
        copy: "Discover learns from what you actually play, or ask Shelby for a mood-matched shortlist.",
      },
    ];
    return (
      <div className="mx-auto flex min-h-screen w-full max-w-5xl flex-col justify-center px-8 pb-16 pt-24">
        <div className="animate-fade-up">
          <p className="font-mono text-[11px] font-medium uppercase tracking-[0.18em] text-accent">
            Welcome
          </p>
          <h1 className="page-title mt-3 text-5xl">Your library starts here</h1>
          <p className="mt-4 max-w-lg text-[13.5px] leading-relaxed text-text-lo">
            Add a game and start playing — playtime, stats, and trends will build up on this
            page. Search for a title, or just launch something you own and it'll be picked up
            automatically.
          </p>
          <Link
            to="/discover"
            className="mt-6 inline-block rounded-lg bg-text-hi px-4 py-2 text-[12.5px] font-semibold text-bg transition-all duration-150 hover:opacity-85 active:scale-[0.98]"
          >
            Find a game
          </Link>
        </div>

        <div className="mt-16 grid animate-fade-up gap-4 sm:grid-cols-3 [animation-delay:120ms]">
          {features.map(({ icon: Icon, title, copy }) => (
            <div key={title} className="rounded-xl border border-border bg-surface/60 p-5">
              <Icon className="h-[18px] w-[18px] text-text-lo" />
              <p className="mt-3 text-[13.5px] font-semibold text-text-hi">{title}</p>
              <p className="mt-1.5 text-xs leading-relaxed text-text-lo">{copy}</p>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // Idle hero: recent activity, not a queue — the game most recently actually played.
  // ("Up next from your backlog" died with the queue model; the library doesn't tell you
  // what to play, it reflects what you play.)
  const jumpBackIn =
    libraryGames
      .filter((g) => g.totalSeconds > 0 && g.lastPlayedAt)
      .sort((a, b) => parseUtc(b.lastPlayedAt!).getTime() - parseUtc(a.lastPlayedAt!).getTime())[0] ??
    null;

  const lifetimeLine = (gameId: number) => {
    const total = libraryGames.find((g) => g.id === gameId)?.totalSeconds;
    return total && total > 0 ? `${formatDuration(total)} lifetime` : null;
  };

  const relativeDays = (timestamp: string) => {
    const days = Math.floor((Date.now() - parseUtc(timestamp).getTime()) / 86_400_000);
    if (days <= 0) return "today";
    if (days === 1) return "yesterday";
    if (days < 14) return `${days} days ago`;
    if (days < 60) return `${Math.floor(days / 7)} weeks ago`;
    return `${Math.floor(days / 30)} months ago`;
  };

  return (
    <div>
      {currentlyPlaying ? (
        <Hero
          label="Now playing"
          live
          title={currentlyPlaying.name}
          coverUrl={currentlyPlaying.coverUrl}
        >
          <p className="mt-3 flex items-baseline gap-4 text-[13px] text-text-lo">
            <SessionTimer startedAt={currentlyPlaying.startedAt} />
            <span>this session</span>
            {lifetimeLine(currentlyPlaying.gameId) && (
              <span>· {lifetimeLine(currentlyPlaying.gameId)}</span>
            )}
            {currentlyPlaying.alsoPlaying > 0 && (
              <span>
                · +{currentlyPlaying.alsoPlaying} more running
              </span>
            )}
          </p>
        </Hero>
      ) : jumpBackIn ? (
        <Hero label="Jump back in" title={jumpBackIn.name} coverUrl={jumpBackIn.coverUrl}>
          <p className="mt-3 text-[13px] text-text-lo">
            Last played {relativeDays(jumpBackIn.lastPlayedAt!)} ·{" "}
            {formatDuration(jumpBackIn.totalSeconds)} total
          </p>
        </Hero>
      ) : (
        <Hero label="Dashboard" title="Your library" coverUrl={null} />
      )}

      <div className="mx-auto w-full max-w-5xl px-8 pb-14">
        <div className="grid gap-4 border-t border-border pt-8 sm:grid-cols-2">
          <WeekCard stats={stats} />
          <MostPlayedCard weekGames={stats.weekGames} />
        </div>

        <div className="mt-11">
          <div className="flex items-center justify-between">
            <h2 className="shelf-label">Games played</h2>
            <div className="flex gap-1 rounded-lg bg-surface p-1">
              {PERIODS.map((p) => (
                <button
                  key={p.value}
                  onClick={() => setPeriod(p.value)}
                  className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                    period === p.value
                      ? "bg-text-hi text-bg"
                      : "text-text-lo hover:text-text-hi"
                  }`}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>

          {stats.gamesPlayed.length === 0 ? (
            <div className="mt-5 rounded-xl border border-dashed border-border-strong/50 px-5 py-9 text-center">
              <p className="text-[13px] text-text-lo">
                No playtime tracked for this period yet — games you play will shelf up here.
              </p>
            </div>
          ) : (
            <div className="mt-5 flex gap-5 overflow-x-auto pb-3">
              {stats.gamesPlayed.map((g) => (
                <GameShelfCard key={g.gameId} game={g} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
