//! Native presentation: the network owns publication; this app owns GPU resources.
use crate::receiver::{Frame, LatestFrame, Receiver};
use eframe::egui::{
    self, ColorImage, PointerButton, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use std::{collections::HashMap, sync::Arc};

struct AvatarTexture {
    uploaded: Arc<Frame>,
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
    tile_size: f32,
    applied_size: Vec2,
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
            tile_size: 192.0,
            applied_size: Vec2::splat(192.0),
            transparent_background: false,
            no_frame: false,
            vertical_stacking: false,
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
        !self.transparent_background
    }

    /// Shared by the native app and small embedding examples. No timer or frame
    /// queue is needed: input and the receiver's request_repaint wake eframe.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // One atomic ordered snapshot; never hold the registry lock during GPU work.
        // Inactive final avatars intentionally use the same cache entry and pixels.
        let snapshots = self.latest.snapshots();
        self.textures
            .retain(|key, _| snapshots.iter().any(|snapshot| &snapshot.session_id == key));
        for snapshot in &snapshots {
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
        let unframed = self.no_frame || self.transparent_background;
        let frame = if count > 1 && !unframed {
            Some(
                self.frame_texture
                    .get_or_insert_with(|| load_avatar_frame(ui.ctx())),
            )
        } else {
            None
        };
        let packed_layout = if unframed && count > 1 {
            let (along, across) = if vertical {
                (rect.height(), rect.width())
            } else {
                (rect.width(), rect.height())
            };
            let padding = 4.0_f32.min(along.min(across) * 0.25);
            let available = along - 2.0 * padding;
            // The gap belongs between image edges, not between virtual tiles.
            // Clamp only while a transient OS resize is too narrow for the gaps.
            // Leave positive image space even on tiny transient surfaces.
            let gap = 5.0_f32.min(available / (2.0 * count as f32 - 1.0));
            // Vertical packing reserves the composition's global top padding.
            let units = count as f32 + if vertical { 5.0 / 64.0 } else { 0.0 };
            let side = ((available - gap * (count - 1) as f32) / units)
                .min((across - 2.0 * padding) * 64.0 / 69.0)
                .max(0.0);
            Some((rect.center(), side, gap))
        } else {
            None
        };
        let radius = 8.0_f32.min(rect.width().min(rect.height()) * 0.25).round();
        // Background and textured meshes must use exactly the same card bounds.
        let card_rect = |center: egui::Pos2, side: f32, gap: f32, index: usize| {
            let (center, min, max) = if vertical {
                (center.y + side * 5.0 / 128.0, rect.top(), rect.bottom())
            } else {
                (center.x, rect.left(), rect.right())
            };
            let first = center - (side * count as f32 + gap * (count - 1) as f32) / 2.0;
            let start = if index == 0 {
                min
            } else {
                first + index as f32 * (side + gap)
            };
            let end = if index + 1 == count {
                max
            } else {
                first + index as f32 * (side + gap) + side
            };
            if vertical {
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), start),
                    egui::pos2(rect.right(), end),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(start, rect.top()),
                    egui::pos2(end, rect.bottom()),
                )
            }
        };
        if !self.transparent_background {
            let fill = egui::Color32::BLACK;
            if let Some((center, side, gap)) = packed_layout {
                for index in 0..count {
                    ui.painter()
                        .rect_filled(card_rect(center, side, gap, index), radius, fill);
                }
            } else {
                ui.painter().rect_filled(rect, 0.0, fill);
            }
        }
        for (index, snapshot) in snapshots.iter().enumerate() {
            let texture = &self.textures[&snapshot.session_id].texture;
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
            let (rect, decoration) = if let Some((center, side, gap)) = packed_layout {
                let extent = side * count as f32 + gap * (count - 1) as f32;
                let offset = -extent / 2.0 + side / 2.0 + index as f32 * (side + gap);
                let center = center
                    + if vertical {
                        Vec2::new(0.0, offset)
                    } else {
                        Vec2::new(offset, 0.0)
                    };
                (
                    egui::Rect::from_center_size(center, egui::vec2(side, side * 69.0 / 64.0)),
                    None,
                )
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
            // Five padding rows balance the sender's five black rows at its foot.
            let scale = (rect.width() / 64.0).min(rect.height() / 69.0);
            let image_rect = egui::Rect::from_min_size(
                egui::pos2(
                    rect.center().x - 32.0 * scale,
                    rect.center().y - 69.0 * scale / 2.0 + 5.0 * scale,
                ),
                Vec2::splat(64.0 * scale),
            );
            if let Some((center, side, gap)) = packed_layout {
                paint_rounded_card_image(
                    ui.painter(),
                    texture.id(),
                    image_rect,
                    card_rect(center, side, gap, index),
                    radius,
                );
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
        if snapshots.is_empty() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for emote\n127.0.0.1:8001\n\nRight-click: options / exit\nDrag: move",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
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
                    for size in [128, 192, 256, 384] {
                        let option = ui.button(format!("{size} × {size}"));
                        content_width = content_width.max(option.rect.width());
                        if option.clicked() {
                            self.tile_size = size as f32;
                            self.menu_open = false;
                        }
                    }
                    ui.separator();
                    let transparency =
                        ui.checkbox(&mut self.transparent_background, "Transparent background");
                    content_width = content_width.max(transparency.rect.width());
                    if transparency.changed() {
                        self.menu_open = false;
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
        // The waiting surface occupies one tile before any frame arrives.
        let size = if self.vertical_stacking {
            Vec2::new(self.tile_size, count as f32 * self.tile_size)
        } else {
            Vec2::new(count as f32 * self.tile_size, self.tile_size)
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
        let output = menu::take();
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
        menu::events(vec![
            Event::PointerMoved(p),
            button(p, PointerButton::Primary, true),
        ]);
        step(ctx, app, vec![]);
        menu::events(vec![button(p, PointerButton::Primary, false)]);
        let output = step(ctx, app, vec![]);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty(),
            "toggling transparency must not move, resize, or close the window"
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
            app.native_shadow_enabled(),
            "opaque default keeps its shadow"
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
    }
}
