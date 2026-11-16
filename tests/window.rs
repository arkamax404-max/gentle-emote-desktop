use eframe::egui::{self, Event, PointerButton, Pos2, Vec2, ViewportCommand};
use gentle_emote_desktop::{
    app::{EmoteApp, native_options, snapped_physical_image_fit},
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
    // Legacy scenarios explicitly select their opaque, framed, horizontal x3 mode.
    let (ctx, mut app, address) = setup_defaults();
    toggle_background(&ctx, &mut app);
    toggle_option(&ctx, &mut app, "No frame");
    select_menu_option(&ctx, &mut app, "Horizontal");
    set_size_step(&ctx, &mut app, 3);
    menu::events(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    step(&ctx, &mut app, vec![]);
    step(&ctx, &mut app, vec![]);
    (ctx, app, address)
}

fn setup_defaults() -> (egui::Context, EmoteApp, std::net::SocketAddr) {
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
    session_request_state(address, id, seq, kind, shade, None);
}

fn session_request_state(
    address: std::net::SocketAddr,
    id: &str,
    seq: u64,
    kind: &str,
    shade: u8,
    state: Option<&str>,
) {
    session_request_named(address, id, seq, kind, shade, state, None);
}

fn session_request_named(
    address: std::net::SocketAddr,
    id: &str,
    seq: u64,
    kind: &str,
    shade: u8,
    state: Option<&str>,
    name: Option<&str>,
) {
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
    let state = state
        .map(|value| format!("X-Emote-State: {value}\r\n"))
        .unwrap_or_default();
    let name = name
        .map(|name| format!("X-Emote-Project-Name: {name}\r\n"))
        .unwrap_or_default();
    let identity = if id.is_empty() {
        String::new() // Exercise the public legacy singleton without identity headers.
    } else {
        format!(
            "X-Emote-Session-Id: {id}\r\nX-Emote-Stream-Id: stream-{id}\r\nX-Emote-Sequence: {seq}\r\n"
        )
    };
    let mut socket = TcpStream::connect(address).unwrap();
    write!(socket, "POST /{kind} HTTP/1.1\r\nHost: localhost\r\nContent-Type: image/png\r\nX-Emote-Version: 1\r\n{state}{name}{identity}Content-Length: {}\r\n\r\n", png.len()).unwrap();
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

/// Every image edge must sit on an integer physical pixel at the context's
/// `pixels_per_point`, the quad must stay square, and its width must be a
/// whole number of 64-pixel source repeats (native scale, never fractional).
fn assert_whole_physical_pixels(image: egui::Rect, ppp: f32, context: &str) {
    for value in [image.min.x, image.max.x, image.min.y, image.max.y] {
        let physical = value * ppp;
        assert!(
            (physical - physical.round()).abs() < 1e-3,
            "{context}: edge {value} must sit on an integer physical pixel at {ppp}"
        );
    }
    assert!(
        (image.width() - image.height()).abs() < 1e-3,
        "{context}: NEAREST pixels must stay square, got {image:?}"
    );
    let physical_width = image.width() * ppp;
    assert!(
        (physical_width / 64.0 - (physical_width / 64.0).round()).abs() < 1e-3,
        "{context}: {physical_width} physical pixels must be a whole number of 64 source pixels"
    );
}

fn avatar_mesh(output: &Output, id: egui::TextureId) -> egui::epaint::Mesh {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == id => Some((**mesh).clone()),
            _ => None,
        })
        .expect("avatar mesh must be painted")
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

fn set_background_opacity(ctx: &egui::Context, app: &mut EmoteApp, fraction: f32) {
    select_menu_option(ctx, app, "Background transparency");
    let enabled = menu::checked(&menu::take(), "Transparent background");
    if !enabled {
        toggle_background(ctx, app);
    }
    set_background_transparency(ctx, app, 1.0 - fraction, true);
}

fn set_background_transparency(
    ctx: &egui::Context,
    app: &mut EmoteApp,
    fraction: f32,
    checked: bool,
) {
    select_menu_option(ctx, app, "Background transparency");
    let point = menu::slider_position("Background transparency", fraction);
    menu::events(vec![
        Event::PointerMoved(point),
        button(point, PointerButton::Primary, true),
    ]);
    step(ctx, app, vec![]);
    menu::events(vec![button(point, PointerButton::Primary, false)]);
    step(ctx, app, vec![]);
    let child = menu::take();
    assert_eq!(menu::checked(&child, "Transparent background"), checked);
    let percentage = if checked {
        (fraction * 100.0).round() as u8
    } else {
        0
    };
    for text in [percentage.to_string(), "%".to_owned()] {
        assert!(
            child
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t)
            if t.galley.text() == text)),
            "slider must display exact {percentage}%"
        );
    }
    assert!(
        child
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Rect(r)
        if r.fill.a()==255 && r.rect.contains(Pos2::new(1.0,1.0)))),
        "menu stays opaque while adjusting opacity"
    );
    menu::events(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    step(ctx, app, vec![]);
}

fn expected_unframed_cards(bounds: egui::Rect, count: usize, vertical: bool) -> Vec<egui::Rect> {
    let along = if vertical {
        bounds.height()
    } else {
        bounds.width()
    };
    let gap = 5.0_f32.min(along / (2.0 * count as f32 - 1.0));
    let span = (along - gap * (count - 1) as f32) / count as f32;
    (0..count)
        .map(|index| {
            let offset = index as f32 * (span + gap);
            if vertical {
                egui::Rect::from_min_size(
                    bounds.min + Vec2::new(0.0, offset),
                    Vec2::new(bounds.width(), span),
                )
            } else {
                egui::Rect::from_min_size(
                    bounds.min + Vec2::new(offset, 0.0),
                    Vec2::new(span, bounds.height()),
                )
            }
        })
        .collect()
}

#[test]
fn unframed_cards_keep_uniform_individual_composition_and_clear_separators() {
    for vertical in [false, true] {
        for count in [2, 3] {
            let (ctx, mut app, address) = setup();
            for (id, shade) in [("a", 42), ("b", 84), ("c", 126)].into_iter().take(count) {
                session_request(address, id, 1, "emote", shade);
            }
            step(&ctx, &mut app, vec![]);
            toggle_option(&ctx, &mut app, "No frame");
            if vertical {
                select_menu_option(&ctx, &mut app, "Vertical");
            }
            for opacity in [0.5, 0.0, 1.0] {
                set_background_opacity(&ctx, &mut app, opacity);
                for size in [128.0, 192.0, 256.0, 384.0] {
                    let dimensions = if vertical {
                        Vec2::new(size, count as f32 * size)
                    } else {
                        Vec2::new(count as f32 * size, size)
                    };
                    let bounds = egui::Rect::from_min_size(Pos2::ZERO, dimensions);
                    let cards = expected_unframed_cards(bounds, count, vertical);
                    let output = surface_step(&ctx, &mut app, dimensions);
                    let images = avatars(&output);
                    assert_eq!(images.len(), count);
                    let paints: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(paints.len(), if opacity == 0.0 { 0 } else { count });
                    for (index, ((_, image), card)) in images.iter().zip(&cards).enumerate() {
                        let fitted = card.shrink(4.0);
                        let (_, expected) = snapped_physical_image_fit(fitted, 1.0);
                        let scale = expected.width() / 64.0;
                        assert!(
                            (image.min - expected.min).length() < 0.001
                                && (image.max - expected.max).length() < 0.001,
                            "each card must apply its own whole-physical-pixel PNG fit and top compensation: index={index}, vertical={vertical}, actual={image:?}, expected={expected:?}"
                        );
                        assert_whole_physical_pixels(*image, 1.0, "unframed card");
                        // The composition (image plus its one-source-row top
                        // padding) is the helper's exact rect and must stay
                        // inside the fitted card on whole physical pixels.
                        let comp = egui::Rect::from_min_max(
                            egui::pos2(expected.min.x, expected.min.y - scale),
                            expected.max,
                        );
                        assert!(
                            fitted.contains_rect(comp),
                            "apply one source row of top padding inside each card: index={index}, vertical={vertical}, comp={comp:?}, fitted={fitted:?}"
                        );
                        if opacity > 0.0 {
                            assert!(
                                (paints[index].rect.min - card.min).length() < 0.001
                                    && (paints[index].rect.max - card.max).length() < 0.001
                            );
                            assert_eq!(paints[index].corner_radius, egui::CornerRadius::same(8));
                        }
                    }
                    for pair in cards.windows(2) {
                        let strip = if vertical {
                            egui::Rect::from_min_max(
                                Pos2::new(bounds.left(), pair[0].bottom() + 0.001),
                                Pos2::new(bounds.right(), pair[1].top() - 0.001),
                            )
                        } else {
                            egui::Rect::from_min_max(
                                Pos2::new(pair[0].right() + 0.001, bounds.top()),
                                Pos2::new(pair[1].left() - 0.001, bounds.bottom()),
                            )
                        };
                        assert!(output.shapes.iter().all(|s|!matches!(&s.shape,egui::Shape::Rect(r) if r.fill.a()>0 && r.rect.intersects(strip))));
                        assert!(images.iter().all(|(_, image)| !image.intersects(strip)));
                    }
                    assert!(output.textures_delta.set.is_empty());
                }
            }
        }
    }
}

#[test]
fn partial_opacity_preserves_single_avatar_rounded_contour() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    step(&ctx, &mut app, vec![]);
    set_background_opacity(&ctx, &mut app, 0.5);
    for size in [128.0, 192.0, 256.0, 384.0] {
        let output = surface_step(&ctx, &mut app, Vec2::splat(size));
        let surface = egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(size));
        let background = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill.a() == 128 => Some(r),
                _ => None,
            })
            .expect("half-opacity background must be painted");
        assert_eq!(background.rect, surface);
        assert_eq!(
            background.corner_radius,
            egui::CornerRadius::same(8),
            "partial opacity must retain all four rounded corners without relying on native shadow"
        );
        let images = avatars(&output);
        assert_eq!(images.len(), 1);
        let inner = surface.shrink(8.0);
        for shape in &output.shapes {
            if let egui::Shape::Mesh(mesh) = &shape.shape {
                if mesh.texture_id != images[0].0 {
                    continue;
                }
                assert!(!mesh.vertices.is_empty());
                for vertex in &mesh.vertices {
                    let nearest = Pos2::new(
                        vertex.pos.x.clamp(inner.left(), inner.right()),
                        vertex.pos.y.clamp(inner.top(), inner.bottom()),
                    );
                    assert!(
                        vertex.pos.distance(nearest) <= 8.001,
                        "image pixels must not cover the rounded background contour: {:?}",
                        vertex.pos
                    );
                    assert_eq!(vertex.color, egui::Color32::WHITE);
                    assert!(
                        (0.0..=1.0).contains(&vertex.uv.x) && (0.0..=1.0).contains(&vertex.uv.y)
                    );
                }
            }
        }
        assert!(output.textures_delta.set.is_empty());
    }
    for fraction in [1.0, 0.0] {
        set_background_opacity(&ctx, &mut app, fraction);
        let output = step(&ctx, &mut app, vec![]);
        let image_id = avatars(&output)[0].0;
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Mesh(mesh)
            if mesh.texture_id == image_id && mesh.vertices.len() == 4)),
            "fully opaque/clear single-avatar paths preserve their original full-UV quad"
        );
    }
}

#[test]
fn single_avatar_frame_follows_opaque_no_frame_and_transparency_controls() {
    let (ctx, mut app, address) = setup();
    let waiting = step(&ctx, &mut app, vec![]);
    assert!(frames(&waiting).is_empty());
    assert!(
        waiting
            .textures_delta
            .set
            .values()
            .flatten()
            .all(|d| d.image.size() != [1100, 1092])
    );
    drop(waiting);
    session_request(address, "a", 1, "emote", 42);
    let initial = step(&ctx, &mut app, vec![]);
    assert_eq!(
        frames(&initial).len(),
        1,
        "an opaque single avatar must have its frame"
    );
    let frame_id = frames(&initial)[0].0;
    let avatar_id = avatars(&initial)[0].0;
    let framed_bounds = avatars(&initial)[0].1;
    assert_eq!(
        initial.textures_delta.set[&frame_id]
            .last()
            .unwrap()
            .options,
        egui::TextureOptions::NEAREST
    );
    drop(initial);

    for no_frame in [true, false] {
        toggle_option(&ctx, &mut app, "No frame");
        let opaque = step(&ctx, &mut app, vec![]);
        assert_eq!(frames(&opaque).len(), usize::from(!no_frame));
        if no_frame {
            assert!(
                (avatars(&opaque)[0].1.width() - 128.0).abs() < 0.001,
                "No frame retains the original full-surface single-avatar composition at its whole-physical-pixel scale"
            );
        } else {
            assert_eq!(avatars(&opaque)[0].1, framed_bounds);
        }
        drop(opaque);
        toggle_background(&ctx, &mut app);
        for percentage in [0.0, 0.5, 1.0] {
            set_background_transparency(&ctx, &mut app, percentage, true);
            let output = step(&ctx, &mut app, vec![]);
            assert_eq!(
                frames(&output).len(),
                usize::from(percentage == 0.0 && !no_frame)
            );
            assert_eq!(avatars(&output)[0].0, avatar_id);
            assert!(output.textures_delta.set.is_empty());
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .is_empty()
            );
            assert!(output.shapes.iter().all(|s| {
                match &s.shape {
                    egui::Shape::Mesh(mesh) => mesh
                        .vertices
                        .iter()
                        .all(|v| v.color == egui::Color32::WHITE),
                    _ => true,
                }
            }));
            let alpha = if percentage == 0.0 { 255 } else { 128 };
            assert_eq!(
                output.shapes.iter().any(|s| matches!(&s.shape,
                egui::Shape::Rect(r) if r.fill.a() == alpha)),
                percentage != 1.0
            );
        }
        toggle_background(&ctx, &mut app);
        set_background_transparency(&ctx, &mut app, 0.5, false);
        let restored = step(&ctx, &mut app, vec![]);
        assert_eq!(
            frames(&restored)
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            if no_frame { vec![] } else { vec![frame_id] }
        );
        assert!(has_black_background(&restored));
        assert!(restored.textures_delta.set.is_empty());
    }
}

#[test]
fn transparency_checkbox_gates_slider_and_restores_opaque_frame_choice() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    let initial = wide_step(&ctx, &mut app, 384.0);
    let frame_id = frames(&initial)[0].0;
    let images: Vec<_> = avatars(&initial).iter().map(|(id, _)| *id).collect();
    drop(initial);
    set_background_transparency(&ctx, &mut app, 0.5, false);
    let disabled = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(frames(&disabled).len(), 2);
    assert!(
        disabled
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill.a() == 255))
    );
    assert!(disabled.textures_delta.set.is_empty());
    drop(disabled);
    // Exercise the real track endpoint before checking partial values.
    toggle_background(&ctx, &mut app);
    set_background_transparency(&ctx, &mut app, 0.0, true);
    let endpoint = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(
        frames(&endpoint).len(),
        2,
        "0% transparency must be opaque even while checked"
    );
    drop(endpoint);
    set_background_transparency(&ctx, &mut app, 0.5, true);
    let half = wide_step(&ctx, &mut app, 384.0);
    let fill: Vec<_> = half
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.fill),
            _ => None,
        })
        .collect();
    assert_eq!(fill.len(), 2);
    assert!(
        fill.iter().all(|color| color.to_array() == [0, 0, 0, 128]),
        "50% opacity applies to the black underlay only"
    );
    assert!(frames(&half).is_empty());
    assert_eq!(
        avatars(&half).iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        images
    );
    assert!(
        half.shapes.iter().all(|s| match &s.shape {
            egui::Shape::Mesh(mesh)
                if half
                    .1
                    .get(&mesh.texture_id)
                    .is_some_and(|name| name.starts_with("emote-session:")) =>
                mesh.vertices
                    .iter()
                    .all(|v| v.color == egui::Color32::WHITE),
            _ => true,
        }),
        "do not fade image pixels or embedded text"
    );
    assert!(half.textures_delta.set.is_empty());
    drop(half);
    set_background_transparency(&ctx, &mut app, 1.0, true);
    let clear = wide_step(&ctx, &mut app, 384.0);
    assert!(
        !clear
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.fill.a()>0))
    );
    assert_eq!(avatars(&clear).len(), 2);
    drop(clear);
    toggle_background(&ctx, &mut app);
    let restored = wide_step(&ctx, &mut app, 384.0);
    assert!(
        restored
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.fill.a()==255)),
        "unchecking transparency forces opaque rather than restoring a partial value"
    );
    drop(restored);
    set_background_transparency(&ctx, &mut app, 1.0, false);
    let opaque = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(
        frames(&opaque)
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![frame_id; 2]
    );
    assert!(opaque.textures_delta.set.is_empty());
    drop(opaque);
    toggle_option(&ctx, &mut app, "No frame");
    toggle_background(&ctx, &mut app);
    set_background_transparency(&ctx, &mut app, 0.5, true);
    toggle_background(&ctx, &mut app);
    set_background_transparency(&ctx, &mut app, 0.5, false);
    let unframed = wide_step(&ctx, &mut app, 384.0);
    assert!(frames(&unframed).is_empty());
    let cards: Vec<_> = unframed
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill.a() == 255 => Some(r.rect),
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 2);
    assert!((cards[1].left() - cards[0].right() - 5.0).abs() < 0.001);
    assert!(unframed.textures_delta.set.is_empty());
}

#[test]
fn transparent_mode_uses_unframed_layout_and_preserves_opaque_frame_choice() {
    for vertical in [false, true] {
        let (ctx, mut app, address) = setup();
        session_request(address, "a", 1, "emote", 42);
        session_request(address, "b", 1, "emote", 84);
        step(&ctx, &mut app, vec![]);
        if vertical {
            select_menu_option(&ctx, &mut app, "Vertical");
        }
        let dimensions = |count: usize, size| {
            if vertical {
                Vec2::new(size, count as f32 * size)
            } else {
                Vec2::new(count as f32 * size, size)
            }
        };
        let initial = surface_step(&ctx, &mut app, dimensions(2, 192.0));
        let frame_id = frames(&initial)[0].0;
        assert_eq!(frames(&initial).len(), 2);
        drop(initial);
        toggle_background(&ctx, &mut app);
        for count in [2, 3] {
            if count == 3 {
                session_request(address, "c", 1, "emote", 126);
                let introduced = surface_step(&ctx, &mut app, dimensions(count, 192.0));
                let new_id = avatars(&introduced)[2].0;
                assert!(
                    introduced
                        .textures_delta
                        .set
                        .iter()
                        .any(|(id, _)| *id == new_id),
                    "the newly introduced avatar must upload its initial texture"
                );
                drop(introduced);
            }
            for size in [128.0, 192.0, 256.0, 384.0] {
                let output = surface_step(&ctx, &mut app, dimensions(count, size));
                let images = avatars(&output);
                assert_eq!(images.len(), count);
                assert!(frames(&output).is_empty());
                assert!(
                    output.shapes.iter().any(|s| matches!(&s.shape,
                        egui::Shape::Rect(r) if r.fill == egui::Color32::from_black_alpha(140)
                    )),
                    "toggle-on default paints 45% transparency (opacity 55, alpha 140)"
                );
                let cards = expected_unframed_cards(
                    egui::Rect::from_min_size(Pos2::ZERO, dimensions(count, size)),
                    count,
                    vertical,
                );
                for ((_, image), card) in images.iter().zip(cards) {
                    let (_, expected) = snapped_physical_image_fit(card.shrink(4.0), 1.0);
                    assert!(
                        (image.center() - expected.center()).length() < 0.001,
                        "automatic transparent layout must apply the whole-physical-pixel fit and compensation per card"
                    );
                }
                assert!(output.textures_delta.set.is_empty());
            }
        }
        session_request(address, "c", 2, "emote/close", 0);
        for extent in [16.0, 4.0] {
            let size = if vertical {
                Vec2::new(192.0, extent)
            } else {
                Vec2::new(extent, 192.0)
            };
            let output = surface_step(&ctx, &mut app, size);
            let images = avatars(&output);
            assert_eq!(images.len(), 2, "narrow resizes must not discard avatars");
            assert!(images.iter().all(|(_, image)| image.is_positive()
                && egui::Rect::from_min_size(Pos2::ZERO, size).contains_rect(*image)));
        }
        toggle_background(&ctx, &mut app);
        let restored = surface_step(&ctx, &mut app, dimensions(2, 192.0));
        assert_eq!(
            frames(&restored)
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![frame_id; 2],
            "returning to opaque restores the original frame preference and cached asset"
        );
        drop(restored);
        toggle_option(&ctx, &mut app, "No frame");
        toggle_background(&ctx, &mut app);
        toggle_background(&ctx, &mut app);
        assert!(
            frames(&surface_step(&ctx, &mut app, dimensions(2, 192.0))).is_empty(),
            "an explicit opaque No frame choice also survives transparency toggles"
        );
    }
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
        assert_eq!(frames(&single).len(), usize::from(!transparent));
        let single_frame = frames(&single).first().map(|(id, _)| *id);
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
            assert_eq!(Some(borders[0].0), single_frame);
            assert!(
                !first.textures_delta.set.contains_key(&borders[0].0),
                "adding a second avatar reuses the single avatar's frame texture"
            );
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
                    // Two cards share the row with a 5.0 separator, so each card
                    // is narrower than its tile; mirror the app's fit exactly.
                    let card = expected_unframed_cards(
                        egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(size * 2.0, size)),
                        2,
                        false,
                    )[i];
                    let fitted = card.shrink(4.0);
                    let (_, expected) = snapped_physical_image_fit(fitted, 1.0);
                    assert!(
                        (image.width() - expected.width()).abs() < 0.001,
                        "transparent geometry follows the whole-physical-pixel fit"
                    );
                } else {
                    let frame = borders[i].1;
                    assert!(tile.contains_rect(frame));
                    assert!(
                        (frame.width() / frame.height() - 1100.0 / 1092.0).abs() < 0.001,
                        "preserve frame aspect ratio"
                    );
                    assert!(
                        image.width() >= 64.0 - 0.001,
                        "keep at least one native physical pixel per source pixel at {size}"
                    );
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
        assert_eq!(
            frames(&remaining).len(),
            usize::from(!transparent),
            "removing the second avatar preserves the remaining avatar's frame policy"
        );
        assert_eq!(
            avatars(&remaining)[0].1,
            original_bounds,
            "restore single-avatar geometry exactly"
        );
    }
}

#[test]
fn stacking_selector_switches_axes_and_preserves_frames_and_transparent_gaps() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    let single_bounds = avatars(&step(&ctx, &mut app, vec![]))[0].1;
    session_request(address, "b", 1, "emote", 84);
    let horizontal = wide_step(&ctx, &mut app, 384.0);
    assert_eq!(frames(&horizontal).len(), 2);
    assert!(avatars(&horizontal)[0].1.right() < avatars(&horizontal)[1].1.left());
    drop(horizontal);
    let selection = select_menu_option(&ctx, &mut app, "Vertical");
    let frame_scale = (65.0 * 3.0 + 3.0) / 851.0;
    let framed_tile = Vec2::new(1100.0 * frame_scale, 1092.0 * frame_scale);
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(
            framed_tile.x,
            2.0 * framed_tile.y
        ))]
    );
    drop(selection);
    let render = |app: &mut EmoteApp, count: usize, size: f32| {
        let mut raw = input(vec![]);
        raw.screen_rect = Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(size, count as f32 * size),
        ));
        Output::new(&ctx, ctx.run_ui(raw, |ui| app.show(ui)))
    };
    let vertical = render(&mut app, 2, 192.0);
    assert_eq!(frames(&vertical).len(), 2);
    assert!(avatars(&vertical)[0].1.bottom() < avatars(&vertical)[1].1.top());
    assert!((avatars(&vertical)[0].1.left() - avatars(&vertical)[1].1.left()).abs() < 0.001);
    drop(vertical);
    toggle_option(&ctx, &mut app, "No frame");
    for count in [2, 3] {
        if count == 3 {
            session_request(address, "c", 1, "emote", 126);
        }
        for size in [128.0, 192.0, 256.0, 384.0] {
            let output = render(&mut app, count, size);
            let images = avatars(&output);
            assert_eq!(images.len(), count);
            assert!(frames(&output).is_empty());
            let cards: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill == egui::Color32::BLACK => Some(r.rect),
                    _ => None,
                })
                .collect();
            assert_eq!(cards.len(), count);
            for pair in cards.windows(2) {
                assert!((pair[1].top() - pair[0].bottom() - 5.0).abs() < 0.001);
                let strip = egui::Rect::from_min_max(
                    egui::pos2(0.0, pair[0].bottom() + 0.001),
                    egui::pos2(size, pair[1].top() - 0.001),
                );
                assert!(
                    output
                        .shapes
                        .iter()
                        .all(|s| !matches!(&s.shape,egui::Shape::Rect(r)
                    if r.fill.a()>0 && r.rect.intersects(strip))),
                    "vertical gaps stay transparent"
                );
                assert!(
                    output
                        .shapes
                        .iter()
                        .any(|s| matches!(&s.shape,egui::Shape::Rect(r)
                    if r.fill==egui::Color32::BLACK && r.rect.contains(pair[0].center()))),
                    "opaque avatar underlay preserved"
                );
            }
        }
    }
    session_request(address, "c", 2, "emote/close", 0);
    render(&mut app, 2, 192.0);
    toggle_background(&ctx, &mut app);
    let transparent = render(&mut app, 2, 192.0);
    assert!(
        transparent.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Rect(r) if r.fill == egui::Color32::from_black_alpha(140)
        )),
        "toggle-on default paints 45% transparency (opacity 55, alpha 140)"
    );
    let clear_images = avatars(&transparent);
    assert!(
        ((clear_images[1].1.center().y - clear_images[0].1.center().y) - 195.0).abs() < 0.001,
        "transparent cards preserve their equal 189.5-point heights plus five-point separation; whole physical pixels snap the second card's fractional top upward by half a pixel"
    );
    drop(transparent);
    toggle_option(&ctx, &mut app, "No frame");
    toggle_background(&ctx, &mut app);
    assert_eq!(frames(&render(&mut app, 2, 192.0)).len(), 2);
    let selection = select_menu_option(&ctx, &mut app, "Horizontal");
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(
            2.0 * framed_tile.x,
            framed_tile.y
        ))]
    );
    drop(selection);
    session_request(address, "b", 2, "emote/close", 0);
    assert_eq!(avatars(&step(&ctx, &mut app, vec![]))[0].1, single_bounds);
}

#[test]
fn no_frame_rounding_clips_bottom_image_corners_to_card_contour() {
    for vertical in [false, true] {
        let (ctx, mut app, address) = setup();
        for (id, shade) in [("a", 42), ("b", 84), ("c", 126)] {
            session_request(address, id, 1, "emote", shade);
        }
        step(&ctx, &mut app, vec![]);
        if vertical {
            select_menu_option(&ctx, &mut app, "Vertical");
        }
        toggle_option(&ctx, &mut app, "No frame");
        for size in [128.0, 192.0, 256.0, 384.0] {
            let dimensions = if vertical {
                Vec2::new(size, 3.0 * size)
            } else {
                Vec2::new(3.0 * size, size)
            };
            let output = surface_step(&ctx, &mut app, dimensions);
            let cards: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(card) if card.fill == egui::Color32::BLACK => Some(card.rect),
                    _ => None,
                })
                .collect();
            let meshes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh)
                        if output
                            .1
                            .get(&mesh.texture_id)
                            .is_some_and(|name| name.starts_with("emote-session:")) =>
                    {
                        Some(mesh)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(meshes.len(), 3);
            for (card, mesh) in cards.iter().zip(meshes) {
                assert!(!mesh.indices.is_empty(), "clipping must keep the image");
                for vertex in &mesh.vertices {
                    let center = egui::pos2(
                        vertex.pos.x.clamp(card.left() + 8.0, card.right() - 8.0),
                        vertex.pos.y.clamp(card.top() + 8.0, card.bottom() - 8.0),
                    );
                    assert!(
                        (vertex.pos - center).length_sq() <= 64.01,
                        "image mesh paints outside the eight-pixel rounded card: {:?}, card={card:?}, vertical={vertical}",
                        vertex.pos
                    );
                    assert!(
                        (0.0..=1.0).contains(&vertex.uv.x) && (0.0..=1.0).contains(&vertex.uv.y)
                    );
                }
            }
            assert!(
                output.textures_delta.set.is_empty(),
                "mask the draw mesh without rewriting cached PNG pixels"
            );
        }
    }
}

#[test]
fn no_frame_rounds_card_corners_without_covering_them_with_avatar_pixels() {
    for vertical in [false, true] {
        let (ctx, mut app, address) = setup();
        for (id, shade) in [("a", 42), ("b", 84), ("c", 126)] {
            session_request(address, id, 1, "emote", shade);
        }
        step(&ctx, &mut app, vec![]);
        if vertical {
            select_menu_option(&ctx, &mut app, "Vertical");
        }
        toggle_option(&ctx, &mut app, "No frame");
        for size in [128.0, 192.0, 256.0, 384.0] {
            let dimensions = if vertical {
                Vec2::new(size, 3.0 * size)
            } else {
                Vec2::new(3.0 * size, size)
            };
            let output = surface_step(&ctx, &mut app, dimensions);
            let cards: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(card) if card.fill == egui::Color32::BLACK => Some(card),
                    _ => None,
                })
                .collect();
            let images = avatars(&output);
            assert_eq!(cards.len(), 3);
            assert_eq!(images.len(), 3);
            for (card, (_, image)) in cards.iter().zip(&images) {
                assert_eq!(
                    card.corner_radius,
                    egui::CornerRadius::same(8),
                    "card rounding must match the original eight-pixel appearance"
                );
                let inset = if vertical {
                    (image.left() - card.rect.left()).min(card.rect.right() - image.right())
                } else {
                    (image.top() - card.rect.top()).min(card.rect.bottom() - image.bottom())
                };
                assert!(
                    inset + 0.001 >= 4.0,
                    "retain the original image bounds and four-point composition margins"
                );
            }
            assert!(
                output.textures_delta.set.is_empty(),
                "round only background paint, not source pixels"
            );
        }
    }
}

#[test]
fn no_frame_keeps_five_logical_pixels_between_adjacent_cards() {
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
                let cards = expected_unframed_cards(bounds, count, false);
                if !transparent {
                    let painted: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Rect(r) if r.fill == egui::Color32::BLACK => Some(r.rect),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(painted.len(), count);
                    for (actual, expected) in painted.iter().zip(&cards) {
                        assert!(
                            (actual.min - expected.min).length() < 0.001
                                && (actual.max - expected.max).length() < 0.001
                        );
                    }
                }
                for pair in cards.windows(2) {
                    let gap = pair[1].left() - pair[0].right();
                    assert!(
                        (gap - 5.0).abs() < 0.001,
                        "five clear pixels belong between card contours"
                    );
                    let strip = egui::Rect::from_min_max(
                        egui::pos2(pair[0].right() + 0.001, bounds.top()),
                        egui::pos2(pair[1].left() - 0.001, bounds.bottom()),
                    );
                    assert!(
                        output.shapes.iter().all(|shape| !matches!(
                            &shape.shape, egui::Shape::Rect(paint)
                            if paint.fill.a() > 0 && paint.rect.intersects(strip)
                        )),
                        "the five-pixel separator must have no opaque background paint"
                    );
                    assert!(images.iter().all(|(_, image)| !image.intersects(strip)));
                    assert!((pair[0].top() - pair[1].top()).abs() < 0.001);
                    assert!((pair[0].size() - pair[1].size()).length() < 0.001);
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
    step(&ctx, &mut app, vec![]);
    toggle_option(&ctx, &mut app, "No frame");
    let single = step(&ctx, &mut app, vec![]);
    let original_single = avatars(&single)[0].1;
    drop(single);
    toggle_option(&ctx, &mut app, "No frame");
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
        (avatars(&hidden)[0].1.width() - 128.0).abs() < 0.001,
        "reclaim the unframed image fit at whole physical pixels"
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
fn size_slider_steps_render_the_chosen_image_and_derive_the_canvas_from_it() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    step(&ctx, &mut app, vec![]);
    for step_index in 1..=5_u8 {
        let step_scale = f32::from(step_index);
        let side = 64.0 * step_scale;
        // The composition is the 64×step image plus its one source row above;
        // the canvas derives from it, never from a square that loses a level.
        let composition = Vec2::new(side, 65.0 * step_scale);
        let frame_scale = (composition.y + 3.0) / 851.0;
        let framed_tile = Vec2::new(1100.0 * frame_scale, 1092.0 * frame_scale);
        let commands = set_size_step(&ctx, &mut app, step_index);
        assert_eq!(
            commands,
            vec![ViewportCommand::InnerSize(framed_tile)],
            "the framed canvas adds only the frame margins around the whole \
             x{step_index} composition"
        );
        let framed = surface_step(&ctx, &mut app, framed_tile);
        let borders = frames(&framed);
        assert_eq!(borders.len(), 1);
        // egui rounds the rendered viewport surface to its 1/32-point UI grid,
        // so the artwork fills the rendered surface, not the requested size.
        assert!(
            (borders[0].1.width() - framed_tile.x).abs() < 1.0 / 32.0 + 0.001,
            "the frame artwork must fill the framed tile: width={:.6} vs {:.6}",
            borders[0].1.width(),
            framed_tile.x
        );
        let framed_image = avatars(&framed)[0].1;
        assert!(borders[0].1.contains_rect(framed_image));
        // The chosen step is the actual rendered image size: at 100% DPI the
        // avatar draws exactly side×side, framed or not.
        assert_eq!(
            framed_image.width(),
            side,
            "x{step_index} framed image must render at the whole chosen multiplier"
        );
        assert_eq!(
            framed_image.height(),
            side,
            "x{step_index} framed image must render at the whole chosen multiplier"
        );
        assert!(framed.textures_delta.set.is_empty());
        drop(framed);

        let toggled = toggle_no_frame_collecting_commands(&ctx, &mut app);
        assert_eq!(
            toggled,
            vec![ViewportCommand::InnerSize(composition)],
            "disabling the frame reclaims its margins down to the whole composition"
        );
        let unframed = surface_step(&ctx, &mut app, composition);
        assert!(frames(&unframed).is_empty());
        let unframed_image = avatars(&unframed)[0].1;
        assert_eq!(
            unframed_image.width(),
            side,
            "x{step_index} unframed image must render at the whole chosen multiplier"
        );
        assert_eq!(
            unframed_image.height(),
            side,
            "x{step_index} unframed image must render at the whole chosen multiplier"
        );
        assert!(unframed.textures_delta.set.is_empty());
        assert!(
            unframed.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty(),
            "resize commands stay deduplicated"
        );
        drop(unframed);
        select_menu_option(&ctx, &mut app, "No frame");
    }
}

#[test]
fn size_slider_canvas_covers_multiple_avatars_orientations_and_scale_factors() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    step(&ctx, &mut app, vec![]);
    let step_scale = 4.0_f32;
    let composition = Vec2::new(64.0 * step_scale, 65.0 * step_scale);
    let frame_scale = (composition.y + 3.0) / 851.0;
    let framed_tile = Vec2::new(1100.0 * frame_scale, 1092.0 * frame_scale);
    let card = composition + Vec2::splat(8.0);

    // Framed pair, horizontal: every tile derives from the composition.
    let commands = set_size_step(&ctx, &mut app, 4);
    assert_eq!(
        commands,
        vec![ViewportCommand::InnerSize(Vec2::new(
            2.0 * framed_tile.x,
            framed_tile.y
        ))],
        "framed multi-avatar rows stack whole framed tiles"
    );
    let horizontal = surface_step(
        &ctx,
        &mut app,
        Vec2::new(2.0 * framed_tile.x, framed_tile.y),
    );
    let images = avatars(&horizontal);
    assert_eq!(images.len(), 2);
    assert_eq!(frames(&horizontal).len(), 2);
    assert!(images[0].1.right() < images[1].1.left());
    for (index, (_, image)) in images.iter().enumerate() {
        assert_eq!(
            image.width(),
            256.0,
            "framed horizontal avatar {index} must render at x4"
        );
        assert_eq!(image.height(), 256.0, "framed horizontal avatar {index}");
    }
    drop(horizontal);

    let selection = select_menu_option(&ctx, &mut app, "Vertical");
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(
            framed_tile.x,
            2.0 * framed_tile.y
        ))],
        "vertical stacking stacks the same framed tiles"
    );
    let vertical = surface_step(
        &ctx,
        &mut app,
        Vec2::new(framed_tile.x, 2.0 * framed_tile.y),
    );
    let images = avatars(&vertical);
    assert_eq!(images.len(), 2);
    assert!(images[0].1.bottom() < images[1].1.top());
    for (index, (_, image)) in images.iter().enumerate() {
        assert_eq!(image.width(), 256.0, "framed vertical avatar {index}");
        assert_eq!(image.height(), 256.0, "framed vertical avatar {index}");
    }
    drop(vertical);

    // Unframed multi cards budget their real four-point insets and five-point
    // gaps instead of deducting them from the image.
    let toggled = toggle_no_frame_collecting_commands(&ctx, &mut app);
    assert_eq!(
        toggled,
        vec![ViewportCommand::InnerSize(Vec2::new(
            card.x,
            2.0 * card.y + 5.0
        ))],
        "the unframed vertical canvas budgets insets and gaps around the composition"
    );
    let unframed_vertical = surface_step(&ctx, &mut app, Vec2::new(card.x, 2.0 * card.y + 5.0));
    let images = avatars(&unframed_vertical);
    assert_eq!(images.len(), 2);
    assert!(frames(&unframed_vertical).is_empty());
    for (index, (_, image)) in images.iter().enumerate() {
        assert_eq!(image.width(), 256.0, "unframed vertical avatar {index}");
        assert_eq!(image.height(), 256.0, "unframed vertical avatar {index}");
    }
    drop(unframed_vertical);
    let selection = select_menu_option(&ctx, &mut app, "Horizontal");
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(
            2.0 * card.x + 5.0,
            card.y
        ))],
        "the unframed horizontal canvas budgets insets and gaps around the composition"
    );
    let unframed_horizontal = surface_step(&ctx, &mut app, Vec2::new(2.0 * card.x + 5.0, card.y));
    let images = avatars(&unframed_horizontal);
    assert_eq!(images.len(), 2);
    assert!(images[0].1.right() < images[1].1.left());
    for (index, (_, image)) in images.iter().enumerate() {
        assert_eq!(image.width(), 256.0, "unframed horizontal avatar {index}");
        assert_eq!(image.height(), 256.0, "unframed horizontal avatar {index}");
    }
    drop(unframed_horizontal);

    // Back to one framed avatar: at fractional DPI the whole physical pixel
    // policy must keep at least the chosen step, never lose a level to
    // clipping or rounding.
    session_request(address, "b", 2, "emote/close", 0);
    step(&ctx, &mut app, vec![]);
    select_menu_option(&ctx, &mut app, "No frame");
    let mut unframed = false;
    for framed_case in [false, true] {
        ctx.set_pixels_per_point(1.0);
        step(&ctx, &mut app, vec![]);
        if unframed == framed_case {
            toggle_option(&ctx, &mut app, "No frame");
        }
        unframed = !framed_case;
        for ppp in [1.25_f32, 1.5, 2.0] {
            ctx.set_pixels_per_point(ppp);
            // Absorb egui's zoom-transition pass, which rescales the previous
            // surface instead of honoring the new raw screen rect once.
            step(&ctx, &mut app, vec![]);
            let surface = if framed_case {
                framed_tile
            } else {
                composition
            };
            let output = surface_step(&ctx, &mut app, surface);
            let image = avatars(&output)[0].1;
            let context = format!("framed={framed_case} ppp={ppp}");
            assert_whole_physical_pixels(image, ppp, &context);
            let expected_physical = 64.0 * (step_scale * ppp).floor();
            assert!(
                (image.width() * ppp - expected_physical).abs() < 1e-3
                    && (image.height() * ppp - expected_physical).abs() < 1e-3,
                "{context}: the chosen step must not lose a whole physical \
                 level: rendered {}×{} physical pixels, expected {expected_physical}",
                image.width() * ppp,
                image.height() * ppp
            );
        }
    }
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
            "Avatar size",
            "x3",
            "Transparent background",
            "Background transparency",
            "No frame",
            "Vertical / Horizontal",
            "Horizontal",
            "Vertical",
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
    let frame_scale = (65.0 * 3.0 + 3.0) / 851.0;
    let framed_tile = Vec2::new(1100.0 * frame_scale, 1092.0 * frame_scale);
    let tiles = avatars(&first);
    assert_eq!(tiles.len(), 2);
    assert!(tiles[0].1.right() < tiles[1].1.left());
    let artwork_scale = 192.0 / 1100.0;
    for (i, (_, rect)) in tiles.iter().enumerate() {
        let opening = egui::Rect::from_min_size(
            Pos2::new(i as f32 * 192.0, (192.0 - 1092.0 * artwork_scale) / 2.0)
                + Vec2::new(125.0, 121.0) * artwork_scale,
            Vec2::splat(851.0 * artwork_scale),
        );
        // The avatar keeps its whole-physical-pixel fit inside the frame opening.
        let (_, expected) = snapped_physical_image_fit(opening, 1.0);
        assert!(
            (rect.min - expected.min).length() < 0.001
                && (rect.max - expected.max).length() < 0.001,
            "framed avatar must use the largest whole physical scale in the opening: {rect:?} vs {expected:?}"
        );
    }
    assert!(
        first.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::InnerSize(Vec2::new(
                2.0 * framed_tile.x,
                framed_tile.y
            )))
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
            .contains(&ViewportCommand::InnerSize(framed_tile))
    );
    drop(closed);

    session_request(address, "b", 3, "emote/close", 0);
    let retained = step(&ctx, &mut app, vec![]);
    assert!(avatars(&retained).is_empty());
    assert!(frames(&retained).is_empty());
    let clocks = clock_meshes(&retained);
    assert_eq!(
        clocks.len(),
        1,
        "the final explicit close paints the internal pixel clock"
    );
    assert!(retained.textures_delta.free.is_empty());
    assert_eq!(
        clock_uploads(&retained).len(),
        1,
        "the shared internal clock texture uploads once"
    );
    // The clock fallback has no frame, so it reclaims the unframed
    // composition — the image plus its one source separation row — even though
    // the framed tile was applied before the final close.
    assert_eq!(
        retained.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(192.0, 195.0))]
    );
    drop(retained);

    session_request(address, "c", 1, "emote", 168);
    let replaced = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&replaced).len(), 1);
    assert_ne!(avatars(&replaced)[0].0, tiles[1].0);
    assert!(replaced.textures_delta.free.contains(&tiles[1].0));
    assert_eq!(
        replaced.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(framed_tile)]
    );
}

fn clock_meshes(output: &Output) -> Vec<(egui::TextureId, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Mesh(mesh) = &shape.shape
                && output
                    .1
                    .get(&mesh.texture_id)
                    .is_some_and(|name| name == "emote-idle-clock")
            {
                Some((mesh.texture_id, mesh.calc_bounds()))
            } else {
                None
            }
        })
        .collect()
}

fn clock_uploads(output: &Output) -> Vec<&egui::epaint::ImageDelta> {
    output
        .textures_delta
        .set
        .values()
        .flatten()
        .filter(|delta| delta.image.size() == [64, 64])
        .collect()
}

#[test]
fn sole_avatar_close_immediately_shows_internal_pixel_clock_and_new_avatar_replaces_it() {
    let (ctx, mut app, address) = setup();
    let waiting = step(&ctx, &mut app, vec![]);
    assert!(clock_meshes(&waiting).is_empty());
    assert!(waiting.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.text().starts_with("Waiting for emote"))));
    drop(waiting);
    session_request(address, "a", 1, "emote", 42);
    let active = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&active).len(), 1);
    assert!(clock_meshes(&active).is_empty());
    drop(active);
    session_request(address, "a", 2, "emote/close", 0);
    // The first clock appearance uploads the shared internal clock texture:
    // 64x64, nearest-neighbor, transparent gaps with opaque ink pixels.
    let first = step(&ctx, &mut app, vec![]);
    let uploads = clock_uploads(&first);
    assert_eq!(uploads.len(), 1, "the internal clock texture uploads once");
    assert_eq!(uploads[0].options, egui::TextureOptions::NEAREST);
    let egui::ImageData::Color(image) = &uploads[0].image;
    let opaque = image.pixels.iter().filter(|p| p.a() == 255).count();
    let clear = image.pixels.iter().filter(|p| p.a() == 0).count();
    assert!(
        opaque > 500 && clear > 1000,
        "clock ink must stay opaque with transparent gaps"
    );
    assert_eq!(clock_meshes(&first).len(), 1);
    drop(first);
    for size in [128.0, 192.0, 256.0, 384.0] {
        let clock = surface_step(&ctx, &mut app, Vec2::splat(size));
        let clocks = clock_meshes(&clock);
        assert_eq!(
            clocks.len(),
            1,
            "the global fallback paints one internal pixel clock"
        );
        let surface = egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(size));
        let (_, expected) = snapped_physical_image_fit(surface, ctx.pixels_per_point());
        assert!(
            (clocks[0].1.min - expected.min).length() < 1e-3
                && (clocks[0].1.max - expected.max).length() < 1e-3,
            "the global fallback clock must use the whole-physical-pixel fit at {size}"
        );
        assert!(avatars(&clock).is_empty());
        assert!(frames(&clock).is_empty());
        assert!(!clock.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.text().starts_with("Waiting for emote"))));
        // A 500ms animation-step boundary may fall between public renders and
        // reupload the shared clock texture; nothing else may ever upload here.
        assert!(clock.textures_delta.set.keys().all(|id| {
            clock
                .1
                .get(id)
                .is_some_and(|name| name == "emote-idle-clock")
        }));
        assert!(
            clock.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                <= std::time::Duration::from_millis(500)
        );
    }
    session_request(address, "b", 1, "emote", 84);
    let restored = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&restored).len(), 1);
    assert!(clock_meshes(&restored).is_empty());
}

#[test]
fn per_session_clock_tiles_run_alongside_normal_avatars_without_hiding_them() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    // Session a declares the clock state; session b sends ordinary pixels.
    session_request_state(address, "a", 1, "emote", 42, Some("clock"));
    session_request(address, "b", 1, "emote", 84);
    let mixed = wide_step(&ctx, &mut app, 384.0);
    let tiles = clock_meshes(&mixed);
    assert_eq!(
        tiles.len(),
        1,
        "each idle session renders its own internal clock tile"
    );
    assert_eq!(
        avatars(&mixed).len(),
        1,
        "the active normal session keeps its own tile"
    );
    assert!(
        tiles[0].1.right() < avatars(&mixed)[0].1.left(),
        "stable tile order: the clock session keeps its first-seen tile"
    );
    assert!(tiles[0].1.left() < avatars(&mixed)[0].1.left());
    assert_eq!(
        frames(&mixed).len(),
        2,
        "per-session clock tiles keep the normal tile frame"
    );
    assert_eq!(
        clock_uploads(&mixed).len(),
        2,
        "only the normal session's pixels and the shared clock texture upload"
    );
    drop(mixed);
    // An ordinary image state exits the clock for that session only.
    session_request_state(address, "a", 2, "emote", 126, None);
    let restored = wide_step(&ctx, &mut app, 384.0);
    assert!(clock_meshes(&restored).is_empty());
    assert_eq!(avatars(&restored).len(), 2);
    assert_eq!(
        restored
            .textures_delta
            .set
            .values()
            .flatten()
            .filter(|d| d.image.size() == [64, 64])
            .count(),
        1,
        "returning to normal uploads only that session's pixels"
    );
    drop(restored);
}

fn clock_at(ctx: &egui::Context, app: &mut EmoteApp, millis: i64) -> Output {
    let now = chrono::DateTime::from_timestamp_millis(millis)
        .unwrap()
        .fixed_offset();
    let mut raw = input(vec![]);
    raw.screen_rect = Some(egui::Rect::from_min_size(
        Pos2::ZERO,
        Vec2::new(576.0, 192.0),
    ));
    Output::new(ctx, ctx.run_ui(raw, |ui| app.show_at(ui, now)))
}

// Fixed time keeps geometry checks independent of animation uploads.
fn compact_surface(ctx: &egui::Context, app: &mut EmoteApp, size: Vec2) -> Output {
    let mut raw = input(vec![]);
    raw.screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, size));
    let now = chrono::DateTime::from_timestamp_millis(5000)
        .unwrap()
        .fixed_offset();
    Output::new(ctx, ctx.run_ui(raw, |ui| app.show_at(ui, now)))
}

fn compact_backgrounds(output: &Output) -> Vec<egui::epaint::RectShape> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn assert_compact_clock(output: &Output, opacity: u8) {
    let backgrounds = compact_backgrounds(output);
    for (id, image) in clock_meshes(output) {
        let mesh = avatar_mesh(output, id);
        assert!(
            mesh.vertices.len() == 4,
            "clock artwork must remain a complete image quad"
        );
        for uv in [
            Pos2::ZERO,
            Pos2::new(1.0, 0.0),
            Pos2::new(0.0, 1.0),
            Pos2::new(1.0, 1.0),
        ] {
            assert!(
                mesh.vertices.iter().any(|v| v.uv == uv),
                "all source corners must remain visible without cropping"
            );
        }
        if opacity > 0 {
            let bg = backgrounds
                .iter()
                .find(|r| r.rect.contains_rect(image))
                .expect("clock background must contain the complete image");
            if backgrounds.len() > 1 || opacity < 100 {
                assert_eq!(bg.corner_radius, egui::CornerRadius::same(8));
            }
            assert_eq!(bg.fill.a(), ((u16::from(opacity) * 255 + 50) / 100) as u8);
        } else {
            assert!(backgrounds.is_empty());
        }
    }
}

#[test]
fn compact_clock_mixed_preserves_neighbor_and_inset_fit() {
    for vertical in [false, true] {
        for scale in [1, 3, 5] {
            for opacity in [100, 50, 0] {
                let (ctx, mut app, address) = setup();
                session_request(address, "a", 1, "emote", 42);
                session_request(address, "b", 1, "emote", 84);
                step(&ctx, &mut app, vec![]);
                toggle_option(&ctx, &mut app, "No frame");
                if vertical {
                    select_menu_option(&ctx, &mut app, "Vertical");
                }
                set_size_step(&ctx, &mut app, scale);
                if opacity < 100 {
                    set_background_opacity(&ctx, &mut app, f32::from(opacity) / 100.0);
                }
                let card = Vec2::new(64.0, 65.0) * f32::from(scale) + Vec2::splat(8.0);
                let size = if vertical {
                    Vec2::new(card.x, 2.0 * card.y + 5.0)
                } else {
                    Vec2::new(2.0 * card.x + 5.0, card.y)
                };
                for ppp in [1.0, 1.25, 1.61] {
                    ctx.set_pixels_per_point(ppp);
                    step(&ctx, &mut app, vec![]);
                    session_request(address, "a", (ppp * 100.0) as u64 * 2, "emote", 42);
                    let before = compact_surface(&ctx, &mut app, size);
                    let images = avatars(&before);
                    let neighbor = avatar_mesh(&before, images[1].0);
                    let backgrounds = compact_backgrounds(&before);
                    session_request_state(
                        address,
                        "a",
                        (ppp * 100.0) as u64 * 2 + 1,
                        "emote",
                        42,
                        Some("clock"),
                    );
                    let after = compact_surface(&ctx, &mut app, size);
                    assert_eq!(avatars(&after), vec![images[1]]);
                    assert_eq!(avatar_mesh(&after, images[1].0), neighbor);
                    let clock = clock_meshes(&after)[0].1;
                    // Rounded normal meshes reconstruct their bounds through
                    // polygon clipping; allow only floating-point roundoff.
                    assert!((clock.min - images[0].1.min).length() < 0.001);
                    assert!((clock.max - images[0].1.max).length() < 0.001);
                    assert_compact_clock(&after, opacity);
                    if opacity > 0 {
                        assert_eq!(compact_backgrounds(&after), backgrounds);
                    }
                    assert!(
                        after.viewport_output[&egui::ViewportId::ROOT]
                            .commands
                            .is_empty()
                    );
                    assert!(!after.textures_delta.set.contains_key(&images[1].0));
                    drop(after);
                    let stable = compact_surface(&ctx, &mut app, size);
                    assert!(stable.textures_delta.set.is_empty());
                    session_request_state(
                        address,
                        "b",
                        (ppp * 100.0) as u64 * 2,
                        "emote",
                        84,
                        Some("clock"),
                    );
                    let all = compact_surface(&ctx, &mut app, size);
                    assert_eq!(clock_meshes(&all).len(), 2);
                    assert_compact_clock(&all, opacity);
                    assert_eq!(compact_backgrounds(&all), backgrounds);
                    assert!(
                        all.viewport_output[&egui::ViewportId::ROOT]
                            .commands
                            .is_empty()
                    );
                    assert_eq!(clock_meshes(&all)[0], clock_meshes(&stable)[0]);
                    assert!((clock_meshes(&all)[1].1.min - images[1].1.min).length() < 0.001);
                    assert!((clock_meshes(&all)[1].1.max - images[1].1.max).length() < 0.001);
                    session_request(address, "b", (ppp * 100.0) as u64 * 2 + 1, "emote", 84);
                }
            }
        }
    }
}

#[test]
fn compact_clock_all_clock_native_size_and_normal_restore() {
    for vertical in [false, true] {
        for scale in [1, 3, 5] {
            let (ctx, mut app, address) = setup();
            session_request_state(address, "a", 1, "emote", 42, Some("clock"));
            session_request_state(address, "b", 1, "emote", 84, Some("clock"));
            step(&ctx, &mut app, vec![]);
            // Explicit opaque frame choice remains authoritative.
            assert_eq!(frames(&step(&ctx, &mut app, vec![])).len(), 2);
            toggle_option(&ctx, &mut app, "No frame");
            if vertical {
                select_menu_option(&ctx, &mut app, "Vertical");
            }
            set_size_step(&ctx, &mut app, scale);
            let tile = Vec2::new(64.0, 65.0) * f32::from(scale);
            let extent = |card: Vec2| {
                if vertical {
                    Vec2::new(card.x, 2.0 * card.y + 5.0)
                } else {
                    Vec2::new(2.0 * card.x + 5.0, card.y)
                }
            };
            let size = extent(tile + Vec2::splat(8.0));
            for ppp in [1.0, 1.25, 1.61] {
                ctx.set_pixels_per_point(ppp);
                step(&ctx, &mut app, vec![]);
                session_request(address, "a", (ppp * 100.0) as u64 * 2, "emote", 42);
                let mixed = compact_surface(&ctx, &mut app, extent(tile + Vec2::splat(8.0)));
                assert_eq!(
                    mixed.viewport_output[&egui::ViewportId::ROOT].commands,
                    vec![],
                    "mixed state must retain the all-clock canvas"
                );
                session_request_state(
                    address,
                    "a",
                    (ppp * 100.0) as u64 * 2 + 1,
                    "emote",
                    42,
                    Some("clock"),
                );
                let requested = compact_surface(&ctx, &mut app, extent(tile + Vec2::splat(8.0)));
                assert_eq!(
                    requested.viewport_output[&egui::ViewportId::ROOT].commands,
                    vec![],
                    "all-clock state must retain the mixed canvas"
                );
                let applied = compact_surface(&ctx, &mut app, size);
                assert_compact_clock(&applied, 100);
                assert_eq!(compact_backgrounds(&applied).len(), 2);
                for (_, image) in clock_meshes(&applied) {
                    assert_whole_physical_pixels(image, ppp, "compact packed clock");
                    assert!(
                        (image.width() * ppp - 64.0 * (f32::from(scale) * ppp).floor()).abs()
                            < 0.001
                    );
                }
                assert!(
                    applied.viewport_output[&egui::ViewportId::ROOT]
                        .commands
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn compact_clock_single_and_global_keep_date_fallback_and_controls() {
    for opacity in [100, 50, 0] {
        let (ctx, mut app, address) = setup();
        session_request_state(address, "a", 1, "emote", 42, Some("clock"));
        step(&ctx, &mut app, vec![]);
        assert_eq!(frames(&step(&ctx, &mut app, vec![])).len(), 1);
        toggle_option(&ctx, &mut app, "No frame");
        if opacity < 100 {
            set_background_opacity(&ctx, &mut app, f32::from(opacity) / 100.0);
        }
        let size = Vec2::new(260.0, 300.0);
        let single = compact_surface(&ctx, &mut app, size);
        assert_compact_clock(&single, opacity);
        if opacity > 0 {
            assert_eq!(
                compact_backgrounds(&single)[0].rect,
                egui::Rect::from_min_size(Pos2::ZERO, size)
            );
        }
        session_request(address, "a", 2, "emote/close", 0);
        let global = compact_surface(&ctx, &mut app, size);
        assert_compact_clock(&global, opacity);
        if opacity > 0 {
            assert_eq!(
                compact_backgrounds(&global)[0].rect,
                egui::Rect::from_min_size(Pos2::ZERO, size)
            );
        }
        assert_clock_pixels(&global, clock_meshes(&global)[0].0, 5000, None);
        assert!(frames(&global).is_empty());
        assert_eq!(compact_backgrounds(&global).len(), usize::from(opacity > 0));
        drop(global);
        // Full root hit target still works outside the compact background.
        let p = Pos2::new(1.0, 1.0);
        step(
            &ctx,
            &mut app,
            vec![
                Event::PointerMoved(p),
                button(p, PointerButton::Primary, true),
            ],
        );
        let drag = step(
            &ctx,
            &mut app,
            vec![Event::PointerMoved(p + Vec2::splat(30.0))],
        );
        assert!(
            drag.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&ViewportCommand::StartDrag)
        );
        step(
            &ctx,
            &mut app,
            vec![button(p, PointerButton::Primary, false)],
        );
        select_menu_option(&ctx, &mut app, "No frame");
        assert!(
            frames(&compact_surface(&ctx, &mut app, size)).is_empty(),
            "global remains unframed"
        );
    }
}

fn assert_uploaded_pixels(output: &Output, id: egui::TextureId, rgba: &[u8]) {
    let delta = output.textures_delta.set[&id].last().unwrap();
    assert_eq!(delta.options, egui::TextureOptions::NEAREST);
    assert_eq!(delta.pos, None);
    assert_eq!(delta.image.size(), [64, 64]);
    let egui::ImageData::Color(image) = &delta.image;
    let expected = egui::ColorImage::from_rgba_unmultiplied([64, 64], rgba);
    assert_eq!(
        image.pixels, expected.pixels,
        "uploaded RGBA must match the selected renderer"
    );
}

fn assert_clock_pixels(output: &Output, id: egui::TextureId, millis: i64, session: Option<&str>) {
    use gentle_emote_desktop::idle_clock_image::{render, render_for_session};
    let now = chrono::DateTime::from_timestamp_millis(millis)
        .unwrap()
        .fixed_offset();
    let expected = session.map_or_else(|| render(now), |session| render_for_session(now, session));
    assert_uploaded_pixels(output, id, &expected.rgba);
}

#[test]
fn session_clock_headers_are_independent_step_cached_and_preserve_normal_neighbor() {
    let (ctx, mut app, address) = setup();
    let long = "LongSession-0123456789";
    session_request_named(
        address,
        "a",
        1,
        "emote",
        42,
        Some("clock"),
        Some("FolderAlpha"),
    );
    session_request_named(address, "b", 1, "emote", 84, Some("clock"), Some(long));
    session_request(address, "normal", 1, "emote", 126);
    let initial = clock_at(&ctx, &mut app, 0);
    let clocks = clock_meshes(&initial);
    let neighbor = avatars(&initial);
    let borders = frames(&initial);
    assert_eq!(clocks.len(), 2);
    assert_eq!(neighbor.len(), 1);
    assert_ne!(clocks[0].0, clocks[1].0, "each ID needs independent pixels");
    for (index, session) in ["FolderAlpha", long].into_iter().enumerate() {
        assert_clock_pixels(&initial, clocks[index].0, 0, Some(session));
    }
    assert_uploaded_pixels(&initial, neighbor[0].0, &[126, 7, 9, 255].repeat(64 * 64));
    drop(initial);
    for millis in [499, 5000, 5499, 10000] {
        let output = clock_at(&ctx, &mut app, millis);
        assert_eq!(clock_meshes(&output), clocks);
        assert_eq!(avatars(&output), neighbor);
        assert_eq!(frames(&output), borders);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty()
        );
        if millis == 499 || millis == 5499 {
            assert!(
                output.textures_delta.set.is_empty(),
                "same 500ms step must not upload"
            );
        } else {
            assert_eq!(clock_uploads(&output).len(), 2);
            for (index, session) in ["FolderAlpha", long].into_iter().enumerate() {
                assert_clock_pixels(&output, clocks[index].0, millis, Some(session));
            }
            assert!(!output.textures_delta.set.contains_key(&neighbor[0].0));
        }
    }
}

#[test]
fn session_clock_cache_releases_on_state_return_and_disappearance() {
    let (ctx, mut app, address) = setup();
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    let initial = clock_at(&ctx, &mut app, 5000);
    let original = avatars(&initial);
    drop(initial);
    session_request_state(address, "a", 2, "emote", 99, Some("clock"));
    let clock = clock_at(&ctx, &mut app, 5000);
    let old_clock = clock_meshes(&clock)[0].0;
    assert_eq!(
        clock_uploads(&clock).len(),
        1,
        "do not upload received clock PNG"
    );
    assert_clock_pixels(&clock, old_clock, 5000, None);
    drop(clock);
    session_request(address, "a", 3, "emote", 126);
    let normal = clock_at(&ctx, &mut app, 5000);
    assert_eq!(
        avatars(&normal),
        original,
        "reuse the ordinary avatar handles and geometry"
    );
    assert!(clock_meshes(&normal).is_empty());
    assert!(normal.textures_delta.free.contains(&old_clock));
    assert_eq!(clock_uploads(&normal).len(), 1);
    assert_uploaded_pixels(&normal, original[0].0, &[126, 7, 9, 255].repeat(64 * 64));
    drop(normal);
    session_request_state(address, "a", 4, "emote", 99, Some("clock"));
    let restarted = clock_at(&ctx, &mut app, 5000);
    let new_clock = clock_meshes(&restarted)[0].0;
    assert_ne!(new_clock, old_clock);
    assert_clock_pixels(&restarted, new_clock, 5000, None);
    drop(restarted);
    session_request(address, "a", 5, "emote/close", 0);
    let gone = clock_at(&ctx, &mut app, 5000);
    assert!(clock_meshes(&gone).is_empty());
    assert!(gone.textures_delta.free.contains(&new_clock));
    assert_eq!(avatars(&gone)[0].0, original[1].0);
    assert!(gone.textures_delta.set.is_empty());
}

#[test]
fn session_clock_final_close_uses_date_only_and_releases_retained_id_cache() {
    let (ctx, mut app, address) = setup();
    session_request_named(
        address,
        "LastID",
        1,
        "emote",
        42,
        Some("clock"),
        Some("LastTitle"),
    );
    let session = clock_at(&ctx, &mut app, 5000);
    let old = clock_meshes(&session)[0].0;
    assert_clock_pixels(&session, old, 5000, Some("LastTitle"));
    drop(session);
    session_request(address, "LastID", 2, "emote/close", 0);
    let global = clock_at(&ctx, &mut app, 5000);
    let fallback = clock_meshes(&global)[0].0;
    assert_ne!(fallback, old);
    assert!(global.textures_delta.free.contains(&old));
    assert_clock_pixels(&global, fallback, 5000, None);
    assert!(avatars(&global).is_empty());
    assert!(frames(&global).is_empty());
    drop(global);
    session_request(address, "new", 1, "emote", 84);
    let replaced = clock_at(&ctx, &mut app, 5000);
    assert!(replaced.textures_delta.free.contains(&fallback));
    assert!(clock_meshes(&replaced).is_empty());
}

#[test]
fn session_clock_legacy_without_id_keeps_date_header() {
    let (ctx, mut app, address) = setup();
    session_request_state(address, "", 1, "emote", 42, Some("clock"));
    let legacy = clock_at(&ctx, &mut app, 5000);
    assert_eq!(clock_meshes(&legacy).len(), 1);
    assert_clock_pixels(&legacy, clock_meshes(&legacy)[0].0, 5000, None);
    assert!(avatars(&legacy).is_empty());
    assert_eq!(clock_uploads(&legacy).len(), 1);
    let id = clock_meshes(&legacy)[0].0;
    drop(legacy);
    session_request_named(
        address,
        "",
        2,
        "emote",
        42,
        Some("clock"),
        Some("LegacyFolder"),
    );
    let named = clock_at(&ctx, &mut app, 5100);
    assert_eq!(clock_meshes(&named)[0].0, id);
    assert_clock_pixels(&named, id, 5100, Some("LegacyFolder"));
}

#[test]
fn session_clock_names_invalidate_within_step_and_never_replace_identity() {
    let (ctx, mut app, address) = setup();
    for id in ["a", "b"] {
        session_request_named(
            address,
            id,
            1,
            "emote",
            42,
            Some("clock"),
            Some("SameTitle"),
        );
    }
    session_request(address, "normal", 1, "emote", 126);
    let initial = clock_at(&ctx, &mut app, 5000);
    let clocks = clock_meshes(&initial);
    let neighbor = avatars(&initial);
    let borders = frames(&initial);
    assert_ne!(clocks[0].0, clocks[1].0);
    for (id, _) in &clocks {
        assert_clock_pixels(&initial, *id, 5000, Some("SameTitle"));
    }
    drop(initial);
    // Same PNG, same stream, same 500ms bucket: only metadata changes.
    for (sequence, millis, encoded, decoded) in [
        (
            2,
            5100,
            Some("NewTitle-0123456789"),
            Some("NewTitle-0123456789"),
        ),
        (3, 5200, Some("Mi%20t%C3%ADtulo"), Some("Mi título")),
        (4, 5300, Some("FolderAlpha"), Some("FolderAlpha")),
        (5, 5400, None, None),
    ] {
        session_request_named(address, "a", sequence, "emote", 42, Some("clock"), encoded);
        let renamed = clock_at(&ctx, &mut app, millis);
        assert_eq!(clock_meshes(&renamed), clocks);
        assert_eq!(avatars(&renamed), neighbor);
        assert_eq!(frames(&renamed), borders);
        assert_eq!(clock_uploads(&renamed).len(), 1);
        assert_clock_pixels(&renamed, clocks[0].0, millis, decoded);
        assert!(!renamed.textures_delta.set.contains_key(&clocks[1].0));
        assert!(!renamed.textures_delta.set.contains_key(&neighbor[0].0));
        assert!(renamed.textures_delta.free.is_empty());
        assert!(
            renamed.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty()
        );
    }
    let stable = clock_at(&ctx, &mut app, 5499);
    assert!(stable.textures_delta.set.is_empty());
}

#[test]
fn size_slider_applies_to_each_avatar_and_count_changes_keep_chosen_size() {
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
    let point = menu::slider_position("Avatar size", 0.75);
    menu::events(vec![
        Event::PointerMoved(point),
        button(point, PointerButton::Primary, true),
    ]);
    let pressed = run(&mut app, vec![]);
    menu::events(vec![button(point, PointerButton::Primary, false)]);
    let released = run(&mut app, vec![]);
    let frame_scale = (65.0 * 4.0 + 3.0) / 851.0;
    let tile = Vec2::new(1100.0 * frame_scale, 1092.0 * frame_scale);
    assert_eq!(
        pressed.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(2.0 * tile.x, tile.y))]
    );
    assert!(
        released.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .is_empty(),
        "resizing is deduplicated once the size is applied"
    );
    session_request(address, "a", 2, "emote/close", 0);
    let closed = run(&mut app, vec![]);
    assert_eq!(
        closed.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(tile)]
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
    for label in ["Avatar size", "x3", "Transparent background", "Exit"] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
}

#[test]
fn menu_actions_resize_square_or_close_without_dragging() {
    for step_index in 1..=5_u8 {
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
        let point = menu::slider_position("Avatar size", f32::from(step_index - 1) / 4.0);
        menu::events(vec![
            Event::PointerMoved(point),
            button(point, PointerButton::Primary, true),
        ]);
        let pressed = step(&ctx, &mut app, vec![]);
        menu::events(vec![button(point, PointerButton::Primary, false)]);
        let released = step(&ctx, &mut app, vec![]);
        let settled = step(&ctx, &mut app, vec![]);
        let mut commands = pressed.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .clone();
        commands.extend(
            released.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .cloned(),
        );
        commands.extend(
            settled.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .cloned(),
        );
        // The waiting surface keeps its native 192×192 square at every step;
        // only actual avatars or the clock derive computed content sizes.
        assert!(
            commands.is_empty(),
            "waiting surface must not resize while choosing a step: {commands:?}"
        );
        assert!(!commands.contains(&ViewportCommand::StartDrag));
    }
    {
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
                    && text.galley.text() == "Exit"
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
        assert!(commands.contains(&ViewportCommand::Close));
        assert!(!commands.contains(&ViewportCommand::StartDrag));
    }
}

fn toggle_background(ctx: &egui::Context, app: &mut EmoteApp) {
    toggle_option(ctx, app, "Transparent background");
}

fn toggle_option(ctx: &egui::Context, app: &mut EmoteApp, label: &str) {
    let output = select_menu_option(ctx, app, label);
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, ViewportCommand::InnerSize(_))),
        "toggle must not move or close the window, but the deliberate \
         framed/unframed content resize may appear: {commands:?}"
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
    // The immediate child renders after its root pass closure, so a click's
    // state change reaches the root's resize block one settle pass later;
    // return that pass so callers observe the click's window commands.
    step(ctx, app, vec![]);
    step(ctx, app, vec![])
}

/// Choose one of the five avatar-size steps (x1..x5) through the public menu
/// slider and return every root viewport command emitted while doing so.
fn set_size_step(ctx: &egui::Context, app: &mut EmoteApp, step_index: u8) -> Vec<ViewportCommand> {
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
    let point = menu::slider_position("Avatar size", f32::from(step_index - 1) / 4.0);
    menu::events(vec![
        Event::PointerMoved(point),
        button(point, PointerButton::Primary, true),
    ]);
    let pressed = step(ctx, app, vec![]);
    menu::events(vec![button(point, PointerButton::Primary, false)]);
    let released = step(ctx, app, vec![]);
    let settled = step(ctx, app, vec![]);
    let child = menu::take();
    assert!(
        child.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == format!("x{step_index}"))),
        "size slider must display the chosen step x{step_index}"
    );
    let mut commands = pressed.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .clone();
    commands.extend(
        released.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .cloned(),
    );
    commands.extend(
        settled.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .cloned(),
    );
    commands
}

fn has_black_background(output: &Output) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.rect.size() == Vec2::splat(192.0) && rect.fill == egui::Color32::BLACK)
    })
}

/// Toggle **No frame** through the public menu and return every root viewport
/// command emitted while doing so (the checkbox reacts on the press pass).
fn toggle_no_frame_collecting_commands(
    ctx: &egui::Context,
    app: &mut EmoteApp,
) -> Vec<ViewportCommand> {
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
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "No frame" => {
                Some(text.pos + Vec2::splat(4.0))
            }
            _ => None,
        })
        .expect("menu option must be painted: No frame");
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    let pressed = step(ctx, app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    let released = step(ctx, app, vec![]);
    let settled = step(ctx, app, vec![]);
    let mut commands = pressed.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .clone();
    commands.extend(
        released.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .cloned(),
    );
    commands.extend(
        settled.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .cloned(),
    );
    commands
}

#[test]
fn startup_options_match_preferred_defaults() {
    let (ctx, mut app, address) = setup_defaults();
    let output = step(&ctx, &mut app, vec![]);
    assert!(
        output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Rect(r) if r.fill == egui::Color32::from_black_alpha(140)
        )),
        "startup background must have 45% transparency"
    );
    select_menu_option(&ctx, &mut app, "Background transparency");
    let child = menu::take();
    assert!(menu::checked(&child, "Transparent background"));
    assert!(menu::checked(&child, "No frame"));
    for label in ["x2", "45", "%"] {
        assert!(
            child.shapes.iter().any(|s| matches!(&s.shape,
                egui::Shape::Text(t) if t.galley.text() == label
            )),
            "startup menu must display {label}"
        );
    }
    menu::events(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    step(&ctx, &mut app, vec![]);
    session_request(address, "one", 1, "emote", 33);
    session_request(address, "two", 1, "emote", 66);
    let output = step(&ctx, &mut app, vec![]);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::InnerSize(Vec2::new(136.0, 281.0))),
        "x2 avatars must request a compact vertical canvas"
    );
    let output = surface_step(&ctx, &mut app, Vec2::new(136.0, 281.0));
    assert!(frames(&output).is_empty());
    let images = avatars(&output);
    assert_eq!(images.len(), 2);
    for (_, rect) in &images {
        assert_eq!(rect.size(), Vec2::splat(128.0));
    }
    assert_eq!(images[0].1.min.x, images[1].1.min.x);
    assert!(images[0].1.max.y < images[1].1.min.y);
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
fn transparent_toggle_defaults_to_45_percent_displayed_transparency() {
    let (ctx, mut app, _) = setup();
    toggle_background(&ctx, &mut app);
    // Checked toggle-on selects 45% displayed transparency: opacity 55,
    // black underlay alpha 140 (rounded 55 * 255 / 100).
    let output = step(&ctx, &mut app, vec![]);
    assert!(
        output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Rect(r) if r.fill == egui::Color32::from_black_alpha(140)
        )),
        "toggle-on must default to 45% displayed transparency (opacity 55, alpha 140)"
    );
    // The menu slider displays exactly 45% with the checkbox still checked.
    select_menu_option(&ctx, &mut app, "Background transparency");
    let child = menu::take();
    assert!(menu::checked(&child, "Transparent background"));
    for text in ["45", "%"] {
        assert!(
            child
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t)
                if t.galley.text() == text)),
            "slider must display 45% after enabling transparent mode"
        );
    }
    menu::events(vec![Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    step(&ctx, &mut app, vec![]);
    // The manual slider remains adjustable from the 45% default.
    set_background_transparency(&ctx, &mut app, 0.5, true);
    // Toggle-off still restores the fully opaque background.
    toggle_background(&ctx, &mut app);
    assert!(
        has_black_background(&step(&ctx, &mut app, vec![])),
        "toggle-off must restore the fully opaque background"
    );
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
fn image_fits_with_one_source_row_of_top_padding_and_full_uv() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    // No frame preserves the original full-surface single-avatar composition.
    toggle_option(&ctx, &mut app, "No frame");
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
        let (scale, expected) = snapped_physical_image_fit(surface, 1.0);
        assert!(scale >= 1.0, "supported sizes keep native physical pixels");
        assert_whole_physical_pixels(expected, 1.0, "single avatar");
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
        let content_bottom = expected.bottom() - 1.0 * scale;
        assert!(
            (top_gap - (surface.bottom() - content_bottom)).abs() <= 1.0 + 0.001,
            "one source row of top padding, centered on whole physical pixels"
        );
        assert!(surface.contains_rect(expected));
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.rect == surface && rect.fill == egui::Color32::BLACK)
        }));
    }
}

#[test]
fn image_uses_whole_physical_pixels_with_snapped_edges_at_every_scale_factor() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    session_request(address, "a", 1, "emote", 42);
    step(&ctx, &mut app, vec![]);
    let mut frame_baselines: Vec<(f32, egui::Rect)> = Vec::new();
    let mut unframed = false;
    for framed in [true, false] {
        // Toggle at the default scale so the menu layout stays canonical.
        ctx.set_pixels_per_point(1.0);
        step(&ctx, &mut app, vec![]);
        if unframed != !framed {
            toggle_option(&ctx, &mut app, "No frame");
            unframed = !framed;
        }
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            ctx.set_pixels_per_point(ppp);
            // Absorb egui's zoom-transition pass, which rescales the previous
            // surface instead of honoring the new raw screen rect once.
            step(&ctx, &mut app, vec![]);
            for size in [128.0, 192.0, 256.0, 384.0] {
                let output = surface_step(&ctx, &mut app, Vec2::splat(size));
                let surface = egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(size));
                let images = avatars(&output);
                assert_eq!(images.len(), 1);
                let (id, image) = images[0];
                let context = format!("framed={framed} ppp={ppp} size={size}");
                assert_whole_physical_pixels(image, ppp, &context);
                assert!(
                    surface.contains_rect(image),
                    "{context}: {image:?} must stay inside {surface:?}"
                );
                if framed {
                    let borders = frames(&output);
                    assert_eq!(borders.len(), 1);
                    let (_, border) = borders[0];
                    let artwork_scale = border.width() / 1100.0;
                    let opening = egui::Rect::from_min_size(
                        border.min + egui::vec2(125.0, 121.0) * artwork_scale,
                        Vec2::splat(851.0 * artwork_scale),
                    );
                    let (_, expected) = snapped_physical_image_fit(opening, ppp);
                    assert!(
                        (image.min - expected.min).length() < 1e-3
                            && (image.max - expected.max).length() < 1e-3,
                        "{context}: framed fit must be the largest whole physical scale in the opening: {image:?} vs {expected:?}"
                    );
                    assert!(
                        border.contains_rect(image),
                        "{context}: the avatar must stay inside the frame opening"
                    );
                    let baseline = frame_baselines
                        .iter()
                        .find(|(s, _)| *s == size)
                        .map(|(_, rect)| *rect)
                        .unwrap_or_else(|| {
                            frame_baselines.push((size, border));
                            border
                        });
                    assert!(
                        (border.min - baseline.min).length() < 1e-3
                            && (border.max - baseline.max).length() < 1e-3,
                        "the decorative frame keeps its fractional artwork fit at every scale factor: {border:?} vs {baseline:?}"
                    );
                } else {
                    let (_, expected) = snapped_physical_image_fit(surface, ppp);
                    assert!(
                        (image.min - expected.min).length() < 1e-3
                            && (image.max - expected.max).length() < 1e-3,
                        "{context}: unframed fit must be the largest whole physical scale: {image:?} vs {expected:?}"
                    );
                    let mesh = avatar_mesh(&output, id);
                    assert_eq!(
                        mesh.vertices.len(),
                        4,
                        "{context}: unframed opaque single avatar keeps its full-UV quad"
                    );
                    for (pos, uv) in [
                        (expected.left_top(), egui::pos2(0.0, 0.0)),
                        (expected.right_top(), egui::pos2(1.0, 0.0)),
                        (expected.left_bottom(), egui::pos2(0.0, 1.0)),
                        (expected.right_bottom(), egui::pos2(1.0, 1.0)),
                    ] {
                        assert!(
                            mesh.vertices.iter().any(|vertex| {
                                vertex.pos.distance(pos) < 1e-3
                                    && vertex.uv == uv
                                    && vertex.color == egui::Color32::WHITE
                            }),
                            "{context}: missing image corner {pos:?} with full UV {uv:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn every_supported_surface_keeps_whole_physical_pixels_or_the_documented_fallback() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    step(&ctx, &mut app, vec![]);
    toggle_option(&ctx, &mut app, "No frame");
    for ppp in [1.0, 1.25, 1.5, 2.0] {
        ctx.set_pixels_per_point(ppp);
        // Absorb egui's zoom-transition pass before measuring geometry.
        step(&ctx, &mut app, vec![]);
        for size in [128.0, 192.0, 256.0, 384.0] {
            let dimensions = Vec2::new(size * 2.0, size);
            let output = surface_step(&ctx, &mut app, dimensions);
            let bounds = egui::Rect::from_min_size(Pos2::ZERO, dimensions);
            let images = avatars(&output);
            assert_eq!(images.len(), 2);
            let cards = expected_unframed_cards(bounds, 2, false);
            for (index, ((_, image), card)) in images.iter().zip(&cards).enumerate() {
                let context = format!("ppp={ppp} size={size} card={index}");
                assert_whole_physical_pixels(*image, ppp, &context);
                assert!(
                    bounds.contains_rect(*image),
                    "{context}: {image:?} must stay inside {bounds:?}"
                );
                let (_, expected) = snapped_physical_image_fit(card.shrink(4.0), ppp);
                assert!(
                    (image.min - expected.min).length() < 1e-3
                        && (image.max - expected.max).length() < 1e-3,
                    "{context}: card fit must be the largest whole physical scale: {image:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn undersized_surfaces_keep_contained_fallback_images_at_any_scale_factor() {
    let (ctx, mut app, address) = setup();
    step(&ctx, &mut app, vec![]);
    session_request(address, "a", 1, "emote", 42);
    session_request(address, "b", 1, "emote", 84);
    step(&ctx, &mut app, vec![]);
    toggle_option(&ctx, &mut app, "No frame");
    for ppp in [1.0, 1.5] {
        ctx.set_pixels_per_point(ppp);
        // Absorb egui's zoom-transition pass before measuring geometry.
        step(&ctx, &mut app, vec![]);
        for vertical in [false, true] {
            for extent in [16.0, 4.0] {
                let size = if vertical {
                    Vec2::new(192.0, extent)
                } else {
                    Vec2::new(extent, 192.0)
                };
                let output = surface_step(&ctx, &mut app, size);
                let bounds = egui::Rect::from_min_size(Pos2::ZERO, size);
                let images = avatars(&output);
                assert_eq!(images.len(), 2, "narrow resizes must not discard avatars");
                for (index, (_, image)) in images.iter().enumerate() {
                    let context =
                        format!("ppp={ppp} vertical={vertical} extent={extent} card={index}");
                    assert!(image.is_positive(), "{context}");
                    assert!(
                        bounds.contains_rect(*image),
                        "{context}: {image:?} must not overflow {bounds:?}"
                    );
                    assert!(
                        image.width() * ppp < 64.0,
                        "{context}: below 65 physical pixels the documented fractional fallback applies"
                    );
                }
            }
        }
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
