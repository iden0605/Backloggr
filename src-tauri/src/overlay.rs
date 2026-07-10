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
// modern default) works. On macOS, floating over another app's fullscreen Space takes THREE
// things together: a non-activating NSPanel (convert_to_panel — a plain NSWindow from an
// inactive app won't render over a fullscreen Space at any level), the shielding+1 window level,
// and the CanJoinAllSpaces|FullScreenAuxiliary collection behavior (elevate_above_fullscreen,
// re-applied per toast because tao resets the level during show()).

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
    {
        convert_to_panel(&win);
        elevate_above_fullscreen(&win);
    }
}

/// macOS: turn the overlay's NSWindow into a non-activating NSPanel. A plain NSWindow belonging
/// to an inactive app won't render over another app's fullscreen Space no matter its level or
/// collection behavior — panels are the sanctioned overlay window kind, and the class swap on a
/// live window is the same trick tauri-nspanel ships (NSPanel adds no ivars over NSWindow;
/// guarded by an instance-size check anyway).
#[cfg(target_os = "macos")]
fn convert_to_panel(win: &tauri::WebviewWindow) {
    use objc::runtime::{Class, Object, NO, YES};
    use objc::{msg_send, sel, sel_impl};
    extern "C" {
        fn object_setClass(obj: *mut Object, cls: *const Class) -> *const Class;
        fn class_getInstanceSize(cls: *const Class) -> usize;
    }
    let Ok(ns) = win.ns_window() else { return };
    let ns = ns as *mut Object;
    let Some(panel_class) = Class::get("NSPanel") else { return };
    unsafe {
        let current_class: *const Class = msg_send![ns, class];
        if class_getInstanceSize(current_class) != class_getInstanceSize(panel_class as *const Class) {
            eprintln!("overlay: window class instance size differs from NSPanel, skipping panel conversion");
            return;
        }
        object_setClass(ns, panel_class);
        // Non-activating: showing the panel must never activate this app, which would kick the
        // game out of fullscreen / steal its input. NSWindowStyleMaskNonactivatingPanel = 1 << 7.
        let style: u64 = msg_send![ns, styleMask];
        let _: () = msg_send![ns, setStyleMask: style | (1u64 << 7)];
        // NSPanel's default is to hide whenever its app deactivates — and this app is never
        // active while a game runs. Without this the panel can never appear in-game at all.
        let _: () = msg_send![ns, setHidesOnDeactivate: NO];
        let _: () = msg_send![ns, setBecomesKeyOnlyIfNeeded: YES];
        let _: () = msg_send![ns, setWorksWhenModal: YES];
    }
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
        // NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorStationary
        // | NSWindowCollectionBehaviorFullScreenAuxiliary
        let behavior: u64 = (1 << 0) | (1 << 4) | (1 << 8);
        let _: () = msg_send![ns, setCollectionBehavior: behavior];
        // Frontmost without focusing/activating — the game must keep input.
        let _: () = msg_send![ns, orderFrontRegardless];

        // Diagnostics for the fullscreen-Steam-game bug: read back what actually stuck. If the
        // toast is still invisible in-game, these lines say whether something re-reset the level
        // or the panel never joined the game's Space (isOnActiveSpace=NO).
        let applied_level: i64 = msg_send![ns, level];
        let applied_behavior: u64 = msg_send![ns, collectionBehavior];
        let visible: objc::runtime::BOOL = msg_send![ns, isVisible];
        let on_active_space: objc::runtime::BOOL = msg_send![ns, isOnActiveSpace];
        eprintln!(
            "overlay: level={applied_level} (target {level}) behavior={applied_behavior:#x} visible={} onActiveSpace={}",
            visible != objc::runtime::NO,
            on_active_space != objc::runtime::NO
        );
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

    // The "saving" fallback must outlast the slowest real save — on a loaded Windows machine a
    // save was observed taking 15-30s+, and the 30s fallback hid the spinner mid-save, leaving
    // the player convinced the save had silently died before the "saved" toast arrived.
    let linger_secs = if kind == "saving" { 120 } else { 4 };
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
