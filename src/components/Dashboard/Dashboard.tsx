import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
    <div className="rounded-lg border border-neutral-800 bg-neutral-900 p-4">
      <p className="text-sm text-neutral-400">{label}</p>
      <p className="mt-1 text-2xl font-semibold">{value}</p>
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
    <div className="flex items-center gap-4 rounded-lg border border-emerald-800 bg-emerald-950/40 p-4">
      {session.coverUrl ? (
        <img
          src={session.coverUrl}
          alt={session.name}
          className="h-16 w-16 rounded object-cover"
        />
      ) : (
        <div className="h-16 w-16 rounded bg-neutral-800" />
      )}
      <div className="flex-1">
        <p className="flex items-center gap-2 text-sm font-medium text-emerald-400">
          <span className="h-2 w-2 animate-pulse rounded-full bg-emerald-400" />
          Currently Playing
        </p>
        <p className="mt-1 text-lg font-semibold">{session.name}</p>
      </div>
      <p className="font-mono text-2xl tabular-nums text-emerald-300">
        {formatElapsed(elapsedSeconds)}
      </p>
    </div>
  );
}

function GamePlaytimeRow({ game }: { game: GamePlaytime }) {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-neutral-800 bg-neutral-900 p-3">
      {game.coverUrl ? (
        <img src={game.coverUrl} alt={game.name} className="h-12 w-12 rounded object-cover" />
      ) : (
        <div className="h-12 w-12 rounded bg-neutral-800" />
      )}
      <p className="flex-1 truncate font-medium">{game.name}</p>
      <p className="text-neutral-400">{formatDuration(game.totalSeconds)}</p>
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
    return <p className="text-red-400">Failed to load dashboard: {error}</p>;
  }

  if (!stats) {
    return <p className="text-neutral-400">Loading dashboard…</p>;
  }

  const chartData = stats.playtimeLast7Days.map((d) => ({
    day: formatDayLabel(d.date),
    hours: Math.round((d.totalSeconds / 3600) * 10) / 10,
  }));

  return (
    <div>
      <h1 className="text-2xl font-semibold">Dashboard</h1>

      {currentlyPlaying && (
        <div className="mt-4">
          <CurrentlyPlayingBanner session={currentlyPlaying} />
        </div>
      )}

      <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatCard label={`Playtime (${PERIODS.find((p) => p.value === period)?.label})`} value={formatDuration(stats.totalPlaytimeSeconds)} />
        <StatCard label="Currently Playing" value={String(stats.gamesPlaying)} />
        <StatCard label="Completed" value={String(stats.gamesCompleted)} />
        <StatCard label="Backlog" value={String(stats.gamesInBacklog)} />
      </div>

      <div className="mt-6 rounded-lg border border-neutral-800 bg-neutral-900 p-4">
        <h2 className="text-lg font-medium">Playtime — Last 7 Days</h2>
        <div className="mt-2 h-48">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData}>
              <XAxis dataKey="day" stroke="#a3a3a3" fontSize={12} />
              <YAxis stroke="#a3a3a3" fontSize={12} allowDecimals={false} />
              <Tooltip
                contentStyle={{ background: "#171717", border: "1px solid #404040" }}
                formatter={(value) => [`${value}h`, "Playtime"]}
              />
              <Bar dataKey="hours" fill="#6366f1" radius={[4, 4, 0, 0]} />
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>

      <div className="mt-6">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-medium">Games Played</h2>
          <div className="flex gap-1 rounded-lg bg-neutral-900 p-1">
            {PERIODS.map((p) => (
              <button
                key={p.value}
                onClick={() => setPeriod(p.value)}
                className={`rounded-md px-3 py-1 text-sm transition-colors ${
                  period === p.value
                    ? "bg-indigo-600 text-white"
                    : "text-neutral-400 hover:text-neutral-200"
                }`}
              >
                {p.label}
              </button>
            ))}
          </div>
        </div>

        {stats.gamesPlayed.length === 0 ? (
          <p className="mt-3 text-sm text-neutral-400">
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
