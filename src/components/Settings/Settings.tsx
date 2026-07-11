import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { SteamImportSection } from "./SteamImport";
import { UpdateSection } from "./UpdateSection";
import { Select } from "../shared/Select";
import { useAppStore } from "../../store/useAppStore";

const MIN_CLIP_SECONDS = 5;
const MAX_CLIP_SECONDS = 120;

// Curated combos only — every one parses for tauri-plugin-global-shortcut on both OSes,
// and none collide with common in-game binds the way bare letter keys would.
const HOTKEY_OPTIONS = [
  "Alt+F9",
  "Alt+F10",
  "Alt+F8",
  "Ctrl+F9",
  "Ctrl+Shift+S",
  "Ctrl+Shift+C",
  "F8",
  "F10",
].map((value) => ({ value, label: value }));

// On-state is chalk, not rust — primary/affirmative controls are chalk in this palette;
// rust stays reserved for live markers. The off state needs a ring + gray knob to be
// visible at all against the card surface.
function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 ${
        on ? "bg-text-hi" : "bg-surface-alt ring-1 ring-inset ring-border-strong"
      }`}
    >
      <span
        className={`absolute left-0.5 top-0.5 h-5 w-5 rounded-full shadow-sm transition-all duration-200 ${
          on ? "translate-x-5 bg-bg" : "translate-x-0 bg-text-lo"
        }`}
      />
    </button>
  );
}

// The uninstall flow drives the OS's own uninstaller — only meaningful on Windows installs.
const IS_WINDOWS = navigator.userAgent.includes("Windows");

export function Settings() {
  const clipHotkey = useAppStore((s) => s.clipHotkey);
  const setClipHotkey = useAppStore((s) => s.setClipHotkey);
  const [clipSeconds, setClipSeconds] = useState<number | null>(null);
  const [micEnabled, setMicEnabled] = useState<boolean | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [confirmUninstall, setConfirmUninstall] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
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

  async function commitHotkey(hotkey: string) {
    const previous = clipHotkey;
    setClipHotkey(hotkey);
    try {
      await invoke("set_clip_hotkey", { hotkey });
    } catch (err) {
      // Registration failed (combo taken by another app) — the old hotkey is still live.
      setError(String(err));
      setClipHotkey(previous);
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

  async function runUninstall() {
    setUninstalling(true);
    try {
      // Hands off to the Windows uninstaller and quits the app — no state to restore on
      // success, the window is about to disappear.
      await invoke("uninstall_app");
    } catch (err) {
      setError(String(err));
      setUninstalling(false);
      setConfirmUninstall(false);
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

      <div className="mt-7 max-w-xl rounded-xl border border-border bg-surface p-5">
        <h2 className="shelf-label">General</h2>
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

      <div className="mt-5 max-w-xl rounded-xl border border-border bg-surface p-5">
        <h2 className="shelf-label">Clips</h2>
        <div className="mt-3 flex items-center justify-between gap-4">
          <div>
            <p className="text-[13.5px] font-medium text-text-hi">Clip length</p>
            <p className="mt-0.5 text-xs text-text-lo">
              How much of the buffer <kbd className="kbd">{clipHotkey}</kbd> saves.
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
            <p className="text-[13.5px] font-medium text-text-hi">Clip hotkey</p>
            <p className="mt-0.5 text-xs text-text-lo">
              The key combo that saves a clip while a game is running. Applies immediately.
            </p>
          </div>
          <Select value={clipHotkey} options={HOTKEY_OPTIONS} onChange={commitHotkey} />
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

      <SteamImportSection />

      <div className="mt-5 max-w-xl rounded-xl border border-border bg-surface p-5">
        <h2 className="shelf-label">About</h2>
        <div className="mt-3 flex items-center justify-between gap-4">
          <p className="text-[13.5px] font-medium text-text-hi">backloggr</p>
          {version && (
            <span className="rounded-md border border-border bg-surface-alt px-2 py-1 font-mono text-xs text-text-lo select-none">
              v{version}
            </span>
          )}
        </div>
        <UpdateSection />

        {IS_WINDOWS && (
          <div className="mt-4 border-t border-border pt-4">
            <div className="flex items-center justify-between gap-4">
              <div>
                <p className="text-[13.5px] font-medium text-text-hi">Uninstall Backloggr</p>
                <p className="mt-0.5 text-xs text-text-lo">
                  Removes the app from this PC. Your library, playtime, settings, and clips
                  stay on disk — a reinstall picks them right back up.
                </p>
              </div>
              {!confirmUninstall && (
                <button
                  onClick={() => setConfirmUninstall(true)}
                  className="shrink-0 rounded-lg border border-danger/40 px-3.5 py-2 text-xs font-semibold text-danger transition-all duration-150 hover:border-danger hover:bg-danger/10 active:scale-[0.98]"
                >
                  Uninstall…
                </button>
              )}
            </div>
            {confirmUninstall && (
              <div className="mt-3 flex animate-fade-up items-center justify-between gap-4 rounded-lg border border-danger/25 bg-danger/10 px-4 py-3">
                <p className="text-xs text-text-hi">
                  Remove Backloggr from this PC? The app will close and the Windows uninstaller
                  will take over.
                </p>
                <div className="flex shrink-0 gap-2">
                  <button
                    onClick={() => setConfirmUninstall(false)}
                    disabled={uninstalling}
                    className="rounded-lg px-3 py-1.5 text-xs font-medium text-text-lo transition-all duration-150 enabled:hover:text-text-hi enabled:active:scale-[0.98]"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={runUninstall}
                    disabled={uninstalling}
                    className="rounded-lg bg-danger px-3.5 py-1.5 text-xs font-semibold text-bg transition-all duration-150 enabled:hover:opacity-85 enabled:active:scale-[0.98] disabled:opacity-60"
                  >
                    {uninstalling ? "Uninstalling…" : "Uninstall"}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
