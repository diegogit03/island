//! Jangada: veículo de água (mar, água rasa e lagoas). O jogador sobe com F, rema com WASD
//! (Shift rema mais rápido gastando stamina) e só desce perto da margem.

use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player;
use crate::protocol::PlayerId;
use crate::world::{TILE, World};

pub const SPEED: f32 = 200.0;
pub const SPRINT_MULT: f32 = 1.25;
/// Meia largura da jangada (colisão com a terra).
pub const RADIUS: f32 = 16.0;
/// Distância máxima até a terra (ou até a jangada) para subir/descer.
pub const STEP_REACH: f32 = 64.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Raft {
    pub id: u32,
    pub pos: Vec2,
    pub facing_left: bool,
    pub moving: bool,
    /// Quem está remando (no máximo um jogador por jangada). Não é salvo.
    pub rider: Option<PlayerId>,
}

/// A jangada inteira cabe na água (e dentro do mapa)?
pub fn can_float(world: &World, pos: Vec2) -> bool {
    let size = world.size_px();
    let inside = pos.x >= TILE && pos.y >= TILE && pos.x <= size.x - TILE && pos.y <= size.y - TILE;
    let probes = [
        pos,
        pos + vec2(-RADIUS, 0.0),
        pos + vec2(RADIUS, 0.0),
        pos + vec2(0.0, -RADIUS * 0.6),
        pos + vec2(0.0, RADIUS * 0.6),
    ];
    inside && probes.iter().all(|&p| world.tile_at(p).is_water())
}

/// Ponto mais próximo de `p` (em anéis até `max`) que satisfaz `ok`.
fn nearest(p: Vec2, max: f32, ok: impl Fn(Vec2) -> bool) -> Option<Vec2> {
    let step = 8.0;
    for ring in 1..=(max / step) as usize {
        let r = ring as f32 * step;
        let points = (std::f32::consts::TAU * r / step).ceil().max(8.0) as usize;
        let found = (0..points)
            .map(|i| p + Vec2::from_angle(i as f32 / points as f32 * std::f32::consts::TAU) * r)
            .find(|&q| ok(q));
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Onde pôr uma jangada nova, na água perto do jogador em `from`.
pub fn launch_spot(world: &World, from: Vec2) -> Option<Vec2> {
    nearest(from, STEP_REACH + RADIUS, |p| can_float(world, p))
}

/// Onde o jogador pisa ao descer da jangada (ou de uma montaria aquática) em `from`.
pub fn shore_spot(world: &World, from: Vec2) -> Option<Vec2> {
    nearest(from, STEP_REACH, |p| !world.blocked(p, player::RADIUS))
}

impl Raft {
    pub fn new(id: u32, pos: Vec2) -> Self {
        Self {
            id,
            pos,
            facing_left: false,
            moving: false,
            rider: None,
        }
    }

    /// Rema na direção `dir`, deslizando ao encostar na margem.
    pub fn steer(&mut self, world: &World, dir: Vec2, speed: f32, dt: f32) {
        let dir = dir.normalize_or_zero();
        let start = self.pos;
        if dir.x.abs() > 0.05 {
            self.facing_left = dir.x < 0.0;
        }
        let delta = dir * speed * dt;
        for axis in [vec2(delta.x, 0.0), vec2(0.0, delta.y)] {
            if can_float(world, self.pos + axis) {
                self.pos += axis;
            }
        }
        self.moving = self.pos != start;
    }
}

/// Jangada como salva e como vai no snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RaftSave {
    pub id: u32,
    pub pos: [f32; 2],
    #[serde(default)]
    pub facing_left: bool,
    #[serde(default)]
    pub moving: bool,
    /// Só no snapshot (para o cliente não oferecer "[F] Subir" numa jangada ocupada).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rider: Option<PlayerId>,
}

impl From<&Raft> for RaftSave {
    fn from(r: &Raft) -> Self {
        Self {
            id: r.id,
            pos: r.pos.into(),
            facing_left: r.facing_left,
            moving: r.moving,
            rider: r.rider,
        }
    }
}

impl From<&RaftSave> for Raft {
    fn from(s: &RaftSave) -> Self {
        Self {
            id: s.id,
            pos: Vec2::from(s.pos),
            facing_left: s.facing_left,
            moving: s.moving,
            rider: s.rider,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Tile;

    #[test]
    fn raft_launches_from_the_beach_and_stays_in_the_water() {
        let world = World::generate(1337, 160, 160);
        // Do spawn (praia, sul da ilha), descendo até achar água.
        let beach = (0..40)
            .map(|i| world.spawn + vec2(0.0, i as f32 * 16.0))
            .find(|&p| launch_spot(&world, p).is_some())
            .expect("água perto da praia");
        let spot = launch_spot(&world, beach).unwrap();
        assert!(can_float(&world, spot));
        let mut raft = Raft::new(1, spot);
        // Rema para o mar (sul) e depois tenta voltar para a terra (norte).
        for _ in 0..300 {
            raft.steer(&world, vec2(0.0, 1.0), SPEED, 1.0 / 60.0);
        }
        assert_eq!(world.tile_at(raft.pos), Tile::DeepWater, "chegou ao mar aberto");
        assert!(
            shore_spot(&world, raft.pos).is_none(),
            "no meio do mar não dá para descer"
        );
        for _ in 0..1200 {
            raft.steer(&world, vec2(0.0, -1.0), SPEED, 1.0 / 60.0);
            assert!(can_float(&world, raft.pos), "nunca sobe na terra");
        }
        assert!(shore_spot(&world, raft.pos).is_some(), "encostou na margem");
    }

    #[test]
    fn save_round_trip() {
        let mut raft = Raft::new(7, vec2(10.0, 20.0));
        raft.facing_left = true;
        let back = Raft::from(&RaftSave::from(&raft));
        assert_eq!(back, raft);
    }
}
