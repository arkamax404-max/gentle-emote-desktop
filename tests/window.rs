use eframe::egui::{self, Event, PointerButton, Pos2, Vec2, ViewportCommand};
use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    receiver::Receiver,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

fn input(events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(192.0))),
        events,
        ..Default::default()
    }
}
fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Default::default(),
    }
}
fn setup() -> (egui::Context, EmoteApp, std::net::SocketAddr) {
    let ctx = egui::Context::default();
    let repaint = ctx.clone();
    let receiver = Receiver::bind("127.0.0.1:0".parse().unwrap(), move || {
        repaint.request_repaint()
    })
    .unwrap();
    let address = receiver.local_addr();
    (ctx, EmoteApp::new(receiver), address)
}
// egui 0.37 requires the renderer to consume deltas. This headless renderer
// inspects them instead; clear even on assertion unwind to avoid a double panic.
struct Output(egui::FullOutput);
impl std::ops::Deref for Output {
    type Target = egui::FullOutput;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.0.textures_delta.clear();
    }
}
fn step(ctx: &egui::Context, app: &mut EmoteApp, events: Vec<Event>) -> Output {
    Output(ctx.run_ui(input(events), |ui| app.show(ui)))
}

#[test]
fn fixed_square_frameless_topmost_options() {
    let options = native_options();
    assert_eq!(options.viewport.inner_size, Some(Vec2::splat(192.0)));
    assert_eq!(options.viewport.decorations, Some(false));
    assert_eq!(options.viewport.resizable, Some(false));
    assert_eq!(
        options.viewport.window_level,
        Some(egui::WindowLevel::AlwaysOnTop)
    );
    assert_eq!(options.viewport.transparent, Some(false));
}

#[test]
fn waiting_text_and_secondary_menu_do_not_start_drag() {
    let (ctx, mut app, _) = setup();
    let output = step(&ctx, &mut app, vec![]);
    let text = format!("{:?}", output.shapes);
    assert!(text.contains("Waiting"), "{text}");
    assert!(text.contains("Right-click"), "{text}");
    let p = Pos2::new(20.0, 20.0);
    step(
        &ctx,
        &mut app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Secondary, true),
        ],
    );
    step(
        &ctx,
        &mut app,
        vec![button(p, PointerButton::Secondary, false)],
    );
    // Popup's first sizing pass is invisible; inspect its next painted pass.
    let output = step(&ctx, &mut app, vec![]);
    assert!(
        !output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::StartDrag)
    );
    let text = format!("{:?}", output.shapes);
    for label in ["128", "192", "256", "384", "Exit"] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
}

#[test]
fn menu_actions_resize_square_or_close_without_dragging() {
    for (label, command) in [
        ("128", ViewportCommand::InnerSize(Vec2::splat(128.0))),
        ("192", ViewportCommand::InnerSize(Vec2::splat(192.0))),
        ("256", ViewportCommand::InnerSize(Vec2::splat(256.0))),
        ("384", ViewportCommand::InnerSize(Vec2::splat(384.0))),
        ("Exit", ViewportCommand::Close),
    ] {
        let (ctx, mut app, _) = setup();
        step(&ctx, &mut app, vec![]);
        let p = Pos2::new(20.0, 20.0);
        step(
            &ctx,
            &mut app,
            vec![
                Event::PointerMoved(p),
                button(p, PointerButton::Secondary, true),
            ],
        );
        step(
            &ctx,
            &mut app,
            vec![button(p, PointerButton::Secondary, false)],
        );
        let output = step(&ctx, &mut app, vec![]);
        let p = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text().starts_with(label)
                {
                    return Some(text.pos + Vec2::splat(4.0));
                }
                None
            })
            .expect("menu label must be painted");
        step(
            &ctx,
            &mut app,
            vec![
                Event::PointerMoved(p),
                button(p, PointerButton::Primary, true),
            ],
        );
        let output = step(
            &ctx,
            &mut app,
            vec![button(p, PointerButton::Primary, false)],
        );
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&command), "{label}: {commands:?}");
        assert!(!commands.contains(&ViewportCommand::StartDrag));
    }
}

#[test]
fn primary_drag_requests_native_move() {
    let (ctx, mut app, _) = setup();
    let p = Pos2::new(20.0, 20.0);
    step(&ctx, &mut app, vec![Event::PointerMoved(p)]);
    step(
        &ctx,
        &mut app,
        vec![button(p, PointerButton::Primary, true)],
    );
    let output = step(
        &ctx,
        &mut app,
        vec![Event::PointerMoved(Pos2::new(50.0, 50.0))],
    );
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::StartDrag)
    );
}

#[test]
fn upload_only_on_new_snapshot_and_use_nearest_filter() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]); // Font upload is unrelated to the emote.
    for shade in [42, 84] {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[shade, 7, 9, 255].repeat(64 * 64))
                .unwrap();
        }
        let mut socket = TcpStream::connect(address).unwrap();
        write!(socket, "POST /emote HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\nContent-Length: {}\r\n\r\n", png.len()).unwrap();
        socket.write_all(&png).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 204"));
        let output = step(&ctx, &mut app, vec![]);
        let updates: Vec<_> = output
            .textures_delta
            .set
            .values()
            .flatten()
            .filter(|delta| delta.image.size() == [64, 64])
            .collect();
        assert_eq!(updates.len(), 1);
        let delta = updates[0];
        assert_eq!(delta.options, egui::TextureOptions::NEAREST);
        let egui::ImageData::Color(image) = &delta.image;
        assert!(
            image
                .pixels
                .iter()
                .all(|pixel| *pixel == egui::Color32::from_rgb(shade, 7, 9))
        );
        let again = step(&ctx, &mut app, vec![]);
        assert!(
            again.textures_delta.set.is_empty(),
            "unchanged Arc must not upload"
        );
    }
}

#[test]
fn burst_coalesces_and_app_drop_cancels_partial_client() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    // Finish every publication before the GUI reads again: the slot is a
    // snapshot, not a backlog of texture uploads for the GUI to drain.
    for shade in [0, 127, 255] {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[shade, 9, 11, 255].repeat(64 * 64))
                .unwrap();
        }
        let mut socket = TcpStream::connect(address).unwrap();
        write!(socket, "POST /emote HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\nContent-Length: {}\r\n\r\n", png.len()).unwrap();
        socket.write_all(&png).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 204"));
    }
    let output = step(&ctx, &mut app, vec![]);
    let updates: Vec<_> = output
        .textures_delta
        .set
        .values()
        .flatten()
        .filter(|delta| delta.image.size() == [64, 64])
        .collect();
    assert_eq!(updates.len(), 1);
    let egui::ImageData::Color(image) = &updates[0].image;
    assert!(
        image
            .pixels
            .iter()
            .all(|pixel| *pixel == egui::Color32::from_rgb(255, 9, 11))
    );
    let again = step(&ctx, &mut app, vec![]);
    assert!(again.textures_delta.set.is_empty());

    // Leave an incomplete request alive. App ownership must cancel it without
    // waiting for its headers to finish, and release the listener synchronously.
    let mut partial = TcpStream::connect(address).unwrap();
    partial.write_all(b"POST /emote HTTP/1.1\r\nHost:").unwrap();
    drop(app);
    let _released = TcpListener::bind(address).expect("app drop must release its listening port");
}
