//! What the keeper carries between penalties.
//!
//! Two different things live here, and keeping them together is the point: the
//! keeper's **memory** (where the last few shots finished) and its **nerve** (the
//! roll it gets for the next one). Both are things that persist across attempts
//! while the attempt itself is thrown away and rebuilt, which is exactly why they
//! do not belong in the state machine next door.
//!
//! The memory is deliberately short. Four penalties is enough for a player who
//! keeps going to the same corner to start finding the keeper there, and short
//! enough that changing corner works immediately. A longer memory would make the
//! keeper unbeatable by pattern and — worse — unreadable, because the player
//! could no longer tell what it had learned from.

use axiom::prelude::Vec3;

use crate::play::nerve::KeeperNerve;

use super::Session;

/// How many past shots the keeper's shading averages over.
pub(super) const SHADE_MEMORY: usize = 4;

impl Session {
    /// The first attempt gets a rolled keeper too — it is a penalty like any
    /// other, and starting every shootout against the average keeper would make
    /// the opening kick the one you could practise against.
    pub(super) fn with_first_nerve(mut self) -> Session {
        let nerve = self.next_nerve();
        self.keeper = crate::play::keeper::Keeper::set(nerve);
        self
    }

    /// The nerve for the next attempt: rolled, unless this session is steady.
    pub(super) fn next_nerve(&mut self) -> KeeperNerve {
        match self.steady {
            true => KeeperNerve::steady(&self.tuning.keeper),
            false => KeeperNerve::roll(&mut self.rng, &self.tuning.keeper),
        }
    }

    /// Note where a shot finished. The *authored* finish, not where a deflection
    /// ended up — that is what the keeper was beaten by.
    pub(super) fn remember(&mut self, finish: Vec3) {
        self.seen.push(finish);
        (self.seen.len() > SHADE_MEMORY).then(|| self.seen.remove(0));
    }

    /// Where the keeper stands for the next penalty, how high it expects the
    /// ball, and how strongly it believes either — shaded toward the average of
    /// the last few finishes, bounded so it never abandons the middle of the goal.
    pub(super) fn shade(&self) -> (f32, f32, f32) {
        let count = self.seen.len().max(1) as f32;
        let gain = self.tuning.keeper.shade_gain;
        let limit = self.tuning.keeper.shade_limit;
        let across = self.seen.iter().map(|p| p.x).sum::<f32>() / count;
        let up = self.seen.iter().map(|p| p.y).sum::<f32>() / count;
        (
            (across * gain).clamp(-limit, limit),
            [1.0, up][usize::from(!self.seen.is_empty())],
            gain * (self.seen.len() as f32 / SHADE_MEMORY as f32).min(1.0),
        )
    }
}
