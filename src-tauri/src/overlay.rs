// In-game overlay toast — a tiny always-on-top, transparent, click-through window pinned to the
// top-right of the primary display, used for clip-save feedback (saving → saved/failed). This is
// how mainstream clipping tools (Medal/ShadowPlay) surface feedback: OS notifications are the
// wrong tool while gaming — macOS suppresses banners whenever a fullscreen app is frontmost
// (observed live: osascript succeeded, nothing appeared), and Windows toasts land in the Action
// Center without showing over most games.
//
// The window is created ONCE, hidden, during app setup — window creation must happen on the main
// thread on macOS, while toasts fire from the async runtime (hotkey handler); show/hide/emit are
// safe from any thread. Creating it at startup also means its webview (the app bundle at
// `#/overlay`) is fully loaded long before the first toast, so no event ever races the listener.
//
// Limits shared with every non-injecting overlay: a game in EXCLUSIVE fullscreen on Windows
// composites its own swapchain and won't show any OS window on top — borderless fullscreen (the
// modern default) works. On macOS the window level + FullScreenAuxiliary collection behavior get
// it above native fullscreen Spaces.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

pub const OVERLAY_LABEL: &str = "clip-overlay";
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 72.0;
const MARGIN: f64 = 14.0;

/// Monotonic toast id — a scheduled hide only fires if no newer toast replaced it meanwhile
/// (e.g. the "saved" update must cancel the "saving" toast's long fallback hide, not be hidden
/// by it).
static TOAST_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Toast {
    kind: &'static str,
    text: String,
}

pub fn init(app: &AppHandle) {
    let win = match WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("index.html#/overlay".into()),
    )
    .title("Clip overlay")
    .inner_size(WIDTH, HEIGHT)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .focusable(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .shadow(false)
    .visible(false)
    .visible_on_all_workspaces(true)
    .build()
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("overlay: failed to create overlay window: {e}");
            return;
        }
    };
    // Click-through — the toast must never eat a mouse click aimed at the game under it.
    let _ = win.set_ignore_cursor_events(true);
    #[cfg(target_os = "macos")]
    elevate_above_fullscreen(&win);
}

/// macOS: raise the NSWindow's level above everything — including the shielding level games use
/// when they capture the display — let it join fullscreen Spaces (CanJoinAllSpaces |
/// FullScreenAuxiliary), and force it frontmost without activating the app. Tauri exposes none
/// of these knobs, hence the raw objc messages.
///
/// Called on EVERY toast (not just window creation): live testing showed the init-time settings
/// don't survive — the toast appeared once the game was tabbed out of but never over the
/// fullscreen game itself. Tauri/tao reasserts its own window level (plain floating) as part of
/// show()/always-on-top handling, so this must run after each show(), on the main thread.
#[cfg(target_os = "macos")]
fn elevate_above_fullscreen(win: &tauri::WebviewWindow) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    extern "C" {
        // The level CGDisplayCapture-style exclusive fullscreen shields the display at — one
        // above it stays visible over everything. (CoreGraphics is already linked via the
        // core-graphics crate clipper.rs uses.)
        fn CGShieldingWindowLevel() -> i32;
    }
    let Ok(ns) = win.ns_window() else { return };
    let ns = ns as *mut Object;
    unsafe {
        let level = CGShieldingWindowLevel() as i64 + 1;
        let _: () = msg_send![ns, setLevel: level];
        // NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary
        let behavior: u64 = (1 << 0) | (1 << 8);
        let _: () = msg_send![ns, setCollectionBehavior: behavior];
        // Frontmost without focusing/activating — the game must keep input.
        let _: () = msg_send![ns, orderFrontRegardless];
    }
}

/// Shows a toast over whatever is on screen. `kind` is one of "saving" / "saved" / "failed" —
/// a later call updates the same window in place (the saving → saved transition). Terminal
/// toasts slide away after a few seconds; a "saving" toast lingers longer as a fallback in case
/// its terminal update never comes (a hung save), but is normally replaced well before that.
pub fn toast(app: &AppHandle, kind: &'static str, text: impl Into<String>) {
    let seq = TOAST_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(win) = app.get_webview_window(OVERLAY_LABEL) else {
        eprintln!("overlay: no overlay window, dropping toast");
        return;
    };

    // Re-anchor to the primary monitor's top-right on every toast — displays can be plugged/
    // unplugged or rearranged mid-session.
    if let Ok(Some(mon)) = win.primary_monitor() {
        let scale = mon.scale_factor();
        let mx = mon.position().x as f64 / scale;
        let my = mon.position().y as f64 / scale;
        let mw = mon.size().width as f64 / scale;
        let _ = win.set_position(LogicalPosition::new(mx + mw - WIDTH - MARGIN, my + MARGIN));
    }
    let _ = win.show();
    // NSWindow calls must run on the main thread; toast() itself fires from the async runtime.
    #[cfg(target_os = "macos")]
    {
        let mt_win = win.clone();
        let _ = win.run_on_main_thread(move || elevate_above_fullscreen(&mt_win));
    }
    let _ = app.emit_to(OVERLAY_LABEL, "overlay-toast", Toast { kind, text: text.into() });

    let linger_secs = if kind == "saving" { 30 } else { 4 };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(linger_secs)).await;
        if TOAST_SEQ.load(Ordering::SeqCst) == seq {
            if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
                let _ = win.hide();
            }
        }
    });
}
