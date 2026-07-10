import { useEffect, useRef, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Loader2, RefreshCw } from "lucide-react";
import { useAppStore } from "../../store/useAppStore";

type Phase =
  | { kind: "checking" }
  | { kind: "latest" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; percent: number | null }
  | { kind: "restarting" }
  | { kind: "error"; message: string };

/**
 * In-app updates, inside the Settings About card. Checks the newest published GitHub
 * release's `latest.json` (tauri-plugin-updater, signature-verified against the pubkey in
 * tauri.conf.json), downloads + installs in place, then relaunches — on Windows the
 * installer exits the app itself, so the "Restarting" state may only flash.
 */
export function UpdateSection() {
  const [phase, setPhase] = useState<Phase>({ kind: "checking" });
  const setUpdateAvailable = useAppStore((s) => s.setUpdateAvailable);
  // The Update object from check() carries the download handle — kept out of React state
  // since it's a class instance we only need imperatively.
  const updateRef = useRef<Awaited<ReturnType<typeof check>>>(null);

  async function runCheck() {
    setPhase({ kind: "checking" });
    try {
      const update = await check();
      if (update) {
        updateRef.current = update;
        setUpdateAvailable(update.version);
        setPhase({ kind: "available", version: update.version });
      } else {
        setPhase({ kind: "latest" });
      }
    } catch (err) {
      // Dev builds and offline machines land here — quiet, factual, retryable.
      setPhase({ kind: "error", message: String(err) });
    }
  }

  useEffect(() => {
    void runCheck();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function install() {
    const update = updateRef.current;
    if (!update) return;
    setPhase({ kind: "downloading", percent: null });
    let total: number | null = null;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          setPhase({
            kind: "downloading",
            percent: total ? Math.min(100, Math.round((received / total) * 100)) : null,
          });
        }
      });
      // Windows exits the app during install; everywhere else we relaunch explicitly.
      setPhase({ kind: "restarting" });
      setUpdateAvailable(null);
      await relaunch();
    } catch (err) {
      setPhase({ kind: "error", message: String(err) });
    }
  }

  return (
    <div className="mt-4 border-t border-border pt-4">
      {phase.kind === "checking" && (
        <p className="flex items-center gap-2 text-xs text-text-lo">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> Checking for updates…
        </p>
      )}

      {phase.kind === "latest" && (
        <div className="flex items-center justify-between gap-4">
          <p className="text-xs text-text-lo">You're on the latest version.</p>
          <button
            onClick={() => void runCheck()}
            className="flex shrink-0 items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-xs font-medium text-text-lo transition-all duration-150 hover:border-border-strong hover:text-text-hi active:scale-[0.98]"
          >
            <RefreshCw className="h-3 w-3" /> Check again
          </button>
        </div>
      )}

      {phase.kind === "available" && (
        <div className="flex items-center justify-between gap-4">
          <div>
            <p className="text-[13.5px] font-medium text-text-hi">
              Update available — v{phase.version}
            </p>
            <p className="mt-0.5 text-xs text-text-lo">
              Downloads and installs in place; your library, playtime, and clips are untouched.
            </p>
          </div>
          <button
            onClick={() => void install()}
            className="shrink-0 rounded-md bg-text-hi px-3.5 py-2 text-xs font-semibold text-bg transition-all duration-150 hover:opacity-85 active:scale-[0.98]"
          >
            Update now
          </button>
        </div>
      )}

      {phase.kind === "downloading" && (
        <div>
          <p className="flex items-center gap-2 text-xs text-text-lo">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            Downloading update{phase.percent !== null ? ` — ${phase.percent}%` : "…"}
          </p>
          <div className="mt-2.5 h-1 overflow-hidden rounded-full bg-surface-alt">
            <div
              className="h-full rounded-full bg-accent transition-[width] duration-200"
              style={{ width: `${phase.percent ?? 12}%` }}
            />
          </div>
        </div>
      )}

      {phase.kind === "restarting" && (
        <p className="flex items-center gap-2 text-xs text-text-lo">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> Installed — restarting…
        </p>
      )}

      {phase.kind === "error" && (
        <div className="flex items-center justify-between gap-4">
          <p className="min-w-0 truncate text-xs text-text-lo" title={phase.message}>
            Couldn't check for updates.
          </p>
          <button
            onClick={() => void runCheck()}
            className="flex shrink-0 items-center gap-1.5 rounded-md border border-border px-2.5 py-1.5 text-xs font-medium text-text-lo transition-all duration-150 hover:border-border-strong hover:text-text-hi active:scale-[0.98]"
          >
            <RefreshCw className="h-3 w-3" /> Retry
          </button>
        </div>
      )}
    </div>
  );
}
