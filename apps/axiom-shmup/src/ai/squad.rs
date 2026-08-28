//! Ported from Claude-of-Duty `src/ai/squad.js:1-113` — the whole file.
//!
//! The squad exists to stop several individually-sensible soldiers from
//! behaving like one many-headed idiot: it hands out permission to peek so
//! they alternate instead of all leaning out together, shares contact
//! reports so one man spotting the player alerts the rest (after a
//! believable call-out delay), rations grenades, and allows only one flanker
//! at a time.
//!
//! ## Members: ids and snapshots, not live `Agent` references
//!
//! The source's `Squad.members` holds the *same* `Agent` objects the AI
//! system's flat `agents` array holds, and `Agent.squad` points straight
//! back — a genuine reference cycle, which is exactly the kind of shape a GC
//! shrugs off and Rust ownership cannot express without `Rc<RefCell<_>>` on
//! at least one side. That orchestration (who owns the canonical `Agent`
//! list, who calls `squad.update()` before or after each agent's own
//! `update()`) is `ai/index.js`'s job — explicitly out of this slice (see
//! `apps/shmup/src/ai/mod.rs`).
//!
//! So this port keeps `Squad::members` as plain agent ids (`Vec<i32>`,
//! mirroring the source's `agent.id`), and every method that needs to read
//! *other* members' live state (`update`'s contact broadcast, `canFlank`)
//! takes a `&[MemberSnapshot]` — a plain, `Copy` read view built by whoever
//! owns the real `Agent`s. Where the source *writes* another member's field
//! directly (`m.lastKnown.copy(...)`, `m.alertness = 1`, `m._setState('alert')`),
//! this port returns the intended writes as data
//! ([`SquadUpdate::contacts`]) instead of mutating through a borrowed
//! reference, and the caller applies them via [`super::agent::Agent::receive_squad_contact`].
//! This is a Rust-idiomatic divergence forced by ownership, not a behavioural
//! one: every value that would have been written is still produced, in the
//! same frame, from the same inputs.

use crate::rng::Rng;

/// The fields [`Squad::update`] and [`Squad::can_flank`] read off another
/// member. Mirrors exactly what `squad.js` touches on `m`/`other` — `state`,
/// `alive`, `hasTarget`, `targetVisible`, `lastKnown`, `lastKnownAge`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberSnapshot {
    pub id: i32,
    pub alive: bool,
    pub state: super::agent::AgentState,
    pub has_target: bool,
    pub target_visible: bool,
    pub last_known: [f64; 3],
    pub last_known_age: f64,
    /// Not read by anything in `squad.js` itself, but carried here so a
    /// single snapshot type also serves `CoverMap::pick`'s squad-bunching
    /// penalty (`nav.js:465-470`, `agent.js:479-485`), which needs another
    /// member's world position — the source passes `sq?.members` (live
    /// `Agent` objects with `.position`) straight through.
    pub position: [f64; 3],
}

/// One member's new contact report, applied by the caller via
/// [`super::agent::Agent::receive_squad_contact`]. Mirrors the loop body at
/// `squad.js:61-70`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactBroadcast {
    pub member_id: i32,
    pub position: [f64; 3],
    pub last_known_age: f64,
}

/// `class Squad`. `squad.js:15-112`.
///
/// One source field is deliberately absent: `this._pending = []`
/// (`squad.js:28`). It is never read or written anywhere in the source — not
/// by `squad.js`, not by `agent.js`, not by `ai/index.js` — so it has no
/// element type to port. The recipe's "dead computation in the source is
/// still part of the source" is honoured by recording it here rather than by
/// inventing a `Vec<?>` for it.
pub struct Squad {
    pub id: u32,
    pub members: Vec<i32>,
    rng: Rng,
    pub peek_tokens: usize,
    /// `this.peekHolders` — public in the source (a plain `Set` field), so
    /// public here. Only `has`/`size`/`add`/`delete`/`clear` are ever used, so
    /// the set's iteration order is not part of the contract.
    pub peek_holders: std::collections::HashSet<i32>,
    /// `this.peekTimer` — public in the source.
    pub peek_timer: f64,
    pub grenade_cooldown: f64,
    pub flanker: Option<i32>,
    pub contact: [f64; 3],
    pub has_contact: bool,
    pub contact_age: f64,
}

impl Squad {
    /// `new Squad(rng)`. `squad.js:16-29`. `id` mirrors the source's
    /// module-level `_nextSquad` counter — the caller assigns it (this port
    /// has no module-global mutable counter; see [`super::agent::next_agent_id`]
    /// for the same call made for agent ids).
    pub fn new(id: u32, rng: Rng) -> Self {
        Squad {
            id,
            members: Vec::new(),
            rng,
            peek_tokens: 1,
            peek_holders: std::collections::HashSet::new(),
            peek_timer: 0.0,
            grenade_cooldown: 6.0,
            flanker: None,
            contact: [0.0, 0.0, 0.0],
            has_contact: false,
            contact_age: f64::INFINITY,
        }
    }

    /// `add(agent)`. `squad.js:31-36`.
    pub fn add(&mut self, agent_id: i32) {
        self.members.push(agent_id);
        self.peek_tokens = 1.max((self.members.len() as f64 * 0.5).round() as usize);
    }

    /// `get alive()`. `squad.js:38-42`.
    pub fn alive_count(&self, members: &[MemberSnapshot]) -> usize {
        self.members
            .iter()
            .filter(|id| members.iter().any(|m| m.id == **id && m.alive))
            .count()
    }

    /// `update(dt)`. `squad.js:45-79`. Returns the contact broadcasts to
    /// apply this frame (`squad.js:61-70`'s writes onto `m`).
    pub fn update(&mut self, dt: f64, members: &[MemberSnapshot]) -> SquadUpdate {
        self.grenade_cooldown -= dt;
        self.contact_age += dt;
        if let Some(f) = self.flanker {
            let still_flanking = members
                .iter()
                .find(|m| m.id == f)
                .is_some_and(|m| m.alive && m.state == super::agent::AgentState::Flank);
            if !still_flanking {
                self.flanker = None;
            }
        }

        // contact sharing: whoever can see the player broadcasts, with a delay.
        // Iterates `self.members` (squad order), matching `for (const m of
        // this.members)` in the source — not the caller's `members` array
        // order, which may hold agents outside this squad too.
        let find = |id: i32| members.iter().find(|m| m.id == id);
        for &id in &self.members {
            let Some(m) = find(id) else { continue };
            if m.alive && m.has_target && m.target_visible {
                self.contact = m.last_known;
                self.has_contact = true;
                self.contact_age = 0.0;
                break;
            }
        }
        let mut contacts = Vec::new();
        if self.has_contact && self.contact_age < 4.0 {
            for &id in &self.members {
                let Some(m) = find(id) else { continue };
                if !m.alive || m.has_target {
                    continue;
                }
                // a call-out only gives a direction to check, never a free kill
                if m.last_known_age > 1.5 {
                    contacts.push(ContactBroadcast {
                        member_id: m.id,
                        position: self.contact,
                        last_known_age: 0.9 + self.rng.float() * 0.8,
                    });
                }
            }
        }

        // rotate the peek tokens so the same man is not always exposed
        self.peek_timer -= dt;
        if self.peek_timer <= 0.0 {
            self.peek_timer = 1.1 + self.rng.float() * 1.2;
            self.peek_holders.clear();
        }

        SquadUpdate { contacts }
    }

    /// `requestPeek(agent, dt)`. `squad.js:82-87`. Ask to lean out of cover;
    /// only [`Squad::peek_tokens`] members may at once.
    pub fn request_peek(&mut self, agent_id: i32) -> bool {
        if self.peek_holders.contains(&agent_id) {
            return true;
        }
        if self.peek_holders.len() >= self.peek_tokens {
            return false;
        }
        self.peek_holders.insert(agent_id);
        true
    }

    /// `releasePeek(agent)`. `squad.js:89-91`.
    pub fn release_peek(&mut self, agent_id: i32) {
        self.peek_holders.remove(&agent_id);
    }

    /// `canFlank(agent)`. `squad.js:94-101`. One flanker at a time, and only
    /// if someone else is holding attention.
    pub fn can_flank(&self, exclude_id: i32, members: &[MemberSnapshot]) -> bool {
        if self.flanker.is_some() {
            return false;
        }
        let shooting = members
            .iter()
            .filter(|m| self.members.contains(&m.id))
            .filter(|m| m.id != exclude_id && m.alive)
            .filter(|m| {
                m.state == super::agent::AgentState::Combat || m.state == super::agent::AgentState::Suppressed
            })
            .count();
        shooting >= 1
    }

    /// `claimFlank(agent)`. `squad.js:103-105`.
    pub fn claim_flank(&mut self, agent_id: i32) {
        self.flanker = Some(agent_id);
    }

    /// `requestGrenade()`. `squad.js:107-111`.
    pub fn request_grenade(&mut self) -> bool {
        if self.grenade_cooldown > 0.0 {
            return false;
        }
        self.grenade_cooldown = 14.0 + self.rng.float() * 12.0;
        true
    }
}

/// [`Squad::update`]'s result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SquadUpdate {
    pub contacts: Vec<ContactBroadcast>,
}
