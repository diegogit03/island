//! Mapa da ilha (tiles) e nós de recurso (árvores, rochas, pedrinhas).
//! Coordenadas de mundo em pixels; `pos` de entidades é o ponto do "pé" (base), usado no y-sort.

use macroquad::prelude::*;

use crate::building::{self, Layer, PlaceError, Structure, StructureKind};
use crate::inventory::Stack;
use crate::rng::{Rng, fbm, hash2};

pub const TILE: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    DeepWater,
    ShallowWater,
    Sand,
    Grass,
    /// Interior rochoso da ilha, com metal.
    Mountain,
    /// Norte gelado da ilha (fauna da era do gelo).
    Snow,
}

impl Tile {
    pub fn walkable(self) -> bool {
        self != Tile::DeepWater
    }

    pub fn is_land(self) -> bool {
        matches!(self, Tile::Sand | Tile::Grass | Tile::Mountain | Tile::Snow)
    }

    pub fn speed_factor(self) -> f32 {
        match self {
            Tile::ShallowWater => 0.55,
            Tile::Sand => 0.9,
            Tile::Mountain => 0.85,
            Tile::Snow => 0.9,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Tree,
    Rock,
    Pebble,
    Bush,
    MetalRock,
}

impl NodeKind {
    /// Golpes até esgotar.
    pub fn max_hp(self) -> u32 {
        match self {
            NodeKind::Tree => 8,
            NodeKind::Rock => 10,
            NodeKind::Pebble => 1,
            NodeKind::Bush => 3,
            NodeKind::MetalRock => 12,
        }
    }

    pub fn respawn_secs(self) -> f32 {
        match self {
            NodeKind::Tree => 90.0,
            NodeKind::Rock => 120.0,
            NodeKind::Pebble => 45.0,
            NodeKind::Bush => 60.0,
            NodeKind::MetalRock => 180.0,
        }
    }

    /// Raio de colisão com o jogador (0 = atravessável).
    pub fn collision_radius(self) -> f32 {
        match self {
            NodeKind::Tree => 9.0,
            NodeKind::Rock | NodeKind::MetalRock => 16.0,
            NodeKind::Pebble | NodeKind::Bush => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResourceNode {
    pub kind: NodeKind,
    pub pos: Vec2,
    pub hp: u32,
    pub respawn_timer: f32,
    pub variant: u8,
    /// Tempo restante do "tremor" visual após um golpe.
    pub shake: f32,
}

impl ResourceNode {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BagKind {
    /// Itens largados pelo jogador.
    Dropped,
    /// Tudo o que o jogador carregava ao morrer.
    Death,
}

impl BagKind {
    pub fn lifetime_secs(self) -> f32 {
        match self {
            BagKind::Dropped => 300.0,
            BagKind::Death => 1800.0,
        }
    }
}

/// Bolsa de itens no chão; some quando esvazia ou expira.
#[derive(Clone, Debug)]
pub struct GroundBag {
    pub pos: Vec2,
    pub stacks: Vec<Stack>,
    pub kind: BagKind,
    pub ttl: f32,
}

pub struct World {
    pub width: usize,
    pub height: usize,
    pub seed: u64,
    tiles: Vec<Tile>,
    pub nodes: Vec<ResourceNode>,
    pub bags: Vec<GroundBag>,
    pub structures: Vec<Structure>,
    pub spawn: Vec2,
}

impl World {
    pub fn generate(seed: u64, width: usize, height: usize) -> Self {
        let mut tiles = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let nx = x as f32 / width as f32 * 2.0 - 1.0;
                let ny = y as f32 / height as f32 * 2.0 - 1.0;
                let dist = (nx * nx + ny * ny).sqrt();
                let noise = fbm(x as f32 / 22.0, y as f32 / 22.0, seed, 4);
                let h = 1.0 - dist * 1.15 + (noise - 0.5) * 0.5;
                let border = x == 0 || y == 0 || x == width - 1 || y == height - 1;
                tiles.push(match h {
                    _ if border => Tile::DeepWater,
                    h if h < 0.18 => Tile::DeepWater,
                    h if h < 0.27 => Tile::ShallowWater,
                    h if h < 0.36 => Tile::Sand,
                    // Montanha: partes altas do interior (não consome RNG, então saves antigos batem).
                    h if h > 0.6 && fbm(x as f32 / 16.0, y as f32 / 16.0, seed.wrapping_add(13), 3) > 0.48 => {
                        Tile::Mountain
                    }
                    _ => Tile::Grass,
                });
            }
        }

        let mut world = Self {
            width,
            height,
            seed,
            tiles,
            nodes: Vec::new(),
            bags: Vec::new(),
            structures: Vec::new(),
            spawn: Vec2::ZERO,
        };
        world.spawn = world.find_beach_spawn();
        world.scatter_nodes();
        world.freeze_north();
        world
    }

    /// Bioma de gelo: a grama e a areia do norte da ilha viram neve. Roda depois de espalhar os nós
    /// e não consome RNG, então os nós (e seus índices em saves antigos) continuam os mesmos.
    fn freeze_north(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let ny = y as f32 / self.height as f32 * 2.0 - 1.0;
                let noise = fbm(x as f32 / 18.0, y as f32 / 18.0, self.seed.wrapping_add(41), 3);
                let i = y * self.width + x;
                if ny + (noise - 0.5) * 0.5 < -0.3 && matches!(self.tiles[i], Tile::Grass | Tile::Sand) {
                    self.tiles[i] = Tile::Snow;
                }
            }
        }
    }

    /// Primeiro tile de areia subindo a partir do centro da borda sul.
    fn find_beach_spawn(&self) -> Vec2 {
        let cx = self.width / 2;
        for y in (0..self.height).rev() {
            for dx in 0..self.width / 4 {
                for x in [cx + dx, cx - dx] {
                    if self.tile(x as i32, y as i32) == Tile::Sand {
                        return vec2((x as f32 + 0.5) * TILE, (y as f32 - 1.5) * TILE);
                    }
                }
            }
        }
        vec2(self.width as f32, self.height as f32) * TILE * 0.5
    }

    fn scatter_nodes(&mut self) {
        let mut rng = Rng::new(self.seed.wrapping_add(1));
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let center = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
                if center.distance(self.spawn) < TILE * 3.0 {
                    continue;
                }
                let forest = self.forest(x, y);
                let tile = self.tile(x, y);
                let kind = match tile {
                    // A montanha segue a mesma sequência do RNG da grama (mantém os índices dos nós
                    // iguais aos de saves antigos); só o tipo do nó muda depois.
                    Tile::Grass | Tile::Mountain => {
                        let tree_p = if forest > 0.55 { 0.22 } else { 0.03 };
                        if rng.chance(tree_p) {
                            Some(NodeKind::Tree)
                        } else if rng.chance(0.012) {
                            Some(NodeKind::Rock)
                        } else if rng.chance(0.015) {
                            Some(NodeKind::Pebble)
                        } else if rng.chance(if forest > 0.45 { 0.05 } else { 0.015 }) {
                            Some(NodeKind::Bush)
                        } else {
                            None
                        }
                    }
                    Tile::Sand if rng.chance(0.03) => Some(NodeKind::Pebble),
                    Tile::Sand if rng.chance(0.004) => Some(NodeKind::Rock),
                    _ => None,
                };
                let kind = if tile == Tile::Mountain {
                    kind.map(|k| mountain_node(k, hash2(x, y, self.seed.wrapping_add(21))))
                } else {
                    kind
                };
                if let Some(kind) = kind {
                    let jitter = vec2(rng.range(-8.0, 8.0), rng.range(-8.0, 8.0));
                    self.nodes.push(ResourceNode {
                        kind,
                        pos: center + jitter,
                        hp: kind.max_hp(),
                        respawn_timer: 0.0,
                        variant: (rng.next_u64() % 4) as u8,
                        shake: 0.0,
                    });
                }
            }
        }
        self.scatter_mountain_extras();
    }

    /// Rochas extras na montanha, com RNG próprio e adicionadas no fim da lista: os índices dos nós
    /// anteriores continuam iguais aos de saves antigos.
    fn scatter_mountain_extras(&mut self) {
        let mut rng = Rng::new(self.seed.wrapping_add(31));
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                if self.tile(x, y) != Tile::Mountain {
                    continue;
                }
                let kind = if rng.chance(0.07) {
                    NodeKind::MetalRock
                } else if rng.chance(0.03) {
                    NodeKind::Rock
                } else {
                    continue;
                };
                let center = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
                if self.nodes.iter().any(|n| n.pos.distance(center) < TILE) {
                    continue;
                }
                self.nodes.push(ResourceNode {
                    kind,
                    pos: center,
                    hp: kind.max_hp(),
                    respawn_timer: 0.0,
                    variant: (rng.next_u64() % 4) as u8,
                    shake: 0.0,
                });
            }
        }
    }

    /// Densidade de floresta (0..1) no tile; define onde nascem árvores e criaturas de mata.
    /// (Na montanha, o que seria árvore vira rocha.)
    pub fn forest(&self, x: i32, y: i32) -> f32 {
        fbm(x as f32 / 12.0, y as f32 / 12.0, self.seed.wrapping_add(7), 3)
    }

    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return Tile::DeepWater;
        }
        self.tiles[y as usize * self.width + x as usize]
    }

    pub fn tile_at(&self, pos: Vec2) -> Tile {
        self.tile((pos.x / TILE).floor() as i32, (pos.y / TILE).floor() as i32)
    }

    pub fn size_px(&self) -> Vec2 {
        vec2(self.width as f32, self.height as f32) * TILE
    }

    /// Um círculo de raio `radius` centrado em `pos` colide com água funda ou com algum nó sólido?
    pub fn blocked(&self, pos: Vec2, radius: f32) -> bool {
        let probes = [pos, pos + vec2(-radius, 0.0), pos + vec2(radius, 0.0)];
        if probes.iter().any(|&p| !self.tile_at(p).walkable()) {
            return true;
        }
        let node_hit = self.nodes.iter().any(|n| {
            let r = n.kind.collision_radius();
            r > 0.0 && n.is_alive() && n.pos.distance_squared(pos) < (r + radius).powi(2)
        });
        node_hit
            || self
                .structures
                .iter()
                .any(|s| s.rect().center().distance(pos) < TILE * 1.5 && s.blocks(pos, radius))
    }

    pub fn structure_at(&self, tile: (i32, i32), layer: Layer) -> Option<usize> {
        self.structures
            .iter()
            .position(|s| s.tile == tile && s.kind.layer() == layer)
    }

    /// Pode posicionar `kind` no `tile`, com o jogador (círculo) em `player_pos`?
    pub fn can_place(
        &self,
        kind: StructureKind,
        tile: (i32, i32),
        player_pos: Vec2,
        player_radius: f32,
    ) -> Result<(), PlaceError> {
        let rect = building::tile_rect(tile);
        if rect.center().distance(player_pos) > building::PLACE_REACH {
            return Err(PlaceError::TooFar);
        }
        if !self.tile(tile.0, tile.1).is_land() {
            return Err(PlaceError::NotLand);
        }
        if self.structure_at(tile, kind.layer()).is_some() {
            return Err(PlaceError::Occupied);
        }
        // Árvores, rochas e afins (mesmo esgotados, pois vão renascer) ocupam o tile.
        let node_inside = self.nodes.iter().any(|n| {
            let r = n.kind.collision_radius().max(6.0);
            building::distance_to_rect(n.pos, rect) < r
        });
        let candidate = Structure::new(kind, tile);
        if node_inside || candidate.blocks(player_pos, player_radius) {
            return Err(PlaceError::Blocked);
        }
        Ok(())
    }

    pub fn place(&mut self, kind: StructureKind, tile: (i32, i32)) {
        self.structures.push(Structure::new(kind, tile));
    }

    /// Alguma estação do tipo `kind` dentro do alcance de `pos`?
    pub fn station_near(&self, kind: StructureKind, pos: Vec2) -> bool {
        self.structures
            .iter()
            .any(|s| s.kind == kind && s.rect().center().distance(pos) <= building::STATION_RANGE)
    }

    /// Fontes de luz ativas: (posição, raio).
    pub fn lights(&self) -> impl Iterator<Item = (Vec2, f32)> + '_ {
        self.structures
            .iter()
            .filter(|s| s.is_lit())
            .map(|s| (s.rect().center() + vec2(0.0, 6.0), s.light_radius()))
    }

    pub fn drop_bag(&mut self, pos: Vec2, stacks: Vec<Stack>, kind: BagKind) {
        if stacks.is_empty() {
            return;
        }
        // Largar várias vezes no mesmo lugar junta tudo numa bolsa só.
        if kind == BagKind::Dropped
            && let Some(bag) = self
                .bags
                .iter_mut()
                .find(|b| b.kind == BagKind::Dropped && b.pos.distance(pos) < 12.0)
        {
            bag.stacks.extend(stacks);
            bag.ttl = kind.lifetime_secs();
            return;
        }
        self.bags.push(GroundBag {
            pos,
            stacks,
            kind,
            ttl: kind.lifetime_secs(),
        });
    }

    /// Ponto mais próximo de `from` (em anéis de meio tile) que satisfaz `ok`; testa até o mapa todo.
    /// Para pousos forçados e jogadores que voltam ao jogo em cima da água.
    pub fn find_near(&self, from: Vec2, ok: impl Fn(Vec2) -> bool) -> Option<Vec2> {
        if ok(from) {
            return Some(from);
        }
        let step = TILE / 2.0;
        let max_ring = (self.width.max(self.height) as f32 * 2.0) as usize;
        for ring in 1..=max_ring {
            let r = ring as f32 * step;
            let points = (std::f32::consts::TAU * r / step).ceil().max(8.0) as usize;
            for i in 0..points {
                let angle = i as f32 / points as f32 * std::f32::consts::TAU;
                let p = from + vec2(angle.cos(), angle.sin()) * r;
                // Água funda é descartada antes da checagem de colisão (bem mais cara).
                if self.tile_at(p).is_land() && ok(p) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Bolsa mais próxima dentro de `reach`.
    pub fn nearest_bag(&self, pos: Vec2, reach: f32) -> Option<usize> {
        self.bags
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.pos.distance(pos)))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Avança respawns, fogo e bolsas.
    /// `players`: posições dos jogadores (nada renasce em cima de ninguém).
    pub fn update(&mut self, dt: f32, players: &[Vec2]) {
        for bag in &mut self.bags {
            bag.ttl -= dt;
        }
        self.bags.retain(|b| b.ttl > 0.0 && !b.stacks.is_empty());

        for s in &mut self.structures {
            s.tick(dt);
        }

        for node in &mut self.nodes {
            node.shake = (node.shake - dt).max(0.0);
            if node.is_alive() {
                continue;
            }
            node.respawn_timer -= dt;
            let reach = node.kind.collision_radius() + 16.0;
            let clear = players.iter().all(|p| node.pos.distance(*p) > reach);
            if node.respawn_timer <= 0.0 && clear {
                node.hp = node.kind.max_hp();
            }
        }
    }
}

/// Na montanha, árvores e arbustos viram rochas, rochas de metal e pedrinhas.
fn mountain_node(kind: NodeKind, roll: f32) -> NodeKind {
    match kind {
        NodeKind::Tree if roll < 0.3 => NodeKind::MetalRock,
        NodeKind::Tree if roll < 0.55 => NodeKind::Rock,
        NodeKind::Rock if roll < 0.5 => NodeKind::MetalRock,
        NodeKind::Tree | NodeKind::Bush => NodeKind::Pebble,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn island_has_beach_and_valid_spawn() {
        let world = World::generate(1337, 160, 160);
        assert_eq!(world.tile_at(world.spawn), Tile::Sand);
        assert!(!world.blocked(world.spawn, 7.0));
        let count = |kind| world.nodes.iter().filter(|n| n.kind == kind).count();
        assert!(count(NodeKind::Tree) > 50);
        assert!(count(NodeKind::Rock) > 5);
        assert!(count(NodeKind::Pebble) > 20);
        assert!(count(NodeKind::Bush) > 20);
        let mountain = (0..160)
            .flat_map(|y| (0..160).map(move |x| (x, y)))
            .filter(|&(x, y)| world.tile(x, y) == Tile::Mountain)
            .count();
        let metal = count(NodeKind::MetalRock);
        assert!(mountain > 100, "deveria haver montanha: {mountain} tiles");
        assert!(metal > 10, "montanha com metal: {metal} rochas em {mountain} tiles");
    }

    #[test]
    fn placement_rules() {
        let mut world = World::generate(1337, 160, 160);
        let player = world.spawn;
        let here = building::tile_of(player);
        let next = (here.0 + 1, here.1);
        let wall = StructureKind::WoodWall;

        assert_eq!(world.can_place(wall, here, player, 7.0), Err(PlaceError::Blocked));
        assert_eq!(
            world.can_place(wall, (here.0 + 10, here.1), player, 7.0),
            Err(PlaceError::TooFar)
        );
        assert_eq!(
            world.can_place(StructureKind::WoodFoundation, here, player, 7.0),
            Ok(())
        );
        assert_eq!(world.can_place(wall, next, player, 7.0), Ok(()));

        world.place(wall, next);
        assert_eq!(world.can_place(wall, next, player, 7.0), Err(PlaceError::Occupied));
        assert_eq!(
            world.can_place(StructureKind::WoodFoundation, next, player, 7.0),
            Ok(())
        );
        assert!(world.blocked(building::tile_rect(next).center(), 7.0));

        let water = (0, 0);
        assert_eq!(
            world.can_place(wall, water, building::tile_rect(water).center(), 7.0),
            Err(PlaceError::NotLand)
        );
    }
}
