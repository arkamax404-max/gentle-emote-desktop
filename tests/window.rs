use eframe::egui::{self, Event, PointerButton, Pos2, Vec2, ViewportCommand};
use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    receiver::Receiver,
};
#[path = "support/menu.rs"]
mod menu;

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
    menu::install();
    ctx.set_embed_viewports(false);
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
struct Output(
    egui::FullOutput,
    std::collections::HashMap<egui::TextureId, String>,
);
impl Output {
    fn new(ctx: &egui::Context, full: egui::FullOutput) -> Self {
        let manager = ctx.tex_manager();
        let manager = manager.read();
        let names = full
            .shapes
            .iter()
            .filter_map(|shape| {
                let egui::Shape::Mesh(mesh) = &shape.shape else {
                    return None;
                };
                manager
                    .meta(mesh.texture_id)
                    .map(|meta| (mesh.texture_id, meta.name.clone()))
            })
            .collect();
        Self(full, names)
    }
}
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
    Output::new(ctx, ctx.run_ui(input(events), |ui| app.show(ui)))
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

fn surface_step(ctx: &egui::Context, app: &mut EmoteApp, size: Vec2) -> Output {
    let mut raw = input(vec![]);
    raw.screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, size));
    Output::new(ctx, ctx.run_ui(raw, |ui| app.show(ui)))
}

fn wide_step(ctx: &egui::Context, app: &mut EmoteApp, width: f32) -> Output {
    surface_step(ctx, app, Vec2::new(width, 192.0))
}

fn avatars(output: &Output) -> Vec<(egui::TextureId, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Mesh(mesh) = &shape.shape
                && output
                    .1
                    .get(&mesh.texture_id)
                    .is_some_and(|name| name == "emote" || name.starts_with("emote-session:"))
            {
                Some((mesh.texture_id, mesh.calc_bounds()))
            } else {
                None
            }
        })
        .collect()
}

fn frames(output: &Output) -> Vec<(egui::TextureId, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            let egui::Shape::Mesh(mesh) = &shape.shape else {
                return None;
            };
            (output
                .1
                .get(&mesh.texture_id)
                .is_some_and(|name| name == "avatar-frame"))
            .then(|| (mesh.texture_id, mesh.calc_bounds()))
        })
        .collect()
}

fn frame_pixels() -> (png::OutputInfo, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../assets/avatar-frame.png"
    )))
    .read_info()
    .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    (info, pixels)
}

#[test]
fn supplied_frame_derivative_has_real_alpha_and_preserves_neon_artwork() {
    let (info, pixels) = frame_pixels();
    assert_eq!(
        (info.width, info.height, info.color_type, info.bit_depth),
        (1100, 1092, png::ColorType::Rgba, png::BitDepth::Eight)
    );
    assert_eq!(pixels[3], 0, "outer checkerboard must be removed");
    for y in 121..972 {
        for x in 125..976 {
            assert_eq!(
                pixels[(y * 1100 + x) * 4 + 3],
                0,
                "safe opening must not cover the avatar"
            );
        }
    }
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255 && p[1] > 180 && p[0] < 180 && p[2] < 120)
            .count()
            > 100
    );
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255 && p[1] > 180 && p[2] > 180 && p[0] < 120)
            .count()
            > 100
    );
}

#[test]
fn multi_avatar_png_frames_preserve_transparency_single_fit_and_labels() {
    let (info, pixels) = frame_pixels();
    for transparent in [false, true] {
        let (ctx, mut app, address) = setup();
        session_request(address, "a", 1, "emote", 42);
        step(&ctx, &mut app, vec![]);
        if transparent {
            toggle_background(&ctx, &mut app);
        }
        let single = step(&ctx, &mut app, vec![]);
        assert!(
            frames(&single).is_empty(),
            "a single avatar remains borderless"
        );
        let original_bounds = avatars(&single)[0].1;
        drop(single);
        session_request(address, "b", 1, "emote", 84);
        let first = wide_step(&ctx, &mut app, 384.0);
        let borders = frames(&first);
        assert_eq!(
            borders.len(),
            if transparent { 0 } else { 2 },
            "only opaque avatars share the PNG frame"
        );
        if !transparent {
            assert_eq!(
                borders[0].0, borders[1].0,
                "upload one shared frame texture"
            );
            let delta = first.textures_delta.set[&borders[0].0].last().unwrap();
            assert_eq!(delta.options, egui::TextureOptions::NEAREST);
        }
        drop(first);
        for size in [128.0, 192.0, 256.0, 384.0] {
            let output = surface_step(&ctx, &mut app, Vec2::new(size * 2.0, size));
            assert!(
                output.textures_delta.set.is_empty(),
                "resizing must reuse unchanged textures"
            );
            assert!(
                !output
                    .shapes
                    .iter()
                    .any(|s| matches!(s.shape, egui::Shape::LineSegment { .. })),
                "the old bevel must not be painted"
            );
            let borders = frames(&output);
            assert_eq!(borders.len(), if transparent { 0 } else { 2 });
            for (i, (_, image)) in avatars(&output).iter().enumerate() {
                let tile =
                    egui::Rect::from_min_size(Pos2::new(i as f32 * size, 0.0), Vec2::splat(size));
                assert!(tile.contains_rect(*image));
                if transparent {
                    let scale = (size - 8.0) / 69.0;
                    assert!(
                        (image.width() - 64.0 * scale).abs() < 0.001,
                        "transparent geometry unchanged"
                    );
                } else {
                    let frame = borders[i].1;
                    assert!(tile.contains_rect(frame));
                    assert!(
                        (frame.width() / frame.height() - 1100.0 / 1092.0).abs() < 0.001,
                        "preserve frame aspect ratio"
                    );
                    assert!(image.width() > size * 0.65, "keep a usable avatar at128");
                    let lo = (image.min - frame.min) / frame.size();
                    let hi = (image.max - frame.min) / frame.size();
                    for y in (lo.y * info.height as f32).floor() as usize
                        ..(hi.y * info.height as f32).ceil() as usize
                    {
                        for x in (lo.x * info.width as f32).floor() as usize
                            ..(hi.x * info.width as f32).ceil() as usize
                        {
                            assert_eq!(
                                pixels[(y * info.width as usize + x) * 4 + 3],
                                0,
                                "frame must not cover any image/label pixel"
                            );
                        }
                    }
                }
            }
        }
        toggle_background(&ctx, &mut app);
        let toggled = wide_step(&ctx, &mut app, 384.0);
        assert_eq!(frames(&toggled).len(), if transparent { 2 } else { 0 });
        drop(toggled);
        toggle_background(&ctx, &mut app);
        for width in [16.0, 4.0] {
            let narrow = wide_step(&ctx, &mut app, width);
            for (i, (_, image)) in avatars(&narrow).iter().enumerate() {
                let tile = egui::Rect::from_min_size(
                    Pos2::new(i as f32 * width / 2.0, 0.0),
                    Vec2::new(width / 2.0, 192.0),
                );
                assert!(image.is_positive() && tile.contains_rect(*image));
                if !transparent {
                    assert!(tile.contains_rect(frames(&narrow)[i].1));
                }
            }
        }
        session_request(address, "b", 2, "emote/close", 0);
        let remaining = step(&ctx, &mut app, vec![]);
        assert!(
            frames(&remaining).is_empty(),
            "remove the frame with the second avatar"
        );
        assert_eq!(
            avatars(&remaining)[0].1,
            original_bounds,
            "restore single-avatar geometry exactly"
        );
    }
}

#[test]
fn no_frame_keeps_five_logical_pixels_between_adjacent_images() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    step(&ctx, &mut app, vec![]);
    toggle_option(&ctx, &mut app, "No frame");
    for count in [2, 3] {
        if count == 3 {
            session_request(address, "c", 1, "emote", 126);
        }
        for transparent in [false, true] {
            if transparent {
                toggle_background(&ctx, &mut app);
            }
            for size in [128.0, 192.0, 256.0, 384.0] {
                let mut raw = input(vec![]);
                let bounds =
                    egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(count as f32 * size, size));
                raw.screen_rect = Some(bounds);
                let output = Output::new(&ctx, ctx.run_ui(raw, |ui| app.show(ui)));
                let images = avatars(&output);
                assert_eq!(images.len(), count);
                assert!(frames(&output).is_empty());
                for pair in images.windows(2) {
                    let gap = pair[1].1.left() - pair[0].1.right();
                    assert!(
                        (gap - 5.0).abs() < 0.001,
                        "{count} images, size {size}, transparent={transparent}: expected 5 px, got {gap}"
                    );
                    let strip = egui::Rect::from_min_max(
                        egui::pos2(pair[0].1.right() + 0.001, bounds.top()),
                        egui::pos2(pair[1].1.left() - 0.001, bounds.bottom()),
                    );
                    assert!(
                        output.shapes.iter().all(|shape| !matches!(
                            &shape.shape, egui::Shape::Rect(paint)
                            if paint.fill.a() > 0 && paint.rect.intersects(strip)
                        )),
                        "the five-pixel separator must have no opaque background paint"
                    );
                    assert!((pair[0].1.top() - pair[1].1.top()).abs() < 0.001);
                    assert!((pair[0].1.size() - pair[1].1.size()).length() < 0.001);
                }
                assert!(images.iter().all(|(_, image)| bounds.contains_rect(*image)));
                assert_eq!(
                    eframe::App::clear_color(&app, &egui::Visuals::default()),
                    [0.0; 4]
                );
                for (_, image) in &images {
                    let underlay = egui::pos2(image.center().x, bounds.top() + 1.0);
                    let black = output.shapes.iter().any(|shape| {
                        matches!(
                            &shape.shape, egui::Shape::Rect(paint)
                            if paint.fill == egui::Color32::BLACK && paint.rect.contains(underlay)
                        )
                    });
                    assert_eq!(
                        black, !transparent,
                        "only the gaps become transparent; preserve the background setting"
                    );
                }
            }
        }
        toggle_background(&ctx, &mut app);
    }
}

#[test]
fn no_frame_toggle_is_independent_of_transparency_and_preserves_avatar_pixels() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    let single = step(&ctx, &mut app, vec![]);
    let original_single = avatars(&single)[0].1;
    drop(single);
    session_request(address, "b", 1, "emote", 84);
    let initial = wide_step(&ctx, &mut app, 384.0);
    let shared_frame = frames(&initial)[0].0;
    let avatar_ids: Vec<_> = avatars(&initial).iter().map(|(id, _)| *id).collect();
    assert_eq!(frames(&initial).len(), 2, "default keeps existing frames");
    drop(initial);

    toggle_option(&ctx, &mut app, "No frame");
    let hidden = wide_step(&ctx, &mut app, 384.0);
    assert!(frames(&hidden).is_empty());
    assert!(
        hidden
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Rect(r)
        if r.fill == egui::Color32::BLACK && r.rect.contains(avatars(&hidden)[0].1.center()))),
        "No frame keeps the opaque avatar underlay outside the transparent gaps"
    );
    assert_eq!(
        avatars(&hidden)
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        avatar_ids
    );
    assert!(
        hidden.textures_delta.set.is_empty(),
        "hiding only the decoration does not reupload image pixels"
    );
    assert!(
        (avatars(&hidden)[0].1.width() - 64.0 * 184.0 / 69.0).abs() < 0.001,
        "reclaim the unframed image fit"
    );
    drop(hidden);

    toggle_background(&ctx, &mut app);
    assert!(!has_black_background(&step(&ctx, &mut app, vec![])));
    toggle_option(&ctx, &mut app, "No frame");
    assert!(
        frames(&wide_step(&ctx, &mut app, 384.0)).is_empty(),
        "transparent mode never draws frames even when no frame is unchecked"
    );
    toggle_background(&ctx, &mut app);
    let restored = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(
        frames(&restored)
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![shared_frame; 2]
    );
    assert!(
        restored.textures_delta.set.is_empty(),
        "reuse the shared frame texture"
    );
    assert!(has_black_background(&step(&ctx, &mut app, vec![])));
    drop(restored);

    toggle_background(&ctx, &mut app);
    toggle_option(&ctx, &mut app, "No frame");
    toggle_background(&ctx, &mut app);
    let opaque_unframed = wide_step(&ctx, &mut app, 384.0);
    assert!(
        frames(&opaque_unframed).is_empty(),
        "no frame selection survives a transparent round trip"
    );
    drop(opaque_unframed);
    session_request(address, "b", 2, "emote/close", 0);
    let remaining = step(&ctx, &mut app, vec![]);
    assert!(frames(&remaining).is_empty());
    assert_eq!(
        avatars(&remaining)[0].1,
        original_single,
        "single avatar geometry unchanged"
    );
}

#[test]
fn small_surface_menu_has_independent_viewport_and_complete_controls() {
    for size in [126.0, 128.0] {
        let (ctx, mut app, _) = setup();
        let run = |app: &mut EmoteApp, events| {
            let mut raw = input(events);
            raw.screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(size)));
            Output::new(&ctx, ctx.run_ui(raw, |ui| app.show(ui)))
        };
        run(&mut app, vec![]);
        let p = Pos2::new(size - 8.0, size - 8.0);
        run(
            &mut app,
            vec![
                Event::PointerMoved(p),
                button(p, PointerButton::Secondary, true),
            ],
        );
        run(&mut app, vec![button(p, PointerButton::Secondary, false)]);
        let root = run(&mut app, vec![]);
        let (_, popup) = root
            .viewport_output
            .iter()
            .find(|(id, _)| **id != egui::ViewportId::ROOT)
            .expect("small avatar options need an independent native viewport");
        assert_eq!(popup.parent, egui::ViewportId::ROOT);
        // egui 0.37 leaves the exported immediate class at its default. Prove
        // the route through the native immediate renderer instead (menu::take
        // below), with a distinct child ID, ROOT parent and no deferred callback.
        assert!(popup.viewport_ui_cb.is_none());
        let bounds = egui::Rect::from_min_size(Pos2::ZERO, popup.builder.inner_size.unwrap());
        assert!(bounds.width() > size);
        assert!(
            root.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty()
        );
        assert!(!format!("{:?}", root.shapes).contains("Transparent background"));
        let child = menu::take();
        let mut text_bounds = egui::Rect::NOTHING;
        for label in [
            "128 × 128",
            "192 × 192",
            "256 × 256",
            "384 × 384",
            "Transparent background",
            "No frame",
            "Exit",
        ] {
            let text = child
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some(text),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing {label}"));
            let label_bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
            text_bounds = text_bounds.union(label_bounds);
            assert!(bounds.contains_rect(label_bounds), "clipped {label}");
        }
        assert!(
            bounds.right() - text_bounds.right() <= 20.0,
            "menu has unnecessary horizontal space"
        );
        assert!(
            bounds.bottom() - text_bounds.bottom() <= 16.0,
            "menu has unnecessary bottom space"
        );
    }
}

#[test]
fn independent_menu_dismisses_and_reopens_without_closing_the_avatar() {
    for dismissal in 0..3 {
        let (ctx, mut app, _) = setup();
        let open = |app: &mut EmoteApp| {
            step(&ctx, app, vec![]);
            let p = Pos2::new(20.0, 20.0);
            step(
                &ctx,
                app,
                vec![
                    Event::PointerMoved(p),
                    button(p, PointerButton::Secondary, true),
                ],
            );
            step(&ctx, app, vec![button(p, PointerButton::Secondary, false)]);
            step(&ctx, app, vec![])
        };
        let first = open(&mut app);
        let id = *first
            .viewport_output
            .keys()
            .find(|id| **id != egui::ViewportId::ROOT)
            .unwrap();
        menu::take();
        // An initial focus=false before the OS grants focus must not self-dismiss.
        let focus = |value| {
            let mut raw = egui::RawInput::default();
            raw.viewports.insert(
                id,
                egui::ViewportInfo {
                    focused: Some(value),
                    ..Default::default()
                },
            );
            menu::raw(raw);
        };
        focus(false);
        step(&ctx, &mut app, vec![]);
        menu::take();
        step(&ctx, &mut app, vec![]);
        menu::take();
        match dismissal {
            0 => menu::events(vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]),
            1 => {
                let mut raw = egui::RawInput::default();
                raw.viewports.insert(
                    id,
                    egui::ViewportInfo {
                        events: vec![egui::ViewportEvent::Close],
                        ..Default::default()
                    },
                );
                menu::raw(raw);
            }
            _ => {
                focus(true);
                step(&ctx, &mut app, vec![]);
                menu::take();
                focus(false);
            }
        }
        let closing = step(&ctx, &mut app, vec![]);
        let child = menu::take();
        assert!(
            closing.viewport_output[&id]
                .commands
                .contains(&ViewportCommand::Close)
        );
        for output in [&*closing, &child] {
            assert!(
                !output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&ViewportCommand::Close)
            );
        }
        step(&ctx, &mut app, vec![]);
        assert!(
            menu::take_optional().is_none(),
            "dismissed popup must stop rendering"
        );
        let reopened = open(&mut app);
        assert!(
            reopened.viewport_output.contains_key(&id),
            "reopen the same popup, not another window"
        );
        assert!(format!("{:?}", menu::take().shapes).contains("Transparent background"));
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
    let artwork_scale = 192.0 / 1100.0;
    let scale = (851.0 * artwork_scale) / 69.0;
    for (i, (_, rect)) in tiles.iter().enumerate() {
        let opening = egui::Rect::from_min_size(
            Pos2::new(i as f32 * 192.0, (192.0 - 1092.0 * artwork_scale) / 2.0)
                + Vec2::new(125.0, 121.0) * artwork_scale,
            Vec2::splat(851.0 * artwork_scale),
        );
        assert!((rect.center().x - opening.center().x).abs() < 0.001);
        assert!((rect.top() - (opening.top() + 5.0 * scale)).abs() < 0.001);
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
        Output::new(&ctx, ctx.run_ui(raw, |ui| app.show(ui)))
    };
    run(
        &mut app,
        vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Secondary, true),
        ],
    );
    run(&mut app, vec![button(p, PointerButton::Secondary, false)]);
    run(&mut app, vec![]);
    let menu = menu::take();
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
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    run(&mut app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    let chosen = run(&mut app, vec![]);
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
    let text = format!("{:?}", menu::take().shapes);
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
        step(&ctx, &mut app, vec![]);
        let output = menu::take();
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
        menu::events(vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Primary, true),
        ]);
        step(&ctx, &mut app, vec![]);
        menu::events(vec![button(p, PointerButton::Primary, false)]);
        let output = step(&ctx, &mut app, vec![]);
        let child = menu::take();
        let mut commands = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .clone();
        commands.extend(
            child.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .clone(),
        );
        if label == "192" {
            assert!(commands.is_empty(), "unchanged size: {commands:?}");
        } else {
            assert!(commands.contains(&command), "{label}: {commands:?}");
        }
        assert!(!commands.contains(&ViewportCommand::StartDrag));
    }
}

fn toggle_background(ctx: &egui::Context, app: &mut EmoteApp) {
    toggle_option(ctx, app, "Transparent background");
}

fn toggle_option(ctx: &egui::Context, app: &mut EmoteApp, label: &str) {
    let output = select_menu_option(ctx, app, label);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty(),
        "toggle must not move, resize, or close the window"
    );
}

fn select_menu_option(ctx: &egui::Context, app: &mut EmoteApp, label: &str) -> Output {
    // Restore the helper's normal surface before hit-testing a click: the last
    // public render may have exercised a narrow transient viewport.
    step(ctx, app, vec![]);
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
    step(ctx, app, vec![]);
    let output = menu::take();
    let p = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == label
            {
                return Some(text.pos + Vec2::splat(4.0));
            }
            None
        })
        .unwrap_or_else(|| panic!("menu option must be painted: {label}"));
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    step(ctx, app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    step(ctx, app, vec![])
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
            let output = Output::new(
                &ctx,
                ctx.run_ui(input(vec![]), |ui| {
                    eframe::App::ui(&mut app, ui, &mut frame);
                }),
            );
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
        let output = Output::new(
            &ctx,
            ctx.run_ui(raw, |ui| {
                surface = ui.max_rect();
                app.show(ui);
            }),
        );
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
