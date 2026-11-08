//! Explicit native process regression: never launch a GUI implicitly in CI.
#![cfg(target_os = "macos")]

use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// Even an assertion failure or timeout must kill and reap the child. Keeping the
// guard armed until try_wait observes exit prevents stray native windows/processes.
struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
#[ignore = "requires a macOS graphical session and free port 8001"]
fn production_busy_port_exits_with_error() {
    let listener = TcpListener::bind("127.0.0.1:8001")
        .expect("native startup test requires port 127.0.0.1:8001 to be free initially");
    let mut child = ChildGuard(Some(
        Command::new(env!("CARGO_BIN_EXE_gentle-emote-desktop"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("launch production executable in a macOS graphical session"),
    ));
    let started = Instant::now();
    loop {
        if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "production executable did not exit within 20 seconds on occupied port 8001"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.0.take().unwrap().wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"");
    assert_eq!(
        output.stderr,
        b"Error: AppCreation(Os { code: 48, kind: AddrInUse, message: \"Address already in use\" })\n"
    );
    drop(listener);
    let _released =
        TcpListener::bind("127.0.0.1:8001").expect("occupied-port test must release its listener");
}
