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
                        let scale = (fitted.width() / 64.0).min(fitted.height() / 69.0);
                        let expected = egui::Rect::from_min_size(
                            Pos2::new(
                                fitted.center().x - 32.0 * scale,
                                fitted.center().y - 69.0 * scale / 2.0 + 5.0 * scale,
                            ),
                            Vec2::splat(64.0 * scale),
                        );
                        assert!(
                            (image.min - expected.min).length() < 0.001
                                && (image.max - expected.max).length() < 0.001,
                            "each card must apply its own PNG fit and top compensation: index={index}, vertical={vertical}, actual={image:?}, expected={expected:?}"
                        );
                        let top = image.top() - card.top();
                        let content_bottom = card.bottom() - (image.bottom() - 5.0 * scale);
                        assert!(
                            (top - content_bottom).abs() < 0.001,
                            "balance each PNG's five source footer rows independently"
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
            let scale = 192.0 / 69.0;
            assert!(
                (avatars(&opaque)[0].1.width() - 64.0 * scale).abs() < 0.001,
                "No frame retains the original full-surface single-avatar fit"
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
                    !output
                        .shapes
                        .iter()
                        .any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.fill.a()>0))
                );
                let cards = expected_unframed_cards(
                    egui::Rect::from_min_size(Pos2::ZERO, dimensions(count, size)),
                    count,
                    vertical,
                );
                for ((_, image), card) in images.iter().zip(cards) {
                    let fitted = card.shrink(4.0);
                    let scale = (fitted.width() / 64.0).min(fitted.height() / 69.0);
                    assert!((image.center().x - fitted.center().x).abs() < 0.001);
                    assert!(
                        (image.center().y - (fitted.center().y + 2.5 * scale)).abs() < 0.001,
                        "automatic transparent layout must apply compensation per card"
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
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(192.0, 384.0))]
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
        !transparent
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.fill.a()>0))
    );
    let clear_images = avatars(&transparent);
    assert!(
        ((clear_images[1].1.center().y - clear_images[0].1.center().y) - 194.5).abs() < 0.001,
        "transparent cards preserve their equal 189.5-point heights plus five-point separation"
    );
    drop(transparent);
    toggle_option(&ctx, &mut app, "No frame");
    toggle_background(&ctx, &mut app);
    assert_eq!(frames(&render(&mut app, 2, 192.0)).len(), 2);
    let selection = select_menu_option(&ctx, &mut app, "Horizontal");
    assert_eq!(
        selection.viewport_output[&egui::ViewportId::ROOT].commands,
        vec![ViewportCommand::InnerSize(Vec2::new(384.0, 192.0))]
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
    assert!(avatars(&retained).is_empty());
    assert!(frames(&retained).is_empty());
    assert!(has_clock(&retained));
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

fn has_clock(output: &Output) -> bool {
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.text().ends_with("\nWaiting Agents...")
                && text.galley.text().len() == 37
                && text.galley.text().as_bytes()[2] == b':'
                && text.galley.text().as_bytes()[5] == b':'
                && text.galley.text()[..8].bytes().enumerate().all(|(i, b)|
                    i == 2 || i == 5 || b.is_ascii_digit())
                && text.galley.text().as_bytes()[8] == b'\n'
                && text.galley.text()[9..19].bytes().enumerate().all(|(i, b)|
                    if i == 2 || i == 5 { b == b'/' } else { b.is_ascii_digit() }))
    })
}

#[test]
fn sole_avatar_close_immediately_shows_clock_and_new_avatar_replaces_it() {
    let (ctx, mut app, address) = setup();
    let waiting = step(&ctx, &mut app, vec![]);
    assert!(!has_clock(&waiting));
    assert!(waiting.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.text().starts_with("Waiting for emote"))));
    drop(waiting);
    session_request(address, "a", 1, "emote", 42);
    let active = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&active).len(), 1);
    assert!(!has_clock(&active));
    drop(active);
    session_request(address, "a", 2, "emote/close", 0);
    for size in [128.0, 192.0, 256.0, 384.0] {
        let clock = surface_step(&ctx, &mut app, Vec2::splat(size));
        assert!(has_clock(&clock));
        let surface = egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(size));
        let text = clock
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with("\nWaiting Agents...") => {
                    Some(text)
                }
                _ => None,
            })
            .expect("clock text must be painted");
        let painted_bounds = text.galley.mesh_bounds.translate(text.pos.to_vec2());
        assert!(
            surface.contains_rect(painted_bounds),
            "clock text must fit at {size}: {painted_bounds:?}"
        );
        assert!(avatars(&clock).is_empty());
        assert!(frames(&clock).is_empty());
        assert!(!clock.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.text().starts_with("Waiting for emote"))));
        assert!(
            clock.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                <= std::time::Duration::from_secs(1)
        );
    }
    session_request(address, "b", 1, "emote", 84);
    let restored = step(&ctx, &mut app, vec![]);
    assert_eq!(avatars(&restored).len(), 1);
    assert!(!has_clock(&restored));
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
