//! Local framebuffer-only diagnostic: cargo run --example transparency_probe.
//! No desktop capture or image files; no changes to production painting/settings.
//! Compare: cargo run --features diagnostic-wgpu --example transparency_probe --
//! --waiting --wgpu --hold-seconds 60 (omit --wgpu for the Glow baseline).
//! Add --no-capture for a visual-only comparison without framebuffer readback.
//! Samples are pre-compositor evidence, never proof of visible transparency.
use eframe::egui::{self, ViewportCommand};
use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    receiver::Receiver,
};
use std::{
    net::{SocketAddr, TcpListener},
    process::Command,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

struct Probe {
    // None is an independent clear-only app, not a production setting override.
    app: Option<EmoteApp>,
    mode: &'static str,
    frames: usize,
    result: Arc<Mutex<Option<bool>>>,
    renderer: &'static str,
    capture: bool,
    hold: Duration,
    started: Instant,
}

impl eframe::App for Probe {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(app) = &mut self.app {
            eframe::App::ui(app, ui, frame);
        }
        // A small opaque identifier leaves all five sample locations untouched.
        if !self.hold.is_zero() {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                ui.scope_id().with("probe-renderer"),
            ));
            let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(156.0, 18.0));
            painter.rect_filled(rect, 2.0, egui::Color32::BLACK);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{} / {}", self.renderer, capture_label(self.capture)),
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
            let remaining = self.hold.saturating_sub(self.started.elapsed());
            if remaining.is_zero() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint_after(remaining);
            }
        }
        self.frames += 1;
        // Let the initial native sizing/paint settle before the single capture.
        if self.frames < 3 {
            ui.ctx().request_repaint();
        } else if self.frames == 3 && self.capture {
            let mode = self.mode;
            let renderer = self.renderer;
            let close_on_capture = self.hold.is_zero();
            let result = self.result.clone();
            let ctx = ui.ctx().clone();
            ui.ctx().request_screenshot(move |image| {
                let [width, height] = image.size;
                println!("{renderer}/{mode}: framebuffer={width}x{height} physical pixels");
                let mut matched = width > 0 && height > 0;
                // Background locations well outside the centered Waiting text
                // and inside the rounded contour; corner measures clear alpha.
                // On the 192-point surface the top sample is at (168, 48),
                // right of and below the badge ending at (164, 26). Normalized
                // framebuffer coordinates preserve that clearance at display scale.
                for (label, x, y, background) in [
                    ("top-background", width * 7 / 8, height / 4, true),
                    ("bottom-background", width / 2, height * 7 / 8, true),
                    ("left-background", width / 16, height / 2, true),
                    ("right-background", width * 15 / 16, height / 2, true),
                    ("corner", 0, 0, false),
                ] {
                    let rgba = image[(x, y)].to_array();
                    let expected = if mode == "waiting-opacity55" && background {
                        140
                    } else {
                        0
                    };
                    let ok = rgba[..3] == [0, 0, 0]
                        && (i16::from(rgba[3]) - expected).abs() <= 1;
                    println!(
                        "{renderer}/{mode}: {label} ({x},{y}) RGBA={rgba:?}, expected alpha={expected} +/-1, match={ok}"
                    );
                    matched &= ok;
                }
                *result.lock().expect("probe result lock") = Some(matched);
                if close_on_capture {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                ctx.request_repaint();
            });
        } else if self.frames == 3 && self.hold.is_zero() {
            ui.ctx().send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

fn capture_label(capture: bool) -> &'static str {
    if capture { "CAPTURE" } else { "NO CAPTURE" }
}

fn run_probe(
    waiting: bool,
    wgpu: bool,
    hold: Duration,
    capture: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut options = native_options();
    if wgpu {
        #[cfg(feature = "diagnostic-wgpu")]
        {
            options.renderer = eframe::Renderer::Wgpu;
        }
        #[cfg(not(feature = "diagnostic-wgpu"))]
        return Err("--wgpu requires --features diagnostic-wgpu".into());
    } else {
        options.renderer = eframe::Renderer::Glow;
    }
    let renderer = if wgpu { "WGPU" } else { "Glow" };
    let mode = if waiting {
        "waiting-opacity55"
    } else {
        "clear-only"
    };
    println!(
        "{renderer}/{mode}: pid={}, hold={}s; framebuffer only, not compositor evidence",
        std::process::id(),
        hold.as_secs()
    );
    let timeout = hold + Duration::from_secs(20);
    let (finished, deadline) = mpsc::channel();
    let watchdog = thread::spawn(move || {
        if deadline.recv_timeout(timeout).is_err() {
            eprintln!(
                "{renderer}/{mode}: timed out after {} seconds",
                timeout.as_secs()
            );
            std::process::exit(1);
        }
    });
    let result = Arc::new(Mutex::new(None));
    let proof = result.clone();
    let (bound, address) = mpsc::channel::<SocketAddr>();
    let native = eframe::run_native(
        &format!(
            "Gentle Emote transparency probe: {renderer} / {mode} / {}",
            capture_label(capture)
        ),
        options,
        Box::new(move |cc| {
            let app = if waiting {
                let ctx = cc.egui_ctx.clone();
                let receiver =
                    Receiver::bind("127.0.0.1:0".parse()?, move || ctx.request_repaint())?;
                println!(
                    "{mode}: diagnostic receiver={} (no requests sent)",
                    receiver.local_addr()
                );
                bound.send(receiver.local_addr())?;
                Some(EmoteApp::new(receiver))
            } else {
                None
            };
            Ok(Box::new(Probe {
                app,
                mode,
                frames: 0,
                result: proof,
                renderer,
                capture,
                hold,
                started: Instant::now(),
            }))
        }),
    );
    let _ = finished.send(());
    watchdog.join().map_err(|_| "watchdog panicked")?;
    native?;
    println!("{renderer}/{mode}: native event loop returned cleanly");
    if waiting {
        let address = address.recv()?;
        let _released = TcpListener::bind(address)?;
        println!("{renderer}/{mode}: clean close; ephemeral port released");
    }
    if !capture {
        println!("{renderer}/{mode}: NO CAPTURE; no framebuffer measurement (visual-only run)");
        return Ok(());
    }
    match *result.lock().map_err(|_| "probe result lock poisoned")? {
        Some(true) => {
            println!(
                "{renderer}/{mode}: PASS framebuffer samples (not a visual/compositor verdict)"
            )
        }
        Some(false) => {
            return Err(format!("{mode}: framebuffer samples differed from expectations").into());
        }
        None => return Err(format!("{mode}: closed without framebuffer observation").into()),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const USAGE: &str = "usage: transparency_probe [--waiting | --clear-only] [--wgpu] [--no-capture] [--hold-seconds 0..300]";
    let mut mode = None;
    let mut wgpu = false;
    let mut capture = true;
    let mut hold = Duration::ZERO;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--waiting" | "--clear-only" if mode.is_none() => mode = Some(arg == "--waiting"),
            "--wgpu" => wgpu = true,
            "--no-capture" => capture = false,
            "--hold-seconds" => {
                let seconds: u64 = args.next().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
                if seconds > 300 {
                    return Err(USAGE.into());
                }
                hold = Duration::from_secs(seconds);
            }
            "--help" | "-h" => {
                println!(
                    "{USAGE}\n--wgpu requires --features diagnostic-wgpu; default renderer: Glow.\nHold keeps the window visible after sampling; no OS capture or images saved.\n--no-capture disables framebuffer readback/measurement, preserving the visual hold."
                );
                return Ok(());
            }
            _ => return Err(USAGE.into()),
        }
    }
    match mode {
        Some(waiting) => run_probe(waiting, wgpu, hold, capture),
        None => {
            // Separate processes give each baseline an independent native root
            // window/event loop. Each child has a hold + 20-second hard deadline.
            let executable = std::env::current_exe()?;
            let mut success = true;
            for mode in ["--waiting", "--clear-only"] {
                let mut child = Command::new(&executable);
                child
                    .arg(mode)
                    .arg("--hold-seconds")
                    .arg(hold.as_secs().to_string());
                if wgpu {
                    child.arg("--wgpu");
                }
                if !capture {
                    child.arg("--no-capture");
                }
                let status = child.status()?;
                println!("{mode}: child exit={status}");
                success &= status.success();
            }
            if !success {
                return Err("one or more framebuffer probes failed".into());
            }
            Ok(())
        }
    }
}
