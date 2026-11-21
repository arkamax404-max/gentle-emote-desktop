#![cfg(unix)]
use eframe::egui::{self, Event, PointerButton, Pos2, Vec2};
use gentle_emote_desktop::{
    app::EmoteApp,
    appearance::{Patch, Store},
    receiver::Receiver,
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
#[path = "support/menu.rs"]
mod menu;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!(
            "appearance-ui-{}-{sequence}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        Self(fs::canonicalize(p).unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn step(ctx: &egui::Context, app: &mut EmoteApp, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(192.0))),
            events,
            ..Default::default()
        },
        |ui| app.show(ui),
    );
    out.textures_delta.clear();
    out
}
fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Default::default(),
    }
}
fn open(ctx: &egui::Context, app: &mut EmoteApp) {
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
}
fn click(ctx: &egui::Context, app: &mut EmoteApp, label: &str) {
    let out = menu::take();
    let p = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos + Vec2::splat(4.0)),
            _ => None,
        })
        .expect("control label");
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    step(ctx, app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    step(ctx, app, vec![]);
    step(ctx, app, vec![]);
}
fn fill(out: &egui::FullOutput) -> egui::Color32 {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect.contains(Pos2::new(1.0, 1.0)) => Some(r.fill),
            _ => None,
        })
        .expect("background")
}
fn await_fill(ctx: &egui::Context, app: &mut EmoteApp, expected: egui::Color32) {
    let start = Instant::now();
    loop {
        if fill(&step(ctx, app, vec![])) == expected {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "confirmed appearance not displayed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn hold_lock(store: &Store) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let lock = store.path().with_file_name("appearance.json.lock");
    fs::create_dir(&lock).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(lock.join("owner"), b"v1:0123456789abcdef0123456789abcdef\n").unwrap();
    fs::set_permissions(lock.join("owner"), fs::Permissions::from_mode(0o600)).unwrap();
    lock
}
fn release_lock(lock: &std::path::Path) {
    fs::remove_file(lock.join("owner")).unwrap();
    fs::remove_dir(lock).unwrap();
}
fn await_panel(ctx: &egui::Context, app: &mut EmoteApp) {
    let start = Instant::now();
    loop {
        step(ctx, app, vec![]);
        if !format!("{:?}", menu::take().shapes).contains("Confirming appearance") {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(5));
    }
    step(ctx, app, vec![]);
}
fn edit_rgb_channel(ctx: &egui::Context, app: &mut EmoteApp, channel: &str, value: u8) {
    let out = menu::take();
    let p = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with(&format!("{channel} ")) => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("editable RGB picker channel");
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    step(ctx, app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    step(ctx, app, vec![]);
    let modifiers = egui::Modifiers {
        command: true,
        mac_cmd: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    menu::events(vec![
        Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        Event::Text(value.to_string()),
        Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        },
    ]);
    step(ctx, app, vec![]);
    step(ctx, app, vec![]);
}
fn setup(store: Store) -> (egui::Context, EmoteApp) {
    let ctx = egui::Context::default();
    menu::install();
    ctx.set_embed_viewports(false);
    let receiver = Receiver::bind("127.0.0.1:0".parse().unwrap(), || {}).unwrap();
    let app = EmoteApp::new(receiver).with_shared_appearance(store, ctx.clone());
    (ctx, app)
}
#[test]
fn shared_ui_loads_polls_and_only_applies_confirmed_settings() {
    let root = Scratch::new();
    let store = Store::new(root.0.join("agent")).unwrap();
    store.patch(0, Patch::Rgb([32, 128, 224])).unwrap();
    let before = fs::read(store.path()).unwrap();
    let (ctx, mut app) = setup(store.clone());
    await_fill(
        &ctx,
        &mut app,
        egui::Color32::from_rgba_unmultiplied(32, 128, 224, 140),
    );
    assert_eq!(
        fs::read(store.path()).unwrap(),
        before,
        "startup does not save"
    );
    // Public menu checkbox submits grouped opacity + checkbox change.
    open(&ctx, &mut app);
    assert!(menu::checked(&menu::take(), "Transparent background"));
    step(&ctx, &mut app, vec![]);
    let _track = menu::slider_position("Background transparency", 0.5);
    step(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, "Transparent background");
    await_fill(&ctx, &mut app, egui::Color32::from_rgb(32, 128, 224));
    assert_eq!(store.load().unwrap().revision, 2);
    assert!(!store.load().unwrap().transparent);
    store.patch(2, Patch::Rgb([10, 20, 30])).unwrap();
    await_fill(&ctx, &mut app, egui::Color32::from_rgb(10, 20, 30));
    // An invalid external source preserves the last confirmed snapshot.
    fs::write(store.path(), b"broken").unwrap();
    std::thread::sleep(Duration::from_millis(550));
    assert_eq!(
        fill(&step(&ctx, &mut app, vec![])),
        egui::Color32::from_rgb(10, 20, 30)
    );
    open(&ctx, &mut app);
    assert!(format!("{:?}", menu::take().shapes).contains("Appearance error:"));
    step(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, "Transparent background");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        fill(&step(&ctx, &mut app, vec![])),
        egui::Color32::from_rgb(10, 20, 30),
        "failed save never applies optimistic controls"
    );
    assert_eq!(fs::read(store.path()).unwrap(), b"broken");
    assert!(!root.0.join("background-opacity.txt").exists());
}

#[test]
fn shared_ui_conflict_refreshes_without_optimism_or_replay() {
    let root = Scratch::new();
    let store = Store::new(root.0.join("agent")).unwrap();
    store.patch(0, Patch::Rgb([32, 128, 224])).unwrap();
    let (ctx, mut app) = setup(store.clone());
    await_fill(
        &ctx,
        &mut app,
        egui::Color32::from_rgba_unmultiplied(32, 128, 224, 140),
    );
    open(&ctx, &mut app);
    let start = Instant::now();
    loop {
        step(&ctx, &mut app, vec![]);
        if !format!("{:?}", menu::take().shapes).contains("Confirming appearance") {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(5));
    }
    let lock = hold_lock(&store);
    step(&ctx, &mut app, vec![]);
    click(&ctx, &mut app, "Transparent background");
    assert_eq!(
        fill(&step(&ctx, &mut app, vec![])),
        egui::Color32::from_rgba_unmultiplied(32, 128, 224, 140),
        "render never applies unconfirmed checkbox proposal"
    );
    assert_eq!(store.load().unwrap().revision, 1);
    // Simulate the other cooperating writer's commit while its fixture owns lock.
    fs::write(store.path(),br#"{"version":1,"revision":2,"desktop":{"backgroundRgb":[10,20,30],"backgroundOpacity":55,"transparentBackground":true},"image":{"keep":true}}"#).unwrap();
    release_lock(&lock);
    await_fill(
        &ctx,
        &mut app,
        egui::Color32::from_rgba_unmultiplied(10, 20, 30, 140),
    );
    std::thread::sleep(Duration::from_millis(550));
    step(&ctx, &mut app, vec![]);
    assert_eq!(
        store.load().unwrap().revision,
        2,
        "conflict must not replay patch"
    );
    assert!(store.load().unwrap().transparent);
    open(&ctx, &mut app);
    std::thread::sleep(Duration::from_millis(50));
    step(&ctx, &mut app, vec![]);
    let out = menu::take();
    assert!(
        format!("{:?}", out.shapes).contains("Appearance conflict:"),
        "panel reload must not erase conflict notice"
    );
    assert!(menu::checked(&out, "Transparent background"));
}

#[test]
fn shared_ui_rgb_picker_and_slider_submit_only_confirmed_patches() {
    let root = Scratch::new();
    let store = Store::new(root.0.join("agent")).unwrap();
    store.patch(0, Patch::Rgb([0, 0, 0])).unwrap();
    fs::write(store.path(), br#"{"version":1,"revision":1,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"image":{"keep":true},"future":7}"#).unwrap();
    let (ctx, mut app) = setup(store.clone());
    await_fill(&ctx, &mut app, egui::Color32::from_black_alpha(140));
    open(&ctx, &mut app);
    await_panel(&ctx, &mut app);
    click(&ctx, &mut app, "Background color");
    let mut rgb = [0; 3];
    for (index, channel, value) in [(0, "R", 32), (1, "G", 128), (2, "B", 224)] {
        let before = fs::read(store.path()).unwrap();
        let revision = store.load().unwrap().revision;
        let lock = hold_lock(&store);
        edit_rgb_channel(&ctx, &mut app, channel, value);
        assert_eq!(
            fill(&step(&ctx, &mut app, vec![])),
            egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 140),
            "picker proposal must not apply before confirmation"
        );
        assert_eq!(fs::read(store.path()).unwrap(), before);
        release_lock(&lock);
        rgb[index] = value;
        await_fill(
            &ctx,
            &mut app,
            egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 140),
        );
        let saved = store.load().unwrap();
        assert_eq!(
            (saved.revision, saved.rgb, saved.opacity, saved.transparent),
            (revision + 1, rgb, 55, true)
        );
        let bytes = fs::read_to_string(store.path()).unwrap();
        assert!(bytes.contains("\"image\":{\"keep\":true}"));
        assert!(bytes.contains("\"future\":7"));
        assert!(
            bytes.contains("\"backgroundOpacity\":55"),
            "RGB patch preserves required opacity"
        );
        assert!(
            bytes.contains("\"transparentBackground\":true"),
            "RGB patch preserves required checkbox"
        );
    }
    let before = fs::read(store.path()).unwrap();
    let revision = store.load().unwrap().revision;
    let lock = hold_lock(&store);
    let p = menu::slider_position("Background transparency", 0.0);
    menu::events(vec![
        Event::PointerMoved(p),
        button(p, PointerButton::Primary, true),
    ]);
    step(&ctx, &mut app, vec![]);
    menu::events(vec![button(p, PointerButton::Primary, false)]);
    step(&ctx, &mut app, vec![]);
    assert_eq!(
        fill(&step(&ctx, &mut app, vec![])),
        egui::Color32::from_rgba_unmultiplied(32, 128, 224, 140)
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
    release_lock(&lock);
    await_fill(&ctx, &mut app, egui::Color32::from_rgb(32, 128, 224));
    let saved = store.load().unwrap();
    assert_eq!(
        (saved.revision, saved.rgb, saved.opacity, saved.transparent),
        (revision + 1, rgb, 100, true)
    );
    assert!(menu::checked(&menu::take(), "Transparent background"));
    drop(app);
    let before = fs::read(store.path()).unwrap();
    let (ctx, mut app) = setup(store.clone());
    await_fill(&ctx, &mut app, egui::Color32::from_rgb(32, 128, 224));
    open(&ctx, &mut app);
    await_panel(&ctx, &mut app);
    assert!(menu::checked(&menu::take(), "Transparent background"));
    assert_eq!(
        fs::read(store.path()).unwrap(),
        before,
        "reconstruction must not write"
    );
}
