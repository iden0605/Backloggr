import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const MIN_CLIP_SECONDS = 5;
const MAX_CLIP_SECONDS = 120;

function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${
        on ? "bg-accent" : "bg-surface-alt"
      }`}
    >
      <span
        className={`absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-bg transition-transform ${
          on ? "translate-x-5" : "translate-x-0"
        }`}
      />
    </button>
  );
}

export function Settings() {
  const [clipSeconds, setClipSeconds] = useState<number | null>(null);
  const [micEnabled, setMicEnabled] = useState<boolean | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<number>("get_clip_seconds")
      .then(setClipSeconds)
      .catch((err) => setError(String(err)));
    invoke<boolean>("get_mic_enabled")
      .then(setMicEnabled)
      .catch((err) => setError(String(err)));
    invoke<boolean>("get_autostart_enabled")
      .then(setAutostart)
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

  async function toggleMic() {
    if (micEnabled === null) return;
    const next = !micEnabled;
    setMicEnabled(next);
    try {
      await invoke("set_mic_enabled", { enabled: next });
    } catch (err) {
      setError(String(err));
      setMicEnabled(!next);
    }
  }

  async function toggleAutostart() {
    if (autostart === null) return;
    const next = !autostart;
    setAutostart(next);
    try {
      await invoke("set_autostart_enabled", { enabled: next });
    } catch (err) {
      setError(String(err));
      setAutostart(!next);
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
          General
        </h2>
        <div className="mt-3 flex items-center justify-between gap-4">
          <div>
            <p className="text-[13.5px] font-medium text-text-hi">Launch on startup</p>
            <p className="mt-0.5 text-xs text-text-lo">
              Start in the background when you log in, so playtime tracking and clipping are
              always on.
            </p>
          </div>
          {autostart !== null && <Toggle on={autostart} onClick={toggleAutostart} />}
        </div>
        <p className="mt-4 border-t border-border pt-4 text-xs text-text-lo">
          Closing the window keeps the app running in the tray — tracking and the clip hotkey
          stay active. Quit from the tray icon.
        </p>
      </div>

      <div className="mt-5 max-w-md rounded-xl border border-border bg-surface p-4">
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

        <div className="mt-4 flex items-center justify-between gap-4 border-t border-border pt-4">
          <div>
            <p className="text-[13.5px] font-medium text-text-hi">Microphone</p>
            <p className="mt-0.5 text-xs text-text-lo">
              Record mic audio alongside clips. Captured continuously, even through tab-outs.
            </p>
          </div>
          {micEnabled !== null && <Toggle on={micEnabled} onClick={toggleMic} />}
        </div>
      </div>
    </div>
  );
}
