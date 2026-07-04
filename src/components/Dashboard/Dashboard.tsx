import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Link } from "react-router-dom";
import { Compass } from "lucide-react";
import {
  Bar,
  BarChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
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
  gamesInBacklog: number;
  gamesPlaying: number;
  gamesPlayed: GamePlaytime[];
  playtimeLast7Days: DailyPlaytime[];
}

interface CurrentlyPlaying {
  gameId: number;
  name: string;
  coverUrl: string | null;
  startedAt: string;
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

function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border bg-surface p-4 transition-colors hover:border-border/80">
      <p className="text-xs font-medium text-text-lo">{label}</p>
      <p className="mt-1.5 text-2xl font-semibold text-text-hi">{value}</p>
    </div>
  );
}

function CurrentlyPlayingBanner({ session }: { session: CurrentlyPlaying }) {
  const [elapsedSeconds, setElapsedSeconds] = useState(() =>
    Math.max(0, (Date.now() - parseUtc(session.startedAt).getTime()) / 1000),
  );

  useEffect(() => {
    const startedMs = parseUtc(session.startedAt).getTime();
    const tick = () => setElapsedSeconds(Math.max(0, (Date.now() - startedMs) / 1000));
    tick();
    const interval = setInterval(tick, 1000);
    return () => clearInterval(interval);
  }, [session.startedAt]);

  return (
    <div className="flex items-center gap-4 rounded-xl border border-accent/25 bg-accent/[0.06] p-4">
      {session.coverUrl ? (
        <img
          src={session.coverUrl}
          alt={session.name}
          className="h-16 w-16 rounded-lg object-cover"
        />
      ) : (
        <div className="h-16 w-16 rounded-lg bg-gradient-to-br from-surface-alt to-surface" />
      )}
      <div className="flex-1">
        <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-accent">
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-accent" />
          Currently Playing
        </p>
        <p className="mt-1.5 text-lg font-semibold text-text-hi">{session.name}</p>
      </div>
      <p className="font-mono text-2xl tabular-nums text-accent">
        {formatElapsed(elapsedSeconds)}
      </p>
    </div>
  );
}

function GamePlaytimeRow({ game }: { game: GamePlaytime }) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-border bg-surface p-3 transition-colors hover:border-border/80 hover:bg-surface-alt/40">
      {game.coverUrl ? (
        <img src={game.coverUrl} alt={game.name} className="h-12 w-12 rounded-lg object-cover" />
      ) : (
        <div className="h-12 w-12 rounded-lg bg-gradient-to-br from-surface-alt to-surface" />
      )}
      <p className="flex-1 truncate text-[13.5px] font-medium text-text-hi">{game.name}</p>
      <p className="font-mono text-xs text-text-lo">{formatDuration(game.totalSeconds)}</p>
    </div>
  );
}

export function Dashboard() {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [currentlyPlaying, setCurrentlyPlaying] = useState<CurrentlyPlaying | null>(null);
  const [period, setPeriod] = useState<Period>("week");
  const [error, setError] = useState<string | null>(null);

  const loadStats = useCallback((p: Period) => {
    invoke<DashboardStats>("get_dashboard_stats", { period: p })
      .then(setStats)
      .catch((e) => setError(String(e)));
  }, []);

  const loadCurrentlyPlaying = useCallback(() => {
    invoke<CurrentlyPlaying | null>("get_currently_playing")
      .then(setCurrentlyPlaying)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    loadStats(period);
  }, [period, loadStats]);

  useEffect(() => {
    loadCurrentlyPlaying();
  }, [loadCurrentlyPlaying]);

  // Refresh live state the moment a session starts/ends, instead of requiring a tab switch.
  useEffect(() => {
    const unlistenStarted = listen("session-started", () => {
      loadCurrentlyPlaying();
      loadStats(period);
    });
    const unlistenEnded = listen("session-ended", () => {
      loadCurrentlyPlaying();
      loadStats(period);
    });
    return () => {
      unlistenStarted.then((f) => f());
      unlistenEnded.then((f) => f());
    };
  }, [period, loadCurrentlyPlaying, loadStats]);

  if (error) {
    return <p className="text-sm text-danger">Failed to load dashboard: {error}</p>;
  }

  if (!stats) {
    return <p className="text-sm text-text-lo">Loading dashboard…</p>;
  }

  const chartData = stats.playtimeLast7Days.map((d) => ({
    day: formatDayLabel(d.date),
    hours: Math.round((d.totalSeconds / 3600) * 10) / 10,
  }));

  // Heuristic first-run check: nothing tracked or completed and no active/backlog games —
  // a brand-new install with an empty library. Guides the user to their first action instead
  // of showing empty charts and zeroed stat cards.
  const isFirstRun =
    !currentlyPlaying &&
    stats.totalPlaytimeSeconds === 0 &&
    stats.gamesPlaying === 0 &&
    stats.gamesInBacklog === 0 &&
    stats.gamesCompleted === 0 &&
    stats.gamesPlayed.length === 0;

  if (isFirstRun) {
    return (
      <div>
        <h1 className="page-title text-[26px]">Dashboard</h1>
        <div className="mt-8 flex flex-col items-start gap-3 rounded-xl border border-dashed border-border p-8">
          <Compass className="h-8 w-8 text-accent" />
          <div>
            <p className="text-sm font-semibold text-text-hi">Welcome to your game backlog</p>
            <p className="mt-1 max-w-md text-[13.5px] text-text-lo">
              Once you add a game and start playing, your playtime, stats, and trends will show up
              here. Search for a game to add it, or just launch something you own from Steam,
              Epic, GOG, Battle.net, or Riot — it'll be picked up automatically.
            </p>
          </div>
          <Link
            to="/search"
            className="rounded-lg bg-accent px-3.5 py-2 text-xs font-semibold text-bg transition-colors hover:bg-accent-hover"
          >
            Search for a game
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div>
      <h1 className="page-title text-[26px]">Dashboard</h1>

      {currentlyPlaying && (
        <div className="mt-5 animate-fade-up">
          <CurrentlyPlayingBanner session={currentlyPlaying} />
        </div>
      )}

      <div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard label={`Playtime (${PERIODS.find((p) => p.value === period)?.label})`} value={formatDuration(stats.totalPlaytimeSeconds)} />
        <StatCard label="Currently Playing" value={String(stats.gamesPlaying)} />
        <StatCard label="Completed" value={String(stats.gamesCompleted)} />
        <StatCard label="Backlog" value={String(stats.gamesInBacklog)} />
      </div>

      <div className="mt-6 rounded-xl border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text-hi">Playtime — Last 7 Days</h2>
        <div className="mt-3 h-48">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData}>
              <XAxis dataKey="day" stroke="#8FA3A8" fontSize={11} tickLine={false} axisLine={false} />
              <YAxis stroke="#8FA3A8" fontSize={11} allowDecimals={false} tickLine={false} axisLine={false} />
              <Tooltip
                cursor={{ fill: "rgba(45, 212, 191, 0.06)" }}
                contentStyle={{
                  background: "#1A262B",
                  border: "1px solid #253337",
                  borderRadius: 10,
                  fontSize: 12,
                  color: "#EEF4F5",
                }}
                formatter={(value) => [`${value}h`, "Playtime"]}
              />
              <Bar dataKey="hours" fill="#2DD4BF" radius={[5, 5, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>

      <div className="mt-6">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-text-hi">Games Played</h2>
          <div className="flex gap-1 rounded-lg bg-surface p-1">
            {PERIODS.map((p) => (
              <button
                key={p.value}
                onClick={() => setPeriod(p.value)}
                className={`rounded-md px-3 py-1 text-xs font-medium transition-colors ${
                  period === p.value
                    ? "bg-accent text-bg"
                    : "text-text-lo hover:text-text-hi"
                }`}
              >
                {p.label}
              </button>
            ))}
          </div>
        </div>

        {stats.gamesPlayed.length === 0 ? (
          <p className="mt-4 text-sm text-text-lo">
            No playtime tracked for this period yet.
          </p>
        ) : (
          <div className="mt-3 space-y-2">
            {stats.gamesPlayed.map((g) => (
              <GamePlaytimeRow key={g.gameId} game={g} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
