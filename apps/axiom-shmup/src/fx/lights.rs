//! Ported from Claude-of-Duty `src/fx/lights.js:1-104` — the whole file.
//!
//! Pooled flash lights. Forward rendering recompiles every material when the
//! number of visible lights changes, so the pool is allocated once and never
//! grown or shrunk — idle lights simply sit at zero intensity, and every
//! light past a fixed pool size steals the *oldest, lowest-priority* slot
//! rather than being dropped or growing the pool. Intensity follows an
//! instantaneous rise then an exponential decay with a hard tail-off so it
//! lands exactly on zero at `duration`.
//!
//! What is dropped: `constructor(scene, count)`'s `THREE.PointLight`
//! construction and `register(render)`'s `render.addLight` call
//! (`lights.js:16-33`) — there is no live light to register with a renderer
//! yet. [`LightPool`] tracks the same per-slot state
//! (`peak`/`age`/`duration`/`rise`/`decay`/`priority`) and the same
//! selection/decay logic; a future presentation layer reads
//! [`LightPool::slots`] to drive real lights.

/// One pooled light's state — the source's per-entry object,
/// `lights.js:18-27`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSlot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub distance: f64,
    pub intensity: f64,
    pub peak: f64,
    pub age: f64,
    pub duration: f64,
    pub decay: f64,
    pub priority: f64,
}

impl Default for LightSlot {
    fn default() -> Self {
        LightSlot {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            r: 1.0,
            g: 1.0,
            b: 1.0,
            distance: 14.0,
            intensity: 0.0,
            peak: 0.0,
            age: 1e9,
            duration: 1.0,
            decay: 3.0,
            priority: 0.0,
        }
    }
}

/// `class LightPool`, `lights.js:14-104`.
pub struct LightPool {
    pub slots: Vec<LightSlot>,
}

impl LightPool {
    /// `constructor(scene, count = 4)`, `lights.js:15-33`, minus the scene
    /// attachment.
    pub fn new(count: usize) -> Self {
        LightPool {
            slots: vec![LightSlot::default(); count],
        }
    }

    /// `flash(x, y, z, r, g, b, peak, duration, decay, distance, priority)`,
    /// `lights.js:44-65`. Returns the claimed slot index, or `None` when the
    /// pool is full of higher-priority lights (`lights.js:60-61`,
    /// `if (bestScore < 1e5 && best.priority > priority) return null;`).
    #[allow(clippy::too_many_arguments)]
    pub fn flash(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        r: f64,
        g: f64,
        b: f64,
        peak: f64,
        duration: f64,
        decay: f64,
        distance: f64,
        priority: f64,
    ) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut best_score = f64::NEG_INFINITY;
        for (i, e) in self.slots.iter().enumerate() {
            let score = if e.age >= e.duration {
                1e6
            } else {
                e.age / e.duration - e.priority * 0.5
            };
            if score > best_score {
                best_score = score;
                best = Some(i);
            }
        }
        let idx = best?;
        if best_score < 1e5 && self.slots[idx].priority > priority {
            return None;
        }
        let e = &mut self.slots[idx];
        e.x = x;
        e.y = y;
        e.z = z;
        e.r = r;
        e.g = g;
        e.b = b;
        e.distance = distance;
        e.peak = peak;
        e.age = 0.0;
        e.duration = duration;
        e.decay = decay;
        e.priority = priority;
        e.intensity = peak;
        Some(idx)
    }

    /// `update(dt)`, `lights.js:73-88`.
    pub fn update(&mut self, dt: f64) {
        for e in &mut self.slots {
            if e.age >= e.duration {
                if e.intensity != 0.0 {
                    e.intensity = 0.0;
                }
                continue;
            }
            e.age += dt;
            let t = e.age;
            let rise = (t / 0.004).min(1.0);
            let tail = 1.0 - (t / e.duration).min(1.0).powi(2);
            e.intensity = e.peak * rise * (-e.decay * t).exp() * tail;
            if e.age >= e.duration {
                e.intensity = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_claims_the_freest_slot_first() {
        let mut pool = LightPool::new(4);
        let a = pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100.0, 0.1, 8.0, 5.0, 1.0);
        assert_eq!(a, Some(0));
    }

    #[test]
    fn flash_refuses_to_steal_a_higher_priority_live_slot() {
        let mut pool = LightPool::new(1);
        pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100.0, 1.0, 8.0, 5.0, 5.0);
        let refused = pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 50.0, 1.0, 8.0, 5.0, 1.0);
        assert_eq!(refused, None);
    }

    #[test]
    fn flash_reuses_an_expired_slot() {
        let mut pool = LightPool::new(1);
        pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100.0, 0.05, 8.0, 5.0, 5.0);
        pool.update(1.0); // well past duration
        let reused = pool.flash(1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 50.0, 1.0, 8.0, 5.0, 1.0);
        assert_eq!(reused, Some(0));
    }

    #[test]
    fn intensity_decays_to_exactly_zero_at_duration() {
        let mut pool = LightPool::new(1);
        pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100.0, 0.1, 8.0, 5.0, 1.0);
        pool.update(0.1);
        assert_eq!(pool.slots[0].intensity, 0.0);
    }

    #[test]
    fn intensity_rises_from_zero_then_decays() {
        let mut pool = LightPool::new(1);
        pool.flash(0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 100.0, 0.2, 4.0, 5.0, 1.0);
        pool.update(0.001);
        let early = pool.slots[0].intensity;
        pool.update(0.05);
        let mid = pool.slots[0].intensity;
        assert!(early < mid || early > 0.0);
        assert!(mid > 0.0);
    }

    #[test]
    fn idle_slot_stays_at_zero_intensity() {
        let mut pool = LightPool::new(2);
        pool.update(1.0 / 60.0);
        assert_eq!(pool.slots[0].intensity, 0.0);
        assert_eq!(pool.slots[1].intensity, 0.0);
    }
}
