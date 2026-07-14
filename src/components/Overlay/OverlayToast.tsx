import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Loader2, CircleCheck, CircleX } from "lucide-react";

interface Toast {
  kind: "saving" | "saved" | "failed";
  text: string;
}

/**
 * Content of the in-game overlay window (see src-tauri/src/overlay.rs) — rendered at #/overlay
 * in a tiny transparent always-on-top window, NOT inside the app shell. The Rust side owns
 * showing/hiding/positioning the window; this component only renders whatever the latest
 * `overlay-toast` event says.
 */
export function OverlayToast() {
  const [toast, setToast] = useState<Toast | null>(null);

  useEffect(() => {
    // The app-wide stylesheet paints an opaque page background; this window must stay
    // see-through around the toast card.
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const unlisten = listen<Toast>("overlay-toast", (event) => setToast(event.payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (!toast) return null;

  // The terminal states get a one-beat pop (keyed on kind so saving → saved replays it) —
  // a saved clip is the proudest moment in the app and deserves more than an icon swap.
  const icon =
    toast.kind === "saving" ? (
      <Loader2 className="h-5 w-5 shrink-0 animate-spin text-accent" />
    ) : toast.kind === "saved" ? (
      <CircleCheck key="saved" className="h-5 w-5 shrink-0 animate-pop-in text-success" />
    ) : (
      <CircleX key="failed" className="h-5 w-5 shrink-0 animate-pop-in text-danger" />
    );

  return (
    <div className="flex h-screen w-screen items-start p-1">
      <div className="flex w-full items-center gap-3 rounded-xl border border-border bg-surface/95 px-4 py-3 shadow-lg">
        {icon}
        <p className="min-w-0 truncate text-[13px] font-medium text-text-hi">{toast.text}</p>
      </div>
    </div>
  );
}
