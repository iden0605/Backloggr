import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const MIN_CLIP_SECONDS = 5;
const MAX_CLIP_SECONDS = 120;

export function Settings() {
  const [clipSeconds, setClipSeconds] = useState<number | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<number>("get_clip_seconds")
      .then(setClipSeconds)
      .catch((err) => setError(String(err)));
  }, []);

  async function commit(value: number) {
    const clamped = Math.min(MAX_CLIP_SECONDS, Math.max(MIN_CLIP_SECONDS, value));
    setClipSeconds(clamped);
    try {
      await invoke("set_clip_seconds", { seconds: clamped });
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div>
      <h1 className="page-title text-[26px]">Settings</h1>
      <p className="mt-1.5 text-[13.5px] text-text-lo">
        Tracking, clipping, AI, and general app settings.
      </p>

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      <div className="mt-7 max-w-md rounded-xl border border-border bg-surface p-4">
        <h2 className="font-mono text-[11px] font-medium uppercase tracking-wider text-text-lo">
          Clips
        </h2>
        <div className="mt-3 flex items-center justify-between gap-4">
          <div>
            <p className="text-[13.5px] font-medium text-text-hi">Clip length</p>
            <p className="mt-0.5 text-xs text-text-lo">
              How much of the buffer <span className="font-mono text-text-hi">Alt+F9</span> saves.
            </p>
          </div>
          {clipSeconds !== null && (
            <div className="flex shrink-0 items-center gap-2">
              <input
                type="number"
                min={MIN_CLIP_SECONDS}
                max={MAX_CLIP_SECONDS}
                value={clipSeconds}
                onChange={(e) => setClipSeconds(Number(e.target.value))}
                onBlur={(e) => commit(Number(e.target.value))}
                className="w-16 rounded-md border border-border bg-surface-alt px-2 py-1.5 text-right font-mono text-[13px] text-text-hi outline-none transition-colors focus:border-accent/40"
              />
              <span className="text-xs text-text-lo">sec</span>
              {saved && <span className="text-xs text-success">Saved</span>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
