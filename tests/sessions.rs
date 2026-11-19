use gentle_emote_desktop::{
    receiver::Frame,
    sessions::{
        CLOCK_SILENCE_TIMEOUT, Event, MAX_ACTIVE_SESSIONS, MAX_KNOWN_SESSIONS, MAX_RETIRED_STREAMS,
        SESSION_TIMEOUT, SessionRegistry, SessionState,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn frame(value: u8) -> Event {
    Event::Frame(
        Arc::new(Frame {
            rgba: vec![value; 16384],
        }),
        SessionState::Normal,
    )
}

fn clock_frame(value: u8) -> Event {
    Event::Frame(
        Arc::new(Frame {
            rgba: vec![value; 16384],
        }),
        SessionState::Clock,
    )
}

#[test]
fn ordered_isolation_retention_reactivation_and_close() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    assert!(!r.apply("unknown", "s", 1, Event::Heartbeat, now).unwrap());
    assert!(r.snapshots().is_empty());
    r.apply("b", "s", 1, frame(1), now).unwrap();
    r.apply("a", "s", 1, frame(2), now).unwrap();
    let first = r.snapshots()[0].frame.clone();
    r.apply("a", "s", 2, frame(3), now).unwrap();
    let snapshots = r.snapshots();
    assert_eq!(
        snapshots
            .iter()
            .map(|s| s.session_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert!(Arc::ptr_eq(&first, &snapshots[0].frame));
    assert_eq!(snapshots[1].frame.rgba[0], 3);
    assert!(!r.expire(now + SESSION_TIMEOUT - Duration::from_nanos(1)));
    assert!(r.expire(now + SESSION_TIMEOUT));
    assert!(!r.expire(now + SESSION_TIMEOUT));
    assert_eq!(r.snapshots().len(), 1);
    assert!(!r.snapshots()[0].active);
    assert_eq!(r.snapshots()[0].session_id.as_deref(), Some("a"));
    assert!(
        !r.apply("a", "s", 2, Event::Heartbeat, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(
        r.apply("a", "s", 3, Event::Heartbeat, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(r.snapshots()[0].active);
    assert_eq!(r.snapshots()[0].frame.rgba[0], 3);
    assert!(
        r.apply("a", "s", 4, Event::Close, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(
        !r.apply("a", "s", 5, Event::Heartbeat, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(
        !r.apply("a", "s", 6, frame(6), now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(!r.snapshots()[0].active);
    r.apply("a", "new", 0, frame(7), now + SESSION_TIMEOUT)
        .unwrap();
    assert!(
        !r.apply("a", "s", u64::MAX, frame(8), now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert_eq!(r.snapshots()[0].frame.rgba[0], 7);
    r.apply("a", "new", 1, Event::Close, now + SESSION_TIMEOUT)
        .unwrap();
    r.apply("c", "s", 0, frame(9), now + SESSION_TIMEOUT)
        .unwrap();
    assert_eq!(r.snapshots().len(), 1);
    assert_eq!(r.snapshots()[0].session_id.as_deref(), Some("c"));
}

#[test]
fn heartbeats_renew_without_image_changes_and_expiry_is_independent() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    r.apply("idle", "s", 0, frame(1), now).unwrap();
    let saved = r.latest().unwrap();
    for n in 1..=12 {
        let at = now + Duration::from_secs(n * 5);
        assert!(!r.apply("idle", "s", n, Event::Heartbeat, at).unwrap());
        assert!(!r.expire(at));
        assert!(Arc::ptr_eq(&saved, &r.latest().unwrap()));
    }
    // Stale metadata must not extend the lease.
    assert!(
        !r.apply(
            "idle",
            "s",
            12,
            Event::Heartbeat,
            now + Duration::from_secs(89)
        )
        .unwrap()
    );
    assert!(r.expire(now + Duration::from_secs(90)));
    assert!(!r.snapshots()[0].active);
    assert!(
        r.apply("idle", "s", 13, frame(2), now + Duration::from_secs(91))
            .unwrap()
    );
    assert!(r.snapshots()[0].active);
}

#[test]
fn bounded_active_and_inactive_identities_and_finite_replay_horizon() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    for n in 0..MAX_ACTIVE_SESSIONS {
        r.apply(&n.to_string(), "s", 0, frame(1), now).unwrap();
    }
    let saved = r.latest().unwrap();
    assert!(r.apply("excess", "s", 0, frame(2), now).is_err());
    assert!(Arc::ptr_eq(&saved, &r.latest().unwrap()));
    assert_eq!(r.snapshots().len(), MAX_ACTIVE_SESSIONS);
    r.expire(now + SESSION_TIMEOUT);
    for n in MAX_ACTIVE_SESSIONS..=MAX_KNOWN_SESSIONS {
        let at = now + Duration::from_secs(31 + n as u64);
        r.apply(&n.to_string(), "s", 0, frame(3), at).unwrap();
        r.apply(&n.to_string(), "s", 1, Event::Close, at).unwrap();
    }
    // Oldest inactive session was evicted: even a lower sequence can start again.
    assert!(
        r.apply("0", "s", 0, frame(4), now + Duration::from_secs(200))
            .unwrap()
    );
    for n in 0..=MAX_RETIRED_STREAMS {
        r.apply(
            "0",
            &format!("new{n}"),
            0,
            frame(5),
            now + Duration::from_secs(201),
        )
        .unwrap();
    }
    assert!(
        !r.apply("0", "new0", 1, frame(6), now + Duration::from_secs(202))
            .unwrap()
    );
    assert!(
        r.apply("0", "s", 0, frame(7), now + Duration::from_secs(203))
            .unwrap()
    );
}

#[test]
fn clock_requires_final_active_close_and_new_frames_restore_avatars() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    assert!(!r.show_clock());
    assert!(r.snapshots().is_empty());
    assert!(!r.apply("unknown", "s", 1, Event::Close, now).unwrap());
    assert!(!r.show_clock());
    r.apply("a", "s", 1, frame(1), now).unwrap();
    r.apply("b", "s", 1, frame(2), now).unwrap();
    let latest = r.latest().unwrap();
    assert!(r.apply("a", "s", 2, Event::Close, now).unwrap());
    assert!(!r.show_clock());
    assert_eq!(r.snapshots().len(), 1);
    assert_eq!(r.snapshots()[0].session_id.as_deref(), Some("b"));
    assert!(r.snapshots()[0].active);
    assert!(r.apply("b", "s", 2, Event::Close, now).unwrap());
    assert!(r.show_clock());
    assert!(!r.snapshots()[0].active);
    assert!(Arc::ptr_eq(&latest, &r.snapshots()[0].frame));
    assert!(Arc::ptr_eq(&latest, &r.latest().unwrap()));
    assert!(!r.apply("b", "s", 3, Event::Close, now).unwrap());
    assert!(!r.apply("b", "s", 4, frame(3), now).unwrap());
    assert!(r.show_clock());
    assert!(!r.expire(now + SESSION_TIMEOUT));
    assert!(r.show_clock());
    r.apply("b", "new", 0, frame(4), now).unwrap();
    assert!(!r.show_clock());
    assert!(r.snapshots()[0].active);
    assert_eq!(r.snapshots()[0].frame.rgba[0], 4);
}

#[test]
fn timeout_and_delayed_close_keep_retained_avatar_not_clock() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    r.apply("a", "s", 1, frame(1), now).unwrap();
    let saved = r.latest().unwrap();
    assert!(r.expire(now + SESSION_TIMEOUT));
    assert!(!r.show_clock());
    assert!(!r.snapshots()[0].active);
    assert!(Arc::ptr_eq(&saved, &r.snapshots()[0].frame));
    assert!(
        !r.apply("a", "s", 2, Event::Close, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(!r.show_clock());
    assert!(Arc::ptr_eq(&saved, &r.latest().unwrap()));
    assert!(Arc::ptr_eq(&saved, &r.snapshots()[0].frame));
    r.apply("b", "s", 1, frame(2), now + SESSION_TIMEOUT)
        .unwrap();
    r.apply("b", "s", 2, Event::Close, now + SESSION_TIMEOUT)
        .unwrap();
    assert!(r.show_clock());
    // A heartbeat can reactivate an expired, non-closed stream with its old pixels.
    r.apply("c", "s", 1, frame(3), now).unwrap();
    assert!(!r.show_clock());
    assert!(r.expire(now + SESSION_TIMEOUT));
    assert!(!r.show_clock());
    assert!(
        r.apply("c", "s", 2, Event::Heartbeat, now + SESSION_TIMEOUT)
            .unwrap()
    );
    assert!(!r.show_clock());
    assert!(r.snapshots()[0].active);
}

#[test]
fn legacy_publication_suppresses_clock_and_legacy_prevents_final_close_clock() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    r.apply("a", "s", 1, frame(1), now).unwrap();
    r.apply("a", "s", 2, Event::Close, now).unwrap();
    assert!(r.show_clock());
    let legacy = Arc::new(Frame {
        rgba: vec![9; 16384],
    });
    r.publish_legacy(legacy.clone(), SessionState::Normal);
    assert!(!r.show_clock());
    r.apply("b", "s", 1, frame(2), now).unwrap();
    assert!(r.apply("b", "s", 2, Event::Close, now).unwrap());
    assert!(!r.show_clock());
    assert_eq!(r.snapshots().len(), 1);
    assert!(r.snapshots()[0].session_id.is_none());
    assert!(Arc::ptr_eq(&legacy, &r.snapshots()[0].frame));
    assert!(!r.expire(now + SESSION_TIMEOUT));
    assert!(!r.show_clock());
}

#[test]
fn legacy_slot_and_pixel_snapshots_remain_compatible() {
    let mut r = SessionRegistry::default();
    assert!(r.latest().is_none());
    let old = Arc::new(Frame {
        rgba: vec![1; 16384],
    });
    r.publish_legacy(old.clone(), SessionState::Normal);
    r.publish_legacy(
        Arc::new(Frame {
            rgba: vec![2; 16384],
        }),
        SessionState::Normal,
    );
    assert_eq!(old.rgba[0], 1);
    assert_eq!(r.latest().unwrap().rgba[0], 2);
    assert_eq!(r.snapshots().len(), 1);
    assert!(r.snapshots()[0].session_id.is_none());
}

#[test]
fn state_metadata_travels_through_snapshots_lifecycle_and_legacy() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    r.apply("a", "s", 1, frame(1), now).unwrap();
    assert_eq!(r.snapshots()[0].state, SessionState::Normal);
    r.apply("a", "s", 2, clock_frame(2), now).unwrap();
    assert_eq!(r.snapshots()[0].state, SessionState::Clock);
    // Heartbeats preserve the stored state.
    r.apply("a", "s", 3, Event::Heartbeat, now).unwrap();
    assert_eq!(r.snapshots()[0].state, SessionState::Clock);
    // An ordinary image state exits the clock.
    r.apply("a", "s", 4, frame(3), now).unwrap();
    assert_eq!(r.snapshots()[0].state, SessionState::Normal);
    // Final explicit close still shows the global clock and retains the state.
    r.apply("a", "s", 5, Event::Close, now).unwrap();
    assert!(r.show_clock());
    assert_eq!(r.snapshots()[0].state, SessionState::Normal);
    // Expiry retention carries the session's stored state too.
    r.apply("b", "s", 1, clock_frame(4), now).unwrap();
    assert!(!r.show_clock());
    assert!(r.expire(now + SESSION_TIMEOUT));
    assert!(!r.show_clock(), "timeout alone never enables the clock");
    assert_eq!(r.snapshots()[0].state, SessionState::Clock);
    // Legacy snapshots carry their own state.
    r.publish_legacy(
        Arc::new(Frame {
            rgba: vec![9; 16384],
        }),
        SessionState::Clock,
    );
    assert!(!r.show_clock());
    assert_eq!(r.snapshots().len(), 1);
    assert_eq!(r.snapshots()[0].session_id, None);
    assert_eq!(r.snapshots()[0].state, SessionState::Clock);
}

#[test]
fn clock_silence_hides_the_title_strictly_after_fifteen_seconds_and_recovers() {
    let mut r = SessionRegistry::default();
    let now = Instant::now();
    r.apply("clock", "s", 1, clock_frame(1), now).unwrap();
    r.apply("normal", "s", 1, frame(2), now).unwrap();
    let silent = |r: &SessionRegistry, at| {
        r.clock_tiles_at(at)
            .into_iter()
            .map(|tile| tile.stale)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        silent(&r, now + CLOCK_SILENCE_TIMEOUT),
        vec![false, false],
        "exactly fifteen seconds is still live contact"
    );
    let stale = now + CLOCK_SILENCE_TIMEOUT + Duration::from_nanos(1);
    assert_eq!(silent(&r, stale), vec![true, false]);
    // Replayed pulses must not restore the title or renew the timer.
    assert!(!r.apply("clock", "s", 1, Event::Heartbeat, stale).unwrap());
    assert_eq!(silent(&r, stale), vec![true, false]);
    r.apply("clock", "s", 2, Event::Heartbeat, stale).unwrap();
    assert_eq!(silent(&r, stale), vec![false, false]);
    r.apply(
        "clock",
        "s",
        3,
        clock_frame(3),
        stale + Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(
        silent(&r, stale + Duration::from_secs(25)),
        vec![false, false],
        "an accepted frame renews contact, and normal avatars never go silent"
    );
    r.expire(now + Duration::from_secs(30));
    r.expire(stale + Duration::from_secs(40));
    let tiles = r.clock_tiles_at(stale + Duration::from_secs(40));
    assert_eq!(tiles.len(), 1);
    assert!(
        tiles[0].stale,
        "the retained final snapshot also reports silence"
    );
    assert_eq!(
        tiles[0].name, None,
        "silence never rewrites the recorded name"
    );
    assert_eq!(
        r.snapshots_with_names()[0].1,
        None,
        "original metadata is unchanged"
    );
}
