// Backloggr web setup ("BackloggrSetup.exe") — the branded install wizard the website serves
// instead of a raw MSI. A single small egui exe (no WebView2 dependency — the machine may not
// have it yet; the MSI's own bootstrapper installs it for the app) that fetches the repo's
// releases/latest/download/latest.json (the same stable URL the in-app updater uses), downloads
// the current MSI with a progress bar, runs it silently via msiexec, and offers Launch.
// Evergreen: the exe never embeds a version, so one website download URL always installs the
// newest release. Dev note: builds and runs on macOS too — the msiexec step is simulated there
// so the UI flow can be exercised on the dev platform.

#![windows_subsystem = "windows"]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Vec2};

const LATEST_JSON_URL: &str =
    "https://github.com/iden0605/Backloggr/releases/latest/download/latest.json";

// Iron & Chalk (tailwind.config.js) — keep in sync with the app's palette.
const BG: Color32 = Color32::from_rgb(0x15, 0x14, 0x14);
const BG_LIFT: Color32 = Color32::from_rgb(0x21, 0x1C, 0x19); // ambient wash top-lift
const SURFACE_ALT: Color32 = Color32::from_rgb(0x24, 0x21, 0x20);
const BORDER_STRONG: Color32 = Color32::from_rgb(0x3A, 0x36, 0x33);
const CHALK: Color32 = Color32::from_rgb(0xED, 0xE8, 0xE0);
const TEXT_LO: Color32 = Color32::from_rgb(0x92, 0x8B, 0x82);
const RUST: Color32 = Color32::from_rgb(0xB9, 0x6A, 0x55);
const SUCCESS: Color32 = Color32::from_rgb(0x7F, 0xA0, 0x8C);
const DANGER: Color32 = Color32::from_rgb(0xD4, 0x57, 0x4E);

const WINDOW_SIZE: Vec2 = Vec2::new(560.0, 400.0);
const TITLEBAR_H: f32 = 44.0;

struct Manifest {
    version: String,
    msi_url: String,
}

enum Phase {
    Fetching,
    Ready,
    Downloading { received: u64, total: Option<u64> },
    Installing,
    Done,
    Failed(String),
}

struct Shared {
    phase: Phase,
    manifest: Option<Manifest>,
}

type State = Arc<Mutex<Shared>>;

fn main() -> eframe::Result {
    let logo = image::load_from_memory(include_bytes!("../assets/logo.png"))
        .expect("bundled logo decodes")
        .into_rgba8();
    let (w, h) = logo.dimensions();
    let icon = egui::IconData {
        rgba: logo.clone().into_raw(),
        width: w,
        height: h,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(WINDOW_SIZE)
            .with_max_inner_size(WINDOW_SIZE)
            .with_resizable(false)
            .with_maximize_button(false)
            .with_decorations(false)
            .with_icon(icon),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Backloggr Setup",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            Ok(Box::new(SetupApp::new(cc, logo)))
        }),
    )
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "archivo".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Archivo-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "archivo-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Archivo-SemiBold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "archivo-bold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Archivo-Bold.ttf"
        ))),
    );
    fonts.font_data.insert(
        "jbmono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Medium.ttf"
        ))),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["archivo".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["jbmono".into()]);
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        vec!["archivo-semibold".into()],
    );
    fonts
        .families
        .insert(FontFamily::Name("bold".into()), vec!["archivo-bold".into()]);
    ctx.set_fonts(fonts);
}

fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}
fn bold() -> egui::FontFamily {
    egui::FontFamily::Name("bold".into())
}

struct SetupApp {
    state: State,
    logo: egui::TextureHandle,
}

impl SetupApp {
    fn new(cc: &eframe::CreationContext<'_>, logo: image::RgbaImage) -> Self {
        let (w, h) = logo.dimensions();
        let tex = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &logo.into_raw()),
            egui::TextureOptions::LINEAR,
        );
        let state: State = Arc::new(Mutex::new(Shared {
            phase: Phase::Fetching,
            manifest: None,
        }));
        spawn_fetch(state.clone(), cc.egui_ctx.clone());
        Self { state, logo: tex }
    }
}

// ---------- background work ----------

fn spawn_fetch(state: State, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = fetch_manifest();
        let mut s = state.lock().unwrap();
        match result {
            Ok(m) => {
                s.manifest = Some(m);
                s.phase = Phase::Ready;
            }
            Err(e) => s.phase = Phase::Failed(format!("Couldn't check the latest version. {e}")),
        }
        ctx.request_repaint();
    });
}

fn fetch_manifest() -> Result<Manifest, String> {
    let resp = ureq::get(LATEST_JSON_URL)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| friendly_net_error(&e))?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    let version = v["version"]
        .as_str()
        .ok_or("release manifest is missing a version")?
        .to_string();
    let msi_url = v["platforms"]["windows-x86_64"]["url"]
        .as_str()
        .ok_or("release manifest has no Windows installer")?
        .to_string();
    Ok(Manifest { version, msi_url })
}

fn friendly_net_error(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("The download server answered with HTTP {code}."),
        ureq::Error::Transport(_) => "Check your internet connection and try again.".to_string(),
    }
}

fn spawn_install(state: State, ctx: egui::Context) {
    // Flip the phase synchronously so a second click (or the dev autorun re-firing while
    // still in Ready) can't start a second download thread.
    let msi_url = {
        let mut s = state.lock().unwrap();
        if !matches!(s.phase, Phase::Ready) {
            return;
        }
        let Some(url) = s.manifest.as_ref().map(|m| m.msi_url.clone()) else {
            return;
        };
        s.phase = Phase::Downloading {
            received: 0,
            total: None,
        };
        url
    };
    std::thread::spawn(move || {
        let result = download_msi(&msi_url, &state, &ctx).and_then(|path| {
            {
                state.lock().unwrap().phase = Phase::Installing;
            }
            ctx.request_repaint();
            run_msiexec(&path)
        });

        let mut s = state.lock().unwrap();
        s.phase = match result {
            Ok(()) => Phase::Done,
            Err(e) => Phase::Failed(e),
        };
        ctx.request_repaint();
    });
}

fn download_msi(url: &str, state: &State, ctx: &egui::Context) -> Result<PathBuf, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| friendly_net_error(&e))?;
    let total = resp
        .header("content-length")
        .and_then(|v| v.parse::<u64>().ok());
    let path = std::env::temp_dir().join("backloggr-setup.msi");
    let mut file = std::fs::File::create(&path)
        .map_err(|e| format!("Couldn't write to the temp folder: {e}"))?;

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|_| "The download was interrupted. Check your connection and try again.")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Couldn't write the installer to disk: {e}"))?;
        received += n as u64;
        state.lock().unwrap().phase = Phase::Downloading { received, total };
        ctx.request_repaint();
    }
    if let Some(t) = total {
        if received < t {
            return Err("The download ended early. Check your connection and try again.".into());
        }
    }
    Ok(path)
}

#[cfg(windows)]
fn run_msiexec(msi: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("msiexec")
        .arg("/i")
        .arg(msi)
        .args(["/qn", "/norestart"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Couldn't start the Windows installer: {e}"))?;
    match status.code() {
        // 0 = success, 3010/1641 = success but a reboot is involved.
        Some(0) | Some(3010) | Some(1641) => Ok(()),
        Some(code) => Err(format!(
            "The Windows installer failed (msiexec exit code {code})."
        )),
        None => Err("The Windows installer was terminated.".into()),
    }
}

// Dev platform (macOS): pretend to install so the whole UI flow can be exercised.
#[cfg(not(windows))]
fn run_msiexec(_msi: &std::path::Path) -> Result<(), String> {
    std::thread::sleep(std::time::Duration::from_millis(2500));
    Ok(())
}

#[cfg(windows)]
fn installed_exe() -> Option<PathBuf> {
    let pf = std::env::var_os("ProgramFiles")?;
    let exe = PathBuf::from(pf).join("Backloggr").join("Backloggr.exe");
    exe.exists().then_some(exe)
}

#[cfg(not(windows))]
fn installed_exe() -> Option<PathBuf> {
    Some(PathBuf::from("/dev/null")) // dev: keep the Launch button visible
}

#[cfg(windows)]
fn launch_app(exe: &std::path::Path) {
    // This process runs elevated (requireAdministrator manifest); spawning the app directly
    // would run it as admin. Going through explorer.exe de-elevates to the desktop user.
    let _ = std::process::Command::new("explorer.exe").arg(exe).spawn();
}

#[cfg(not(windows))]
fn launch_app(_exe: &std::path::Path) {}

// ---------- UI ----------

impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                paint_backdrop(ui, rect);
                self.titlebar(ui, rect);

                // Snapshot the phase so the lock isn't held across UI code.
                enum View {
                    Fetching,
                    Ready { version: String },
                    Downloading { received: u64, total: Option<u64> },
                    Installing,
                    Done { version: String },
                    Failed(String),
                }
                let view = {
                    let s = self.state.lock().unwrap();
                    let version = s
                        .manifest
                        .as_ref()
                        .map(|m| m.version.clone())
                        .unwrap_or_default();
                    match &s.phase {
                        Phase::Fetching => View::Fetching,
                        Phase::Ready => View::Ready { version },
                        Phase::Downloading { received, total } => View::Downloading {
                            received: *received,
                            total: *total,
                        },
                        Phase::Installing => View::Installing,
                        Phase::Done => View::Done { version },
                        Phase::Failed(msg) => View::Failed(msg.clone()),
                    }
                };

                match view {
                    View::Fetching => {
                        self.header(ui);
                        ui.add_space(26.0);
                        mono_label(
                            ui,
                            &format!("CHECKING LATEST VERSION{}", dots(ctx)),
                            TEXT_LO,
                            11.0,
                        );
                        ctx.request_repaint_after(std::time::Duration::from_millis(120));
                    }
                    View::Ready { version } => {
                        self.header(ui);
                        ui.add_space(14.0);
                        version_pill(ui, &format!("v{version}"));
                        ui.add_space(24.0);
                        // Dev-only: drive the flow without a click (macOS UI testing).
                        let autorun = cfg!(debug_assertions)
                            && std::env::var_os("BACKLOGGR_SETUP_AUTORUN").is_some();
                        if chalk_button(ui, "Install", 220.0).clicked() || autorun {
                            spawn_install(self.state.clone(), ctx.clone());
                        }
                    }
                    View::Downloading { received, total } => {
                        self.header(ui);
                        ui.add_space(30.0);
                        let frac = total.map(|t| (received as f32 / t as f32).clamp(0.0, 1.0));
                        progress_bar(ui, ctx, frac);
                        ui.add_space(12.0);
                        let label = match total {
                            Some(t) => format!(
                                "DOWNLOADING · {:.0}% · {:.1} MB",
                                (received as f64 / t as f64) * 100.0,
                                t as f64 / 1e6
                            ),
                            None => format!("DOWNLOADING · {:.1} MB", received as f64 / 1e6),
                        };
                        mono_label(ui, &label, TEXT_LO, 11.0);
                    }
                    View::Installing => {
                        self.header(ui);
                        ui.add_space(30.0);
                        progress_bar(ui, ctx, None);
                        ui.add_space(12.0);
                        mono_label(ui, &format!("INSTALLING{}", dots(ctx)), TEXT_LO, 11.0);
                        ctx.request_repaint_after(std::time::Duration::from_millis(33));
                    }
                    View::Done { version } => {
                        ui.add_space(46.0);
                        checkmark(ui);
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Ready to play.")
                                    .family(semibold())
                                    .size(22.0)
                                    .color(CHALK),
                            );
                        });
                        ui.add_space(6.0);
                        mono_label(
                            ui,
                            &format!("BACKLOGGR {version} INSTALLED"),
                            TEXT_LO,
                            11.0,
                        );
                        ui.add_space(26.0);
                        if let Some(exe) = installed_exe() {
                            if chalk_button(ui, "Launch Backloggr", 220.0).clicked() {
                                launch_app(&exe);
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            ui.add_space(10.0);
                        }
                        if ghost_button(ui, "Close", 220.0).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    View::Failed(msg) => {
                        ui.add_space(52.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("Something went wrong")
                                    .family(semibold())
                                    .size(20.0)
                                    .color(DANGER),
                            );
                            ui.add_space(10.0);
                            ui.allocate_ui(Vec2::new(400.0, 60.0), |ui| {
                                ui.label(
                                    egui::RichText::new(msg).size(13.5).color(TEXT_LO),
                                );
                            });
                        });
                        ui.add_space(22.0);
                        if chalk_button(ui, "Try again", 220.0).clicked() {
                            let mut s = self.state.lock().unwrap();
                            if s.manifest.is_some() {
                                s.phase = Phase::Ready;
                            } else {
                                s.phase = Phase::Fetching;
                                drop(s);
                                spawn_fetch(self.state.clone(), ctx.clone());
                            }
                        }
                        ui.add_space(10.0);
                        if ghost_button(ui, "Close", 220.0).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }

                footer(ui, rect);
            });
    }
}

impl SetupApp {
    /// Logo + wordmark + tagline, shared by every pre-Done state.
    fn header(&self, ui: &mut egui::Ui) {
        ui.add_space(56.0);
        ui.vertical_centered(|ui| {
            ui.add(
                egui::Image::new(&self.logo)
                    .fit_to_exact_size(Vec2::splat(72.0))
                    .corner_radius(CornerRadius::same(16)),
            );
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new("Backloggr")
                    .family(bold())
                    .size(30.0)
                    .color(CHALK),
            );
            ui.add_space(4.0);
        });
        mono_label(ui, "GAME LIBRARY · PLAYTIME · CLIPS", TEXT_LO, 10.5);
    }

    fn titlebar(&self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TITLEBAR_H));
        let resp = ui.interact(bar, egui::Id::new("titlebar"), Sense::click_and_drag());
        if resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().text(
            Pos2::new(rect.min.x + 18.0, rect.min.y + TITLEBAR_H / 2.0),
            Align2::LEFT_CENTER,
            "BACKLOGGR SETUP",
            FontId::new(10.0, egui::FontFamily::Monospace),
            TEXT_LO,
        );

        // Glyphs are painted as line segments — the embedded Archivo subset has no ✕/– glyphs.
        let btn = |ui: &mut egui::Ui, i: usize, close: bool| -> egui::Response {
            let size = 30.0;
            let center = Pos2::new(
                rect.max.x - 24.0 - i as f32 * 36.0,
                rect.min.y + TITLEBAR_H / 2.0,
            );
            let r = Rect::from_center_size(center, Vec2::splat(size));
            let resp = ui
                .interact(r, egui::Id::new(("winbtn", i)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let color = if resp.hovered() { CHALK } else { TEXT_LO };
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(6), SURFACE_ALT);
            }
            let stroke = egui::Stroke::new(1.5, color);
            let e = 5.0;
            if close {
                ui.painter().line_segment(
                    [center + Vec2::new(-e, -e), center + Vec2::new(e, e)],
                    stroke,
                );
                ui.painter().line_segment(
                    [center + Vec2::new(-e, e), center + Vec2::new(e, -e)],
                    stroke,
                );
            } else {
                ui.painter().line_segment(
                    [center + Vec2::new(-e, 3.0), center + Vec2::new(e, 3.0)],
                    stroke,
                );
            }
            resp
        };
        if btn(ui, 0, true).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if btn(ui, 1, false).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }
}

// ---------- drawing helpers ----------

/// The app's ambient-wash backdrop: top-lift gradient, rust radial up top,
/// verdigris low-right, hairline border.
fn paint_backdrop(ui: &egui::Ui, rect: Rect) {
    let p = ui.painter();
    vertical_fade(p, rect, BG_LIFT, BG);
    radial_glow(
        p,
        Pos2::new(rect.center().x, rect.min.y - 40.0),
        rect.width() * 0.62,
        RUST.gamma_multiply(0.10),
    );
    radial_glow(
        p,
        Pos2::new(rect.max.x - 60.0, rect.max.y + 30.0),
        rect.width() * 0.45,
        SUCCESS.gamma_multiply(0.06),
    );
    p.rect_stroke(
        rect.shrink(0.5),
        CornerRadius::ZERO,
        egui::Stroke::new(1.0, BORDER_STRONG),
        egui::StrokeKind::Inside,
    );
}

fn vertical_fade(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
}

/// Smooth radial gradient (colored center fading to transparent) via a triangle fan.
fn radial_glow(p: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(
            center + Vec2::new(a.cos(), a.sin()) * radius,
            Color32::TRANSPARENT,
        );
    }
    for i in 1..=n {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(mesh);
}

fn mono_label(ui: &mut egui::Ui, text: &str, color: Color32, size: f32) {
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(text)
                .family(egui::FontFamily::Monospace)
                .size(size)
                .color(color),
        );
    });
}

fn version_pill(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        let font = FontId::new(11.0, egui::FontFamily::Monospace);
        let galley = ui.painter().layout_no_wrap(text.into(), font.clone(), CHALK);
        let size = galley.size() + Vec2::new(24.0, 12.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(99), SURFACE_ALT);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(99),
            egui::Stroke::new(1.0, BORDER_STRONG),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, CHALK);
    });
}

/// Chalk primary button — matches the app convention (chalk bg, dark text, hover ~85%,
/// press scale 0.98). Rust stays reserved for the live progress bar.
fn chalk_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    styled_button(ui, label, width, true)
}

fn ghost_button(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    styled_button(ui, label, width, false)
}

fn styled_button(ui: &mut egui::Ui, label: &str, width: f32, primary: bool) -> egui::Response {
    let mut clicked_resp = None;
    ui.vertical_centered(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let draw_rect = if resp.is_pointer_button_down_on() {
            Rect::from_center_size(rect.center(), rect.size() * 0.98)
        } else {
            rect
        };
        let p = ui.painter();
        if primary {
            let fill = if resp.hovered() {
                CHALK.gamma_multiply(0.85)
            } else {
                CHALK
            };
            p.rect_filled(draw_rect, CornerRadius::same(10), fill);
            p.text(
                draw_rect.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new(15.0, semibold()),
                BG,
            );
        } else {
            let (stroke_color, text_color) = if resp.hovered() {
                (BORDER_STRONG.gamma_multiply(1.6), CHALK)
            } else {
                (BORDER_STRONG, TEXT_LO)
            };
            p.rect_stroke(
                draw_rect,
                CornerRadius::same(10),
                egui::Stroke::new(1.0, stroke_color),
                egui::StrokeKind::Inside,
            );
            p.text(
                draw_rect.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new(14.0, semibold()),
                text_color,
            );
        }
        clicked_resp = Some(resp);
    });
    clicked_resp.unwrap()
}

/// Rust progress bar — determinate when `frac` is Some, sweeping indeterminate otherwise.
fn progress_bar(ui: &mut egui::Ui, ctx: &egui::Context, frac: Option<f32>) {
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(320.0, 6.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(3), SURFACE_ALT);
        match frac {
            Some(f) => {
                if f > 0.0 {
                    let fill = Rect::from_min_size(
                        rect.min,
                        Vec2::new((rect.width() * f).max(6.0), rect.height()),
                    );
                    p.rect_filled(fill, CornerRadius::same(3), RUST);
                }
            }
            None => {
                let t = (ctx.input(|i| i.time) * 0.55).fract() as f32;
                let seg_w = rect.width() * 0.30;
                let x = rect.min.x - seg_w + t * (rect.width() + seg_w);
                let fill = Rect::from_min_size(
                    Pos2::new(x.max(rect.min.x), rect.min.y),
                    Vec2::new(
                        (x + seg_w).min(rect.max.x) - x.max(rect.min.x),
                        rect.height(),
                    ),
                );
                if fill.width() > 0.0 {
                    p.rect_filled(fill, CornerRadius::same(3), RUST);
                }
            }
        }
    });
}

fn checkmark(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
        let p = ui.painter();
        p.circle_filled(rect.center(), 32.0, SUCCESS.gamma_multiply(0.15));
        p.circle_stroke(rect.center(), 31.5, egui::Stroke::new(1.0, SUCCESS));
        let c = rect.center();
        let pts = [
            Pos2::new(c.x - 12.0, c.y + 1.0),
            Pos2::new(c.x - 4.0, c.y + 9.0),
            Pos2::new(c.x + 13.0, c.y - 9.0),
        ];
        p.line_segment([pts[0], pts[1]], egui::Stroke::new(3.0, SUCCESS));
        p.line_segment([pts[1], pts[2]], egui::Stroke::new(3.0, SUCCESS));
    });
}

fn dots(ctx: &egui::Context) -> String {
    let n = ((ctx.input(|i| i.time) * 2.5) as usize) % 4;
    ".".repeat(n)
}

fn footer(ui: &mut egui::Ui, rect: Rect) {
    ui.painter().text(
        Pos2::new(rect.center().x, rect.max.y - 20.0),
        Align2::CENTER_CENTER,
        "backloggr.com",
        FontId::new(10.5, egui::FontFamily::Monospace),
        TEXT_LO.gamma_multiply(0.8),
    );
}
