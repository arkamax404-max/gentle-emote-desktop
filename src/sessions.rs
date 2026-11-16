//! Bounded session lifecycle state. Callers serialize mutations and notify after unlocking.
use crate::receiver::Frame;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

pub const SESSION_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_ACTIVE_SESSIONS: usize = 32;
pub const MAX_KNOWN_SESSIONS: usize = 64;
pub const MAX_RETIRED_STREAMS: usize = 16;

/// Sender-declared presentation state carried on the latest accepted frame.
/// `Clock` renders the desktop's internal pixel clock in that session's tile;
/// any ordinary image state (and every unknown/absent metadata value, which
/// the receiver maps here) renders the received pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Normal,
    Clock,
}

/// Visible avatars, in first-seen order. `None` identifies the legacy single slot.
/// An inactive snapshot is the retained final avatar, not a live lease.
#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub session_id: Option<String>,
    pub frame: Arc<Frame>,
    pub active: bool,
    pub state: SessionState,
}

pub enum Event {
    Frame(Arc<Frame>, SessionState),
    Heartbeat,
    Close,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CapacityError;

struct Session {
    id: String,
    stream: String,
    sequence: u64,
    retired: VecDeque<String>,
    frame: Arc<Frame>,
    state: SessionState,
    active: bool,
    closed: bool,
    seen: Instant,
}

/// Pure monotonic-time state machine; networking supplies validated IDs and decoded frames.
/// A single instance lives behind the receiver's single publication mutex.
#[derive(Default)]
pub struct SessionRegistry {
    sessions: Vec<Session>,
    legacy: Option<Arc<Frame>>,
    legacy_state: SessionState,
    latest: Option<Arc<Frame>>,
    retained: Option<SessionSnapshot>,
    clock_after_close: bool,
}
impl SessionRegistry {
    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.latest.clone()
    }

    /// Presentation switches to a clock only after the final active avatar closes.
    /// Compatibility snapshots and the latest image remain retained independently.
    pub fn show_clock(&self) -> bool {
        self.clock_after_close
    }

    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        let mut snapshots = Vec::new();
        if let Some(frame) = &self.legacy {
            snapshots.push(SessionSnapshot {
                session_id: None,
                frame: frame.clone(),
                active: true,
                state: self.legacy_state,
            });
        }
        snapshots.extend(
            self.sessions
                .iter()
                .filter(|s| s.active)
                .map(|s| SessionSnapshot {
                    session_id: Some(s.id.clone()),
                    frame: s.frame.clone(),
                    active: true,
                    state: s.state,
                }),
        );
        if snapshots.is_empty() {
            snapshots.extend(self.retained.clone());
        }
        snapshots
    }

    pub fn publish_legacy(&mut self, frame: Arc<Frame>, state: SessionState) {
        self.latest = Some(frame.clone());
        self.legacy = Some(frame);
        self.legacy_state = state;
        self.retained = None;
        self.clock_after_close = false;
    }

    /// Returns whether visible snapshots changed. Stale, closed and unknown metadata
    /// requests are successful no-ops, including their sequence and lease timestamps.
    pub fn apply(
        &mut self,
        id: &str,
        stream: &str,
        sequence: u64,
        event: Event,
        now: Instant,
    ) -> Result<bool, CapacityError> {
        let index = self.sessions.iter().position(|s| s.id == id);
        let is_frame = matches!(event, Event::Frame(..));
        if let Some(i) = index {
            let s = &self.sessions[i];
            if s.retired.iter().any(|old| old == stream)
                || (s.stream == stream && (s.closed || sequence <= s.sequence))
                || (s.stream != stream && !is_frame)
            {
                return Ok(false);
            }
        } else if !is_frame {
            return Ok(false);
        }
        let activates = !matches!(event, Event::Close);
        if activates
            && index.is_none_or(|i| !self.sessions[i].active)
            && self.sessions.iter().filter(|s| s.active).count() >= MAX_ACTIVE_SESSIONS
        {
            return Err(CapacityError);
        }
        let i = match index {
            Some(i) => i,
            None => {
                if self.sessions.len() == MAX_KNOWN_SESSIONS {
                    // Evict the least recently communicating inactive identity only.
                    let victim = self
                        .sessions
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| !s.active)
                        .min_by_key(|(_, s)| s.seen)
                        .map(|(i, _)| i)
                        .ok_or(CapacityError)?;
                    self.sessions.remove(victim);
                }
                let Event::Frame(frame, _) = &event else {
                    unreachable!()
                };
                self.sessions.push(Session {
                    id: id.into(),
                    stream: stream.into(),
                    sequence,
                    retired: VecDeque::new(),
                    frame: frame.clone(),
                    state: SessionState::Normal,
                    active: false,
                    closed: false,
                    seen: now,
                });
                self.sessions.len() - 1
            }
        };
        let s = &mut self.sessions[i];
        if s.stream != stream {
            s.retired
                .push_back(std::mem::replace(&mut s.stream, stream.into()));
            if s.retired.len() > MAX_RETIRED_STREAMS {
                s.retired.pop_front();
            }
            s.closed = false;
        }
        s.sequence = sequence;
        s.seen = now;
        let changed = match event {
            Event::Frame(frame, state) => {
                s.frame = frame.clone();
                s.state = state;
                self.latest = Some(frame);
                true
            }
            Event::Heartbeat => !s.active,
            Event::Close => s.active,
        };
        s.active = activates;
        s.closed = !activates;
        if activates {
            self.clock_after_close = false;
            self.retained = None;
        } else if changed {
            self.retained = Some(SessionSnapshot {
                session_id: Some(s.id.clone()),
                frame: s.frame.clone(),
                active: false,
                state: s.state,
            });
        }
        if !activates && changed {
            self.clock_after_close =
                self.legacy.is_none() && !self.sessions.iter().any(|s| s.active);
        }
        Ok(changed)
    }

    /// Production calls this independently of image traffic; tests inject Instants.
    pub fn expire(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for s in &mut self.sessions {
            if s.active && now.saturating_duration_since(s.seen) >= SESSION_TIMEOUT {
                s.active = false;
                self.retained = Some(SessionSnapshot {
                    session_id: Some(s.id.clone()),
                    frame: s.frame.clone(),
                    active: false,
                    state: s.state,
                });
                changed = true;
            }
        }
        if changed {
            self.clock_after_close = false;
        }
        changed
    }
}
