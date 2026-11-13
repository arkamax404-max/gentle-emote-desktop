use eframe::egui;
use std::cell::RefCell;

thread_local! {
    static NEXT: RefCell<Option<egui::RawInput>> = const { RefCell::new(None) };
    static PAINTED: RefCell<Option<egui::FullOutput>> = const { RefCell::new(None) };
}

// egui's immediate renderer hook is thread-local, like this test-only adapter.
// Run the child on the same Context to exercise actual viewport command routing.
pub fn install() {
    NEXT.with(|next| *next.borrow_mut() = None);
    PAINTED.with(|painted| *painted.borrow_mut() = None);
    egui::Context::set_immediate_viewport_renderer(|ctx, mut viewport| {
        let mut raw = NEXT
            .with(|next| next.borrow_mut().take())
            .unwrap_or_default();
        raw.viewport_id = viewport.ids.this;
        raw.screen_rect.get_or_insert_with(|| {
            egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                viewport.builder.inner_size.expect("independent menu size"),
            )
        });
        raw.viewports.entry(viewport.ids.this).or_default().parent = Some(viewport.ids.parent);
        let mut output = ctx.run_ui(raw, |ui| (viewport.viewport_ui_cb)(ui));
        output.textures_delta.clear();
        PAINTED.with(|painted| *painted.borrow_mut() = Some(output));
    });
}

pub fn raw(input: egui::RawInput) {
    NEXT.with(|next| *next.borrow_mut() = Some(input));
}

pub fn events(events: Vec<egui::Event>) {
    raw(egui::RawInput {
        events,
        ..Default::default()
    });
}

pub fn take_optional() -> Option<egui::FullOutput> {
    PAINTED.with(|painted| painted.borrow_mut().take())
}

pub fn take() -> egui::FullOutput {
    take_optional().expect("independent menu must render")
}
