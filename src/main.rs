use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    receiver::Receiver,
};

fn main() -> eframe::Result {
    eframe::run_native(
        "Gentle Emote",
        native_options(),
        Box::new(|cc| {
            // Context is a cheap thread-safe handle. The callback only wakes eframe;
            // it never owns/drops the server or waits for GUI work to complete.
            let ctx = cc.egui_ctx.clone();
            let receiver = Receiver::start(move || ctx.request_repaint())?;
            // App creation propagates bind errors to run_native and main exits
            // nonzero. Window/backend errors follow the same path; no port fallback.
            Ok(Box::new(EmoteApp::new(receiver)))
        }),
    )
}
