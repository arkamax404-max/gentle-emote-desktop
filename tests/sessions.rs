use gentle_emote_desktop::{
    receiver::Frame,
    sessions::{
        Event, MAX_ACTIVE_SESSIONS, MAX_KNOWN_SESSIONS, MAX_RETIRED_STREAMS, SESSION_TIMEOUT,
        SessionRegistry,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn frame(value: u8) -> Event {
    Event::Frame(Arc::new(Frame {
        rgba: vec![value; 16384],
    }))
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
fn legacy_slot_and_pixel_snapshots_remain_compatible() {
    let mut r = SessionRegistry::default();
    assert!(r.latest().is_none());
    let old = Arc::new(Frame {
        rgba: vec![1; 16384],
    });
    r.publish_legacy(old.clone());
    r.publish_legacy(Arc::new(Frame {
        rgba: vec![2; 16384],
    }));
    assert_eq!(old.rgba[0], 1);
    assert_eq!(r.latest().unwrap().rgba[0], 2);
    assert_eq!(r.snapshots().len(), 1);
    assert!(r.snapshots()[0].session_id.is_none());
}
