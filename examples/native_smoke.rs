//! Run on the main thread in a graphical session: cargo run --example native_smoke.
//! Only this wrapper has a deadline and framebuffer observation, not the product.
use eframe::egui::{self, ViewportCommand};
use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    receiver::Receiver,
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

struct Smoke {
    app: EmoteApp,
    observed: Arc<AtomicBool>,
    capture_requested: bool,
}
impl eframe::App for Smoke {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        eframe::App::clear_color(&self.app, visuals)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        // Metadata is public egui state, not a test-only hook in the product.
        let uploaded = ui.ctx().tex_manager().read().allocated().any(|(_, meta)| {
            meta.name == "emote"
                && meta.size == [64, 64]
                && meta.options == egui::TextureOptions::NEAREST
        });
        if uploaded && !self.capture_requested {
            // eframe captures its own framebuffer after painting; this is not an
            // OS screenshot API and requires no screen-recording permission.
            let observed = self.observed.clone();
            let ctx = ui.ctx().clone();
            ui.ctx().request_screenshot(move |image| {
                let center = image[(image.size[0] / 2, image.size[1] / 2)];
                if center == egui::Color32::from_rgb(42, 99, 151) {
                    observed.store(true, Ordering::SeqCst);
                } else {
                    eprintln!("unexpected framebuffer center: {center:?}");
                }
                ctx.send_viewport_cmd(ViewportCommand::Close);
                ctx.request_repaint();
            });
            self.capture_requested = true;
        }
    }
}

fn send_png(address: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()?
            .write_image_data(&[42, 99, 151, 255].repeat(64 * 64))?;
    }
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(
        socket,
        "POST /emote HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\nContent-Length: {}\r\n\r\n",
        png.len()
    )?;
    socket.write_all(&png)?;
    let mut response = String::new();
    socket.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.1 204") {
        return Err(format!("unexpected HTTP response: {response}").into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A hard deadline also covers a stuck native event loop/startup. Normal
    // paths cancel and join both threads; fatal timeout exits the entire process
    // (and therefore all its threads/sockets), never leaving an orphan server.
    let (finished, deadline) = mpsc::channel();
    let watchdog = thread::spawn(move || {
        if deadline.recv_timeout(Duration::from_secs(20)).is_err() {
            eprintln!("native smoke timed out after 20 seconds");
            std::process::exit(1);
        }
    });
    let (ready, address) = mpsc::channel::<SocketAddr>();
    let sender = thread::spawn(move || {
        let address = address.recv_timeout(Duration::from_secs(15))?;
        send_png(address)
    });
    let observed = Arc::new(AtomicBool::new(false));
    let proof = observed.clone();
    let (bound, actual_address) = mpsc::channel();
    let result = eframe::run_native(
        "Gentle Emote native smoke",
        native_options(),
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let receiver = Receiver::bind("127.0.0.1:0".parse()?, move || ctx.request_repaint())?;
            let address = receiver.local_addr();
            bound.send(address)?;
            ready.send(address)?;
            Ok(Box::new(Smoke {
                app: EmoteApp::new(receiver),
                observed: proof,
                capture_requested: false,
            }))
        }),
    );
    let sent = sender.join();
    let _ = finished.send(());
    watchdog.join().map_err(|_| "watchdog panicked")?;
    result?;
    sent.map_err(|_| "sender panicked")?
        .map_err(|error| error.to_string())?;
    // run_native has dropped the app: prove its ephemeral listening port is free.
    let address = actual_address.recv()?;
    let _released = TcpListener::bind(address)?;
    if !observed.load(Ordering::SeqCst) {
        return Err("native framebuffer was not verified".into());
    }
    println!(
        "PASS: native startup, HTTP 204, nearest 64x64 texture, framebuffer RGB(42,99,151), clean close/port release"
    );
    Ok(())
}
