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

fn session_request(address: std::net::SocketAddr, id: &str, seq: u64, kind: &str, shade: u8) {
    let mut png = Vec::new();
    if kind == "emote" {
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
    write!(socket, "POST /{kind} HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\nX-Emote-Session-Id: {id}\r\nX-Emote-Stream-Id: stream-{id}\r\nX-Emote-Sequence: {seq}\r\nContent-Length: {}\r\n\r\n", png.len()).unwrap();
    socket.write_all(&png).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204"), "{response}");
}

fn wide_step(ctx: &egui::Context, app: &mut EmoteApp, width: f32) -> Output {
    let mut raw = input(vec![]);
    raw.screen_rect = Some(egui::Rect::from_min_size(
        Pos2::ZERO,
        Vec2::new(width, 192.0),
    ));
    Output(ctx.run_ui(raw, |ui| app.show(ui)))
}

fn avatars(output: &Output) -> Vec<(egui::TextureId, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Mesh(mesh) = &shape.shape {
                Some((mesh.texture_id, mesh.calc_bounds()))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn multi_avatar_bevels_frame_tiles_and_preserve_single_avatar_fit() {
    let segments = |output: &Output| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke } => Some((*points, *stroke)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for transparent in [false, true] {
        let (ctx, mut app, address) = setup();
        session_request(address, "a", 1, "emote", 42);
        // Register the hit target before sending the transparency menu's click.
        step(&ctx, &mut app, vec![]);
        if transparent {
            toggle_background(&ctx, &mut app);
        }
        let single = step(&ctx, &mut app, vec![]);
        assert!(segments(&single).is_empty(), "a single avatar has no bevel");
        let original_bounds = avatars(&single)[0].1;
        drop(single);

        session_request(address, "b", 1, "emote", 84);
        let multiple = wide_step(&ctx, &mut app, 384.0);
        let mut expected = Vec::new();
        for i in 0..2 {
            let tile =
                egui::Rect::from_min_size(Pos2::new(i as f32 * 192.0, 0.0), Vec2::splat(192.0));
            let edge = tile.shrink(1.5);
            let light = egui::Stroke::new(3.0, egui::Color32::from_gray(160));
            let dark = egui::Stroke::new(3.0, egui::Color32::from_gray(64));
            expected.extend([
                ([edge.left_top(), edge.right_top()], light),
                ([edge.left_top(), edge.left_bottom()], light),
                ([edge.right_top(), edge.right_bottom()], dark),
                ([edge.left_bottom(), edge.right_bottom()], dark),
            ]);
            let image = avatars(&multiple)[i].1;
            assert!(
                tile.shrink(4.0).contains_rect(image),
                "the complete PNG stays inside the frame margin"
            );
            assert!((image.width() - 64.0 * (184.0 / 69.0)).abs() < 0.001);
            assert!((image.top() - (4.0 + 5.0 * (184.0 / 69.0))).abs() < 0.001);
        }
        assert_eq!(
            segments(&multiple),
            expected,
            "every tile has four three-point light/dark edges in either background mode"
        );
        drop(multiple);

        // A resize can temporarily leave many tiles in a narrow old viewport.
        for width in [16.0, 4.0] {
            let narrow = wide_step(&ctx, &mut app, width);
            let edges = segments(&narrow);
            assert_eq!(edges.len(), 8);
            for (i, (_, image)) in avatars(&narrow).iter().enumerate() {
                let tile = egui::Rect::from_min_size(
                    Pos2::new(i as f32 * width / 2.0, 0.0),
                    Vec2::new(width / 2.0, 192.0),
                );
                assert!(image.is_positive());
                assert!(
                    tile.shrink(4.0_f32.min(tile.width() * 0.25))
                        .contains_rect(*image)
                );
                for (points, stroke) in &edges[i * 4..(i + 1) * 4] {
                    assert_eq!(stroke.width, 3.0_f32.min(tile.width() * 0.5));
                    assert!(tile.contains_rect(
                        egui::Rect::from_two_pos(points[0], points[1]).expand(stroke.width * 0.5)
                    ));
                }
            }
        }

        session_request(address, "b", 2, "emote/close", 0);
        let remaining = step(&ctx, &mut app, vec![]);
        assert!(
            segments(&remaining).is_empty(),
            "the bevel disappears with the second avatar"
        );
        assert_eq!(
            avatars(&remaining)[0].1,
            original_bounds,
            "single-avatar geometry is restored exactly"
        );
    }
}

#[test]
fn neighboring_sessions_isolate_uploads_and_release_removed_textures() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    let first = wide_step(&ctx, &mut app, 384.0);
    let tiles = avatars(&first);
    assert_eq!(tiles.len(), 2);
    assert!(tiles[0].1.right() < tiles[1].1.left());
    let scale = 184.0 / 69.0;
    for (i, (_, rect)) in tiles.iter().enumerate() {
        assert!((rect.center().x - (i as f32 * 192.0 + 96.0)).abs() < 0.001);
        assert!((rect.top() - (4.0 + 5.0 * scale)).abs() < 0.001);
        assert!((rect.width() - 64.0 * scale).abs() < 0.001);
    }
    assert!(
        first.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::InnerSize(Vec2::new(384.0, 192.0)))
    );
    assert_eq!(
        first
            .textures_delta
            .set
            .values()
            .flatten()
            .filter(|delta| delta.image.size() == [64, 64])
            .count(),
        2
    );
    drop(first);

    session_request(address, "a", 2, "emote", 126);
    let updated = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(avatars(&updated), tiles);
    let changes: Vec<_> = updated
        .textures_delta
        .set
        .iter()
        .filter(|(_, deltas)| deltas.iter().any(|d| d.image.size() == [64, 64]))
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(*changes[0].0, tiles[0].0);
    let delta = changes[0].1.last().unwrap();
    assert_eq!(delta.options, egui::TextureOptions::NEAREST);
    let egui::ImageData::Color(image) = &delta.image;
    assert!(
        image
            .pixels
            .iter()
            .all(|p| *p == egui::Color32::from_rgb(126, 7, 9))
    );
    assert!(
        updated.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
    drop(updated);

    session_request(address, "b", 2, "emote/heartbeat", 0);
    let heartbeat = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(avatars(&heartbeat), tiles);
    assert!(heartbeat.textures_delta.set.is_empty());
    assert!(
        heartbeat.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
    drop(heartbeat);

    session_request(address, "a", 3, "emote/close", 0);
    let closed = step(&ctx, &mut app, vec![]);
    assert_eq!(
        avatars(&closed).iter().map(|a| a.0).collect::<Vec<_>>(),
        vec![tiles[1].0]
    );
    assert!(closed.textures_delta.free.contains(&tiles[0].0));
    assert!(closed.textures_delta.set.is_empty());
    assert!(
        closed.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::InnerSize(Vec2::splat(192.0)))
    );
    drop(closed);

    session_request(address, "b", 3, "emote/close", 0);
    let retained = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&retained)[0].0, tiles[1].0);
    assert!(retained.textures_delta.set.is_empty());
    assert!(retained.textures_delta.free.is_empty());
    assert!(
        retained.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
    drop(retained);

    session_request(address, "c", 1, "emote", 168);
    let replaced = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&replaced).len(), 1);
    assert_ne!(avatars(&replaced)[0].0, tiles[1].0);
    assert!(replaced.textures_delta.free.contains(&tiles[1].0));
    assert!(
        replaced.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
}

#[test]
fn size_menu_applies_to_each_avatar_and_count_changes_keep_chosen_size() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    wide_step(&ctx, &mut app, 384.0);
    // Open the existing full-surface menu in the second tile.
    let p = Pos2::new(300.0, 20.0);
    let run = |app: &mut EmoteApp, events| {
        let mut raw = input(events);
        raw.screen_rect = Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(384.0, 192.0),
        ));
        Output(ctx.run_ui(raw, |ui| app.show(ui)))
    };
    run(
        &mut app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Secondary, true),
        ],
    );
    run(&mut app, vec![button(p, PointerButton::Secondary, false)]);
    let menu = run(&mut app, vec![]);
    let p = menu
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with("256") => {
                Some(t.pos + Vec2::splat(4.0))
            }
            _ => None,
        })
        .unwrap();
    drop(menu);
    run(
        &mut app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Primary, true),
        ],
    );
    let chosen = run(&mut app, vec![button(p, PointerButton::Primary, false)]);
    assert_eq!(
        chosen.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(512.0, 256.0))]
    );
    drop(chosen);
    assert!(
        run(&mut app, vec![]).viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty()
    );
    session_request(address, "a", 2, "emote/close", 0);
    let closed = run(&mut app, vec![]);
    assert_eq!(
        closed.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::splat(256.0))]
    );
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
    // Native capability is enabled; painting still defaults to opaque black.
    assert_eq!(options.viewport.transparent, Some(true));
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
    for label in ["128", "192", "256", "384", "Transparent background", "Exit"] {
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
        if label == "192" {
            assert!(commands.is_empty(), "unchanged size: {commands:?}");
        } else {
            assert!(commands.contains(&command), "{label}: {commands:?}");
        }
        assert!(!commands.contains(&ViewportCommand::StartDrag));
    }
}

fn toggle_background(ctx: &egui::Context, app: &mut EmoteApp) {
    let p = Pos2::new(20.0, 20.0);
    step(
        ctx,
        app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Secondary, true),
        ],
    );
    step(ctx, app, vec![button(p, PointerButton::Secondary, false)]);
    let output = step(ctx, app, vec![]);
    let p = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "Transparent background"
            {
                return Some(text.pos + Vec2::splat(4.0));
            }
            None
        })
        .expect("transparency checkbox must be painted");
    step(
        ctx,
        app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Primary, true),
        ],
    );
    let output = step(ctx, app, vec![button(p, PointerButton::Primary, false)]);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty(),
        "toggle must not move, resize, or close the window"
    );
}

fn has_black_background(output: &Output) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.rect.size() == Vec2::splat(192.0) && rect.fill == egui::Color32::BLACK)
    })
}

#[test]
fn background_toggle_round_trip_and_transparent_renderer_clear() {
    let (ctx, mut app, _) = setup();
    assert_eq!(
        eframe::App::clear_color(&app, &egui::Visuals::default()),
        [0.0; 4]
    );
    assert!(has_black_background(&step(&ctx, &mut app, vec![])));
    toggle_background(&ctx, &mut app);
    assert!(!has_black_background(&step(&ctx, &mut app, vec![])));
    toggle_background(&ctx, &mut app);
    assert!(has_black_background(&step(&ctx, &mut app, vec![])));
}

#[test]
fn native_ui_handles_missing_window_across_background_transitions() {
    let (ctx, mut app, _) = setup();
    let mut frame = eframe::Frame::_new_kittest();
    assert!(frame.winit_window().is_none());
    for opaque in [true, false, true, false, true] {
        for _ in 0..3 {
            let output = Output(ctx.run_ui(input(vec![]), |ui| {
                eframe::App::ui(&mut app, ui, &mut frame);
            }));
            assert_eq!(has_black_background(&output), opaque);
            assert_eq!(
                eframe::App::clear_color(&app, &egui::Visuals::default()),
                [0.0; 4]
            );
        }
        toggle_background(&ctx, &mut app);
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
                .write_image_data(
                    &[shade, 7, 9, 255, 0, 0, 0, 255, 80, 40, 20, 128, 0, 0, 0, 0]
                        .repeat(64 * 64 / 4),
                )
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
        assert!(image.pixels.iter().enumerate().all(|(i, pixel)| {
            *pixel
                == [
                    egui::Color32::from_rgb(shade, 7, 9),
                    egui::Color32::BLACK,
                    egui::Color32::from_rgba_unmultiplied(80, 40, 20, 128),
                    egui::Color32::TRANSPARENT,
                ][i % 4]
        }));
        assert!(has_black_background(&output));
        toggle_background(&ctx, &mut app);
        let again = step(&ctx, &mut app, vec![]);
        assert!(!has_black_background(&again));
        assert!(
            again.textures_delta.set.is_empty(),
            "background toggle must not upload unchanged pixels"
        );
        toggle_background(&ctx, &mut app);
    }
}

#[test]
fn image_fits_with_five_source_rows_of_top_padding_and_full_uv() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    let mut pixels = [42, 7, 9, 255].repeat(64 * 64);
    pixels[59 * 64 * 4..].copy_from_slice(&[0, 0, 0, 255].repeat(5 * 64));
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    let mut socket = TcpStream::connect(address).unwrap();
    write!(socket, "POST /emote HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\nContent-Length: {}\r\n\r\n", png.len()).unwrap();
    socket.write_all(&png).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204"));

    for size in [
        Vec2::splat(128.0),
        Vec2::splat(192.0),
        Vec2::splat(256.0),
        Vec2::splat(384.0),
        Vec2::new(256.0, 192.0),
        Vec2::new(128.0, 256.0),
    ] {
        let mut raw = input(vec![]);
        raw.screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, size));
        let mut surface = egui::Rect::NOTHING;
        let output = Output(ctx.run_ui(raw, |ui| {
            surface = ui.max_rect();
            app.show(ui);
        }));
        let meshes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), 1);
        let mesh = meshes[0];
        let scale = (surface.width() / 64.0).min(surface.height() / 69.0);
        let expected = egui::Rect::from_min_size(
            egui::pos2(
                surface.center().x - 32.0 * scale,
                surface.center().y - 69.0 * scale / 2.0 + 5.0 * scale,
            ),
            Vec2::splat(64.0 * scale),
        );
        assert_eq!(mesh.vertices.len(), 4);
        for (pos, uv) in [
            (expected.left_top(), egui::pos2(0.0, 0.0)),
            (expected.right_top(), egui::pos2(1.0, 0.0)),
            (expected.left_bottom(), egui::pos2(0.0, 1.0)),
            (expected.right_bottom(), egui::pos2(1.0, 1.0)),
        ] {
            assert!(
                mesh.vertices.iter().any(|vertex| {
                    vertex.pos.distance(pos) < 0.001
                        && vertex.uv == uv
                        && vertex.color == egui::Color32::WHITE
                }),
                "missing image corner {pos:?} with full UV {uv:?}"
            );
        }
        let top_gap = expected.top() - surface.top();
        let content_bottom = expected.bottom() - 5.0 * scale;
        assert!((top_gap - (surface.bottom() - content_bottom)).abs() < 0.001);
        assert!(surface.contains_rect(expected));
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.rect == surface && rect.fill == egui::Color32::BLACK)
        }));
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
