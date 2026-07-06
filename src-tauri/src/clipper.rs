// ffmpeg rolling-buffer capture and clip extraction (Stage 6).
//
// Approach: ffmpeg's `segment` muxer writes the capture into fixed-length MPEG-TS segment files
// (`segment_000.ts`, ...) and `-segment_wrap N` cycles back to `segment_000` once N files exist —
// a free ring buffer with no manual cleanup. MPEG-TS (not MP4) matters: a TS file is readable up
// to its current write position even while ffmpeg is still appending to it, so `save_clip` can
// include the live segment and capture footage right up to the hotkey press. An MP4 segment only
// becomes readable once its `moov` index is written at close, which forced an earlier version to
// drop the newest (still-open) segment and lose the final 0-10 seconds of every clip.
//
// Capture runs only while a game session is open: `tracker.rs` calls `ensure_capture` on every
// poll while sessions are active — which also acts as a watchdog (restarts a crashed ffmpeg) and,
// on Windows, upgrades a desktop-fallback capture to window-scoped capture once the game's window
// can finally be resolved (games often spend their first seconds in a splash/launcher phase with
// no visible window yet). `stop` is called once no sessions remain open and on app exit.
//
// Tab-outs follow the same model mainstream clipping tools (Medal/ShadowPlay) use: the buffer
// NEVER stops or swaps — one continuous capture per session, and non-game content is excluded at
// CLIP time, not capture time. On Windows with a window-scoped capture there's nothing to
// exclude (gdigrab keeps grabbing the game's own window through alt-tabs — real gameplay
// continuity). When capture isn't window-scoped (macOS always; Windows before the title
// resolves), a 1-second focus sampler logs when the game wasn't frontmost, and `save_clip`
// blacks out those time ranges in the output during its re-encode — desktop pixels transiently
// exist in the temp ring buffer but never appear in a saved clip. Two earlier architectures
// (save-time segment dropping; live pause/black-generator swapping on focus changes) both proved
// buggy in live testing — swap-kills created partial/0-byte segments, broke duration math, and
// raced the capture device; continuous-capture-plus-output-masking is the design that works.

use crate::db::DbState;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

const SEGMENT_SECONDS: u32 = 10;
/// Ring buffer depth — 18 * 10s = 3 minutes of rolling footage available to save from.
const BUFFER_SEGMENTS: u32 = 18;
/// Fallback when no `clip_seconds` row exists in `settings` yet.
const DEFAULT_CLIP_SECONDS: u32 = 30;
const MIN_CLIP_SECONDS: u32 = 5;
const MAX_CLIP_SECONDS: u32 = 120;
const CLIP_SECONDS_SETTING_KEY: &str = "clip_seconds";
const MIC_ENABLED_SETTING_KEY: &str = "mic_enabled";

/// Path to the ffmpeg binary: the bundled sidecar next to the app executable when one exists
/// (installed Windows builds — `bundle.externalBin` in tauri.windows.conf.json puts it there, so
/// end users never install ffmpeg themselves), falling back to a PATH lookup (dev builds, macOS).
/// Resolved once — the install layout can't change mid-run.
fn ffmpeg_path() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
            .filter(|sidecar| sidecar.exists())
            .unwrap_or_else(|| PathBuf::from("ffmpeg"))
    })
}

/// Format of raw PCM riding into the capture ffmpeg over stdin — the Windows WASAPI loopback
/// path (see loopback.rs). Present on every platform so `CaptureInputs` construction stays
/// cfg-free; only the Windows branch ever populates it.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy)]
pub struct PipeAudioSpec {
    pub sample_rate: u32,
    pub channels: u16,
    /// ffmpeg raw-PCM demuxer name ("f32le" / "s16le").
    pub format: &'static str,
}

pub struct Capture {
    child: Child,
    /// Whether this capture is scoped to the game's own window (`gdigrab title=` on Windows). A
    /// window-scoped capture keeps grabbing the game's contents even while it's unfocused —
    /// Medal/ShadowPlay-like continuous gameplay through alt-tabs — so no focus sampling is
    /// needed at all. On macOS ffmpeg's avfoundation input has no window mode (whole displays
    /// only), so this is always `false` there and the focus sampler + save-time black-out
    /// compensate.
    window_scoped: bool,
}

pub struct CaptureSlot {
    /// Bumped on every session-level transition (fresh start, stop). A focus sampler captures
    /// the generation it was spawned for and exits when it no longer matches, so a stale sampler
    /// from a previous game can never log against the current one.
    generation: u64,
    /// The tracked game's exe name, for focus checks and window resolution.
    exe_name: Option<String>,
    /// Consecutive watchdog respawns this session. A capture that keeps dying instantly is most
    /// likely failing on its audio input (mic permission denied, device vanished) — after two
    /// strikes, subsequent spawns drop audio rather than crash-looping forever.
    respawn_strikes: u8,
    /// Set when the capture child was killed on purpose (mic setting changed) so the watchdog
    /// treats the next respawn as a fresh start (wiping the buffer — segments recorded with a
    /// different stream layout can't be concatenated with the new spawn's) instead of counting
    /// an audio-input strike against it.
    restart_requested: bool,
    phase: Option<Capture>,
}

impl CaptureSlot {
    pub fn idle() -> Self {
        CaptureSlot {
            generation: 0,
            exe_name: None,
            respawn_strikes: 0,
            restart_requested: false,
            phase: None,
        }
    }
}

pub struct CaptureState(pub Mutex<CaptureSlot>);

/// Guards against two `save_clip` calls overlapping (e.g. a double hotkey press): concurrent
/// saves would race on the same second-resolution work and produce interleaved ffmpeg runs.
static SAVE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct SaveGuard;

impl SaveGuard {
    fn acquire() -> Result<Self, String> {
        if SAVE_IN_PROGRESS.swap(true, Ordering::SeqCst) {
            Err("A clip is already being saved.".into())
        } else {
            Ok(SaveGuard)
        }
    }
}

impl Drop for SaveGuard {
    fn drop(&mut self) {
        SAVE_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

fn buffer_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("clip_buffer")
}

fn clips_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("clips")
}

/// Windows-only: finds the title of a visible top-level window owned by a process named
/// `exe_name`, so capture can be scoped to just that window (`gdigrab -i title=...`) instead of
/// the whole desktop — meaning tabbing out, or having other windows on top, doesn't leak into the
/// recording. Best-effort: returns `None` on any lookup failure (game not yet rendering a window,
/// API error, etc.); the caller falls back to full-desktop capture and retries via
/// `ensure_capture` each poll.
///
/// Untested on real Windows hardware, like the rest of this project's Windows-only code paths
/// (see ABOUT.md) — API usage verified against the windows 0.58 crate source, compile-checked by
/// the windows-check CI workflow.
#[cfg(target_os = "windows")]
fn process_exe_basename(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(|s| s.to_string())
    }
}

#[cfg(target_os = "windows")]
fn find_window_title_for_exe(exe_name: &str) -> Option<String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Search {
        // Pre-normalized (lowercased, ".exe" stripped) — the stored exe_name may or may not
        // carry the ".exe" suffix depending on how it was captured (sysinfo on Windows includes
        // it, manual entry may not), while QueryFullProcessImageNameW's basename always has it.
        exe_name_normalized: String,
        found: Option<String>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if let Some(name) = process_exe_basename(pid) {
                if crate::tracker::normalize_exe_name(&name) == search.exe_name_normalized {
                    let mut buf = [0u16; 512];
                    let len = GetWindowTextW(hwnd, &mut buf);
                    if len > 0 {
                        search.found = Some(String::from_utf16_lossy(&buf[..len as usize]));
                        return BOOL(0); // stop enumeration
                    }
                }
            }
        }
        BOOL(1) // continue
    }

    let mut search = Search {
        exe_name_normalized: crate::tracker::normalize_exe_name(exe_name),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

/// macOS: the on-screen bounds (in display POINTS, global coordinates) of a normal-layer window
/// owned by a process whose name matches `exe_name`, plus the main display's point size — via
/// CGWindowListCopyWindowInfo, which needs no TCC permission for bounds (only window *names*
/// require the Screen Recording grant). Used by `save_clip` to crop the saved clip down to just
/// the game's window: avfoundation can only capture whole displays (unlike Windows' gdigrab
/// window capture), so the scoping happens at save time on the output instead. Returns `None`
/// when the window can't be found or sits (partly) outside the main display — the caller falls
/// back to the uncropped full frame.
#[cfg(target_os = "macos")]
fn game_window_rect_points(exe_name: &str) -> Option<((f64, f64, f64, f64), (f64, f64))> {
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::display::CGDisplay;
    use core_graphics::geometry::CGRect;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly,
    };

    let exe_norm = crate::tracker::normalize_exe_name(exe_name);
    let mut sys = sysinfo::System::new();
    sys.refresh_processes();
    let pids: Vec<i64> = sys
        .processes()
        .iter()
        .filter(|(_, p)| {
            let name = crate::tracker::normalize_exe_name(p.name());
            name == exe_norm || name.contains(&exe_norm) || exe_norm.contains(&name)
        })
        .map(|(pid, _)| pid.as_u32() as i64)
        .collect();
    if pids.is_empty() {
        return None;
    }

    let display = CGDisplay::main();
    let display_bounds = display.bounds();
    let (dw, dh) = (display_bounds.size.width, display_bounds.size.height);

    let windows = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let pid_key = CFString::from_static_string("kCGWindowOwnerPID");
    let bounds_key = CFString::from_static_string("kCGWindowBounds");

    let mut best: Option<(f64, f64, f64, f64)> = None;
    for item in windows.iter() {
        let dict: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*item as *const _) };
        let Some(pid) = dict.find(&pid_key).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64())
        else {
            continue;
        };
        if !pids.contains(&pid) {
            continue;
        }
        // Fullscreen games sit on non-zero window layers, so no layer filtering — tiny utility/
        // tooltip windows are excluded by size below, and the LARGEST matching window wins.
        let Some(bounds_dict) = dict.find(&bounds_key).and_then(|v| v.downcast::<CFDictionary>())
        else {
            continue;
        };
        let Some(rect) = CGRect::from_dict_representation(&bounds_dict) else { continue };
        let (x, y, w, h) = (rect.origin.x, rect.origin.y, rect.size.width, rect.size.height);
        // Ignore tiny utility/tooltip windows; keep the largest real window.
        if w < 120.0 || h < 90.0 {
            continue;
        }
        if best.map_or(true, |(_, _, bw, bh)| w * h > bw * bh) {
            best = Some((x, y, w, h));
        }
    }
    let rect = best?;

    // Only croppable when fully on the main display — avfoundation captures "Capture screen 0"
    // (the main display), so a window on a second monitor isn't in the frame at all.
    let (x, y, w, h) = rect;
    if x < 0.0 || y < 0.0 || x + w > dw || y + h > dh {
        return None;
    }
    Some((rect, (dw, dh)))
}

/// Builds an ffmpeg `crop=` filter cutting the frame down to the game window's current bounds.
/// `input_w`/`input_h` are the capture's real pixel dimensions (probed from a segment file) —
/// the point→pixel scale factor is derived from those against the display's point size, because
/// nothing else reliably matches how avfoundation picks its capture resolution on Retina
/// displays (it captures the native panel resolution, which is neither the point size nor the
/// scaled backing size).
#[cfg(target_os = "macos")]
fn crop_filter_for_game_window(exe_name: &str, input_w: u32, input_h: u32) -> Option<String> {
    let ((x, y, w, h), (dw, dh)) = game_window_rect_points(exe_name)?;
    let scale_x = input_w as f64 / dw;
    let scale_y = input_h as f64 / dh;
    // Round to even values — yuv420p requires even dimensions.
    let even = |v: f64| (v.max(0.0) as u32) & !1;
    let (cx, cy) = (even(x * scale_x), even(y * scale_y));
    let (mut cw, mut ch) = (even(w * scale_x), even(h * scale_y));
    cw = cw.min(input_w.saturating_sub(cx));
    ch = ch.min(input_h.saturating_sub(cy));
    if cw < 64 || ch < 64 {
        return None;
    }
    // A window covering (nearly) the whole display — fullscreen game — needs no crop.
    if cw >= input_w - 4 && ch >= input_h - 4 {
        return None;
    }
    Some(format!("crop={cw}:{ch}:{cx}:{cy}"))
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn crop_filter_for_game_window(_exe_name: &str, _input_w: u32, _input_h: u32) -> Option<String> {
    None
}

/// Whether a window-scoped capture could be started for this exe right now — used by
/// `ensure_capture` to decide when a desktop-fallback capture is worth restarting scoped.
#[cfg(target_os = "windows")]
fn window_now_resolvable(exe_name: Option<&str>) -> bool {
    exe_name.is_some_and(|e| find_window_title_for_exe(e).is_some())
}

#[cfg(not(target_os = "windows"))]
fn window_now_resolvable(_exe_name: Option<&str>) -> bool {
    false
}

/// Whether `exe_name` is the currently focused/frontmost application — used to keep non-window-
/// scoped capture (macOS always, Windows before a title resolves) from ever letting a clip show
/// content from something the player alt-tabbed/swiped to instead of the game (see
/// `filter_unfocused_segments`). `None` means "couldn't determine" (API failure) — treated as "no
/// opinion" by the caller, not as unfocused, since misclassifying a genuinely-focused game as
/// unfocused would needlessly shrink or fail every clip.
///
/// Windows: `GetForegroundWindow` is exact — it names the literal window with input focus.
/// macOS: best-effort via `osascript`/System Events, matching the frontmost process's display
/// name against the tracked exe name — approximate because System Events reports app display
/// names ("Dave The Diver"), not the executable's filename, so an oddly-named executable can
/// false-negative. Acceptable for a dev-only path; not used at all once Windows capture is
/// window-scoped, where this check doesn't matter anyway.
#[cfg(target_os = "windows")]
pub async fn is_frontmost(exe_name: &str) -> Option<bool> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let name = process_exe_basename(pid)?;
        Some(crate::tracker::normalize_exe_name(&name) == crate::tracker::normalize_exe_name(exe_name))
    }
}

#[cfg(target_os = "macos")]
pub async fn is_frontmost(exe_name: &str) -> Option<bool> {
    // `lsappinfo` (LaunchServices) needs no TCC permission — an earlier osascript/System Events
    // version required an Automation grant the dev binary never gets prompted for when launched
    // from a terminal, so every check failed silently and focus filtering never engaged.
    let front = tokio::process::Command::new("lsappinfo")
        .arg("front")
        .output()
        .await
        .ok()?;
    let asn = String::from_utf8_lossy(&front.stdout).trim().to_string();
    if !front.status.success() || asn.is_empty() {
        return None;
    }
    let info = tokio::process::Command::new("lsappinfo")
        .args(["info", "-only", "name", &asn])
        .output()
        .await
        .ok()?;
    // Output shape: "LSDisplayName"="Dave The Diver"
    let raw = String::from_utf8_lossy(&info.stdout);
    let frontmost = raw.split('=').nth(1)?.trim().trim_matches('"').to_lowercase();
    if frontmost.is_empty() {
        return None;
    }
    let exe_norm = crate::tracker::normalize_exe_name(exe_name);
    Some(frontmost.contains(&exe_norm) || exe_norm.contains(&frontmost))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub async fn is_frontmost(_exe_name: &str) -> Option<bool> {
    None
}

/// Rolling log of focus samples (see `is_frontmost`), one appended per watcher tick while a
/// session is active. No longer used to filter segments at save time (black-out segments made
/// that obsolete) — retained for a future refinement that precision-trims the ≤1s swipe-
/// transition flash off the end of the segment recorded right before a black-out swap.
pub struct FocusLog(pub Mutex<std::collections::VecDeque<(std::time::SystemTime, bool)>>);

// 1-second watcher samples; must comfortably out-span the ~180s the segment buffer can cover.
const FOCUS_LOG_CAPACITY: usize = 256;

pub fn record_focus_sample(app: &AppHandle, focused: bool) {
    let state = app.state::<FocusLog>();
    let mut log = state.0.lock().unwrap_or_else(|e| e.into_inner());
    log.push_back((std::time::SystemTime::now(), focused));
    while log.len() > FOCUS_LOG_CAPACITY {
        log.pop_front();
    }
}

/// Device names that are system-audio LOOPBACKS rather than real microphones — virtual devices
/// that replay whatever the system (i.e. the game) outputs. macOS has no built-in loopback;
/// BlackHole/Soundflower/Loopback are the standard installs. Windows sometimes exposes the
/// driver-level "Stereo Mix", or "virtual-audio-capturer" from screen-capture-recorder.
fn is_loopback_device_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    ["blackhole", "soundflower", "loopback", "stereo mix", "what u hear", "virtual-audio-capturer"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Windows-only: dshow audio device names, in listing order.
#[cfg(target_os = "windows")]
fn dshow_audio_devices() -> Vec<String> {
    let Ok(output) = std::process::Command::new(ffmpeg_path())
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        // Lines look like: [dshow @ ...] "Microphone (Realtek Audio)" (audio)
        .filter(|line| line.contains("(audio)"))
        .filter_map(|line| {
            let start = line.find('"')?;
            let rest = &line[start + 1..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// Windows-only: the dshow device to use as the MIC. dshow has no "default device" concept
/// (unlike avfoundation), so prefer a device that calls itself a microphone — headsets, webcams,
/// and built-in arrays all do — over whatever happens to be listed first, and never a loopback.
#[cfg(target_os = "windows")]
fn dshow_mic_device(devices: &[String]) -> Option<String> {
    devices
        .iter()
        .filter(|d| !is_loopback_device_name(d))
        .find(|d| d.to_lowercase().contains("mic"))
        .or_else(|| devices.iter().find(|d| !is_loopback_device_name(d)))
        .cloned()
}

/// macOS: device indexes parsed fresh from ffmpeg's avfoundation listing (stderr) — the screen
/// video device ("Capture screen 0"), a system-audio loopback device when one is installed
/// (BlackHole etc., see `is_loopback_device_name`), and a real (non-loopback) microphone,
/// preferring one that names itself a mic — the fallback for when the system-default input IS a
/// loopback (see `capture_input_args`). Indexes must be parsed per spawn: connected devices
/// (iPhone, headsets) shift them, which is exactly how a hardcoded mic index silently became
/// BlackHole in live testing.
#[cfg(target_os = "macos")]
fn avfoundation_devices() -> (Option<u32>, Option<u32>, Option<u32>) {
    let Ok(output) = std::process::Command::new(ffmpeg_path())
        .args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
    else {
        return (None, None, None);
    };
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Lines look like: [AVFoundation indev @ 0x...] [2] Capture screen 0 — the entry index is
    // the LAST bracket (the line starts with the logger's own "[AVFoundation indev @ ...]").
    fn entry(line: &str) -> Option<(u32, &str)> {
        let idx = line.rfind('[')?;
        let rest = &line[idx + 1..];
        let (num, name) = rest.split_once(']')?;
        Some((num.trim().parse().ok()?, name.trim()))
    }

    let (mut screen, mut loopback) = (None, None);
    let mut real_mics: Vec<(u32, String)> = Vec::new();
    let mut in_audio_section = false;
    for line in stderr.lines() {
        if line.contains("AVFoundation audio devices") {
            in_audio_section = true;
            continue;
        }
        let Some((idx, name)) = entry(line) else { continue };
        if !in_audio_section && name.starts_with("Capture screen") && screen.is_none() {
            screen = Some(idx);
        }
        if in_audio_section {
            if is_loopback_device_name(name) {
                if loopback.is_none() {
                    loopback = Some(idx);
                }
            } else {
                real_mics.push((idx, name.to_string()));
            }
        }
    }
    let mic = real_mics
        .iter()
        .find(|(_, name)| name.to_lowercase().contains("mic"))
        .or_else(|| real_mics.first())
        .map(|(idx, _)| *idx);
    (screen, loopback, mic)
}

/// macOS: name of the system-default audio INPUT device — the one avfoundation's `default`
/// keyword resolves to — parsed from `system_profiler SPAudioDataType`. `None` when it can't be
/// determined (callers should then trust `default` as before).
#[cfg(target_os = "macos")]
fn macos_default_input_name() -> Option<String> {
    let output = std::process::Command::new("system_profiler")
        .arg("SPAudioDataType")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Each device is a "Device Name:" header line followed by "Key: Value" property lines;
    // headers end with ':' and property lines contain ": ".
    let mut current_device: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.contains(": ") {
            current_device = Some(trimmed.trim_end_matches(':'));
        } else if trimmed == "Default Input Device: Yes" {
            return current_device.map(str::to_string);
        }
    }
    None
}

/// Everything `spawn_ffmpeg` needs to know about the capture inputs.
struct CaptureInputs {
    /// The `-f ... -i ...` argument runs, in input order. Input 0 always carries the video.
    args: Vec<String>,
    /// Whether the capture is scoped to the game's window (see `Capture::window_scoped`).
    window_scoped: bool,
    /// Stream specifiers ("0:a", "1:a", ...) of every audio track across the inputs. 0 = no
    /// audio; 1 = mapped straight through; 2 = mixed with `amix` (mic + system loopback).
    audio_maps: Vec<String>,
    /// Set when one of the inputs is raw PCM over stdin (Windows WASAPI loopback) —
    /// `spawn_ffmpeg` then pipes stdin and hands it to `loopback::start`.
    pipe_audio: Option<PipeAudioSpec>,
}

/// OS-specific capture inputs for ffmpeg. On Windows, video scopes to the tracked game's window
/// when its title resolves, falling back to the full desktop otherwise. macOS video is always
/// the whole screen — ffmpeg's avfoundation input has no window mode, a real, documented
/// limitation of the dev-only Mac path; the focus sampler + save-time black-out/crop compensate.
/// `None` on unsupported OSes.
///
/// Audio (all captured through tab-outs — only the video gets masked at save time):
/// - `with_mic`: the user's microphone. macOS uses avfoundation's `default` keyword (follows the
///   system-default input — built-in, headset, whatever the user actively uses) — UNLESS the
///   default input is itself a loopback device (BlackHole's install can leave it as default, and
///   recording it as the "mic" is guaranteed silence — observed live), in which case a real mic
///   is picked by name instead. Windows picks the most microphone-looking dshow device since
///   dshow has no default-device concept (and never a loopback, same guard).
/// - GAME/system audio: Windows captures the default OUTPUT device natively via WASAPI loopback
///   (loopback.rs — zero setup, works with any headphones/speakers, PCM piped over stdin), with
///   a dshow loopback device (Stereo Mix/virtual-audio-capturer) only as fallback when that
///   can't open. macOS (dev-only) still needs an installed loopback device (BlackHole etc. — see
///   `is_loopback_device_name`), added unconditionally as its own input: an unrouted loopback
///   just contributes silence.
///
/// See `CaptureSlot::respawn_strikes` for the fallback that drops ALL audio inputs if they keep
/// killing the capture.
fn capture_input_args(
    #[allow(unused_variables)] exe_name: Option<&str>,
    with_mic: bool,
    with_loopback: bool,
) -> Option<CaptureInputs> {
    // Live inputs each get a generous queue so one slow device can't stall the others.
    fn push_input(args: &mut Vec<String>, input: &[&str]) {
        args.extend(["-thread_queue_size".into(), "512".into()]);
        args.extend(input.iter().map(|s| s.to_string()));
    }

    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            let title = exe_name.and_then(find_window_title_for_exe);
            let (mut args, window_scoped) = match title {
                Some(t) => {
                    let mut a = Vec::new();
                    push_input(&mut a, &["-f", "gdigrab", "-i", &format!("title={t}")]);
                    (a, true)
                }
                None => {
                    let mut a = Vec::new();
                    push_input(&mut a, &["-f", "gdigrab", "-i", "desktop"]);
                    (a, false)
                }
            };

            let devices = dshow_audio_devices();
            let mut audio_maps = Vec::new();
            let mut pipe_audio = None;
            let mut input_idx = 1;
            if with_mic {
                if let Some(mic) = dshow_mic_device(&devices) {
                    push_input(&mut args, &["-f", "dshow", "-i", &format!("audio={mic}")]);
                    audio_maps.push(format!("{input_idx}:a"));
                    input_idx += 1;
                }
            }
            if with_loopback {
                // Game/system audio: native WASAPI loopback first — captures the default output
                // device directly (any headphones/speakers, zero setup, the way Medal/ShadowPlay
                // do it), raw PCM piped into stdin by loopback.rs. A dshow loopback DEVICE is
                // only the fallback for the rare machine where the WASAPI route can't open.
                if let Some(spec) = crate::loopback::default_output_spec() {
                    let (ar, ac) = (spec.sample_rate.to_string(), spec.channels.to_string());
                    push_input(
                        &mut args,
                        &["-f", spec.format, "-ar", &ar, "-ac", &ac, "-i", "pipe:0"],
                    );
                    audio_maps.push(format!("{input_idx}:a"));
                    pipe_audio = Some(spec);
                } else if let Some(lb) = devices.iter().find(|d| is_loopback_device_name(d)) {
                    push_input(&mut args, &["-f", "dshow", "-i", &format!("audio={lb}")]);
                    audio_maps.push(format!("{input_idx}:a"));
                }
            }
            Some(CaptureInputs { args, window_scoped, audio_maps, pipe_audio })
        }
        #[cfg(not(target_os = "windows"))]
        None
    } else if cfg!(target_os = "macos") {
        #[cfg(target_os = "macos")]
        {
            let (screen, loopback, real_mic) = avfoundation_devices();
            let screen = screen.unwrap_or(2);
            let mut args = Vec::new();
            let mut audio_maps = Vec::new();
            // Mic selector: normally avfoundation's `default` keyword, but never a loopback —
            // if the system-default input IS one (BlackHole set itself as default input in live
            // testing), `default` would record guaranteed silence, so fall back to a real mic
            // by index; no real mic installed means no mic track at all.
            let mic_selector = if with_mic {
                match macos_default_input_name() {
                    Some(name) if is_loopback_device_name(&name) => match real_mic {
                        Some(idx) => {
                            eprintln!(
                                "clipper: default input '{name}' is a loopback — using real mic [{idx}] instead"
                            );
                            Some(idx.to_string())
                        }
                        None => {
                            eprintln!(
                                "clipper: default input '{name}' is a loopback and no real mic exists — skipping mic"
                            );
                            None
                        }
                    },
                    _ => Some("default".to_string()),
                }
            } else {
                None
            };
            // Screen + mic ride in one avfoundation input ("video:audio").
            let input = match &mic_selector {
                Some(sel) => format!("{screen}:{sel}"),
                None => format!("{screen}:none"),
            };
            push_input(&mut args, &["-f", "avfoundation", "-i", &input]);
            if mic_selector.is_some() {
                audio_maps.push("0:a".into());
            }
            // The loopback (game/system audio) needs its own avfoundation instance — one input
            // can only open a single audio device.
            if with_loopback {
                if let Some(lb) = loopback {
                    push_input(&mut args, &["-f", "avfoundation", "-i", &format!("none:{lb}")]);
                    audio_maps.push("1:a".into());
                }
            }
            Some(CaptureInputs { args, window_scoped: false, audio_maps, pipe_audio: None })
        }
        #[cfg(not(target_os = "macos"))]
        None
    } else {
        None
    }
}

fn wipe_dir(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn capture_pid_file(dir: &Path) -> PathBuf {
    dir.join("capture.pid")
}

/// Kills a capture ffmpeg left behind by a previous life of this app. The `RunEvent::Exit`
/// cleanup only runs on a graceful quit — an app crash, force-kill, or (constantly, in dev)
/// `tauri dev`'s hard-kill on rebuild orphans the child ffmpeg, which then keeps recording the
/// full screen indefinitely and floods the buffer with unpaused/unscoped footage that the new
/// instance's save path happily picks up. Every spawn records its ffmpeg PID to `capture.pid`;
/// this reads it back and kills the process — only after verifying the PID still belongs to an
/// ffmpeg, since PIDs get reused. Called at app startup and before every fresh capture spawn.
pub fn reap_orphan_capture(app: &AppHandle) {
    let dir = buffer_dir(app);
    let path = capture_pid_file(&dir);
    let Ok(contents) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let Ok(pid) = contents.trim().parse::<u32>() else { return };

    let mut sys = sysinfo::System::new();
    sys.refresh_processes();
    if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid)) {
        if process.name().to_lowercase().contains("ffmpeg") {
            eprintln!("clipper: killing orphaned capture ffmpeg (pid {pid}) from a previous run");
            process.kill();
        }
    }
}

/// Deletes the oldest segments beyond the ring-buffer depth. ffmpeg's `-segment_wrap` only rings
/// within a single spawn's own filename sequence — pause/resume and crash restarts each spawn a
/// fresh ffmpeg with a fresh sequence prefix (so it can't overwrite the previous run's still-
/// valid footage), which means the overall cap has to be enforced here instead.
fn trim_buffer(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut segments: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "ts"))
        .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path())))
        .collect();
    segments.sort_by_key(|(t, _)| *t);
    let excess = segments.len().saturating_sub(BUFFER_SEGMENTS as usize);
    for (_, path) in segments.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

/// Monotonic per-spawn sequence for segment filename prefixes — see `trim_buffer` for why every
/// spawn needs its own namespace.
static SPAWN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Spawns one rolling-buffer ffmpeg process writing into `dir`. Uses the bundled sidecar ffmpeg
/// when present, PATH otherwise — see `ffmpeg_path`.
fn spawn_ffmpeg(dir: &Path, inputs: &CaptureInputs) -> Option<Child> {
    let seq = SPAWN_SEQ.fetch_add(1, Ordering::SeqCst);
    let pattern = dir.join(format!("segment_{seq:05}_%03d.ts"));

    // std::process, not tokio::process: this can run from sync contexts before any Tokio reactor
    // is guaranteed to exist (spawning a tokio::process::Child without one panics with "no
    // reactor running"), and fire-and-forget is all that's needed.
    let mut cmd = std::process::Command::new(ffmpeg_path());
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(&inputs.args)
        .args(["-framerate", "30"]);

    // Explicit stream mapping — with multiple live inputs, ffmpeg's default "best stream"
    // selection is not what we want. Two audio tracks (mic + system loopback) get mixed into
    // one; `normalize=0` keeps real volumes instead of halving both to guarantee headroom.
    match inputs.audio_maps.as_slice() {
        [] => {
            cmd.args(["-map", "0:v"]);
        }
        [only] => {
            cmd.args(["-map", "0:v", "-map", only]);
        }
        [first, second, ..] => {
            // Each input is upconverted to 48kHz stereo BEFORE mixing — amix otherwise
            // negotiates the lowest common format, and a 16kHz-mono headset mic would drag the
            // game audio down with it (observed live).
            cmd.args([
                "-filter_complex",
                &format!(
                    "[{first}]aresample=48000,aformat=channel_layouts=stereo[a0];\
                     [{second}]aresample=48000,aformat=channel_layouts=stereo[a1];\
                     [a0][a1]amix=inputs=2:duration=longest:normalize=0[aout]"
                ),
                "-map",
                "0:v",
                "-map",
                "[aout]",
            ]);
        }
    }

    cmd
        // Screen-capture inputs (avfoundation/gdigrab) report irregular timestamps, so libx264's
        // default keyframe-interval heuristic (frame-count based) doesn't land near real 10s
        // wall-clock boundaries — the segment muxer only cuts at keyframes, so without forcing
        // them explicitly a segment can run for minutes without ever rotating. `-r 30` gives a
        // stable output frame rate and `-force_key_frames` forces one exactly every
        // SEGMENT_SECONDS.
        .args(["-r", "30"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p"])
        // No-op when the spawn has no audio input; encodes the mic track when it does.
        .args(["-c:a", "aac", "-b:a", "160k"])
        .args(["-force_key_frames", &format!("expr:gte(t,n_forced*{SEGMENT_SECONDS})")])
        .args(["-f", "segment"])
        .args(["-segment_format", "mpegts"])
        .args(["-segment_time", &SEGMENT_SECONDS.to_string()])
        .args(["-segment_wrap", &BUFFER_SEGMENTS.to_string()])
        .args(["-reset_timestamps", "1"])
        .arg(pattern)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // stdin carries the WASAPI loopback PCM when that input is in play (Windows); otherwise it
    // stays closed so ffmpeg can't block reading it.
    if inputs.pipe_audio.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    match cmd.spawn() {
        Ok(mut child) => {
            // `pipe_audio` is only ever set on Windows; the loopback feed owns the pipe from
            // here on and dies with the child (broken pipe) — nothing to store or join.
            if let (Some(spec), Some(stdin)) = (inputs.pipe_audio, child.stdin.take()) {
                #[cfg(target_os = "windows")]
                crate::loopback::start(stdin, spec);
                #[cfg(not(target_os = "windows"))]
                drop((stdin, spec));
            }
            // Recorded so a future life of this app can reap this ffmpeg if we die without
            // running our exit cleanup — see reap_orphan_capture.
            let _ = std::fs::write(capture_pid_file(dir), child.id().to_string());
            Some(child)
        }
        Err(e) => {
            eprintln!("clipper: failed to start capture buffer (ffmpeg missing?): {e}");
            None
        }
    }
}

/// Logs a focus sample every second while a non-window-scoped capture is live — the samples
/// drive `save_clip`'s output black-out (unfocused time ranges get masked in the saved clip).
/// The capture itself is never touched: the buffer records continuously, exactly like
/// Medal/ShadowPlay-style tools, and exclusion happens at clip time. Exits when its generation
/// is superseded (session ended / fresh capture started) or the capture becomes window-scoped
/// (gdigrab keeps grabbing the game's own window through alt-tabs, so there's nothing to mask).
fn spawn_focus_sampler(app: &AppHandle, generation: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let exe = {
                let state = app.state::<CaptureState>();
                let slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
                if slot.generation != generation || slot.phase.is_none() {
                    return;
                }
                if matches!(slot.phase, Some(Capture { window_scoped: true, .. })) {
                    return; // upgraded to window-scoped — focus no longer matters
                }
                slot.exe_name.clone()
            };
            let Some(exe) = exe else { continue };

            // The focus check happens outside any lock (it can shell out on macOS).
            if let Some(focused) = is_frontmost(&exe).await {
                record_focus_sample(&app, focused);
            }
        }
    });
}

/// Reconciles capture with "a game session is active" — called by tracker.rs on every poll while
/// at least one session is open. Jobs:
/// - starts capture (and its focus sampler, when not window-scoped) if it isn't running yet;
/// - watchdog: restarts it if the ffmpeg process died (capture error, its window closed, disk
///   trouble) — without this, a dead capture would silently leave `save_clip` stitching clips
///   out of stale footage;
/// - Windows: upgrades a desktop-fallback capture to window-scoped once the game's window can be
///   resolved, discarding the desktop footage recorded in the meantime.
///
/// Focus changes never touch the capture process — the buffer records continuously and unfocused
/// spans are masked at save time (see the module comment).
pub fn ensure_capture(app: &AppHandle, exe_name: Option<&str>) {
    let state = app.state::<CaptureState>();
    let mut slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(exe) = exe_name {
        slot.exe_name = Some(exe.to_string());
    }

    // `wipe` distinguishes "existing footage must be discarded" (fresh session start; desktop-
    // fallback footage after an upgrade) from "existing footage is legit game content" (crash
    // restart keeps it).
    let (respawn, wipe) = match &mut slot.phase {
        None => {
            slot.respawn_strikes = 0;
            (true, true)
        }
        Some(Capture { child, window_scoped }) => match child.try_wait() {
            Ok(None) => {
                if !*window_scoped && window_now_resolvable(exe_name) {
                    let _ = child.kill();
                    (true, true)
                } else {
                    // Healthy run — a past strike was transient, not the audio input.
                    slot.respawn_strikes = 0;
                    (false, false)
                }
            }
            _ if slot.restart_requested => {
                // Deliberate kill (mic setting changed) — not an audio-input failure, and the
                // old footage has a different stream layout than the new spawn will produce.
                slot.restart_requested = false;
                (true, true)
            }
            _ => {
                slot.respawn_strikes = slot.respawn_strikes.saturating_add(1);
                if slot.respawn_strikes == 2 {
                    eprintln!(
                        "clipper: capture died twice in a row — retrying without the audio input"
                    );
                }
                (true, false)
            }
        },
    };

    let dir = buffer_dir(app);
    if respawn {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("clipper: failed to create buffer dir: {e}");
            return;
        }
        if wipe {
            // A fresh start is also the moment to kill any ffmpeg orphaned by a previous life
            // of the app — otherwise it keeps flooding the freshly-wiped buffer with footage
            // that this instance would then serve up in clips.
            reap_orphan_capture(app);
            wipe_dir(&dir);
        }

        let exe = slot.exe_name.clone();
        let mic_enabled = {
            let db = app.state::<DbState>();
            db.0.lock().ok().map(|conn| read_mic_enabled(&conn)).unwrap_or(true)
        };
        // After two capture deaths in a row, drop every audio input (mic AND loopback) — audio
        // devices are the usual suspect for instant spawn failures.
        let audio_healthy = slot.respawn_strikes < 2;
        let Some(inputs) =
            capture_input_args(exe.as_deref(), mic_enabled && audio_healthy, audio_healthy)
        else {
            eprintln!("clipper: no screen-capture input configured for this OS, buffer not started");
            slot.phase = None;
            return;
        };
        let window_scoped = inputs.window_scoped;
        match spawn_ffmpeg(&dir, &inputs) {
            Some(child) => {
                slot.generation += 1;
                slot.phase = Some(Capture { child, window_scoped });
                if !window_scoped {
                    spawn_focus_sampler(app, slot.generation);
                }
            }
            None => slot.phase = None,
        }
    }

    trim_buffer(&dir);
}

/// Ends capture entirely — called whenever the last tracked game session ends (so nothing records
/// while the player isn't in a game) and on app exit as a final-cleanup safety net. Bumping the
/// generation detaches any focus sampler. Also wipes the buffer directory: without this, old
/// segment files would sit on disk until the next capture start, letting `save_clip` stitch
/// together a "clip" from footage of a game the player already quit. Idempotent and cheap when
/// already stopped.
pub fn stop(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    let mut slot = match state.0.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let Some(mut capture) = slot.phase.take() else {
        return;
    };
    slot.generation += 1;
    slot.exe_name = None;
    drop(slot);

    let _ = capture.child.kill();
    let dir = buffer_dir(app);
    let _ = std::fs::remove_file(capture_pid_file(&dir));
    wipe_dir(&dir);

    // Stale focus samples must not bleed into the next session's save-time masking.
    let focus = app.state::<FocusLog>();
    focus.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Whether a game session currently has capture. `save_clip` uses this to refuse outright when
/// no game is being tracked, rather than relying on the buffer directory happening to be empty.
fn is_capturing(app: &AppHandle) -> bool {
    let state = app.state::<CaptureState>();
    let slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    slot.phase.is_some()
}

/// Whether the current capture is window-scoped (see `Capture::window_scoped`) — a window-scoped
/// clip needs no focus masking.
fn is_window_scoped(app: &AppHandle) -> bool {
    let state = app.state::<CaptureState>();
    let slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    matches!(slot.phase, Some(Capture { window_scoped: true, .. }))
}

fn read_clip_seconds(conn: &rusqlite::Connection) -> u32 {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [CLIP_SECONDS_SETTING_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|v| v.parse::<u32>().ok())
    .unwrap_or(DEFAULT_CLIP_SECONDS)
}

#[tauri::command]
pub fn get_clip_seconds(db: tauri::State<DbState>) -> Result<u32, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(read_clip_seconds(&conn))
}

#[tauri::command]
pub fn set_clip_seconds(db: tauri::State<DbState>, seconds: u32) -> Result<(), String> {
    let seconds = seconds.clamp(MIN_CLIP_SECONDS, MAX_CLIP_SECONDS);
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![CLIP_SECONDS_SETTING_KEY, seconds.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn read_mic_enabled(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [MIC_ENABLED_SETTING_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .map(|v| v != "0")
    .unwrap_or(true)
}

#[tauri::command]
pub fn get_mic_enabled(db: tauri::State<DbState>) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(read_mic_enabled(&conn))
}

#[tauri::command]
pub fn set_mic_enabled(
    app: AppHandle,
    db: tauri::State<DbState>,
    enabled: bool,
) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![MIC_ENABLED_SETTING_KEY, if enabled { "1" } else { "0" }],
        )
        .map_err(|e| e.to_string())?;
    }
    // Apply immediately to a live capture: kill it and let the tracker's next poll (≤5s)
    // respawn it with the new setting — the setting is only read at spawn time, and live
    // testing showed a toggle that silently doesn't apply until the next game session reads as
    // broken. `restart_requested` keeps the watchdog from counting this as an audio failure.
    let state = app.state::<CaptureState>();
    let mut slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    if slot.phase.is_some() {
        slot.restart_requested = true;
    }
    if let Some(capture) = slot.phase.as_mut() {
        let _ = capture.child.kill();
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: i64,
    pub game_id: Option<i64>,
    pub game_name: Option<String>,
    pub file_path: String,
    pub thumbnail_path: Option<String>,
    pub duration_seconds: Option<i64>,
    pub created_at: String,
    pub title: Option<String>,
    pub notes: Option<String>,
}

/// Reads back the just-inserted row so `save_clip`/`get_clips` share one shape.
fn load_clip(conn: &rusqlite::Connection, id: i64) -> Result<Clip, String> {
    conn.query_row(
        "SELECT c.id, c.game_id, g.name, c.file_path, c.thumbnail_path, c.duration_seconds,
                c.created_at, c.title, c.notes
         FROM clips c LEFT JOIN games g ON g.id = c.game_id
         WHERE c.id = ?1",
        [id],
        |row| {
            Ok(Clip {
                id: row.get(0)?,
                game_id: row.get(1)?,
                game_name: row.get(2)?,
                file_path: row.get(3)?,
                thumbnail_path: row.get(4)?,
                duration_seconds: row.get(5)?,
                created_at: row.get(6)?,
                title: row.get(7)?,
                notes: row.get(8)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_clips(db: tauri::State<DbState>) -> Result<Vec<Clip>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.game_id, g.name, c.file_path, c.thumbnail_path, c.duration_seconds,
                    c.created_at, c.title, c.notes
             FROM clips c LEFT JOIN games g ON g.id = c.game_id
             ORDER BY c.created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let clips = stmt
        .query_map([], |row| {
            Ok(Clip {
                id: row.get(0)?,
                game_id: row.get(1)?,
                game_name: row.get(2)?,
                file_path: row.get(3)?,
                thumbnail_path: row.get(4)?,
                duration_seconds: row.get(5)?,
                created_at: row.get(6)?,
                title: row.get(7)?,
                notes: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(clips)
}

#[tauri::command]
pub fn delete_clip(db: tauri::State<DbState>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let (file_path, thumbnail_path): (String, Option<String>) = conn
        .query_row(
            "SELECT file_path, thumbnail_path FROM clips WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM clips WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&file_path);
    if let Some(thumb) = thumbnail_path {
        let _ = std::fs::remove_file(thumb);
    }
    Ok(())
}

/// The game (id + name) to attach a clip to: whichever game has an open (`ended_at IS NULL`)
/// session right now, same source of truth `get_currently_playing` uses.
fn currently_playing_game(conn: &rusqlite::Connection) -> Option<(i64, String)> {
    conn.query_row(
        "SELECT g.id, g.name FROM sessions s JOIN games g ON g.id = s.game_id
         WHERE s.ended_at IS NULL ORDER BY s.started_at DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .ok()
    .flatten()
}

/// Picks the segment files that cover the trailing `seconds` of buffer, oldest first (ffmpeg's
/// concat demuxer requires playback order), returning them together with their total covered
/// duration so `save_clip` can cut the output to exactly the requested length. The newest
/// (still-being-written) segment IS included — TS is readable mid-write, and it holds the
/// footage from right before the hotkey press.
///
/// Selection accumulates each segment's real covered duration from mtime deltas between
/// consecutive files rather than assuming a fixed 10s per file, so partial segments don't skew
/// the math. Falls back to whatever exists if the buffer hasn't filled that far yet (e.g. right
/// after a game launches).
/// Returned by `recent_segments`: the files plus the wall-clock window they cover, anchored on
/// the newest file's mtime and summed per-file spans. Both the `-ss` output cut and the focus-
/// mask timeline are computed against this window, so it must reflect the footage that's
/// actually in the files — see the comments inside `recent_segments` for the two timestamp
/// traps (recycled birth times, capture-death holes) that previously broke it.
struct SelectedSegments {
    files: Vec<PathBuf>,
    wall_start: std::time::SystemTime,
    total_secs: f64,
}

fn recent_segments(dir: &PathBuf, seconds: u32) -> Result<SelectedSegments, String> {
    // Only mtimes — NEVER file creation times. `-segment_wrap` recycles segment files in place,
    // and an overwritten file keeps its original birth time on macOS, so once the ring wraps
    // (~3 min into a session) birth-based math thought a 10s segment spanned minutes. That
    // inflated `total_secs`, pushed the output `-ss` seek past the end of the real footage, and
    // produced the "unplayable 262-byte clip" failures live testing hit after longer sessions.
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "ts"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            // A spawn that dies instantly (e.g. respawn racing the killed process's device
            // release — the watchdog heals it seconds later) leaves a 0-byte segment behind,
            // and ffmpeg's concat demuxer hard-fails on empty inputs.
            if meta.len() == 0 {
                return None;
            }
            Some((meta.modified().ok()?, e.path()))
        })
        .collect();
    entries.sort_by_key(|(modified, _)| *modified);

    if entries.is_empty() {
        return Err("No footage captured yet — the buffer needs a few seconds to fill.".into());
    }

    // Each segment's covered duration is the mtime DELTA to its predecessor (a segment's mtime
    // is when its last frame was written, its predecessor's mtime is when its first frame was) —
    // capped at the nominal segment length. A delta far beyond the nominal length means a hole
    // in the footage (capture died and was respawned): selection stops there, because concat
    // splices across the hole and every wall-clock-anchored calculation (the `-ss` cut, the
    // focus-mask timeline) would silently shift by the hole's width for everything before it.
    const GAP_SECS: f64 = SEGMENT_SECONDS as f64 + 8.0;

    let wall_end = entries.last().map(|(m, _)| *m).unwrap_or_else(std::time::SystemTime::now);

    let mut total_secs: f64 = 0.0;
    let mut picked: Vec<PathBuf> = Vec::new();
    for window in entries.windows(2).rev() {
        let (prev_mtime, _) = &window[0];
        let (mtime, path) = &window[1];
        let delta = mtime
            .duration_since(*prev_mtime)
            .map(|d| d.as_secs_f64())
            .unwrap_or(SEGMENT_SECONDS as f64);
        if delta > GAP_SECS {
            break;
        }
        picked.push(path.clone());
        total_secs += delta.min(SEGMENT_SECONDS as f64);
        if total_secs >= seconds as f64 {
            break;
        }
    }
    // The oldest file in the buffer has no predecessor to diff against — assume a full segment.
    if total_secs < seconds as f64 && picked.len() == entries.len() - 1 {
        picked.push(entries[0].1.clone());
        total_secs += SEGMENT_SECONDS as f64;
    }
    if picked.is_empty() {
        // Single file in the buffer (capture just started) — take it as-is.
        picked.push(entries[0].1.clone());
        total_secs = SEGMENT_SECONDS as f64;
    }
    picked.reverse();

    let wall_start = wall_end
        .checked_sub(std::time::Duration::from_secs_f64(total_secs))
        .unwrap_or(wall_end);

    Ok(SelectedSegments { files: picked, wall_start, total_secs })
}

/// Reads the real duration of a finished clip by parsing `Duration: HH:MM:SS.cc` from
/// `ffmpeg -i`'s metadata dump — the segment-count estimate can be off by up to a whole segment
/// because the live segment is partial. Falls back to `None` (caller estimates) if parsing fails.
async fn probe_duration_seconds(path: &Path) -> Option<i64> {
    let output = tokio::process::Command::new(ffmpeg_path())
        .arg("-i")
        .arg(path)
        .args(["-hide_banner"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .await
        .ok()?;
    // ffmpeg exits non-zero with "At least one output file must be specified" — expected; the
    // metadata we want is still printed to stderr before that.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().find(|l| l.trim_start().starts_with("Duration:"))?;
    let value = line.trim_start().strip_prefix("Duration:")?.trim().split(',').next()?;
    let mut parts = value.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600 + minutes * 60 + seconds.round() as i64)
}

/// Concatenates the trailing `seconds` of the rolling buffer into a standalone clip file (stream
/// Builds the ffmpeg `enable` expression masking every unfocused span inside the concat
/// timeline. `wall_start` is the wall-clock moment the concat's t=0 corresponds to (save time
/// minus total covered duration). Samples are merged into ranges (gaps under 3s bridge — the 1s
/// sampler can miss a beat) and padded ±1.5s so the swipe-in/out transition frames land inside
/// the mask; erring toward masking more is deliberate. Returns `None` when nothing needs
/// masking.
fn blackout_enable_expr(
    app: &AppHandle,
    wall_start: std::time::SystemTime,
    total_secs: f64,
) -> Option<String> {
    // Asymmetric pads: the sampler runs at 1s cadence, so the FIRST unfocused sample can land a
    // full second after the real tab-out (plus the swipe animation before it) — the leading pad
    // has to swallow that latency or a glimpse of the desktop leaks at the mask's start edge.
    const PAD_BEFORE: f64 = 2.0;
    const PAD_AFTER: f64 = 1.5;
    const MERGE_GAP: f64 = 3.0;
    const MAX_RANGES: usize = 24;

    let state = app.state::<FocusLog>();
    let log = state.0.lock().unwrap_or_else(|e| e.into_inner());

    let mut times: Vec<f64> = log
        .iter()
        .filter(|(_, focused)| !focused)
        .filter_map(|(t, _)| t.duration_since(wall_start).ok().map(|d| d.as_secs_f64()))
        .filter(|t| *t <= total_secs + PAD_BEFORE)
        .collect();
    drop(log);
    if times.is_empty() {
        return None;
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut ranges: Vec<(f64, f64)> = Vec::new();
    for t in times {
        let (start, end) = ((t - PAD_BEFORE).max(0.0), (t + PAD_AFTER).min(total_secs));
        match ranges.last_mut() {
            Some((_, last_end)) if start - *last_end <= MERGE_GAP => *last_end = end,
            _ => ranges.push((start, end)),
        }
    }
    ranges.truncate(MAX_RANGES);

    let expr = ranges
        .iter()
        .map(|(a, b)| format!("between(t,{a:.1},{b:.1})"))
        .collect::<Vec<_>>()
        .join("+");
    Some(expr)
}

/// Parses the capture's pixel dimensions from `ffmpeg -i`'s stream metadata for a segment file
/// (same trick as `probe_duration_seconds`). Needed to map the game window's point-space bounds
/// onto capture pixels for the save-time crop.
async fn probe_dimensions(path: &Path) -> Option<(u32, u32)> {
    let output = tokio::process::Command::new(ffmpeg_path())
        .arg("-i")
        .arg(path)
        .args(["-hide_banner"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .await
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().find(|l| l.contains(" Video:"))?;
    // The dimensions token looks like "2560x1664" among comma-separated stream parameters. The
    // minimum-size check matters: the codec-tag token ("0x31637661") appears earlier in the
    // line and would otherwise parse as 0×31637661.
    line.split(&[',', ' '][..]).find_map(|tok| {
        let (w, h) = tok.split_once('x')?;
        let (w, h): (u32, u32) =
            (w.parse().ok()?, h.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok()?);
        (w >= 16 && h >= 16 && h < 20000).then_some((w, h))
    })
}

/// The exe name of the game the current capture is for — used by the save-time window crop.
fn current_exe_name(app: &AppHandle) -> Option<String> {
    let state = app.state::<CaptureState>();
    let slot = state.0.lock().unwrap_or_else(|e| e.into_inner());
    slot.exe_name.clone()
}

/// The bundled "Tabbed out" overlay image (vignette + label, pre-rendered because ffmpeg's
/// `drawtext` filter isn't reliably compiled in — the Homebrew build hard-fails on it). `None`
/// (resource missing) degrades to a flat black mask rather than failing the save.
fn tabbed_out_overlay(app: &AppHandle) -> Option<PathBuf> {
    use tauri::path::BaseDirectory;
    app.path()
        .resolve("resources/tabbed_out.png", BaseDirectory::Resource)
        .ok()
        .filter(|p| p.exists())
}

/// Cuts the trailing `seconds` of the rolling buffer into a standalone clip file and rolls a
/// `clips` row for it, best-effort attached to whichever game is currently being tracked.
/// Extracts a thumbnail for the Clips grid; progress/success feedback shows via the in-game
/// overlay toast (see overlay.rs).
#[tauri::command]
pub async fn save_clip(
    app: AppHandle,
    db: tauri::State<'_, DbState>,
    seconds: Option<u32>,
) -> Result<Clip, String> {
    let _guard = SaveGuard::acquire()?;

    if !is_capturing(&app) {
        return Err("No game is currently being tracked — nothing to clip.".into());
    }

    let seconds = match seconds {
        Some(s) => s.clamp(MIN_CLIP_SECONDS, MAX_CLIP_SECONDS),
        None => {
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            read_clip_seconds(&conn)
        }
    };

    // Fired before the (multi-second) re-encode below so the player gets immediate feedback
    // that the hotkey registered — the overlay toast then updates in place to saved/failed.
    crate::overlay::toast(&app, "saving", format!("Saving the last {seconds}s…"));
    let _ = app.emit("clip-saving", seconds);

    let dir = buffer_dir(&app);
    let selected = recent_segments(&dir, seconds)?;
    let segments = &selected.files;

    let out_dir = clips_dir(&app);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    // Millisecond precision so rapid consecutive saves can't collide on the same filename
    // (second-resolution names let a double press overwrite the first clip's file while both
    // DB rows pointed at it).
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f");
    let clip_path = out_dir.join(format!("clip_{timestamp}.mp4"));
    let thumb_path = out_dir.join(format!("clip_{timestamp}.jpg"));

    let list_path = out_dir.join(format!("clip_{timestamp}.txt"));
    let list_contents = segments
        .iter()
        .map(|p| format!("file '{}'", p.to_string_lossy().replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&list_path, list_contents).map_err(|e| e.to_string())?;

    // Crop the output down to just the game's window (macOS only — Windows' window-scoped
    // capture already contains nothing else). The window's current bounds at save time are used
    // for the whole clip; a window dragged mid-clip will show slightly offset early footage,
    // which is the accepted tradeoff for never recording desktop pixels around the game.
    let crop = match probe_dimensions(&segments[0]).await {
        Some((w, h)) if !is_window_scoped(&app) => current_exe_name(&app)
            .and_then(|exe| crop_filter_for_game_window(&exe, w, h))
            .map(|c| format!("{c},"))
            .unwrap_or_default(),
        _ => String::new(),
    };

    // Mask unfocused spans in the OUTPUT (never needed for a window-scoped capture, which only
    // ever contains the game's own window). The filter timeline runs before the output-side
    // `-ss` cut, so range times are concat times, not output times. The mask is the bundled
    // "Tabbed out" vignette image scaled to the frame — an image overlay costs nothing per
    // frame, unlike the per-pixel `geq` gradient it replaced, which pushed a 30s save from ~3s
    // to ~10s at Retina resolutions.
    let mask_expr = (!is_window_scoped(&app))
        .then(|| blackout_enable_expr(&app, selected.wall_start, selected.total_secs))
        .flatten();
    let overlay_png = mask_expr.as_ref().and_then(|_| tabbed_out_overlay(&app));

    // The selection covers AT LEAST the requested length (whole segments); `-ss`/`-t` on the
    // output cut it to exactly the requested trailing window — clips used to drift 24-40s for a
    // "30s" setting when this was done by whole-segment granularity alone. Re-encode, not
    // `-c copy`: output-accurate seeking and the mask filter both require decoding anyway, and
    // it makes the output robust to any stream-parameter drift between spawns. ~2-4s for a 30s
    // clip at veryfast; every mainstream clipping tool re-encodes on save.
    let skip_secs = (selected.total_secs - seconds as f64).max(0.0);
    let mut cmd = tokio::process::Command::new(ffmpeg_path());
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-fflags", "+genpts"])
        .args(["-f", "concat", "-safe", "0"])
        .arg("-i")
        .arg(&list_path);
    match (&mask_expr, &overlay_png) {
        // Vignette overlay: scale the image to the (cropped) frame via scale2ref, then overlay
        // it only during masked ranges. Needs -filter_complex for the second input, which in
        // turn needs explicit -map ("0:a?" keeps the mic track when the buffer has one).
        (Some(expr), Some(png)) => {
            cmd.arg("-i").arg(png).args([
                "-filter_complex",
                &format!(
                    "[0:v]{crop}fps=30[base];[1:v][base]scale2ref=w=iw:h=ih[ov][b];\
                     [b][ov]overlay=0:0:enable='{expr}'[v]"
                ),
                "-map",
                "[v]",
                "-map",
                "0:a?",
            ]);
        }
        // Overlay image missing — flat black fill, still masked correctly.
        (Some(expr), None) => {
            cmd.args([
                "-vf",
                &format!("{crop}drawbox=x=0:y=0:w=iw:h=ih:t=fill:color=black:enable='{expr}',fps=30"),
            ]);
        }
        _ => {
            cmd.args(["-vf", &format!("{crop}fps=30")]);
        }
    }
    let concat_output = cmd
        .args(["-ss", &format!("{skip_secs:.2}")])
        .args(["-t", &seconds.to_string()])
        .args(["-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p"])
        // The audio track (mic) rides through untouched by the video mask — deliberately
        // recorded across tab-outs too. No-op for audio-less buffers.
        .args(["-c:a", "aac", "-b:a", "160k"])
        .arg(&clip_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&list_path);

    if !concat_output.status.success() {
        // Surface ffmpeg's own first error line — "failed to assemble" alone made a whole class
        // of bugs (0-byte segments in the list) undiagnosable from the app's logs.
        let stderr = String::from_utf8_lossy(&concat_output.stderr);
        let detail = stderr.lines().next().unwrap_or("no error output");
        eprintln!("clipper: clip assembly failed: {stderr}");
        return Err(format!("ffmpeg failed to assemble the clip ({detail})"));
    }

    // Best-effort — a missing thumbnail shouldn't fail the whole save.
    let _ = tokio::process::Command::new(ffmpeg_path())
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-ss", "1"])
        .arg("-i")
        .arg(&clip_path)
        .args(["-vframes", "1"])
        .arg(&thumb_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    let thumbnail_path = thumb_path.exists().then(|| thumb_path.to_string_lossy().to_string());

    // A zero-frame output is a failed save even when ffmpeg exits 0 — it happily writes an
    // empty MP4 when the `-ss` seek lands past the end of the input. The duration math above is
    // supposed to prevent that, but if it's ever wrong again, refuse to catalog the junk file as
    // a "clip" (live testing produced unplayable 262-byte entries in the gallery this way).
    let duration_seconds = match probe_duration_seconds(&clip_path).await {
        Some(d) if d > 0 => d,
        _ => {
            let _ = std::fs::remove_file(&clip_path);
            let _ = std::fs::remove_file(&thumb_path);
            return Err("Clip came out empty — the capture buffer had no usable footage.".into());
        }
    };
    let clip_path_str = clip_path.to_string_lossy().to_string();

    let (clip_id, game_name) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let playing = currently_playing_game(&conn);
        let game_id = playing.as_ref().map(|(id, _)| *id);
        let id = conn
            .query_row(
                "INSERT INTO clips (game_id, file_path, thumbnail_path, duration_seconds, title)
                 VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
                rusqlite::params![
                    game_id,
                    clip_path_str,
                    thumbnail_path,
                    duration_seconds,
                    format!("Clip - {}", chrono::Local::now().format("%b %-d, %-I:%M %p")),
                ],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        (id, playing.map(|(_, name)| name))
    };

    let clip = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        load_clip(&conn, clip_id)?
    };

    let toast_text = match &game_name {
        Some(name) => format!("Clip saved — last {duration_seconds}s of {name}"),
        None => format!("Clip saved — last {duration_seconds}s"),
    };
    crate::overlay::toast(&app, "saved", toast_text);

    let _ = app.emit("clip-saved", clip.clone());
    Ok(clip)
}

/// Runs `save_clip` from the global-shortcut handler, which isn't itself async — spawns onto the
/// async runtime. Failures surface on the in-game overlay toast (the player is in a game and
/// can't see the app window) in addition to the `clip-save-failed` event for the Clips view.
pub fn save_clip_from_hotkey(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let db = app.state::<DbState>();
        match save_clip(app.clone(), db, None).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("clipper: hotkey save failed: {e}");
                crate::overlay::toast(&app, "failed", e.as_str());
                let _ = app.emit("clip-save-failed", e);
            }
        }
    });
}
