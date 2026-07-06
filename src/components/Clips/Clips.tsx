import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Film, Play, Trash2 } from "lucide-react";
import { EmptyState } from "../shared/EmptyState";

interface Clip {
  id: number;
  gameId: number | null;
  gameName: string | null;
  filePath: string;
  thumbnailPath: string | null;
  durationSeconds: number | null;
  createdAt: string;
  title: string | null;
  notes: string | null;
}

function formatDuration(seconds: number | null): string {
  if (!seconds) return "";
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m > 0 ? `${m}m ${s}s` : `${s}s`;
}

function formatCreatedAt(raw: string): string {
  return new Date(`${raw.replace(" ", "T")}Z`).toLocaleString();
}

export function Clips() {
  const [clips, setClips] = useState<Clip[]>([]);
  const [clipSeconds, setClipSeconds] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<Clip | null>(null);

  async function loadClips() {
    try {
      setClips(await invoke<Clip[]>("get_clips"));
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    loadClips();
    invoke<number>("get_clip_seconds")
      .then(setClipSeconds)
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    // Save progress/success feedback lives on the in-game overlay toast (OverlayToast.tsx) —
    // this view only refreshes the gallery and surfaces failure detail inline.
    const unlistenSaved = listen("clip-saved", () => loadClips());
    const unlistenFailed = listen<string>("clip-save-failed", (event) => {
      setError(event.payload);
    });
    return () => {
      unlistenSaved.then((f) => f());
      unlistenFailed.then((f) => f());
    };
  }, []);

  async function removeClip(id: number) {
    try {
      await invoke("delete_clip", { id });
      await loadClips();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div>
      <h1 className="page-title text-[26px]">Clips</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Press <kbd className="kbd">Alt+F9</kbd> anytime while a game is running to save the
        last {clipSeconds ?? "30"} seconds. Adjustable in Settings. Nothing records while
        you're not playing.
      </p>

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      {clips.length === 0 ? (
        <div className="mt-8">
          <EmptyState icon={Film} title="No clips yet">
            Launch a game and hit <kbd className="kbd">Alt+F9</kbd> after a good moment — the
            clip lands here, attached to the game it came from.
          </EmptyState>
        </div>
      ) : (
        <div className="mt-7 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
          {clips.map((clip) => (
            <ClipCard key={clip.id} clip={clip} onPlay={() => setPlaying(clip)} onDelete={() => removeClip(clip.id)} />
          ))}
        </div>
      )}

      {playing && <ClipPlayer clip={playing} onClose={() => setPlaying(null)} />}
    </div>
  );
}

function ClipCard({
  clip,
  onPlay,
  onDelete,
}: {
  clip: Clip;
  onPlay: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="group overflow-hidden rounded-xl border border-border bg-surface transition-all duration-200 hover:-translate-y-0.5 hover:border-border-strong hover:shadow-[0_16px_32px_-16px_rgba(0,0,0,0.6)]">
      <button
        onClick={onPlay}
        className="relative flex aspect-video w-full items-center justify-center bg-surface-alt"
      >
        {clip.thumbnailPath ? (
          <img
            src={convertFileSrc(clip.thumbnailPath)}
            alt={clip.title ?? "Clip thumbnail"}
            className="h-full w-full object-cover"
          />
        ) : (
          <Film className="h-8 w-8 text-text-lo/50" />
        )}
        <span className="absolute inset-0 flex items-center justify-center bg-bg/0 opacity-0 transition-opacity group-hover:bg-bg/40 group-hover:opacity-100">
          <span className="flex h-10 w-10 items-center justify-center rounded-full bg-bg/75 backdrop-blur-sm">
            <Play className="ml-0.5 h-4 w-4 text-text-hi" />
          </span>
        </span>
        {clip.durationSeconds && (
          <span className="absolute bottom-1.5 right-1.5 rounded bg-bg/80 px-1.5 py-0.5 font-mono text-[10px] text-text-hi">
            {formatDuration(clip.durationSeconds)}
          </span>
        )}
      </button>
      <div className="flex items-start justify-between gap-2 p-2.5">
        <div className="min-w-0">
          <p className="truncate text-xs font-semibold text-text-hi">{clip.title ?? "Clip"}</p>
          <p className="mt-0.5 truncate text-[11px] text-text-lo">
            {clip.gameName ?? "Unknown game"} · {formatCreatedAt(clip.createdAt)}
          </p>
        </div>
        <button
          onClick={onDelete}
          className="shrink-0 rounded-md p-1 text-text-lo/70 transition-colors hover:bg-danger/10 hover:text-danger"
        >
          <Trash2 className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}

function ClipPlayer({ clip, onClose }: { clip: Clip; onClose: () => void }) {
  return (
    <div
      className="fixed inset-0 z-50 flex animate-fade-in items-center justify-center bg-bg/80 p-6 backdrop-blur-sm"
      onClick={onClose}
    >
      <div className="w-full max-w-3xl animate-fade-up" onClick={(e) => e.stopPropagation()}>
        <video
          src={convertFileSrc(clip.filePath)}
          controls
          autoPlay
          className="max-h-[80vh] w-full rounded-xl border border-border shadow-2xl"
        />
        <p className="mt-3 text-center text-[12.5px] text-text-lo">
          {clip.gameName ?? "Unknown game"} · {formatCreatedAt(clip.createdAt)}
        </p>
      </div>
    </div>
  );
}
