//! Combate à distância: pedras disparadas pelo estilingue.

use macroquad::prelude::*;

use crate::world::World;

pub const SLINGSHOT_SPEED: f32 = 420.0;
/// Tempo de voo antes de cair (define o alcance: ~250 px).
pub const SLINGSHOT_FLIGHT: f32 = 0.6;
pub const SLINGSHOT_DAMAGE: f32 = 2.0;
pub const SLINGSHOT_TORPOR: f32 = 20.0;
pub const SLINGSHOT_COOLDOWN: f32 = 0.7;

#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    /// Posição no chão (a pedra é desenhada acima, na altura do peito).
    pub pos: Vec2,
    pub vel: Vec2,
    pub ttl: f32,
    pub damage: f32,
    pub torpor: f32,
    /// Jogador que atirou.
    pub owner: u32,
}

impl Projectile {
    pub fn stone(from: Vec2, dir: Vec2, owner: u32) -> Self {
        Self {
            pos: from,
            vel: dir.normalize_or(Vec2::X) * SLINGSHOT_SPEED,
            ttl: SLINGSHOT_FLIGHT,
            damage: SLINGSHOT_DAMAGE,
            torpor: SLINGSHOT_TORPOR,
            owner,
        }
    }

    /// Avança a pedra. Retorna false quando ela deve sumir (fim do voo ou bateu em algo).
    pub fn update(&mut self, world: &World, dt: f32) -> bool {
        self.pos += self.vel * dt;
        self.ttl -= dt;
        self.ttl > 0.0 && !world.solid(self.pos, 1.0)
    }

    /// Acerta um alvo circular?
    pub fn hits(&self, center: Vec2, radius: f32) -> bool {
        self.pos.distance(center) < radius + 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::{self, StructureKind};

    #[test]
    fn stone_flies_until_its_range_ends() {
        let world = World::generate(1337, 160, 160);
        let mut stone = Projectile::stone(world.spawn, vec2(0.0, 1.0), 0);
        let mut steps = 0;
        while stone.update(&world, 1.0 / 60.0) {
            steps += 1;
        }
        let flown = stone.pos.distance(world.spawn);
        assert!(flown > 100.0 || steps < 36, "voou {flown}");
        assert!(flown <= SLINGSHOT_SPEED * SLINGSHOT_FLIGHT + 8.0);
    }

    #[test]
    fn stone_stops_at_walls() {
        let mut world = World::generate(1337, 160, 160);
        let here = building::tile_of(world.spawn);
        world.place(StructureKind::WoodWall, (here.0 + 2, here.1));
        let mut stone = Projectile::stone(world.spawn, vec2(1.0, 0.0), 0);
        while stone.update(&world, 1.0 / 60.0) {}
        assert!(stone.pos.x < building::tile_rect((here.0 + 2, here.1)).right());
        assert!(stone.ttl > 0.0, "parou na parede antes do fim do voo");
    }

    #[test]
    fn hit_test() {
        let stone = Projectile::stone(Vec2::ZERO, Vec2::X, 0);
        assert!(stone.hits(vec2(10.0, 0.0), 8.0));
        assert!(!stone.hits(vec2(30.0, 0.0), 8.0));
    }
}
