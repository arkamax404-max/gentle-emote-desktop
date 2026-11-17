//! Bounded session lifecycle state. Callers serialize mutations and notify after unlocking.
use crate::receiver::Frame;
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

pub const SESSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Accepted frames and heartbeats are the only contact signals, so a clock tile
/// stops presenting its project name well before the lifecycle lease expires.
/// Strictly greater than this duration counts as silent; legacy senders have no
/// pulse protocol and are never silent.
pub const CLOCK_SILENCE_TIMEOUT: Duration = Duration::from_secs(15);
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

/// One visible clock tile: the snapshot to draw, the sender's project name as
/// untouched metadata, and whether that name may still be presented.
///
/// The substitute title is presentation copy and belongs to the renderer; this
/// type reports the silence rule only, so no copy lives in the domain.
#[derive(Clone, Debug)]
pub struct ClockTile {
    pub snapshot: SessionSnapshot,
    pub name: Option<String>,
    pub stale: bool,
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
    project_name: Option<String>,
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
    legacy_project_name: Option<String>,
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

    /// Names and frames come from the same serialized registry observation.
    /// This is raw metadata: no contact filtering happens here, so presentation
    /// code reads `clock_tiles_at` instead.
    pub fn snapshots_with_names(&self) -> Vec<(SessionSnapshot, Option<String>)> {
        self.snapshots()
            .into_iter()
            .map(|snapshot| {
                let name = match snapshot.session_id.as_deref() {
                    Some(id) => self
                        .sessions
                        .iter()
                        .find(|s| s.id == id)
                        .and_then(|s| s.project_name.clone()),
                    None => self.legacy_project_name.clone(),
                };
                (snapshot, name)
            })
            .collect()
    }

    /// Clock presentation only: report silent sessions without changing metadata
    /// or the independent 30-second lifecycle lease. Accepted frames and heartbeats
    /// both count as contact; a replayed pulse renews neither.
    pub fn clock_tiles_at(&self, now: Instant) -> Vec<ClockTile> {
        // One pass over the sessions, so the rule stays linear in the visible tiles.
        let silent: HashSet<&str> = self
            .sessions
            .iter()
            .filter(|session| now.saturating_duration_since(session.seen) > CLOCK_SILENCE_TIMEOUT)
            .map(|session| session.id.as_str())
            .collect();
        self.snapshots_with_names()
            .into_iter()
            .map(|(snapshot, name)| {
                let stale = snapshot.state == SessionState::Clock
                    && snapshot
                        .session_id
                        .as_deref()
                        .is_some_and(|id| silent.contains(id));
                ClockTile {
                    snapshot,
                    name,
                    stale,
                }
            })
            .collect()
    }

    pub fn publish_legacy(&mut self, frame: Arc<Frame>, state: SessionState) {
        self.publish_legacy_named(frame, state, None);
    }

    pub(crate) fn publish_legacy_named(
        &mut self,
        frame: Arc<Frame>,
        state: SessionState,
        project_name: Option<String>,
    ) {
        self.latest = Some(frame.clone());
        self.legacy = Some(frame);
        self.legacy_state = state;
        self.legacy_project_name = project_name;
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
        self.apply_named(id, stream, sequence, event, None, now)
    }

    // The receiver supplies bounded, validated image metadata. Keeping this
    // entry point crate-private preserves the original public event contract.
    pub(crate) fn apply_named(
        &mut self,
        id: &str,
        stream: &str,
        sequence: u64,
        event: Event,
        project_name: Option<String>,
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
                    project_name: None,
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
                s.project_name = project_name;
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

#[cfg(test)]
mod clock_title_tests {
    use super::*;

    #[test]
    fn named_clock_reports_silence_without_losing_its_metadata() {
        let mut registry = SessionRegistry::default();
        let now = Instant::now();
        let frame = Arc::new(Frame {
            rgba: vec![0; 16384],
        });
        registry
            .apply_named(
                "clock",
                "s",
                1,
                Event::Frame(frame.clone(), SessionState::Clock),
                Some("ProjectAlpha".into()),
                now,
            )
            .unwrap();
        registry
            .apply_named(
                "normal",
                "s",
                1,
                Event::Frame(frame.clone(), SessionState::Normal),
                Some("Other".into()),
                now,
            )
            .unwrap();
        registry.publish_legacy_named(frame, SessionState::Clock, Some("Legacy".into()));
        let titles = |registry: &SessionRegistry, at| {
            registry
                .clock_tiles_at(at)
                .into_iter()
                .map(|tile| (tile.name.unwrap_or_default(), tile.stale))
                .collect::<Vec<_>>()
        };
        let fresh = |name: &str| (name.to_owned(), false);
        let silent = |name: &str| (name.to_owned(), true);
        assert_eq!(
            titles(&registry, now + CLOCK_SILENCE_TIMEOUT),
            [fresh("Legacy"), fresh("ProjectAlpha"), fresh("Other")]
        );
        let stale = now + CLOCK_SILENCE_TIMEOUT + Duration::from_nanos(1);
        assert_eq!(
            titles(&registry, stale),
            [fresh("Legacy"), silent("ProjectAlpha"), fresh("Other")]
        );
        assert!(registry.snapshots().iter().all(|snapshot| snapshot.active));
        assert_eq!(
            registry.snapshots_with_names()[1].1.as_deref(),
            Some("ProjectAlpha")
        );
        registry
            .apply("clock", "s", 2, Event::Heartbeat, stale)
            .unwrap();
        assert_eq!(
            titles(&registry, stale),
            [fresh("Legacy"), fresh("ProjectAlpha"), fresh("Other")]
        );
    }
}
