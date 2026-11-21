#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gentle_emote_desktop::{
    app::{EmoteApp, native_options},
    appearance::{Store, resolve_agent_dir},
    receiver::Receiver,
};

const HELP: &str = "Gentle Emote Desktop\n\nUsage: gentle-emote-desktop [--agent-dir ABSOLUTE]\n\nShared appearance: --agent-dir > PI_CODING_AGENT_DIR > HOME/.pi/agent\n--agent-dir ABSOLUTE  Select the shared agent directory (never a project path).\n-h, --help            Print help without starting the GUI or receiver.\n";

fn arguments() -> Result<Option<Store>, String> {
    let mut root = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print!("{HELP}");
            return Ok(None);
        }
        if arg != "--agent-dir" {
            return Err("unknown argument".into());
        }
        if root.is_some() {
            return Err("--agent-dir may be specified only once".into());
        }
        root = Some(args.next().ok_or("--agent-dir requires an absolute path")?);
    }
    let env = std::env::var_os("PI_CODING_AGENT_DIR");
    let home = std::env::var_os("HOME");
    let path = resolve_agent_dir(root.as_deref(), env.as_deref(), home.as_deref())
        .map_err(|e| e.to_string())?;
    Store::new(path).map(Some).map_err(|e| e.to_string())
}

fn main() -> eframe::Result {
    // Parse before native setup: help and invalid roots never bind the receiver,
    // initialize a renderer, or access any preference file.
    let store = match arguments() {
        Ok(Some(store)) => store,
        Ok(None) => return Ok(()),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(2);
        }
    };
    eframe::run_native(
        "Gentle Emote",
        native_options(),
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let receiver = Receiver::start(move || ctx.request_repaint())?;
            Ok(Box::new(
                EmoteApp::new(receiver).with_shared_appearance(store, cc.egui_ctx.clone()),
            ))
        }),
    )
}
