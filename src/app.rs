//! Native presentation: the network owns publication; this app owns GPU resources.
use crate::idle_clock_image::{self, ANIMATION_PERIOD_MS};
use crate::receiver::{Frame, LatestFrame, Receiver};
use crate::sessions::SessionState;
use eframe::egui::{
    self, ColorImage, PointerButton, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use std::{collections::HashMap, sync::Arc, time::Duration};

struct AvatarTexture {
    uploaded: Arc<Frame>,
    texture: TextureHandle,
}

struct ClockTexture {
    step: i64,
    name: Option<String>,
    texture: TextureHandle,
}

fn load_avatar_frame(ctx: &egui::Context) -> TextureHandle {
    // This trusted embedded asset is validated by the public rendering tests.
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../assets/avatar-frame.png"
    )))
    .read_info()
    .expect("valid embedded avatar frame");
    let mut rgba = vec![0; reader.output_buffer_size().expect("bounded frame asset")];
    let info = reader
        .next_frame(&mut rgba)
        .expect("complete embedded frame");
    assert_eq!(
        (info.width, info.height, info.color_type, info.bit_depth),
        (1100, 1092, png::ColorType::Rgba, png::BitDepth::Eight)
    );
    ctx.load_texture(
        "avatar-frame",
        ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &rgba[..info.buffer_size()],
        ),
        TextureOptions::NEAREST,
    )
}

/// Fit the virtual 64×65 composition (the image plus its one padding row) into
/// `rect` with the largest whole number of physical pixels per source pixel at
/// the context's current `pixels_per_point`, snapping every image edge onto an
/// integer physical pixel. `TextureOptions::NEAREST` then always shows square,
/// unskewed pixels whatever scale factor the display reports. Faithful source
/// pixels take priority over filling the rectangle, so margins may grow. Only
/// below one physical pixel per source pixel does the fit degrade to the
/// legacy fractional contain scale; the image never leaves `rect`. Returns the
/// scale in logical points and the image rectangle (`Rect::NOTHING`, skip
/// painting, when nothing fits).
pub fn snapped_physical_image_fit(rect: egui::Rect, pixels_per_point: f32) -> (f32, egui::Rect) {
    let ppp = pixels_per_point.max(f32::EPSILON);
    // Inward-snapped integer physical bounds can never leave `rect`.
    let min_x = (rect.min.x * ppp).ceil();
    let max_x = (rect.max.x * ppp).floor();
    let min_y = (rect.min.y * ppp).ceil();
    let max_y = (rect.max.y * ppp).floor();
    let width = (max_x - min_x).max(0.0);
    let height = (max_y - min_y).max(0.0);
    let per_texel = (width / 64.0).min(height / 65.0);
    if per_texel < 1.0 {
        // Legacy fractional fallback below one physical pixel per source pixel:
        // native pixel fidelity is impossible, so keep the previous exact
        // contain fit instead of snapping away a sub-pixel image.
        let scale = (rect.width() / 64.0).min(rect.height() / 65.0);
        if scale <= 0.0 {
            return (0.0, egui::Rect::NOTHING);
        }
        let image = egui::Rect::from_center_size(
            rect.center() + egui::vec2(0.0, 0.5 * scale),
            Vec2::splat(64.0 * scale),
        );
        return (scale, image);
    }
    let physical = per_texel.floor();
    if physical <= 0.0 {
        return (0.0, egui::Rect::NOTHING);
    }
    // Center on whole physical pixels; the padding row stays above the image.
    let start_x = min_x + ((width - 64.0 * physical) / 2.0).floor();
    let start_y = min_y + ((height - 65.0 * physical) / 2.0).floor();
    let image = egui::Rect::from_min_max(
        egui::pos2(start_x / ppp, (start_y + physical) / ppp),
        egui::pos2(
            (start_x + 64.0 * physical) / ppp,
            (start_y + 65.0 * physical) / ppp,
        ),
    );
    (physical / ppp, image)
}

/// Intersect the image quad with the rounded card before deriving its UVs.
/// Background rounding alone cannot mask opaque pixels at the image's foot.
fn paint_rounded_card_image(
    painter: &egui::Painter,
    texture: egui::TextureId,
    image: egui::Rect,
    card: egui::Rect,
    radius: f32,
) {
    if image.width() <= 0.0 || image.height() <= 0.0 {
        return;
    }
    let radius = radius.min(card.width() / 2.0).min(card.height() / 2.0);
    let mut points = Vec::with_capacity(36);
    use std::f32::consts::{FRAC_PI_2, PI};
    for (center, start) in [
        (
            egui::pos2(card.right() - radius, card.top() + radius),
            -FRAC_PI_2,
        ),
        (
            egui::pos2(card.right() - radius, card.bottom() - radius),
            0.0,
        ),
        (
            egui::pos2(card.left() + radius, card.bottom() - radius),
            FRAC_PI_2,
        ),
        (egui::pos2(card.left() + radius, card.top() + radius), PI),
    ] {
        for step in 0..=8 {
            let angle = start + FRAC_PI_2 * step as f32 / 8.0;
            points.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
        }
    }
    // Clip the convex outline to the original image, not the other way around:
    // padding keeps its own background, and the complete source UV fit is stable.
    for (horizontal, boundary, greater) in [
        (true, image.left(), true),
        (true, image.right(), false),
        (false, image.top(), true),
        (false, image.bottom(), false),
    ] {
        let input = std::mem::take(&mut points);
        let Some(mut previous) = input.last().copied() else {
            return;
        };
        let distance = |point: egui::Pos2| {
            let coordinate = if horizontal { point.x } else { point.y };
            if greater {
                coordinate - boundary
            } else {
                boundary - coordinate
            }
        };
        let mut previous_distance = distance(previous);
        for current in input {
            let current_distance = distance(current);
            if (previous_distance >= 0.0) != (current_distance >= 0.0) {
                let t = previous_distance / (previous_distance - current_distance);
                points.push(previous + (current - previous) * t);
            }
            if current_distance >= 0.0 {
                points.push(current);
            }
            previous = current;
            previous_distance = current_distance;
        }
    }
    if points.len() < 3 {
        return;
    }
    let mut mesh = egui::epaint::Mesh::with_texture(texture);
    for point in points {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: point,
            uv: egui::pos2(
                ((point.x - image.left()) / image.width()).clamp(0.0, 1.0),
                ((point.y - image.top()) / image.height()).clamp(0.0, 1.0),
            ),
            color: egui::Color32::WHITE,
        });
    }
    for index in 1..mesh.vertices.len() - 1 {
        mesh.indices
            .extend_from_slice(&[0, index as u32, (index + 1) as u32]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Paint every clock pixel over its allocated card; avatar masking is unchanged.
fn paint_clock_image(
    painter: &egui::Painter,
    texture: egui::TextureId,
    image: egui::Rect,
    card: egui::Rect,
    opacity: u8,
    radius: f32,
) {
    if !image.is_positive() {
        return;
    }
    if opacity > 0 {
        let alpha = ((u16::from(opacity) * 255 + 50) / 100) as u8;
        painter.rect_filled(card, radius, egui::Color32::from_black_alpha(alpha));
    }
    painter.image(
        texture,
        image,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::splat(192.0))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_window_level(egui::WindowLevel::AlwaysOnTop),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    }
}

pub struct EmoteApp {
    // Ownership is deliberate: dropping the app closes the socket and joins the
    // receiver on this thread, never inside its own repaint callback.
    _receiver: Receiver,
    latest: LatestFrame,
    textures: HashMap<Option<String>, AvatarTexture>,
    frame_texture: Option<TextureHandle>,
    clocks: HashMap<Option<String>, ClockTexture>,
    /// Avatar-size slider step x1..x5: the avatar's 64-point image renders at
    /// that many points per source texel, and the canvas derives from that
    /// composition (image plus its one separation row) plus the visible
    /// decorations — never from a pre-chosen square that loses a level.
    size_step: u8,
    applied_size: Vec2,
    background_opacity: u8,
    transparent_background: bool,
    no_frame: bool,
    vertical_stacking: bool,
    menu_open: bool,
    menu_position: Option<egui::Pos2>,
    menu_size: Vec2,
    menu_had_focus: bool,
    #[cfg(target_os = "windows")]
    applied_native_shadow: Option<bool>,
}
impl EmoteApp {
    pub fn new(receiver: Receiver) -> Self {
        Self {
            latest: receiver.latest(),
            _receiver: receiver,
            textures: HashMap::new(),
            frame_texture: None,
            clocks: HashMap::new(),
            size_step: 2,
            applied_size: Vec2::splat(192.0),
            background_opacity: 55,
            transparent_background: true,
            no_frame: true,
            vertical_stacking: true,
            menu_open: false,
            menu_position: None,
            menu_size: egui::vec2(208.0, 176.0),
            menu_had_focus: false,
            #[cfg(target_os = "windows")]
            applied_native_shadow: None,
        }
    }

    #[cfg(any(target_os = "windows", test))]
    fn native_shadow_enabled(&self) -> bool {
        self.background_opacity == 100
    }

    /// Shared by the native app and small embedding examples. Input and receiver
    /// notifications wake eframe; only clock mode schedules a 500ms repaint.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.show_at(ui, chrono::Local::now().fixed_offset());
    }

    /// Draw at a supplied clock instant for deterministic embedding and pixel checks.
    /// Native `show` continues to read the local wall clock once per draw.
    pub fn show_at(&mut self, ui: &mut egui::Ui, now: chrono::DateTime<chrono::FixedOffset>) {
        // One atomic ordered snapshot; never hold the registry lock during GPU work.
        // Inactive final avatars intentionally use the same cache entry and pixels.
        let (snapshots, show_clock) = self.latest.display_snapshot();
        // Cache only visible clock keys, bounded by the registry's snapshot cap.
        // IDs remain cache identity, never display-name fallbacks. The global
        // clock has no name; legacy frames may supply one without an ID.
        let clock_inputs: Vec<_> = if show_clock {
            vec![(None, None)]
        } else {
            snapshots
                .iter()
                .filter(|(s, _)| s.state == SessionState::Clock)
                .map(|(s, name)| (s.session_id.clone(), name.clone()))
                .collect()
        };
        let any_clock = !clock_inputs.is_empty();
        self.clocks
            .retain(|key, _| clock_inputs.iter().any(|(id, _)| id == key));
        let step = idle_clock_image::animation_step(now);
        for (key, name) in clock_inputs {
            if self
                .clocks
                .get(&key)
                .is_some_and(|clock| clock.step == step && clock.name == name)
            {
                continue;
            }
            let frame = name.as_deref().map_or_else(
                || idle_clock_image::render(now),
                |name| idle_clock_image::render_for_session(now, name),
            );
            let image = ColorImage::from_rgba_unmultiplied([64, 64], &frame.rgba);
            if let Some(clock) = self.clocks.get_mut(&key) {
                clock.texture.set(image, TextureOptions::NEAREST);
                clock.step = step;
                clock.name = name;
            } else {
                let texture =
                    ui.ctx()
                        .load_texture("emote-idle-clock", image, TextureOptions::NEAREST);
                self.clocks.insert(
                    key,
                    ClockTexture {
                        step,
                        name,
                        texture,
                    },
                );
            }
        }
        // Ordinary avatar geometry and texture handling still consume exactly
        // the original ordered snapshots; names affect only clock pixels.
        let snapshots: Vec<_> = snapshots
            .into_iter()
            .map(|(snapshot, _)| snapshot)
            .collect();
        self.textures
            .retain(|key, _| snapshots.iter().any(|snapshot| &snapshot.session_id == key));
        for snapshot in &snapshots {
            // Clock-state sessions render their keyed internal clock texture;
            // their received pixels stay validated in the registry but are
            // never uploaded, and returning to normal reuploads on demand.
            if snapshot.state == SessionState::Clock {
                continue;
            }
            let cached = self.textures.get_mut(&snapshot.session_id);
            if let Some(cached) = cached {
                if !Arc::ptr_eq(&cached.uploaded, &snapshot.frame) {
                    cached.texture.set(
                        ColorImage::from_rgba_unmultiplied([64, 64], &snapshot.frame.rgba),
                        TextureOptions::NEAREST,
                    );
                    cached.uploaded = snapshot.frame.clone();
                }
            } else {
                // Keep the legacy name for embedding/smoke consumers. Session names
                // are stable and distinct from that singleton and from each other.
                let name = snapshot
                    .session_id
                    .as_ref()
                    .map_or_else(|| "emote".to_owned(), |id| format!("emote-session:{id}"));
                let texture = ui.ctx().load_texture(
                    name,
                    ColorImage::from_rgba_unmultiplied([64, 64], &snapshot.frame.rgba),
                    TextureOptions::NEAREST,
                );
                self.textures.insert(
                    snapshot.session_id.clone(),
                    AvatarTexture {
                        uploaded: snapshot.frame.clone(),
                        texture,
                    },
                );
            }
        }
        let count = snapshots.len().max(1);
        let vertical = self.vertical_stacking;
        let rect = ui.max_rect();
        // One full-window hit target lets the image and placeholder share exactly
        // the same controls. Painting text does not steal drag/menu interactions.
        let response = ui.interact(
            rect,
            ui.scope_id().with("emote-surface"),
            egui::Sense::click_and_drag(),
        );
        let partially_opaque = (1..100).contains(&self.background_opacity);
        let unframed = self.no_frame || self.background_opacity < 100;
        // The waiting surface has no artwork; every actual opaque avatar may
        // use the same frame, including the single-avatar case.
        let framed = !show_clock && !snapshots.is_empty() && !unframed;
        let frame = if framed {
            Some(
                self.frame_texture
                    .get_or_insert_with(|| load_avatar_frame(ui.ctx())),
            )
        } else {
            None
        };
        let packed_layout = if unframed && count > 1 {
            let along = if vertical {
                rect.height()
            } else {
                rect.width()
            }
            .max(0.0);
            // Reserve clear separators first, then give every card equal space.
            // Keep cards positive during tiny transient OS resizes.
            let gap = 5.0_f32.min(along / (2.0 * count as f32 - 1.0));
            let span = (along - gap * (count - 1) as f32) / count as f32;
            Some((span, gap))
        } else {
            None
        };
        let radius = 8.0_f32.min(rect.width().min(rect.height()) * 0.25).round();
        // Each image is composed inside its own card, never a global image group.
        let card_rect = |span: f32, gap: f32, index: usize| {
            let offset = index as f32 * (span + gap);
            if vertical {
                egui::Rect::from_min_size(
                    rect.min + Vec2::new(0.0, offset),
                    Vec2::new(rect.width(), span),
                )
            } else {
                egui::Rect::from_min_size(
                    rect.min + Vec2::new(offset, 0.0),
                    Vec2::new(span, rect.height()),
                )
            }
        };
        if self.background_opacity > 0 {
            let alpha = ((u16::from(self.background_opacity) * 255 + 50) / 100) as u8;
            let fill = egui::Color32::from_black_alpha(alpha);
            if let Some((span, gap)) = packed_layout {
                for index in 0..snapshots.len() {
                    ui.painter()
                        .rect_filled(card_rect(span, gap, index), radius, fill);
                }
            } else if !show_clock {
                ui.painter()
                    .rect_filled(rect, if partially_opaque { radius } else { 0.0 }, fill);
            }
        }
        for (index, snapshot) in snapshots.iter().filter(|_| !show_clock).enumerate() {
            // Idle sessions render the internal clock in their own tile while
            // ordinary neighbors keep their received pixels.
            let texture = if snapshot.state == SessionState::Clock {
                &self.clocks[&snapshot.session_id].texture
            } else {
                &self.textures[&snapshot.session_id].texture
            };
            // Divide the full surface into neighboring tiles, preserving the single
            // avatar's fit even while the OS is processing a resize request.
            let (offset, size) = if vertical {
                let height = rect.height() / count as f32;
                (
                    Vec2::new(0.0, index as f32 * height),
                    Vec2::new(rect.width(), height),
                )
            } else {
                let width = rect.width() / count as f32;
                (
                    Vec2::new(index as f32 * width, 0.0),
                    Vec2::new(width, rect.height()),
                )
            };
            let rect = egui::Rect::from_min_size(rect.min + offset, size);
            let (rect, decoration) = if let Some((span, gap)) = packed_layout {
                let card = card_rect(span, gap, index);
                // Clock and avatar slots share the same fit, even at DPI.
                let padding = 4.0_f32.min(card.width().min(card.height()) * 0.25);
                (card.shrink(padding), None)
            } else if let Some(frame) = &frame {
                let artwork = frame.size_vec2();
                let scale = (rect.width() / artwork.x).min(rect.height() / artwork.y);
                let border = egui::Rect::from_center_size(rect.center(), artwork * scale);
                // Verified clear source rectangle, including two source pixels
                // of safety. Rounded frame corners never overlap avatar labels.
                let opening = egui::Rect::from_min_max(
                    border.min + egui::vec2(125.0, 121.0) * scale,
                    border.min + egui::vec2(976.0, 972.0) * scale,
                );
                (opening, Some((frame.id(), border)))
            } else if count > 1 {
                // Both unframed modes retain the same fit, including narrow resizes.
                (
                    rect.shrink(4.0_f32.min(rect.width().min(rect.height()) * 0.25)),
                    None,
                )
            } else {
                (rect, None)
            };
            // Keep one source row of padding above the complete image, scaled
            // in whole physical pixels with snapped edges so NEAREST keeps
            // square pixels at any display scale factor.
            let (_, image_rect) = snapped_physical_image_fit(rect, ui.ctx().pixels_per_point());
            if !image_rect.is_positive() {
                continue;
            }
            if unframed && snapshot.state == SessionState::Clock {
                paint_clock_image(
                    ui.painter(),
                    texture.id(),
                    image_rect,
                    rect,
                    0, // The shared allocated-card background is already painted.
                    radius,
                );
            } else if let Some((span, gap)) = packed_layout {
                paint_rounded_card_image(
                    ui.painter(),
                    texture.id(),
                    image_rect,
                    card_rect(span, gap, index),
                    radius,
                );
            } else if partially_opaque {
                // Without native shadow, a single card needs the same explicit
                // background/image contour as the stacked unframed cards.
                paint_rounded_card_image(ui.painter(), texture.id(), image_rect, rect, radius);
            } else {
                ui.painter().image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            if let Some((texture, border)) = decoration {
                ui.painter().image(
                    texture,
                    border,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        if show_clock {
            // Global fallback after the final explicit close: one internal
            // clock, no decorative frame (same close behavior as before).
            let texture = &self.clocks[&None].texture;
            let (_, image_rect) = snapped_physical_image_fit(rect, ui.ctx().pixels_per_point());
            paint_clock_image(
                ui.painter(),
                texture.id(),
                image_rect,
                rect,
                self.background_opacity,
                radius,
            );
        } else if snapshots.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for emote\n127.0.0.1:8001\n\nRight-click: options / exit\nDrag: move",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
        }
        // Clock animation ticks every 500ms (walkers advance, glyphs stay
        // second-stable); avatars without clocks keep event-driven repaints.
        if any_clock {
            ui.ctx()
                .request_repaint_after(Duration::from_millis(ANIMATION_PERIOD_MS));
        }
        let ctx = ui.ctx().clone();
        let menu_id = egui::ViewportId::from_hash_of("emote-options");
        let opening = response.secondary_clicked();
        if opening {
            self.menu_open = true;
            self.menu_had_focus = false;
            self.menu_position = ctx.input(|input| {
                Some(input.viewport().inner_rect?.min + response.interact_pointer_pos()?.to_vec2())
            });
        }
        if self.menu_open {
            let mut builder = egui::ViewportBuilder::default()
                .with_title("Emote options")
                .with_inner_size(self.menu_size)
                .with_decorations(false)
                .with_resizable(false)
                .with_transparent(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop);
            if let Some(position) = self.menu_position {
                builder = builder.with_position(position);
            }
            // Synchronous child rendering keeps settings on the UI thread; no
            // registry ownership, queue or cross-thread mutable state is shared.
            ctx.show_viewport_immediate(menu_id, builder, |ui, _class| {
                let (focused, close) = ui.input(|input| {
                    (
                        input.viewport().focused,
                        input.viewport().close_requested() || input.key_pressed(egui::Key::Escape),
                    )
                });
                if focused == Some(true) {
                    self.menu_had_focus = true;
                }
                if close || (self.menu_had_focus && focused == Some(false)) {
                    self.menu_open = false;
                    return;
                }
                ui.painter()
                    .rect_filled(ui.max_rect(), 0.0, ui.visuals().window_fill());
                let mut content_width = 0.0_f32;
                let content = egui::Frame::new().inner_margin(8).show(ui, |ui| {
                    let size_caption = ui.label("Avatar size");
                    content_width = content_width.max(size_caption.rect.width());
                    // Five whole-integer steps over 64-point content sides;
                    // the value stays live like the transparency slider. The
                    // row width must include the value text, not just the rail.
                    let slider_row = ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.size_step, 1..=5)
                                .custom_formatter(|value, _| format!("x{}", value as u8)),
                        );
                        ui.min_rect().width()
                    });
                    content_width = content_width.max(slider_row.inner);
                    ui.separator();
                    let transparency =
                        ui.checkbox(&mut self.transparent_background, "Transparent background");
                    content_width = content_width.max(transparency.rect.width());
                    if transparency.changed() {
                        // Default to 45% displayed transparency (opacity 55)
                        // instead of full invisibility.
                        self.background_opacity =
                            if self.transparent_background { 55 } else { 100 };
                        self.menu_open = false;
                    }
                    let label = ui.label("Background transparency");
                    content_width = content_width.max(label.rect.width());
                    let mut percentage = 100 - self.background_opacity;
                    let slider = ui.add_enabled(
                        self.transparent_background,
                        egui::Slider::new(&mut percentage, 0..=100).suffix("%"),
                    );
                    content_width = content_width.max(slider.rect.width());
                    if slider.changed() {
                        self.background_opacity = 100 - percentage;
                        ctx.request_repaint();
                    }
                    let no_frame = ui.checkbox(&mut self.no_frame, "No frame");
                    content_width = content_width.max(no_frame.rect.width());
                    if no_frame.changed() {
                        self.menu_open = false;
                    }
                    ui.separator();
                    let orientation = ui.label("Vertical / Horizontal");
                    content_width = content_width.max(orientation.rect.width());
                    let choices = ui.horizontal(|ui| {
                        let horizontal =
                            ui.radio_value(&mut self.vertical_stacking, false, "Horizontal");
                        let vertical =
                            ui.radio_value(&mut self.vertical_stacking, true, "Vertical");
                        if horizontal.clicked() || vertical.clicked() {
                            self.menu_open = false;
                        }
                        horizontal.rect.union(vertical.rect).width()
                    });
                    content_width = content_width.max(choices.inner);
                    ui.separator();
                    let exit = ui.button("Exit");
                    content_width = content_width.max(exit.rect.width());
                    if exit.clicked() {
                        ui.ctx()
                            .send_viewport_cmd_to(egui::ViewportId::ROOT, ViewportCommand::Close);
                        self.menu_open = false;
                    }
                });
                // Size to the controls, not the full-width separators or the
                // available viewport. Keep just eight points of outer padding.
                let size = egui::vec2(
                    (content_width + 16.0).ceil(),
                    content.response.rect.height().ceil(),
                );
                if size != self.menu_size {
                    self.menu_size = size;
                    ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(size));
                }
            });
            if !self.menu_open {
                ctx.send_viewport_cmd_to(menu_id, ViewportCommand::Close);
                ctx.request_repaint();
            } else if opening {
                if let Some(position) = self.menu_position {
                    ctx.send_viewport_cmd_to(menu_id, ViewportCommand::OuterPosition(position));
                }
                ctx.send_viewport_cmd_to(menu_id, ViewportCommand::Focus);
            }
        }
        // Resize only for a changed count/selection, not every repaint or heartbeat.
        // The canvas derives from the chosen image size, never the reverse: the
        // avatar renders its 64-point image at `step` points per source texel
        // (one whole physical pixel per texel per step at 100% DPI), the
        // composition adds its one source separation row, and the canvas adds
        // only the margins that composition, the packed-card insets and gaps,
        // and the visible frame actually need. Deriving the canvas from a
        // square 64×step instead made the inward-snapped fit lose a whole step
        // from x2 upward (x5 drew 256 instead of 320 points).
        let step = f32::from(self.size_step);
        let composition = Vec2::new(64.0 * step, 65.0 * step);
        let tile = if framed {
            // The frame's square clear opening (851 of the 1100 artwork
            // columns) must contain the whole composition; three logical
            // points of slack absorb the inward whole-physical-pixel snapping
            // (up to two physical pixels) and egui's 1/32-point viewport
            // rounding, so the framed image never loses a level either.
            let scale = (composition.y + 3.0) / 851.0;
            Vec2::new(1100.0 * scale, 1092.0 * scale)
        } else {
            composition
        };
        let size = if snapshots.is_empty() && !show_clock {
            // The waiting surface keeps the native 192×192 initial square.
            Vec2::splat(192.0)
        } else if unframed && count > 1 {
            // Packed cards budget their real four-point insets and five-point
            // gaps instead of deducting them from the image.
            let card = tile + Vec2::splat(8.0);
            if vertical {
                Vec2::new(card.x, count as f32 * card.y + (count - 1) as f32 * 5.0)
            } else {
                Vec2::new(count as f32 * card.x + (count - 1) as f32 * 5.0, card.y)
            }
        } else if vertical {
            Vec2::new(tile.x, count as f32 * tile.y)
        } else {
            Vec2::new(count as f32 * tile.x, tile.y)
        };
        if size != self.applied_size {
            ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(size));
            self.applied_size = size;
        }
        // Secondary dragging belongs to the context menu, never to window motion.
        // The popup owns its pointer presses, so selecting an item cannot move us.
        if response.drag_started_by(PointerButton::Primary) && !self.menu_open {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }
}
impl eframe::App for EmoteApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Clear alpha too, so switching modes never leaves an opaque old frame.
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowExtWindows;

            // Apply after show so a checkbox change takes effect immediately.
            // The undecorated shadow also introduces a thin native perimeter.
            let shadow = self.native_shadow_enabled();
            if self.applied_native_shadow != Some(shadow)
                && let Some(window) = _frame.winit_window()
            {
                window.set_undecorated_shadow(shadow);
                self.applied_native_shadow = Some(shadow);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Pos2};
    mod menu {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/menu.rs"
        ));
    }

    fn step(ctx: &egui::Context, app: &mut EmoteApp, events: Vec<Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(192.0))),
                events,
                ..Default::default()
            },
            |ui| app.show(ui),
        );
        // Headless rendering consumes no textures; acknowledge their deltas.
        output.textures_delta.clear();
        output
    }

    fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        }
    }

    fn toggle_background(ctx: &egui::Context, app: &mut EmoteApp) {
        background_control(ctx, app, None);
    }

    fn background_control(ctx: &egui::Context, app: &mut EmoteApp, fraction: Option<f32>) {
        // Match the integration round trip's idle pass between checkbox clicks:
        // let the previous popup close before opening the menu again.
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
        // The popup needs its first sizing pass before its labels are painted.
        step(ctx, app, vec![]);
        let p = if let Some(fraction) = fraction {
            menu::slider_position("Background transparency", fraction)
        } else {
            menu::take()
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
                .expect("transparency checkbox must be painted")
        };
        menu::events(vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Primary, true),
        ]);
        step(ctx, app, vec![]);
        menu::events(vec![button(p, PointerButton::Primary, false)]);
        let output = step(ctx, app, vec![]);
        if fraction.is_some() {
            assert!(menu::checked(&menu::take(), "Transparent background"));
            menu::events(vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]);
            step(ctx, app, vec![]);
        }
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .all(|command| matches!(command, ViewportCommand::InnerSize(_))),
            "toggling transparency must not move or close the window; only the \
             deliberate framed/unframed content resize may appear"
        );
    }

    #[test]
    fn native_shadow_policy_follows_background_toggle_round_trip() {
        let ctx = egui::Context::default();
        menu::install();
        ctx.set_embed_viewports(false);
        let receiver = Receiver::bind("127.0.0.1:0".parse().unwrap(), || {}).unwrap();
        let mut app = EmoteApp::new(receiver);
        step(&ctx, &mut app, vec![]);

        // This private policy must also drive the native App::ui update after
        // show(), so the checkbox changes the native window in the same frame.
        // A headless test cannot prove the OS applied it or removed its outline.
        assert!(
            !app.native_shadow_enabled(),
            "transparent default disables the native undecorated shadow"
        );
        toggle_background(&ctx, &mut app);
        assert!(
            app.native_shadow_enabled(),
            "unchecking the default transparency restores the opaque shadow policy"
        );
        toggle_background(&ctx, &mut app);
        assert!(
            !app.native_shadow_enabled(),
            "transparent mode must disable the native undecorated shadow"
        );
        toggle_background(&ctx, &mut app);
        assert!(
            app.native_shadow_enabled(),
            "returning to opaque mode must restore its shadow policy"
        );
        toggle_background(&ctx, &mut app);
        background_control(&ctx, &mut app, Some(0.5));
        assert!(
            !app.native_shadow_enabled(),
            "partial opacity also disables native shadow"
        );
        background_control(&ctx, &mut app, Some(0.0));
        assert!(
            app.native_shadow_enabled(),
            "checked at zero is actually opaque"
        );
        background_control(&ctx, &mut app, Some(0.5));
        toggle_background(&ctx, &mut app);
        assert!(
            app.native_shadow_enabled(),
            "unchecking resets partial transparency and restores native shadow"
        );
    }
}
