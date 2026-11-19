//! Session-local card policy. Never mutates receiver lifecycle or stored pixels.
use crate::sessions::{ClockTile, SessionState};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CardId {
    Session(String),
    Legacy,
    GlobalFallback,
}
impl CardId {
    pub(crate) fn of(tile: &ClockTile) -> Self {
        tile.snapshot
            .session_id
            .clone()
            .map_or(Self::Legacy, Self::Session)
    }
}

#[derive(Default)]
pub(crate) struct CardPresentation {
    order: Vec<CardId>,
    dismissed: HashSet<CardId>,
    fallback_dismissed: bool,
}
impl CardPresentation {
    pub(crate) fn visible(
        &mut self,
        mut tiles: Vec<ClockTile>,
        fallback: bool,
    ) -> (Vec<ClockTile>, bool) {
        // A fallback is a separate presentation, not the retained legacy/ID tile.
        // Do not let the retained normal snapshot clear its dismissal.
        if fallback {
            if self.fallback_dismissed {
                return (Vec::new(), false);
            }
            return (tiles, true);
        }
        self.fallback_dismissed = false;
        for tile in &tiles {
            if tile.snapshot.state == SessionState::Normal {
                self.dismissed.remove(&CardId::of(tile));
            }
        }
        self.order
            .retain(|id| tiles.iter().any(|tile| CardId::of(tile) == *id));
        for tile in &tiles {
            let id = CardId::of(tile);
            if !self.order.contains(&id) {
                self.order.push(id);
            }
        }
        tiles.retain(|tile| !self.dismissed.contains(&CardId::of(tile)));
        tiles.sort_by_key(|tile| self.order.iter().position(|id| *id == CardId::of(tile)));
        (tiles, false)
    }

    pub(crate) fn move_card(&mut self, id: &CardId, neighbor: &CardId) {
        if let (Some(a), Some(b)) = (
            self.order.iter().position(|key| key == id),
            self.order.iter().position(|key| key == neighbor),
        ) {
            self.order.swap(a, b);
        }
    }

    pub(crate) fn dismiss(&mut self, id: CardId) {
        if id == CardId::GlobalFallback {
            self.fallback_dismissed = true;
        } else {
            self.dismissed.insert(id);
        }
    }
}
