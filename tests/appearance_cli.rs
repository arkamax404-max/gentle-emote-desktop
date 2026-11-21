use std::process::Command;

const HELP: &str = "Gentle Emote Desktop\n\nUsage: gentle-emote-desktop [--agent-dir ABSOLUTE]\n\nShared appearance: --agent-dir > PI_CODING_AGENT_DIR > HOME/.pi/agent\n--agent-dir ABSOLUTE  Select the shared agent directory (never a project path).\n-h, --help            Print help without starting the GUI or receiver.\n";

#[test]
fn help_and_invalid_arguments_exit_before_native_startup() {
    for args in [vec!["--help"], vec!["-h"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_gentle-emote-desktop"))
            .args(args)
            .env("PI_CODING_AGENT_DIR", "relative")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, HELP.as_bytes());
        assert_eq!(out.stderr, b"");
    }
    for (args, message) in [
        (vec!["--agent-dir"], "--agent-dir requires an absolute path"),
        (
            vec!["--agent-dir", ""],
            "agent directory must be absolute and nonempty",
        ),
        (
            vec!["--agent-dir", "relative"],
            "agent directory must be absolute and nonempty",
        ),
        (vec!["--unknown"], "unknown argument"),
        (
            vec!["--agent-dir", "/one", "--agent-dir", "/two"],
            "--agent-dir may be specified only once",
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_gentle-emote-desktop"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(out.stdout, b"");
        assert_eq!(out.stderr, format!("Error: {message}\n").as_bytes());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_gentle-emote-desktop"))
        .env("PI_CODING_AGENT_DIR", "relative")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stdout, b"");
    assert_eq!(
        out.stderr,
        b"Error: agent directory must be absolute and nonempty\n"
    );
}
