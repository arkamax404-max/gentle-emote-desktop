//! Native presentation: the network owns publication; this app owns GPU resources.
use crate::receiver::{Frame, LatestFrame, Receiver};
use eframe::egui::{
    self, ColorImage, PointerButton, TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use std::sync::Arc;

pub fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::splat(192.0))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(false)
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
    uploaded: Option<Arc<Frame>>,
    texture: Option<TextureHandle>,
}
impl EmoteApp {
    pub fn new(receiver: Receiver) -> Self {
        Self {
            latest: receiver.latest(),
            _receiver: receiver,
            uploaded: None,
            texture: None,
        }
    }

    /// Shared by the native app and small embedding examples. No timer or frame
    /// queue is needed: input and the receiver's request_repaint wake eframe.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // Option means "nothing received yet"; cloning an Arc keeps a snapshot
        // alive without copying its pixels or holding the receiver's mutex.
        if let Some(frame) = self.latest.get() {
            let changed = self
                .uploaded
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &frame));
            if changed {
                // PNG pixels are unpremultiplied. egui converts them for blending;
                // NEAREST prevents interpolating adjacent pixel-art texels.
                let image = ColorImage::from_rgba_unmultiplied([64, 64], &frame.rgba);
                if let Some(texture) = &mut self.texture {
                    texture.set(image, TextureOptions::NEAREST);
                } else {
                    self.texture = Some(ui.ctx().load_texture(
                        "emote",
                        image,
                        TextureOptions::NEAREST,
                    ));
                }
                self.uploaded = Some(frame);
            }
        }
        let rect = ui.max_rect();
        // One full-window hit target lets the image and placeholder share exactly
        // the same controls. Painting text does not steal drag/menu interactions.
        let response = ui.interact(
            rect,
            ui.scope_id().with("emote-surface"),
            egui::Sense::click_and_drag(),
        );
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
        if let Some(texture) = &self.texture {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for emote\n127.0.0.1:8001\n\nRight-click: size / exit\nDrag: move",
                egui::FontId::proportional(12.0),
                ui.visuals().text_color(),
            );
        }
        response.context_menu(|ui| {
            for size in [128, 192, 256, 384] {
                if ui.button(format!("{size} × {size}")).clicked() {
                    // Viewport commands ask the OS to act, unlike changing an
                    // egui layout size. Logical pixels follow the display scale.
                    ui.ctx()
                        .send_viewport_cmd(ViewportCommand::InnerSize(Vec2::splat(size as f32)));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                ui.close();
            }
        });
        // Secondary dragging belongs to the context menu, never to window motion.
        // The popup owns its pointer presses, so selecting an item cannot move us.
        if response.drag_started_by(PointerButton::Primary) && !response.context_menu_opened() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }
}
impl eframe::App for EmoteApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}
