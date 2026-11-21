//! Native shared-store worker. Render only uses nonblocking channel operations.
use crate::appearance::{Patch, Snapshot, Status, Store};
use std::{
    io,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

enum Command {
    Reload,
    Patch(u64, Patch),
}
pub(crate) struct Event {
    pub snapshot: Option<Snapshot>,
    pub error: Option<String>,
    pub completion: bool,
    pub clear_error: bool,
}
pub(crate) struct Worker {
    commands: Option<mpsc::SyncSender<Command>>,
    events: Option<mpsc::Receiver<Event>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn start(store: Store, ctx: eframe::egui::Context) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(2);
        // Backpressure bounds memory if the native UI stops rendering. Drop
        // disconnects this queue before joining, waking a blocked worker send.
        let (events_tx, events_rx) = mpsc::sync_channel(4);
        let handle = thread::Builder::new()
            .name("appearance-store".into())
            .spawn(move || {
                let mut previous = None;
                let mut previous_error = None;
                let mut deadline = Instant::now();
                loop {
                    let command =
                        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                            Ok(cmd) => Some(cmd),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };
                    let completion = command.is_some();
                    let saving = matches!(command, Some(Command::Patch(..)));
                    let report = match command {
                        Some(Command::Patch(revision, patch)) => {
                            store.patch_report(revision, patch)
                        }
                        _ => store.load_report(),
                    };
                    let warning = report.warning();
                    let clear_error = saving && report.status == Status::Saved && warning.is_none();
                    let event = if let Some(snapshot) = report.snapshot {
                        let changed =
                            previous.as_ref() != Some(&snapshot) || previous_error.is_some();
                        previous = Some(snapshot.clone());
                        previous_error = None;
                        (completion || changed).then_some(Event {
                            snapshot: Some(snapshot),
                            error: warning,
                            completion,
                            clear_error,
                        })
                    } else {
                        let message = warning.unwrap_or_else(|| {
                            format!("Appearance error: {}", report.status.code())
                        });
                        let changed = previous_error.as_ref() != Some(&message);
                        previous_error = Some(message.clone());
                        (completion || changed).then_some(Event {
                            snapshot: None,
                            error: Some(message),
                            completion,
                            clear_error: false,
                        })
                    };
                    if let Some(event) = event {
                        // Wake before and after send: a full queue must not hide its
                        // own backpressure from an event-driven native renderer.
                        ctx.request_repaint();
                        if events_tx.send(event).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    if Instant::now() >= deadline {
                        deadline = Instant::now() + Duration::from_millis(500);
                    }
                }
            })?;
        Ok(Self {
            commands: Some(tx),
            events: Some(events_rx),
            thread: Some(handle),
        })
    }
    pub fn reload(&self) -> bool {
        self.commands
            .as_ref()
            .is_some_and(|tx| tx.try_send(Command::Reload).is_ok())
    }
    pub fn patch(&self, revision: u64, patch: Patch) -> bool {
        self.commands
            .as_ref()
            .is_some_and(|tx| tx.try_send(Command::Patch(revision, patch)).is_ok())
    }
    pub fn drain(&self) -> impl Iterator<Item = Event> + '_ {
        self.events
            .as_ref()
            .into_iter()
            .flat_map(|rx| rx.try_iter())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        drop(self.commands.take());
        drop(self.events.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
