import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link } from "react-router-dom";
import { Clapperboard, Sparkles, Timer } from "lucide-react";
import {
  Bar,
  BarChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Game } from "../../store/useAppStore";

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
}

interface CurrentlyPlaying {
  gameId: number;
  name: string;
  coverUrl: string | null;
  startedAt: string;
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

function formatDayLabel(date: string): string {
  const d = new Date(`${date}T00:00:00`);
  return d.toLocaleDateString(undefined, { weekday: "short" });
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
          <img
            src={coverUrl}
            alt=""
            aria-hidden
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
          <img
            src={coverUrl}
            alt={title}
            className="h-40 w-64 shrink-0 rounded-xl object-cover shadow-[0_24px_48px_-16px_rgba(0,0,0,0.8)] ring-1 ring-white/10"
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

function StatStrip({ stats, periodLabel }: { stats: DashboardStats; periodLabel: string }) {
  const cells = [
    { value: formatDuration(stats.totalPlaytimeSeconds), label: `Played · ${periodLabel}` },
    { value: String(stats.gamesInLibrary), label: "In library" },
    { value: String(stats.gamesPlayedCount), label: "Played" },
    { value: String(stats.gamesCompleted), label: "Completed" },
  ];
  return (
    <div className="grid grid-cols-2 divide-border border-y border-border sm:grid-cols-4 sm:divide-x">
      {cells.map((cell) => (
        <div key={cell.label} className="px-5 py-5 first:pl-0">
          <p className="text-[26px] font-bold tabular-nums tracking-tight text-text-hi">{cell.value}</p>
          <p className="shelf-label mt-1">{cell.label}</p>
        </div>
      ))}
    </div>
  );
}

function GameShelfCard({ game }: { game: GamePlaytime }) {
  return (
    <div className="group w-44 shrink-0 cursor-default">
      {game.coverUrl ? (
        <img
          src={game.coverUrl}
          alt={game.name}
          className="aspect-video w-full rounded-lg object-cover transition-all duration-200 group-hover:-translate-y-1 group-hover:shadow-[0_16px_30px_-12px_rgba(0,0,0,0.7)]"
        />
      ) : (
        <div className="aspect-video w-full rounded-lg bg-gradient-to-br from-surface-alt to-surface transition-transform duration-200 group-hover:-translate-y-1" />
      )}
      <p className="mt-2.5 truncate text-[13px] font-medium text-text-hi">{game.name}</p>
      <p className="mt-0.5 font-mono text-[11px] text-text-lo">{formatDuration(game.totalSeconds)}</p>
    </div>
  );
}

export function Dashboard() {
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

  const chartData = stats.playtimeLast7Days.map((d) => ({
    day: formatDayLabel(d.date),
    hours: Math.round((d.totalSeconds / 3600) * 10) / 10,
  }));

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
        copy: "A rolling buffer records while you play. Hit Alt+F9 after a great moment and it's saved.",
      },
      {
        icon: Sparkles,
        title: "Know what to play next",
        copy: "Discover learns from what you actually play, or describe a mood in chat and get a shortlist.",
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
            className="mt-6 inline-block rounded-lg bg-text-hi px-4 py-2 text-[12.5px] font-semibold text-bg transition-opacity hover:opacity-85"
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
        <StatStrip
          stats={stats}
          periodLabel={PERIODS.find((p) => p.value === period)?.label ?? ""}
        />

        <div className="mt-10">
          <h2 className="shelf-label">Last 7 days</h2>
          {chartData.every((d) => d.hours === 0) ? (
            <div className="mt-4 flex h-44 items-center justify-center rounded-xl border border-dashed border-border-strong/50">
              <p className="text-[13px] text-text-lo">
                No sessions in the last 7 days — playtime will chart here.
              </p>
            </div>
          ) : (
          <div className="mt-4 h-44">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={chartData}>
                <XAxis dataKey="day" stroke="#928B82" fontSize={11} tickLine={false} axisLine={false} />
                <YAxis stroke="#928B82" fontSize={11} allowDecimals={false} tickLine={false} axisLine={false} />
                <Tooltip
                  cursor={{ fill: "rgba(185, 106, 85, 0.08)" }}
                  contentStyle={{
                    background: "#1C1A19",
                    border: "1px solid #3A3633",
                    borderRadius: 10,
                    fontSize: 12,
                    color: "#EDE8E0",
                  }}
                  formatter={(value) => [`${value}h`, "Playtime"]}
                />
                <Bar dataKey="hours" fill="#B96A55" radius={[5, 5, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
          )}
        </div>

        <div className="mt-10">
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
