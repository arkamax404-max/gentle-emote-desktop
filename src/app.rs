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
    tile_size: f32,
    applied_size: Vec2,
    transparent_background: bool,
    #[cfg(target_os = "windows")]
    applied_native_shadow: Option<bool>,
}
impl EmoteApp {
    pub fn new(receiver: Receiver) -> Self {
        Self {
            latest: receiver.latest(),
            _receiver: receiver,
            textures: HashMap::new(),
            tile_size: 192.0,
            applied_size: Vec2::splat(192.0),
            transparent_background: false,
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
        let rect = ui.max_rect();
        // One full-window hit target lets the image and placeholder share exactly
        // the same controls. Painting text does not steal drag/menu interactions.
        let response = ui.interact(
            rect,
            ui.scope_id().with("emote-surface"),
            egui::Sense::click_and_drag(),
        );
        // The window supports alpha, but remains opaque black until selected.
        // Only the surface changes: opaque black pixels in the PNG stay black.
        if !self.transparent_background {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        }
        for (index, snapshot) in snapshots.iter().enumerate() {
            let texture = &self.textures[&snapshot.session_id].texture;
            // Divide the full surface into neighboring tiles, preserving the single
            // avatar's fit even while the OS is processing a resize request.
            let rect = egui::Rect::from_min_size(
                rect.min + Vec2::new(index as f32 * rect.width() / count as f32, 0.0),
                Vec2::new(rect.width() / count as f32, rect.height()),
            );
            let rect = if count > 1 {
                let stroke_width = 3.0_f32.min(rect.width().min(rect.height()) * 0.5);
                let edge = rect.shrink(stroke_width * 0.5);
                let light = egui::Stroke::new(stroke_width, egui::Color32::from_gray(160));
                let dark = egui::Stroke::new(stroke_width, egui::Color32::from_gray(64));
                for (points, stroke) in [
                    ([edge.left_top(), edge.right_top()], light),
                    ([edge.left_top(), edge.left_bottom()], light),
                    ([edge.right_top(), edge.right_bottom()], dark),
                    ([edge.left_bottom(), edge.right_bottom()], dark),
                ] {
                    ui.painter().line_segment(points, stroke);
                }
                // Keep narrow tiles valid while the OS applies a wider viewport.
                rect.shrink(4.0_f32.min(rect.width().min(rect.height()) * 0.25))
            } else {
                rect
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
            ui.painter().image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
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
        response.context_menu(|ui| {
            for size in [128, 192, 256, 384] {
                if ui.button(format!("{size} × {size}")).clicked() {
                    self.tile_size = size as f32;
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .checkbox(&mut self.transparent_background, "Transparent background")
                .changed()
            {
                ui.ctx().request_repaint();
                ui.close();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                ui.close();
            }
        });
        // Resize only for a changed count/selection, not every repaint or heartbeat.
        // The waiting surface occupies one tile before any frame arrives.
        let size = Vec2::new(count as f32 * self.tile_size, self.tile_size);
        if size != self.applied_size {
            ui.ctx().send_viewport_cmd(ViewportCommand::InnerSize(size));
            self.applied_size = size;
        }
        // Secondary dragging belongs to the context menu, never to window motion.
        // The popup owns its pointer presses, so selecting an item cannot move us.
        if response.drag_started_by(PointerButton::Primary) && !response.context_menu_opened() {
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
            "toggling transparency must not move, resize, or close the window"
        );
    }

    #[test]
    fn native_shadow_policy_follows_background_toggle_round_trip() {
        let ctx = egui::Context::default();
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
