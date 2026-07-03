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
use tauri_plugin_notification::NotificationExt;

const SEGMENT_SECONDS: u32 = 10;
/// Ring buffer depth — 18 * 10s = 3 minutes of rolling footage available to save from.
const BUFFER_SEGMENTS: u32 = 18;
/// Fallback when no `clip_seconds` row exists in `settings` yet.
const DEFAULT_CLIP_SECONDS: u32 = 30;
const MIN_CLIP_SECONDS: u32 = 5;
const MAX_CLIP_SECONDS: u32 = 120;
const CLIP_SECONDS_SETTING_KEY: &str = "clip_seconds";

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
    phase: Option<Capture>,
}

impl CaptureSlot {
    pub fn idle() -> Self {
        CaptureSlot { generation: 0, exe_name: None, respawn_strikes: 0, phase: None }
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

/// Windows-only: first dshow audio device name (the default mic, typically), enumerated at
/// spawn time via ffmpeg's device listing. Mic capture is the audio ffmpeg CAN do with zero
/// external setup on Windows — full system-audio loopback (game sound, Discord calls) needs
/// either a third-party virtual device (VB-Cable — an install we refuse to require) or a
/// WASAPI-loopback capture backend beyond plain ffmpeg; that upgrade is planned alongside the
/// packaging work, not patched here.
#[cfg(target_os = "windows")]
fn first_dshow_audio_device() -> Option<String> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        // Lines look like: [dshow @ ...] "Microphone (Realtek Audio)" (audio)
        if line.contains("(audio)") {
            let start = line.find('"')?;
            let rest = &line[start + 1..];
            let end = rest.find('"')?;
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// OS-specific capture-input args for ffmpeg, plus whether the capture is scoped to the game's
/// window (`true` = structurally contains only the game's own window). On Windows, scopes to the
/// tracked game's window when its title resolves, falling back to the full desktop otherwise.
/// macOS is always full-screen (`false`) — ffmpeg's avfoundation input has no window mode, a
/// real, documented limitation of the dev-only Mac path; the focus sampler + save-time black-out
/// compensate. `None` on unsupported OSes.
///
/// `with_audio` adds a mic input (audio is deliberately captured through tab-outs — the video
/// gets masked at save time, the audio track never does). See `CaptureSlot::respawn_strikes` for
/// the fallback that turns this off if the audio input keeps killing the capture.
fn capture_input_args(
    #[allow(unused_variables)] exe_name: Option<&str>,
    with_audio: bool,
) -> Option<(Vec<String>, bool)> {
    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        let title = exe_name.and_then(find_window_title_for_exe);
        #[cfg(not(target_os = "windows"))]
        let title: Option<String> = None;

        // `mut` is only exercised by the Windows-gated audio extend below.
        #[allow(unused_mut)]
        let (mut args, scoped) = match title {
            Some(t) => (
                vec!["-f".into(), "gdigrab".into(), "-i".into(), format!("title={t}")],
                true,
            ),
            None => (
                vec!["-f".into(), "gdigrab".into(), "-i".into(), "desktop".into()],
                false,
            ),
        };

        #[cfg(target_os = "windows")]
        if with_audio {
            if let Some(mic) = first_dshow_audio_device() {
                args.extend([
                    "-f".into(),
                    "dshow".into(),
                    "-i".into(),
                    format!("audio={mic}"),
                ]);
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = with_audio;

        Some((args, scoped))
    } else if cfg!(target_os = "macos") {
        // The device indexes avfoundation reports aren't portable across Macs — run
        // `ffmpeg -f avfoundation -list_devices true -i ""` to find yours. On the dev machine
        // this was written on: screen = 2 ("Capture screen 0"), mic = 1 ("MacBook Air
        // Microphone"). "video:audio" in one input; ":none" skips audio.
        let input = if with_audio { "2:1" } else { "2:none" };
        Some((
            vec!["-f".into(), "avfoundation".into(), "-i".into(), input.into()],
            false,
        ))
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

/// Spawns one rolling-buffer ffmpeg process writing into `dir`. Assumes `ffmpeg` is resolvable on
/// PATH — bundling it as a Tauri sidecar binary is a packaging follow-up (release-workflow stage,
/// task 16), not a capture-logic change.
fn spawn_ffmpeg(dir: &Path, input_args: &[String]) -> Option<Child> {
    let seq = SPAWN_SEQ.fetch_add(1, Ordering::SeqCst);
    let pattern = dir.join(format!("segment_{seq:05}_%03d.ts"));

    // std::process, not tokio::process: this can run from sync contexts before any Tokio reactor
    // is guaranteed to exist (spawning a tokio::process::Child without one panics with "no
    // reactor running"), and fire-and-forget is all that's needed.
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(input_args)
        .args(["-framerate", "30"])
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            // Recorded so a future life of this app can reap this ffmpeg if we die without
            // running our exit cleanup — see reap_orphan_capture.
            let _ = std::fs::write(capture_pid_file(dir), child.id().to_string());
            Some(child)
        }
        Err(e) => {
            eprintln!("clipper: failed to start capture buffer (is ffmpeg on PATH?): {e}");
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
        let with_audio = slot.respawn_strikes < 2;
        let Some((input_args, window_scoped)) = capture_input_args(exe.as_deref(), with_audio)
        else {
            eprintln!("clipper: no screen-capture input configured for this OS, buffer not started");
            slot.phase = None;
            return;
        };
        match spawn_ffmpeg(&dir, &input_args) {
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

/// Native notification. macOS delivers via `osascript` because Notification Center silently
/// ignores unbundled dev binaries — permission reads Granted and the plugin's `.show()` returns
/// Ok, but nothing ever appears (observed live; a known limitation of non-bundled apps). The
/// packaged app and Windows use the notification plugin normally.
async fn notify(app: &AppHandle, title: &str, body: &str) {
    if cfg!(target_os = "macos") {
        // {:?} produces a double-quoted, escaped string — valid AppleScript string syntax.
        let script = format!("display notification {body:?} with title {title:?}");
        let _ = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    } else if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("clipper: could not show notification: {e}");
    }
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
/// Selection accumulates each segment's REAL covered duration (its file birth→modified span)
/// rather than assuming a fixed 10s per file, so crash-restart partials don't skew the math.
/// Falls back to whatever exists if the buffer hasn't filled that far yet (e.g. right after a
/// game launches).
/// Returned by `recent_segments`: the files plus the precise wall-clock window they cover —
/// taken straight from the oldest file's birth time and the newest file's mtime, NOT from
/// summing per-file spans. Sums of truncated spans drifted several seconds, which shifted the
/// save-time black-out mask onto the wrong footage (post-refocus gameplay got blacked while the
/// tab-out itself leaked through).
struct SelectedSegments {
    files: Vec<PathBuf>,
    wall_start: std::time::SystemTime,
    total_secs: f64,
}

fn recent_segments(dir: &PathBuf, seconds: u32) -> Result<SelectedSegments, String> {
    let mut entries: Vec<(std::time::SystemTime, std::time::SystemTime, PathBuf)> =
        std::fs::read_dir(dir)
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
                let modified = meta.modified().ok()?;
                let born = meta.created().ok().unwrap_or_else(|| {
                    modified
                        .checked_sub(std::time::Duration::from_secs(SEGMENT_SECONDS as u64))
                        .unwrap_or(modified)
                });
                Some((born, modified, e.path()))
            })
            .collect();
    entries.sort_by_key(|(_, modified, _)| *modified);

    if entries.is_empty() {
        return Err("No footage captured yet — the buffer needs a few seconds to fill.".into());
    }

    let mut covered: u64 = 0;
    let mut picked: Vec<(std::time::SystemTime, std::time::SystemTime, PathBuf)> = Vec::new();
    for (born, modified, path) in entries.into_iter().rev() {
        picked.push((born, modified, path));
        let span = modified
            .duration_since(born)
            .map(|d| d.as_secs())
            .unwrap_or(SEGMENT_SECONDS as u64)
            .clamp(1, SEGMENT_SECONDS as u64);
        covered += span;
        if covered >= seconds as u64 {
            break;
        }
    }
    picked.reverse();

    // Precise window from the file timestamps themselves (fractional; no per-file truncation).
    let wall_start = picked.first().map(|(born, _, _)| *born).unwrap_or_else(std::time::SystemTime::now);
    let wall_end = picked
        .last()
        .map(|(_, modified, _)| *modified)
        .unwrap_or_else(std::time::SystemTime::now);
    let total_secs = wall_end
        .duration_since(wall_start)
        .map(|d| d.as_secs_f64())
        .unwrap_or(covered as f64);

    Ok(SelectedSegments {
        files: picked.into_iter().map(|(_, _, p)| p).collect(),
        wall_start,
        total_secs,
    })
}

/// Reads the real duration of a finished clip by parsing `Duration: HH:MM:SS.cc` from
/// `ffmpeg -i`'s metadata dump — the segment-count estimate can be off by up to a whole segment
/// because the live segment is partial. Falls back to `None` (caller estimates) if parsing fails.
async fn probe_duration_seconds(path: &Path) -> Option<i64> {
    let output = tokio::process::Command::new("ffmpeg")
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

/// Cuts the trailing `seconds` of the rolling buffer into a standalone clip file and rolls a
/// `clips` row for it, best-effort attached to whichever game is currently being tracked.
/// Extracts a thumbnail for the Clips grid, and fires a native OS notification on success.
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

    // Mask unfocused spans in the OUTPUT (never needed for a window-scoped capture, which only
    // ever contains the game's own window). The filter timeline runs before the output-side
    // `-ss` cut, so range times are concat times, not output times.
    let filter = match (!is_window_scoped(&app))
        .then(|| blackout_enable_expr(&app, selected.wall_start, selected.total_secs))
        .flatten()
    {
        Some(expr) => {
            format!("drawbox=x=0:y=0:w=iw:h=ih:t=fill:color=black:enable='{expr}',fps=30")
        }
        None => "fps=30".to_string(),
    };

    // The selection covers AT LEAST the requested length (whole segments); `-ss`/`-t` on the
    // output cut it to exactly the requested trailing window — clips used to drift 24-40s for a
    // "30s" setting when this was done by whole-segment granularity alone. Re-encode, not
    // `-c copy`: output-accurate seeking and the mask filter both require decoding anyway, and
    // it makes the output robust to any stream-parameter drift between spawns. ~2-4s for a 30s
    // clip at veryfast; every mainstream clipping tool re-encodes on save.
    let skip_secs = (selected.total_secs - seconds as f64).max(0.0);
    let concat_output = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-fflags", "+genpts"])
        .args(["-f", "concat", "-safe", "0"])
        .arg("-i")
        .arg(&list_path)
        .args(["-vf", &filter])
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
    let _ = tokio::process::Command::new("ffmpeg")
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

    let duration_seconds = match probe_duration_seconds(&clip_path).await {
        Some(d) => d,
        None => (selected.total_secs.min(seconds as f64)) as i64,
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

    let notif_body = match &game_name {
        Some(name) => format!("Saved the last {duration_seconds}s of {name}."),
        None => format!("Saved the last {duration_seconds}s."),
    };
    notify(&app, "Clip saved", &notif_body).await;

    let _ = app.emit("clip-saved", clip.clone());
    Ok(clip)
}

/// Runs `save_clip` from the global-shortcut handler, which isn't itself async — spawns onto the
/// async runtime. Failures surface as a native notification (the player is in a game and can't
/// see the app window) in addition to the `clip-save-failed` event for the Clips view.
pub fn save_clip_from_hotkey(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let db = app.state::<DbState>();
        match save_clip(app.clone(), db, None).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("clipper: hotkey save failed: {e}");
                notify(&app, "Clip not saved", &e).await;
                let _ = app.emit("clip-save-failed", e);
            }
        }
    });
}
