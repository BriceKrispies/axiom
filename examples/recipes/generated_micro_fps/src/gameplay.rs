//! The deterministic **gameplay ruleset**: player spawn, enemy health, weapon
//! pickup, hitscan shooting, damage, death, gate unlock, and the win condition.
//!
//! Gameplay is not a procedural operator, so it is expressed as a small,
//! self-contained deterministic simulation over the generated [`LevelLayout`].
//! It advances by high-level [`Intent`]s (the same intents a live control mapping
//! — WASD + fire — would produce), which keeps the *rules* provable in a headless
//! test even where full live input wiring is out of scope (see the project
//! notes).

use axiom::prelude::Vec3;

use crate::grammar::{EnemyKind, LevelLayout};

/// Tunable gameplay constants (all in world units / per-step).
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// Starting player health.
    pub player_health: i32,
    /// Grunt (variant A) health.
    pub grunt_health: i32,
    /// Sentry (variant B) health.
    pub sentry_health: i32,
    /// Damage one hitscan shot deals.
    pub weapon_damage: i32,
    /// Max hitscan range (XZ).
    pub shoot_range: f32,
    /// How close the player must be to grab the weapon.
    pub pickup_radius: f32,
    /// Range at which an enemy damages the player.
    pub attack_range: f32,
    /// Damage a single in-range enemy deals per step.
    pub enemy_dps: i32,
    /// Player move speed per step.
    pub move_speed: f32,
    /// How close to the exit counts as reaching it.
    pub exit_radius: f32,
}

impl Rules {
    /// The shipped, balanced ruleset.
    pub const fn facility() -> Self {
        Self {
            player_health: 100,
            grunt_health: 30,
            sentry_health: 50,
            weapon_damage: 25,
            // Long facility sightlines: hitscan reaches across the combat room so
            // the player can engage from cover before enemies close to melee.
            shoot_range: 40.0,
            pickup_radius: 2.5,
            attack_range: 3.0,
            enemy_dps: 8,
            move_speed: 3.0,
            exit_radius: 3.0,
        }
    }
}

/// One live enemy.
#[derive(Debug, Clone, Copy)]
pub struct Enemy {
    /// Variant.
    pub kind: EnemyKind,
    /// Position.
    pub pos: Vec3,
    /// Remaining health.
    pub health: i32,
}

impl Enemy {
    fn alive(&self) -> bool {
        self.health > 0
    }
}

/// A per-step player intent (what a control mapping would emit).
#[derive(Debug, Clone, Copy)]
pub enum Intent {
    /// Move toward a world point at move speed.
    MoveToward(Vec3),
    /// Fire the weapon at the nearest enemy in range/line of sight.
    Shoot,
    /// Do nothing this step.
    Wait,
}

/// The full deterministic game state.
#[derive(Debug, Clone)]
pub struct GameState {
    rules: Rules,
    /// Player position.
    pub player: Vec3,
    /// Player health.
    pub health: i32,
    /// Whether the player has picked up the weapon.
    pub has_weapon: bool,
    /// The live enemies.
    pub enemies: Vec<Enemy>,
    /// Whether the gate has unlocked (all enemies cleared).
    pub gate_open: bool,
    /// Whether the player has won (reached the exit with the gate open).
    pub won: bool,
    /// Whether the player has died.
    pub dead: bool,
    weapon_pos: Vec3,
    exit_pos: Vec3,
}

/// XZ-plane distance (gameplay happens on the floor).
fn dist_xz(a: Vec3, b: Vec3) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

impl GameState {
    /// Start a game from the generated layout.
    pub fn new(layout: &LevelLayout, rules: Rules) -> Self {
        let enemies = layout
            .enemies
            .iter()
            .map(|e| Enemy {
                kind: e.kind,
                pos: e.position,
                health: match e.kind {
                    EnemyKind::Grunt => rules.grunt_health,
                    EnemyKind::Sentry => rules.sentry_health,
                },
            })
            .collect();
        Self {
            rules,
            player: layout.player_spawn,
            health: rules.player_health,
            has_weapon: false,
            enemies,
            gate_open: false,
            won: false,
            dead: false,
            weapon_pos: layout.weapon_pos,
            exit_pos: layout.exit_pos,
        }
    }

    /// How many enemies are still alive.
    pub fn enemies_alive(&self) -> usize {
        self.enemies.iter().filter(|e| e.alive()).count()
    }

    /// Advance one deterministic step under `intent`. A no-op once won or dead.
    pub fn step(&mut self, intent: Intent) {
        if self.won || self.dead {
            return;
        }
        match intent {
            Intent::MoveToward(target) => self.move_toward(target),
            Intent::Shoot => self.shoot(),
            Intent::Wait => {}
        }
        self.try_pickup();
        self.take_enemy_damage();
        self.resolve_death();
        self.try_unlock_gate();
        self.try_win();
    }

    fn move_toward(&mut self, target: Vec3) {
        let d = dist_xz(self.player, target);
        if d > 1e-4 {
            let step = self.rules.move_speed.min(d);
            let nx = (target.x - self.player.x) / d;
            let nz = (target.z - self.player.z) / d;
            self.player = Vec3::new(self.player.x + nx * step, self.player.y, self.player.z + nz * step);
        }
    }

    fn shoot(&mut self) {
        if !self.has_weapon {
            return;
        }
        let range = self.rules.shoot_range;
        let target = self
            .enemies
            .iter_mut()
            .filter(|e| e.alive())
            .filter(|e| dist_xz(e.pos, self.player) <= range)
            .min_by(|a, b| dist_xz(a.pos, self.player).total_cmp(&dist_xz(b.pos, self.player)));
        if let Some(enemy) = target {
            enemy.health -= self.rules.weapon_damage;
        }
    }

    fn try_pickup(&mut self) {
        if dist_xz(self.player, self.weapon_pos) <= self.rules.pickup_radius {
            self.has_weapon = true;
        }
    }

    fn take_enemy_damage(&mut self) {
        let hits: i32 = self
            .enemies
            .iter()
            .filter(|e| e.alive())
            .filter(|e| dist_xz(e.pos, self.player) <= self.rules.attack_range)
            .count() as i32;
        self.health -= hits * self.rules.enemy_dps;
    }

    fn resolve_death(&mut self) {
        if self.health <= 0 {
            self.health = 0;
            self.dead = true;
        }
    }

    fn try_unlock_gate(&mut self) {
        if self.enemies_alive() == 0 {
            self.gate_open = true;
        }
    }

    fn try_win(&mut self) {
        if self.gate_open && dist_xz(self.player, self.exit_pos) <= self.rules.exit_radius {
            self.won = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::build_level;
    use crate::style::Style;

    fn fresh() -> GameState {
        GameState::new(&build_level(&Style::facility()), Rules::facility())
    }

    #[test]
    fn player_starts_unarmed_with_full_health_and_a_locked_gate() {
        let g = fresh();
        assert_eq!(g.health, 100);
        assert!(!g.has_weapon);
        assert!(!g.gate_open);
        assert_eq!(g.enemies_alive(), 4);
    }

    #[test]
    fn a_full_playthrough_wins() {
        let mut g = fresh();
        // Grab the weapon.
        for _ in 0..10 {
            g.step(Intent::MoveToward(g.weapon_pos));
        }
        assert!(g.has_weapon, "reached and picked up the weapon");
        // Clear every enemy from range (shoot, staying out of melee by aiming
        // from the weapon spot which is far from the combat room).
        for _ in 0..40 {
            if g.enemies_alive() == 0 {
                break;
            }
            g.step(Intent::Shoot);
        }
        assert_eq!(g.enemies_alive(), 0, "all enemies cleared");
        assert!(g.gate_open, "gate unlocks once enemies are cleared");
        assert!(!g.dead, "cleared them before dying");
        // Walk to the exit.
        for _ in 0..80 {
            if g.won {
                break;
            }
            g.step(Intent::MoveToward(g.exit_pos));
        }
        assert!(g.won, "reached the exit with the gate open");
    }

    #[test]
    fn standing_in_melee_kills_the_player() {
        let mut g = fresh();
        // March straight into the enemies with no weapon and just wait.
        let enemy = g.enemies[0].pos;
        for _ in 0..40 {
            g.step(Intent::MoveToward(enemy));
        }
        for _ in 0..40 {
            if g.dead {
                break;
            }
            g.step(Intent::Wait);
        }
        assert!(g.dead, "melee damage is lethal");
        assert!(!g.won);
    }

    #[test]
    fn the_gate_stays_locked_while_an_enemy_lives_and_blocks_the_win() {
        let mut g = fresh();
        for _ in 0..10 {
            g.step(Intent::MoveToward(g.weapon_pos));
        }
        // Kill all but one.
        for _ in 0..40 {
            if g.enemies_alive() <= 1 {
                break;
            }
            g.step(Intent::Shoot);
        }
        assert!(!g.gate_open, "one enemy left → gate stays locked");
        // Try to leave anyway.
        let exit = g.exit_pos;
        for _ in 0..80 {
            g.step(Intent::MoveToward(exit));
        }
        assert!(!g.won, "cannot win through a locked gate");
    }
}
