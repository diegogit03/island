//! Desenho do mundo em visão 3/4. Arte provisória feita com primitivas:
//! cada entidade tem uma função `draw_*` isolada para ser trocada por sprites depois.

use macroquad::prelude::*;

use std::collections::HashSet;

use crate::building::{self, Material, Piece, Structure, StructureKind};
use crate::creatures::{Creature, Species, State, Temperament, Terrain};
use crate::items::ItemId;
use crate::player::{Player, SWING_TIME};
use crate::protocol::PopupStyle;
use crate::raft::Raft;
use crate::rng::hash2;
use crate::ui::draw_item_icon;
use crate::world::{BagKind, GroundBag, NodeKind, ResourceNode, TILE, Tile, World};

pub const DEEP_WATER: Color = Color::new(0.09, 0.31, 0.55, 1.0);
const SHALLOW_WATER: Color = Color::new(0.20, 0.55, 0.74, 1.0);
const SAND: Color = Color::new(0.89, 0.80, 0.59, 1.0);
const GRASS: Color = Color::new(0.38, 0.63, 0.28, 1.0);
const MOUNTAIN: Color = Color::new(0.55, 0.52, 0.48, 1.0);
const SNOW: Color = Color::new(0.92, 0.94, 0.97, 1.0);
const SWAMP: Color = Color::new(0.36, 0.42, 0.25, 1.0);
const SWAMP_WATER: Color = Color::new(0.25, 0.35, 0.28, 1.0);
const DESERT: Color = Color::new(0.88, 0.72, 0.46, 1.0);
const SHADOW: Color = Color::new(0.0, 0.0, 0.0, 0.22);

fn shade(c: Color, f: f32) -> Color {
    Color::new(c.r * f, c.g * f, c.b * f, c.a)
}

pub fn draw_ground(world: &World, view: Rect, time: f32) {
    let x0 = (view.x / TILE).floor().max(0.0) as i32;
    let y0 = (view.y / TILE).floor().max(0.0) as i32;
    let x1 = ((view.right() / TILE).ceil() as i32).min(world.width as i32);
    let y1 = ((view.bottom() / TILE).ceil() as i32).min(world.height as i32);

    for y in y0..y1 {
        for x in x0..x1 {
            let tile = world.tile(x, y);
            let (px, py) = (x as f32 * TILE, y as f32 * TILE);
            let h = hash2(x, y, world.seed);
            let variation = 0.95 + h * 0.08;
            let color = match tile {
                Tile::DeepWater => shade(DEEP_WATER, 0.97 + (time * 0.8 + h * 6.0).sin() * 0.03),
                Tile::ShallowWater => shade(
                    SHALLOW_WATER,
                    1.0 + (time * 1.5 + x as f32 * 0.7 + y as f32 * 0.5).sin() * 0.04,
                ),
                Tile::Sand => shade(SAND, variation),
                Tile::Grass => shade(GRASS, variation),
                Tile::Mountain => shade(MOUNTAIN, 0.92 + h * 0.14),
                Tile::Snow => shade(SNOW, 0.97 + h * 0.04),
                Tile::Swamp => shade(SWAMP, variation),
                Tile::Desert => shade(DESERT, variation),
                Tile::SwampWater => shade(
                    SWAMP_WATER,
                    1.0 + (time * 1.2 + x as f32 * 0.6 + y as f32 * 0.4).sin() * 0.03,
                ),
            };
            draw_rectangle(px, py, TILE + 0.5, TILE + 0.5, color);

            match tile {
                Tile::Grass if h > 0.6 => {
                    let c = shade(GRASS, 0.78);
                    let (gx, gy) = (px + 6.0 + h * 16.0, py + 10.0 + (h * 37.0) % 14.0);
                    draw_line(gx, gy, gx - 2.0, gy - 5.0, 1.5, c);
                    draw_line(gx + 3.0, gy, gx + 4.0, gy - 6.0, 1.5, c);
                }
                Tile::Mountain if h > 0.55 => {
                    // Pedrinhas e rachaduras.
                    let c = shade(MOUNTAIN, 0.75);
                    draw_circle(px + 4.0 + h * 22.0, py + 6.0 + (h * 41.0) % 20.0, 1.6, c);
                    let (cx, cy) = (px + (h * 67.0) % 26.0 + 3.0, py + (h * 29.0) % 24.0 + 4.0);
                    draw_line(cx, cy, cx + 5.0, cy + 3.0, 1.0, c);
                }
                Tile::Desert if h > 0.5 => {
                    // Ondulações das dunas.
                    let c = shade(DESERT, 0.86);
                    let (dx, dy) = (px + 3.0 + h * 14.0, py + 8.0 + (h * 41.0) % 16.0);
                    draw_line(dx, dy, dx + 6.0, dy - 2.0, 1.2, c);
                    draw_line(dx + 6.0, dy - 2.0, dx + 13.0, dy, 1.2, c);
                    if h > 0.85 {
                        draw_circle(
                            px + (h * 61.0) % 24.0 + 4.0,
                            py + (h * 29.0) % 22.0 + 5.0,
                            1.4,
                            shade(DESERT, 0.7),
                        );
                    }
                }
                Tile::Swamp if h > 0.55 => {
                    // Juncos com ponta marrom e uma poça de lama.
                    let (rx, ry) = (px + 4.0 + h * 22.0, py + 14.0 + (h * 37.0) % 14.0);
                    let reed = shade(SWAMP, 0.7);
                    for (dx, hgt) in [(0.0, 9.0), (3.0, 12.0), (6.0, 8.0)] {
                        draw_line(rx + dx, ry, rx + dx - 1.0, ry - hgt, 1.5, reed);
                    }
                    draw_rectangle(rx + 1.5, ry - 13.0, 2.0, 4.0, Color::from_rgba(110, 76, 44, 255));
                    if h > 0.8 {
                        let mud = Color::from_rgba(78, 70, 46, 255);
                        draw_ellipse(
                            px + (h * 59.0) % 20.0 + 6.0,
                            py + (h * 23.0) % 20.0 + 6.0,
                            5.0,
                            2.5,
                            0.0,
                            mud,
                        );
                    }
                }
                Tile::SwampWater if h > 0.62 => {
                    // Vitória-régia.
                    let (lx, ly) = (px + 6.0 + h * 18.0, py + 6.0 + (h * 47.0) % 18.0);
                    draw_ellipse(lx, ly, 5.5, 3.2, 0.0, Color::from_rgba(76, 128, 58, 255));
                    draw_triangle(
                        vec2(lx, ly),
                        vec2(lx + 5.5, ly - 1.0),
                        vec2(lx + 5.5, ly + 1.0),
                        shade(SWAMP_WATER, 1.0),
                    );
                    if h > 0.9 {
                        draw_circle(lx - 1.0, ly - 1.0, 1.4, Color::from_rgba(236, 190, 210, 255));
                    }
                }
                Tile::Snow if h > 0.55 => {
                    // Montinhos de neve azulados e um brilho.
                    let c = Color::from_rgba(200, 214, 230, 255);
                    draw_ellipse(px + 5.0 + h * 20.0, py + 8.0 + (h * 43.0) % 18.0, 4.0, 1.6, 0.0, c);
                    if h > 0.85 {
                        draw_circle(px + (h * 71.0) % 26.0 + 3.0, py + (h * 31.0) % 24.0 + 4.0, 1.0, WHITE);
                    }
                }
                Tile::Sand if h > 0.75 => {
                    draw_circle(px + h * 28.0, py + (h * 53.0) % 28.0, 1.2, shade(SAND, 0.82));
                }
                Tile::ShallowWater => draw_foam(world, x, y, px, py, time),
                _ => {}
            }
        }
    }

    // Pisos e tapetes, rentes ao chão: antes de tudo (fundações primeiro).
    for layer_floor in [true, false] {
        for s in world
            .structures
            .iter()
            .filter(|s| s.kind.flat() && (s.kind.layer() == building::Layer::Floor) == layer_floor)
        {
            let r = s.rect();
            if !r.overlaps(&view) {
                continue;
            }
            match s.kind.material() {
                Some(m) if layer_floor => draw_foundation(r, m),
                _ => draw_rug(r),
            }
            draw_structure_health(s);
        }
    }
}

const WOOD_TOP: Color = Color::new(0.80, 0.60, 0.38, 1.0);
const WOOD_FRONT: Color = Color::new(0.52, 0.36, 0.20, 1.0);
const WOOD_LINE: Color = Color::new(0.38, 0.25, 0.13, 1.0);
/// Altura visual de paredes e portas (em pixels de mundo).
const WALL_H: f32 = 24.0;

fn draw_foundation(r: Rect, m: Material) {
    match m {
        Material::Wood => {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.60, 0.43, 0.25, 1.0));
            for i in 1..4 {
                let y = r.y + i as f32 * r.h / 4.0;
                draw_line(r.x, y, r.right(), y, 1.0, WOOD_LINE);
            }
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, WOOD_LINE);
        }
        Material::Stone => {
            let (_, front, line) = material_colors(m);
            draw_rectangle(r.x, r.y, r.w, r.h, shade(front, 1.1));
            let half = r.w / 2.0;
            draw_line(r.x + half, r.y, r.x + half, r.bottom(), 1.0, line);
            draw_line(r.x, r.y + half, r.right(), r.y + half, 1.0, line);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, line);
        }
        Material::Metal => {
            let (_, front, line) = material_colors(m);
            draw_rectangle(r.x, r.y, r.w, r.h, shade(front, 1.15));
            for (dx, dy) in [(4.0, 4.0), (r.w - 4.0, 4.0), (4.0, r.h - 4.0), (r.w - 4.0, r.h - 4.0)] {
                draw_circle(r.x + dx, r.y + dy, 1.2, line);
            }
            draw_line(
                r.x + 8.0,
                r.y + 10.0,
                r.right() - 8.0,
                r.y + 10.0,
                1.0,
                shade(front, 0.9),
            );
            draw_line(
                r.x + 8.0,
                r.y + 22.0,
                r.right() - 8.0,
                r.y + 22.0,
                1.0,
                shade(front, 0.9),
            );
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, line);
        }
    }
}

/// Espuma nas bordas da água rasa que encostam em terra.
fn draw_foam(world: &World, x: i32, y: i32, px: f32, py: f32, time: f32) {
    let alpha = 0.35 + (time * 2.0 + (x + y) as f32 * 0.6).sin() * 0.15;
    let foam = Color::new(1.0, 1.0, 1.0, alpha);
    let w = 4.0;
    if world.tile(x, y - 1).is_land() {
        draw_rectangle(px, py, TILE, w, foam);
    }
    if world.tile(x, y + 1).is_land() {
        draw_rectangle(px, py + TILE - w, TILE, w, foam);
    }
    if world.tile(x - 1, y).is_land() {
        draw_rectangle(px, py, w, TILE, foam);
    }
    if world.tile(x + 1, y).is_land() {
        draw_rectangle(px + TILE - w, py, w, TILE, foam);
    }
}

enum Drawable<'a> {
    Node(&'a ResourceNode),
    Bag(&'a GroundBag),
    Structure(&'a Structure),
    Creature(&'a Creature),
    Avatar(&'a Avatar<'a>),
    Raft(&'a Raft),
    Tent(Vec2, usize),
    CampFire(Vec2),
}

/// Um personagem a desenhar (o próprio jogador ou outro na partida).
pub struct Avatar<'a> {
    pub player: &'a Player,
    pub held: Option<ItemId>,
    /// `id` da criatura montada.
    pub mount: Option<u32>,
    pub shirt: Color,
    pub hair: Color,
    /// Faixa na cabeça com a cor da tribo (membros recrutados).
    pub band: Option<Color>,
    /// Nome acima da cabeça (`None` para o próprio jogador).
    pub name: Option<&'a str>,
    /// Barra de vida sob o nome (NPCs feridos).
    pub health: Option<f32>,
    /// Saqueador: nome e barra em vermelho.
    pub hostile: bool,
}

/// Aparência dos saqueadores: (camisa, cabelo, faixa).
pub const RAIDER_LOOK: (Color, Color, Color) = (
    Color::new(0.25, 0.20, 0.20, 1.0),
    Color::new(0.08, 0.07, 0.07, 1.0),
    Color::new(0.78, 0.12, 0.10, 1.0),
);

/// Cabelo dos jogadores.
pub const PLAYER_HAIR: Color = Color::new(0.31, 0.2, 0.13, 1.0);

/// Cor de camisa de cada jogador, pelo `id`.
pub fn shirt_color(id: u32) -> Color {
    const SHIRTS: [(u8, u8, u8); 6] = [
        (196, 82, 58),
        (58, 120, 196),
        (70, 160, 90),
        (200, 160, 50),
        (150, 80, 180),
        (60, 170, 170),
    ];
    let (r, g, b) = SHIRTS[(id as usize).saturating_sub(1) % SHIRTS.len()];
    Color::from_rgba(r, g, b, 255)
}

/// Tudo o que aparece no mundo (y-sort) num frame.
pub struct Scene<'a> {
    pub world: &'a World,
    pub creatures: &'a [Creature],
    pub rafts: &'a [Raft],
    /// Pedras em voo.
    pub projectiles: &'a [Vec2],
    /// Personagens vivos.
    pub avatars: &'a [Avatar<'a>],
    /// Posição do jogador local (copas de árvore ficam translúcidas quando ele está atrás).
    pub local: Option<Vec2>,
    /// Anel de destaque do alvo atual: (posição, raio).
    pub ring: Option<(Vec2, f32)>,
    pub view: Rect,
    pub time: f32,
}

/// Altura da sela acima do chão, para desenhar o jogador montado.
fn saddle_height(species: Species) -> f32 {
    match species {
        Species::Raptor => 16.0,
        Species::Pteranodon => 20.0,
        Species::Argentavis => 24.0,
        Species::Mammoth => 36.0,
        Species::Smilodon => 19.0,
        Species::Beelzebufo => 18.0,
        Species::Sarcosuchus => 13.0,
        Species::Megalodon => 14.0,
        Species::Ichthyosaur => 13.0,
        Species::Mosasaur => 19.0,
        Species::Kraken => 19.0,
        Species::Kangaroo => 22.0,
        _ => 10.0,
    }
}

/// Quanto um voador no ar sobe na tela (em pixels de mundo).
const FLY_HEIGHT: f32 = 56.0;

/// Deslocamento vertical do desenho de uma criatura voando.
pub fn lift(c: &Creature) -> f32 {
    c.altitude * FLY_HEIGHT
}

/// Altura do topo da criatura acima do pé (barras, nomes, "[F]" e o losango de domesticado).
/// Bichos do mar são compridos e baixos: o topo fica mais perto do corpo.
pub fn head_height(c: &Creature) -> f32 {
    let k = if c.knocked_out {
        1.4
    } else if c.species.terrain() == Terrain::Sea {
        1.2
    } else {
        2.8
    };
    c.species.radius() * k + lift(c)
}

/// Voadores bem acima do chão são desenhados depois de tudo (por cima das copas).
fn high_up(c: &Creature) -> bool {
    c.altitude > 0.5
}

/// Desenha nós, estruturas, criaturas e jogador ordenados por Y (quem está mais "embaixo" fica na frente).
pub fn draw_entities(scene: &Scene) {
    let Scene {
        world,
        creatures,
        avatars,
        view,
        time,
        ..
    } = *scene;
    if let Some((pos, r)) = scene.ring {
        draw_ellipse_lines(pos.x, pos.y, r, r * 0.45, 0.0, 1.5, Color::new(1.0, 1.0, 1.0, 0.7));
    }

    let player_pos = scene.local.unwrap_or(vec2(f32::MIN, f32::MIN));
    let margin = view.w.max(view.h) * 0.1 + 64.0;
    let culled = Rect::new(
        view.x - margin,
        view.y - margin,
        view.w + margin * 2.0,
        view.h + margin * 2.0,
    );
    let mut list: Vec<(f32, Drawable)> = world
        .nodes
        .iter()
        .filter(|n| culled.contains(n.pos))
        .map(|n| (n.pos.y, Drawable::Node(n)))
        .collect();
    list.extend(
        world
            .bags
            .iter()
            .filter(|b| culled.contains(b.pos))
            .map(|b| (b.pos.y, Drawable::Bag(b))),
    );
    list.extend(
        world
            .structures
            .iter()
            .filter(|s| !s.kind.flat() && s.kind.layer() != building::Layer::Roof && s.rect().overlaps(&culled))
            .map(|s| (s.sort_y(), Drawable::Structure(s))),
    );
    list.extend(
        creatures
            .iter()
            .filter(|c| culled.contains(c.pos) && !high_up(c))
            .map(|c| (c.pos.y, Drawable::Creature(c))),
    );
    for camp in world.camps.iter().filter(|c| culled.contains(c.center)) {
        list.push((camp.center.y, Drawable::CampFire(camp.center)));
        for (i, &t) in camp.tents.iter().enumerate() {
            list.push((t.y, Drawable::Tent(t, i)));
        }
    }
    // A jangada vem logo antes de quem está em cima dela (mesmo Y).
    list.extend(
        scene
            .rafts
            .iter()
            .filter(|r| culled.contains(r.pos))
            .map(|r| (r.pos.y - 0.5, Drawable::Raft(r))),
    );
    // Montado, o personagem é desenhado junto da montaria (logo depois dela).
    list.extend(
        avatars
            .iter()
            .filter(|a| a.mount.is_none())
            .map(|a| (a.player.pos.y, Drawable::Avatar(a))),
    );
    list.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut flyers: Vec<&Creature> = creatures
        .iter()
        .filter(|c| culled.contains(c.pos) && high_up(c))
        .collect();
    flyers.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
    // Sombra de quem voa alto fica no chão, embaixo de árvores e construções.
    for c in &flyers {
        draw_shadow(c);
    }

    let draw_with_riders = |c: &Creature| {
        // Nadando (no chão de uma lagoa ou água rasa): parte de baixo coberta pela água.
        let water = match world.tile_at(c.pos) {
            Tile::ShallowWater if c.altitude < 0.1 => Some(SHALLOW_WATER),
            Tile::SwampWater if c.altitude < 0.1 => Some(SWAMP_WATER),
            Tile::DeepWater if c.altitude < 0.1 => Some(DEEP_WATER),
            _ => None,
        };
        draw_creature(c, time, !high_up(c) && water.is_none(), water);
        for rider in avatars.iter().filter(|a| a.mount == Some(c.id)) {
            let seat = saddle_height(c.species) + lift(c);
            draw_player(rider.player, rider.held, rider.shirt, rider.hair, rider.band, seat);
        }
    };
    for (_, d) in list {
        match d {
            Drawable::Node(n) => draw_node(n, player_pos, world.tile_at(n.pos)),
            Drawable::Bag(b) => draw_bag(b, time),
            Drawable::Structure(s) => draw_structure(world, s, time),
            Drawable::Creature(c) => draw_with_riders(c),
            Drawable::Avatar(a) => draw_player(a.player, a.held, a.shirt, a.hair, a.band, 0.0),
            Drawable::Raft(r) => draw_raft(r, time),
            Drawable::Tent(pos, variant) => draw_tent(pos, variant),
            Drawable::CampFire(pos) => draw_campfire(Rect::new(pos.x - 16.0, pos.y - 16.0, TILE, TILE), true, time),
        }
    }
    for c in flyers {
        draw_with_riders(c);
    }

    for stone in scene.projectiles {
        let (x, y) = (stone.x, stone.y);
        draw_ellipse(x, y, 2.5, 1.0, 0.0, SHADOW);
        draw_circle(x, y - 14.0, 2.2, Color::from_rgba(150, 150, 158, 255));
    }
}

/// `ground`: tile embaixo do nó (no gelo, árvores viram pinheiros nevados; no pântano, árvores com musgo).
fn draw_node(node: &ResourceNode, player_pos: Vec2, ground: Tile) {
    let shake = if node.shake > 0.0 {
        (node.shake * 60.0).sin() * 2.0
    } else {
        0.0
    };
    let pos = node.pos + vec2(shake, 0.0);
    match (node.kind, node.is_alive()) {
        (NodeKind::Tree, true) => {
            // Copa translúcida quando o jogador está atrás da árvore.
            let behind = player_pos.y < node.pos.y
                && player_pos.y > node.pos.y - 70.0
                && (player_pos.x - node.pos.x).abs() < 26.0;
            draw_tree(pos, node.variant, if behind { 0.45 } else { 1.0 }, ground);
        }
        (NodeKind::Tree, false) => {
            draw_ellipse(pos.x, pos.y, 9.0, 4.0, 0.0, SHADOW);
            draw_rectangle(pos.x - 5.0, pos.y - 6.0, 10.0, 6.0, Color::from_rgba(110, 72, 40, 255));
            draw_ellipse(pos.x, pos.y - 6.0, 5.0, 2.0, 0.0, Color::from_rgba(196, 158, 108, 255));
        }
        (NodeKind::Rock, true) => draw_rock(pos, node.variant),
        (NodeKind::MetalRock, true) => draw_metal_rock(pos, node.variant),
        (NodeKind::Pebble, true) => draw_pebbles(pos, node.variant),
        (NodeKind::Bush, alive) => draw_bush(pos, node.variant, alive),
        (NodeKind::Cactus, alive) => draw_cactus(pos, node.variant, alive),
        _ => {}
    }
}

fn draw_tree(pos: Vec2, variant: u8, alpha: f32, ground: Tile) {
    let (x, y) = (pos.x, pos.y);
    draw_ellipse(x, y, 16.0, 6.0, 0.0, SHADOW);
    draw_rectangle(x - 4.0, y - 26.0, 8.0, 26.0, Color::from_rgba(110, 72, 40, 255));
    draw_rectangle(x - 4.0, y - 26.0, 3.0, 26.0, Color::from_rgba(132, 90, 52, 255));

    let leaf = |r, g, b| Color::from_rgba(r, g, b, 255).with_alpha(alpha);
    if ground == Tile::Desert {
        if variant.is_multiple_of(2) {
            // Palmeira: folhas compridas saindo do topo e cocos.
            let top = vec2(x + 3.0, y - 40.0);
            draw_line(x, y - 26.0, top.x, top.y, 6.0, Color::from_rgba(132, 96, 58, 255));
            for (dx, dy) in [(-22.0, 6.0), (-14.0, -8.0), (0.0, -12.0), (14.0, -8.0), (22.0, 6.0)] {
                let tip = top + vec2(dx, dy);
                draw_triangle(top, tip, top + vec2(dx * 0.5, dy * 0.5 + 4.0), leaf(70, 140, 60));
            }
            for dx in [-2.0, 3.0] {
                draw_circle(top.x + dx, top.y + 3.0, 2.5, Color::from_rgba(110, 76, 40, 255));
            }
        } else {
            // Árvore seca: galhos sem folhas.
            let bark = leaf(120, 96, 72);
            draw_line(x, y - 26.0, x - 2.0, y - 42.0, 4.0, bark);
            draw_line(x - 1.0, y - 34.0, x - 14.0, y - 44.0, 2.5, bark);
            draw_line(x - 1.0, y - 38.0, x + 12.0, y - 50.0, 2.5, bark);
            draw_line(x + 6.0, y - 44.0, x + 14.0, y - 42.0, 1.5, bark);
        }
    } else if ground == Tile::Swamp {
        // Árvore do pântano: copa larga e baixa, com barba-de-velho pendurada.
        draw_ellipse(x, y - 34.0, 24.0, 12.0, 0.0, leaf(52, 74, 40));
        draw_ellipse(x - 4.0, y - 41.0, 18.0, 9.0, 0.0, leaf(72, 96, 52));
        let moss = leaf(150, 158, 118);
        for i in 0..5 {
            let mx = x - 18.0 + i as f32 * 9.0 + variant as f32;
            let len = 8.0 + ((i * 7 + variant as usize) % 5) as f32 * 2.5;
            draw_line(mx, y - 28.0, mx + 1.0, y - 28.0 + len, 1.5, moss);
        }
    } else if ground == Tile::Snow {
        // Pinheiro com neve nos galhos
        for (i, w) in [22.0, 17.0, 12.0].into_iter().enumerate() {
            let base = y - 18.0 - i as f32 * 13.0;
            draw_triangle(
                vec2(x - w, base),
                vec2(x + w, base),
                vec2(x, base - 24.0),
                leaf(34, 82, 58),
            );
            let cap = Color::new(0.95, 0.97, 1.0, alpha);
            draw_triangle(
                vec2(x - w * 0.55, base - 10.0),
                vec2(x + w * 0.55, base - 10.0),
                vec2(x, base - 24.0),
                cap,
            );
            draw_line(x - w, base, x - w * 0.4, base - 2.0, 2.0, cap);
        }
    } else if variant.is_multiple_of(2) {
        // Folhosa
        let dark = leaf(46, 104, 46);
        let mid = leaf(62, 130, 56);
        draw_circle(x - 12.0, y - 32.0, 14.0, dark);
        draw_circle(x + 12.0, y - 32.0, 14.0, dark);
        draw_circle(x, y - 42.0, 19.0, mid);
        draw_circle(x - 6.0, y - 48.0, 8.0, leaf(92, 160, 72));
    } else {
        // Pinheiro
        for (i, w) in [22.0, 17.0, 12.0].into_iter().enumerate() {
            let base = y - 18.0 - i as f32 * 13.0;
            let c = if i % 2 == 0 {
                leaf(40, 96, 56)
            } else {
                leaf(52, 116, 64)
            };
            draw_triangle(vec2(x - w, base), vec2(x + w, base), vec2(x, base - 24.0), c);
        }
    }
}

fn draw_bush(pos: Vec2, variant: u8, berries: bool) {
    let (x, y) = (pos.x, pos.y);
    let dark = Color::from_rgba(44, 100, 48, 255);
    let mid = Color::from_rgba(60, 124, 58, 255);
    draw_ellipse(x, y, 14.0, 5.0, 0.0, SHADOW);
    draw_circle(x - 7.0, y - 7.0, 8.0, dark);
    draw_circle(x + 7.0, y - 7.0, 8.0, dark);
    draw_circle(x, y - 12.0, 9.0, mid);
    if berries {
        let red = Color::from_rgba(200, 40, 70, 255);
        let spots: [(f32, f32); 5] = [(-8.0, -8.0), (6.0, -5.0), (1.0, -15.0), (-3.0, -4.0), (9.0, -11.0)];
        for (i, &(dx, dy)) in spots.iter().enumerate() {
            if (i + variant as usize) % 5 != 4 {
                draw_circle(x + dx, y + dy, 2.2, red);
            }
        }
    }
}

fn draw_bag(bag: &GroundBag, time: f32) {
    let (x, y) = (bag.pos.x, bag.pos.y);
    match bag.kind {
        BagKind::Death => {
            // Feixe de luz para achar a bolsa de longe, como no ARK.
            let pulse = 0.18 + (time * 2.5).sin() * 0.06;
            draw_rectangle(x - 3.0, y - 120.0, 6.0, 120.0, Color::new(0.75, 0.55, 1.0, pulse));
            draw_ellipse(x, y, 10.0, 4.0, 0.0, SHADOW);
            draw_circle(x, y - 7.0, 8.0, Color::from_rgba(110, 70, 140, 255));
            draw_rectangle(x - 3.0, y - 17.0, 6.0, 4.0, Color::from_rgba(80, 50, 100, 255));
        }
        BagKind::Dropped => {
            draw_ellipse(x, y, 8.0, 3.0, 0.0, SHADOW);
            let bob = (time * 3.0 + x).sin() * 1.0;
            match bag.stacks.as_slice() {
                [only] => draw_item_icon(only.item, Rect::new(x - 8.0, y - 18.0 + bob, 16.0, 16.0)),
                _ => {
                    draw_circle(x, y - 6.0 + bob, 6.0, Color::from_rgba(150, 110, 70, 255));
                    draw_rectangle(x - 2.0, y - 14.0 + bob, 4.0, 3.0, Color::from_rgba(110, 80, 50, 255));
                }
            }
        }
    }
}

pub fn bag_label(bag: &GroundBag) -> String {
    match (bag.kind, bag.stacks.as_slice()) {
        (BagKind::Death, _) => "[F] Pegar seus itens".to_owned(),
        (_, [only]) if only.count > 1 => format!("[F] Pegar {} {}", only.count, only.item.name()),
        (_, [only]) => format!("[F] Pegar {}", only.item.name()),
        (_, stacks) => format!("[F] Pegar bolsa ({} pilhas)", stacks.len()),
    }
}

/// `me`: nome do jogador local (a cama dele diz que é dele).
pub fn structure_label(s: &Structure, me: &str) -> String {
    match s.kind {
        kind if kind.piece() == Piece::Gate && s.open => "[F] Fechar portão".to_owned(),
        kind if kind.piece() == Piece::Gate => "[F] Abrir portão".to_owned(),
        kind if kind.opens() && s.open => "[F] Fechar porta".to_owned(),
        kind if kind.opens() => "[F] Abrir porta".to_owned(),
        StructureKind::Bed => match s.owner.as_deref() {
            Some(owner) if owner == me => "Sua cama (ponto de renascer)".to_owned(),
            Some(owner) => format!("[F] Cama de {owner} (tornar sua)"),
            None => "[F] Tornar esta cama sua".to_owned(),
        },
        StructureKind::StorageBox => "[F] Abrir caixa".to_owned(),
        StructureKind::Workbench => "[F] Usar bancada".to_owned(),
        StructureKind::Campfire if s.is_lit() => "[F] Abrir fogueira (acesa)".to_owned(),
        StructureKind::Campfire => "[F] Abrir fogueira".to_owned(),
        StructureKind::Forge if s.is_lit() => "[F] Abrir forja (acesa)".to_owned(),
        StructureKind::Forge => "[F] Abrir forja".to_owned(),
        _ => String::new(),
    }
}

/// Dica de interação ("[F] ...") acima de um ponto do mundo (desenhada em espaço de tela).
pub fn draw_prompt(label: &str, world_pos: Vec2, camera: &Camera2D) {
    let at = camera.world_to_screen(world_pos);
    let size = 20.0;
    let w = measure_text(label, None, size as u16, 1.0).width;
    draw_rectangle(
        at.x - w / 2.0 - 6.0,
        at.y - 18.0,
        w + 12.0,
        24.0,
        Color::new(0.0, 0.0, 0.0, 0.6),
    );
    draw_text(label, at.x - w / 2.0, at.y, size, WHITE);
}

fn draw_structure(world: &World, s: &Structure, time: f32) {
    let r = s.rect();
    let material = s.kind.material().unwrap_or(Material::Wood);
    let (top, front, line) = material_colors(material);
    match s.kind.piece() {
        Piece::Wall => draw_wall(r, material),
        Piece::Door if s.open => {
            // Batentes nas laterais; folha da porta encostada à esquerda.
            for x in [r.x, r.right() - 5.0] {
                draw_rectangle(x, r.y - WALL_H, 5.0, r.h + WALL_H, front);
            }
            draw_rectangle(r.x + 5.0, r.y - WALL_H + 4.0, 4.0, r.h + WALL_H - 4.0, top);
        }
        Piece::Door => {
            draw_wall(r, material);
            let face = r.bottom() - WALL_H;
            draw_rectangle(r.x + 6.0, face + 2.0, r.w - 12.0, WALL_H - 2.0, shade(front, 0.85));
            draw_rectangle_lines(r.x + 6.0, face + 2.0, r.w - 12.0, WALL_H - 2.0, 1.5, line);
            let knob = if material == Material::Wood {
                Color::new(0.85, 0.75, 0.4, 1.0)
            } else {
                Color::new(0.9, 0.9, 0.95, 1.0)
            };
            draw_circle(r.right() - 10.0, face + WALL_H * 0.55, 1.6, knob);
        }
        Piece::Window => {
            draw_wall(r, material);
            // Vidro também no alto (numa fileira vertical, a face da frente fica escondida).
            draw_rectangle(
                r.x + 8.0,
                r.y - WALL_H + 10.0,
                r.w - 16.0,
                6.0,
                Color::new(0.55, 0.75, 0.9, 0.9),
            );
            let face = r.bottom() - WALL_H;
            let glass = Rect::new(r.x + 7.0, face + 4.0, r.w - 14.0, WALL_H - 10.0);
            draw_rectangle(glass.x, glass.y, glass.w, glass.h, Color::new(0.55, 0.75, 0.9, 0.9));
            draw_line(glass.x + 2.0, glass.y + 2.0, glass.x + 7.0, glass.y + 2.0, 1.5, WHITE);
            draw_line(glass.center().x, glass.y, glass.center().x, glass.bottom(), 1.5, line);
            draw_line(glass.x, glass.center().y, glass.right(), glass.center().y, 1.5, line);
            draw_rectangle_lines(glass.x, glass.y, glass.w, glass.h, 2.0, line);
            draw_rectangle(glass.x - 2.0, glass.bottom(), glass.w + 4.0, 2.5, top);
        }
        Piece::Gate if s.open => {
            for post in building::gate_posts(r, s.vertical) {
                draw_post(post, material);
            }
            // Folhas abertas encostadas nos batentes.
            let [a, b] = building::gate_posts(r, s.vertical);
            if s.vertical {
                draw_rectangle(a.x + 4.0, a.bottom() - WALL_H, r.w - 8.0, 4.0, top);
                draw_rectangle(b.x + 4.0, b.y - WALL_H, r.w - 8.0, 4.0, top);
            } else {
                draw_rectangle(a.right(), r.y - WALL_H + 6.0, 4.0, r.h + WALL_H - 8.0, top);
                draw_rectangle(b.x - 4.0, r.y - WALL_H + 6.0, 4.0, r.h + WALL_H - 8.0, top);
            }
        }
        Piece::Gate => {
            draw_wall(r, material);
            let face = Rect::new(r.x, r.bottom() - WALL_H, r.w, WALL_H);
            if !s.vertical {
                // Travessas em X e batentes mais escuros nas pontas.
                let mid = face.center().x;
                for (x0, x1) in [(face.x + 6.0, mid - 3.0), (mid + 3.0, face.right() - 6.0)] {
                    draw_line(x0, face.y + 3.0, x1, face.bottom() - 3.0, 2.0, line);
                    draw_line(x0, face.bottom() - 3.0, x1, face.y + 3.0, 2.0, line);
                }
                draw_line(mid, face.y, mid, face.bottom(), 2.0, line);
                for x in [face.x, face.right() - 5.0] {
                    draw_rectangle(x, r.y - WALL_H, 5.0, r.h + WALL_H, shade(front, 0.8));
                }
            } else {
                for y in [r.y - WALL_H, r.bottom() - WALL_H - 5.0] {
                    draw_rectangle(r.x, y, r.w, 5.0, shade(top, 0.8));
                }
            }
        }
        Piece::Fence => draw_fence(world, s, material),
        Piece::Foundation => draw_foundation(r, material),
        Piece::Roof => {}
        Piece::Station | Piece::Furniture => draw_furnishing(s, r, time),
    }
    draw_structure_health(s);
}

/// Estações (fogueira, forja, bancada, caixa) e móveis.
fn draw_furnishing(s: &Structure, r: Rect, time: f32) {
    let base = r.bottom() - 4.0;
    match s.kind {
        StructureKind::StorageBox => {
            let (x, w) = (r.x + 5.0, r.w - 10.0);
            draw_ellipse(r.center().x, base, w / 2.0 + 2.0, 4.0, 0.0, SHADOW);
            draw_rectangle(x, base - 14.0, w, 14.0, WOOD_FRONT);
            draw_rectangle(x, base - 22.0, w, 8.0, WOOD_TOP);
            draw_rectangle(
                r.center().x - 2.0,
                base - 16.0,
                4.0,
                6.0,
                Color::new(0.75, 0.70, 0.55, 1.0),
            );
            for sx in [x + 3.0, x + w - 6.0] {
                draw_rectangle(sx, base - 22.0, 3.0, 22.0, WOOD_LINE);
            }
        }
        StructureKind::Workbench | StructureKind::Table => {
            draw_ellipse(r.center().x, base, r.w / 2.0, 4.0, 0.0, SHADOW);
            for lx in [r.x + 4.0, r.right() - 8.0] {
                draw_rectangle(lx, base - 14.0, 4.0, 14.0, WOOD_LINE);
            }
            draw_rectangle(r.x + 1.0, base - 22.0, r.w - 2.0, 6.0, WOOD_TOP);
            draw_rectangle(r.x + 1.0, base - 16.0, r.w - 2.0, 3.0, WOOD_FRONT);
            if s.kind == StructureKind::Workbench {
                // Ferramentas em cima
                let stone = Color::from_rgba(150, 150, 158, 255);
                draw_line(r.x + 7.0, base - 24.0, r.x + 16.0, base - 27.0, 2.0, WOOD_LINE);
                draw_rectangle(r.x + 14.0, base - 30.0, 5.0, 4.0, stone);
                draw_circle(r.right() - 8.0, base - 25.0, 3.0, stone);
            } else {
                // Uma caneca e um prato.
                draw_rectangle(r.x + 8.0, base - 28.0, 4.0, 6.0, Color::from_rgba(200, 200, 205, 255));
                draw_ellipse(
                    r.right() - 10.0,
                    base - 22.0,
                    5.0,
                    2.0,
                    0.0,
                    Color::from_rgba(235, 230, 220, 255),
                );
            }
        }
        StructureKind::Chair => {
            let (cx, w) = (r.center().x, 14.0);
            draw_ellipse(cx, base, 8.0, 3.0, 0.0, SHADOW);
            for lx in [cx - w / 2.0, cx + w / 2.0 - 2.0] {
                draw_rectangle(lx, base - 10.0, 2.0, 10.0, WOOD_LINE);
            }
            draw_rectangle(cx - w / 2.0, base - 22.0, w, 9.0, WOOD_FRONT);
            draw_rectangle(cx - w / 2.0, base - 13.0, w, 4.0, WOOD_TOP);
        }
        StructureKind::Bed => {
            draw_ellipse(r.center().x, base, r.w / 2.0, 4.0, 0.0, SHADOW);
            draw_rectangle(r.x + 3.0, r.y - 2.0, r.w - 6.0, r.h - 2.0, WOOD_FRONT);
            draw_rectangle(r.x + 3.0, r.y - 8.0, r.w - 6.0, 8.0, WOOD_LINE);
            draw_rectangle(
                r.x + 5.0,
                r.y,
                r.w - 10.0,
                r.h - 8.0,
                Color::from_rgba(236, 232, 222, 255),
            );
            draw_rectangle(r.x + 8.0, r.y + 2.0, r.w - 16.0, 6.0, WHITE);
            draw_rectangle(
                r.x + 5.0,
                r.y + 11.0,
                r.w - 10.0,
                r.h - 19.0,
                Color::from_rgba(170, 52, 52, 255),
            );
            draw_line(
                r.x + 5.0,
                r.y + 11.0,
                r.right() - 5.0,
                r.y + 11.0,
                1.5,
                Color::from_rgba(120, 30, 30, 255),
            );
            draw_rectangle(r.x + 3.0, base - 4.0, r.w - 6.0, 4.0, WOOD_LINE);
        }
        StructureKind::Torch => {
            let c = vec2(r.center().x, base);
            draw_ellipse(c.x, c.y, 5.0, 2.0, 0.0, SHADOW);
            draw_line(c.x, c.y, c.x, c.y - 22.0, 3.0, WOOD_LINE);
            draw_rectangle(c.x - 3.0, c.y - 25.0, 6.0, 4.0, Color::from_rgba(90, 70, 50, 255));
            let flick = (time * 13.0 + r.x).sin() * 1.5;
            draw_triangle(
                vec2(c.x - 4.0, c.y - 25.0),
                vec2(c.x + 4.0, c.y - 25.0),
                vec2(c.x + flick * 0.5, c.y - 37.0 - flick),
                Color::from_rgba(240, 120, 30, 255),
            );
            draw_triangle(
                vec2(c.x - 2.0, c.y - 25.0),
                vec2(c.x + 2.0, c.y - 25.0),
                vec2(c.x - flick * 0.3, c.y - 31.0),
                Color::from_rgba(255, 220, 100, 255),
            );
        }
        StructureKind::FlowerPot => {
            let c = vec2(r.center().x, base);
            draw_ellipse(c.x, c.y, 8.0, 3.0, 0.0, SHADOW);
            let clay = Color::from_rgba(184, 98, 60, 255);
            draw_triangle(
                vec2(c.x - 7.0, c.y - 12.0),
                vec2(c.x + 7.0, c.y - 12.0),
                vec2(c.x + 5.0, c.y),
                clay,
            );
            draw_triangle(
                vec2(c.x - 7.0, c.y - 12.0),
                vec2(c.x + 5.0, c.y),
                vec2(c.x - 5.0, c.y),
                clay,
            );
            draw_rectangle(c.x - 8.0, c.y - 14.0, 16.0, 3.0, shade(clay, 1.15));
            let leaf = Color::from_rgba(70, 140, 60, 255);
            for (dx, dy) in [(-5.0, -18.0), (4.0, -19.0), (0.0, -23.0), (-2.0, -16.0)] {
                draw_circle(c.x + dx, c.y + dy, 4.0, leaf);
            }
            for (dx, dy, col) in [
                (-4.0, -22.0, Color::from_rgba(240, 90, 120, 255)),
                (4.0, -24.0, Color::from_rgba(250, 210, 80, 255)),
                (1.0, -27.0, Color::from_rgba(200, 120, 230, 255)),
            ] {
                draw_circle(c.x + dx, c.y + dy, 2.2, col);
            }
        }
        StructureKind::Rug => draw_rug(r),
        StructureKind::Campfire => draw_campfire(r, s.is_lit(), time),
        StructureKind::Forge => draw_forge(r, s.is_lit(), time),
        _ => {}
    }
}

fn draw_rug(r: Rect) {
    let inner = Rect::new(r.x + 3.0, r.y + 5.0, r.w - 6.0, r.h - 10.0);
    draw_rectangle(inner.x, inner.y, inner.w, inner.h, Color::from_rgba(150, 48, 52, 255));
    let gold = Color::from_rgba(220, 180, 90, 255);
    draw_rectangle_lines(inner.x + 2.0, inner.y + 2.0, inner.w - 4.0, inner.h - 4.0, 1.5, gold);
    let c = inner.center();
    draw_triangle(vec2(c.x - 6.0, c.y), vec2(c.x, c.y - 5.0), vec2(c.x + 6.0, c.y), gold);
    draw_triangle(vec2(c.x - 6.0, c.y), vec2(c.x, c.y + 5.0), vec2(c.x + 6.0, c.y), gold);
    for x in [inner.x, inner.right()] {
        for i in 0..4 {
            let y = inner.y + 3.0 + i as f32 * (inner.h - 6.0) / 3.0;
            draw_line(x, y, x + if x == inner.x { -2.0 } else { 2.0 }, y, 1.0, gold);
        }
    }
}

/// Cores (topo, frente, linhas) de cada material.
fn material_colors(m: Material) -> (Color, Color, Color) {
    match m {
        Material::Wood => (WOOD_TOP, WOOD_FRONT, WOOD_LINE),
        Material::Stone => (
            Color::from_rgba(172, 168, 160, 255),
            Color::from_rgba(128, 124, 118, 255),
            Color::from_rgba(84, 80, 76, 255),
        ),
        Material::Metal => (
            Color::from_rgba(160, 170, 182, 255),
            Color::from_rgba(106, 116, 130, 255),
            Color::from_rgba(60, 66, 78, 255),
        ),
    }
}

/// Textura da face da frente: tábuas, blocos de pedra ou chapas com rebites.
fn draw_wall_face(face: Rect, m: Material, line: Color) {
    match m {
        Material::Wood => {
            let n = (face.w / 8.0).round().max(1.0) as i32;
            for i in 1..n {
                let x = face.x + i as f32 * face.w / n as f32;
                draw_line(x, face.y, x, face.bottom(), 1.0, line);
            }
        }
        Material::Stone => {
            let rows = 3;
            let h = face.h / rows as f32;
            for row in 0..rows {
                let y = face.y + row as f32 * h;
                if row > 0 {
                    draw_line(face.x, y, face.right(), y, 1.0, line);
                }
                let offset = if row % 2 == 0 { 0.0 } else { 5.0 };
                let mut x = face.x + offset + 10.0;
                while x < face.right() - 1.0 {
                    draw_line(x, y, x, y + h, 1.0, line);
                    x += 10.0;
                }
            }
        }
        Material::Metal => {
            let n = (face.w / 16.0).round().max(1.0) as i32;
            for i in 0..n {
                let x0 = face.x + i as f32 * face.w / n as f32;
                if i > 0 {
                    draw_line(x0, face.y, x0, face.bottom(), 1.5, line);
                }
                for (dx, dy) in [(3.0, 3.0), (face.w / n as f32 - 3.0, 3.0)] {
                    draw_circle(x0 + dx, face.y + dy, 0.9, line);
                    draw_circle(x0 + dx, face.bottom() - dy, 0.9, line);
                }
            }
        }
    }
}

/// Bloco de parede em 3/4: face de cima deslocada para cima + face frontal com a textura do material.
fn draw_wall(r: Rect, m: Material) {
    let (top, front, line) = material_colors(m);
    draw_rectangle(r.x, r.y - WALL_H, r.w, r.h, top);
    // Moldura interna na face de cima, para não confundir com o piso.
    draw_rectangle_lines(r.x + 4.0, r.y - WALL_H + 4.0, r.w - 8.0, r.h - 8.0, 1.0, line);
    let face = Rect::new(r.x, r.bottom() - WALL_H, r.w, WALL_H);
    draw_rectangle(face.x, face.y, face.w, face.h, front);
    draw_wall_face(face, m, line);
    draw_line(r.x, face.y, r.right(), face.y, 1.5, line);
    draw_rectangle_lines(r.x, r.y - WALL_H, r.w, r.h + WALL_H, 1.0, line);
}

/// Batente fino (portão aberto).
fn draw_post(r: Rect, m: Material) {
    let (top, front, line) = material_colors(m);
    draw_rectangle(r.x, r.y - WALL_H, r.w, r.h, top);
    draw_rectangle(r.x, r.bottom() - WALL_H, r.w, WALL_H, front);
    draw_rectangle_lines(r.x, r.y - WALL_H, r.w, r.h + WALL_H, 1.0, line);
}

/// Cerca baixa: mourão no meio do tile e travessas até as cercas e paredes vizinhas.
fn draw_fence(world: &World, s: &Structure, m: Material) {
    let (top, front, line) = material_colors(m);
    let c = s.rect().center() + vec2(0.0, 6.0);
    let joins = |dx: i32, dy: i32| {
        world
            .structure_at((s.tile.0 + dx, s.tile.1 + dy), building::Layer::Object)
            .is_some_and(|i| {
                matches!(
                    world.structures[i].kind.piece(),
                    Piece::Fence | Piece::Wall | Piece::Door | Piece::Window | Piece::Gate
                )
            })
    };
    let (left, right, up, down) = (joins(-1, 0), joins(1, 0), joins(0, -1), joins(0, 1));
    let half = TILE / 2.0;
    let horizontal = left || right || !(up || down);
    for h in [7.0, 14.0] {
        if horizontal {
            let x0 = if left || !(right || up || down) {
                c.x - half
            } else {
                c.x
            };
            let x1 = if right || !(left || up || down) {
                c.x + half
            } else {
                c.x
            };
            draw_line(x0, c.y - h, x1, c.y - h, 2.5, front);
        }
    }
    if up {
        draw_rectangle(c.x - 1.5, c.y - half - 14.0, 3.0, half, front);
    }
    if down {
        draw_rectangle(c.x - 1.5, c.y - 14.0, 3.0, half, front);
    }
    draw_ellipse(c.x, c.y, 4.0, 1.5, 0.0, SHADOW);
    draw_rectangle(c.x - 2.5, c.y - 19.0, 5.0, 19.0, top);
    draw_rectangle_lines(c.x - 2.5, c.y - 19.0, 5.0, 19.0, 1.0, line);
}

/// Barra de vida sobre estruturas danificadas.
fn draw_structure_health(s: &Structure) {
    let frac = s.health_frac();
    if frac >= 1.0 {
        return;
    }
    let r = s.rect();
    let y = if s.kind.flat() { r.y - 6.0 } else { r.y - WALL_H - 6.0 };
    let w = (r.w - 8.0).min(48.0);
    let x = r.center().x - w / 2.0;
    draw_rectangle(x, y, w, 3.0, Color::new(0.0, 0.0, 0.0, 0.6));
    let color = if frac < 0.3 {
        Color::new(0.9, 0.3, 0.25, 1.0)
    } else {
        Color::new(0.95, 0.75, 0.3, 1.0)
    };
    draw_rectangle(x, y, w * frac, 3.0, color);
}

/// Tetos por cima de tudo. Os tiles de `hidden` (o cômodo onde o jogador está) ficam quase transparentes.
pub fn draw_roofs(world: &World, view: Rect, hidden: &HashSet<(i32, i32)>) {
    let roofs: HashSet<(i32, i32)> = world
        .structures
        .iter()
        .filter(|s| s.kind.layer() == building::Layer::Roof)
        .map(|s| s.tile)
        .collect();
    for s in world
        .structures
        .iter()
        .filter(|s| s.kind.layer() == building::Layer::Roof && s.rect().overlaps(&view))
    {
        let alpha = if hidden.contains(&s.tile) { 0.15 } else { 1.0 };
        let (top, line) = match s.kind.material().unwrap_or(Material::Wood) {
            Material::Wood => (Color::from_rgba(186, 150, 86, 255), Color::from_rgba(128, 96, 50, 255)),
            Material::Stone => (Color::from_rgba(112, 116, 128, 255), Color::from_rgba(72, 74, 86, 255)),
            Material::Metal => (
                Color::from_rgba(146, 158, 170, 255),
                Color::from_rgba(90, 100, 114, 255),
            ),
        };
        let (top, line) = (top.with_alpha(alpha), line.with_alpha(alpha));
        let r = s.rect();
        let y = r.y - WALL_H;
        draw_rectangle(r.x, y, r.w, r.h, top);
        match s.kind.material() {
            Some(Material::Metal) => {
                for i in 1..8 {
                    let x = r.x + i as f32 * r.w / 8.0;
                    draw_line(x, y, x, y + r.h, 1.0, line);
                }
            }
            Some(Material::Stone) => {
                for row in 0..4 {
                    let ry = y + row as f32 * 8.0 + 8.0;
                    draw_line(r.x, ry, r.right(), ry, 1.0, line);
                    let offset = if row % 2 == 0 { 4.0 } else { 0.0 };
                    for i in 0..4 {
                        let x = r.x + offset + i as f32 * 8.0;
                        draw_line(x, ry - 8.0, x, ry, 1.0, line);
                    }
                }
            }
            _ => {
                for row in 1..4 {
                    let ry = y + row as f32 * 8.0;
                    draw_line(r.x, ry, r.right(), ry, 1.5, line);
                }
            }
        }
        // Bordas onde o teto acaba (e o beiral na frente).
        let (x, t) = (s.tile.0, s.tile.1);
        if !roofs.contains(&(x - 1, t)) {
            draw_line(r.x, y, r.x, y + r.h, 2.0, line);
        }
        if !roofs.contains(&(x + 1, t)) {
            draw_line(r.right(), y, r.right(), y + r.h, 2.0, line);
        }
        if !roofs.contains(&(x, t - 1)) {
            draw_line(r.x, y, r.right(), y, 2.0, line);
        }
        if !roofs.contains(&(x, t + 1)) {
            draw_rectangle(r.x, y + r.h, r.w, 4.0, line);
        }
    }
}

/// Forja de pedra em 3/4, com a boca brilhando quando acesa.
fn draw_forge(r: Rect, lit: bool, time: f32) {
    let top = Color::from_rgba(150, 142, 136, 255);
    let front = Color::from_rgba(112, 104, 100, 255);
    let line = Color::from_rgba(80, 74, 70, 255);
    draw_rectangle(r.x + 2.0, r.y - WALL_H + 4.0, r.w - 4.0, r.h - 6.0, top);
    let front_y = r.bottom() - WALL_H;
    draw_rectangle(r.x + 2.0, front_y, r.w - 4.0, WALL_H, front);
    for row in 0..3 {
        let y = front_y + row as f32 * 8.0;
        draw_line(r.x + 2.0, y, r.right() - 2.0, y, 1.0, line);
    }
    // Chaminé.
    draw_rectangle(r.right() - 10.0, r.y - WALL_H - 8.0, 6.0, 12.0, front);
    let mouth = Rect::new(r.center().x - 7.0, r.bottom() - 13.0, 14.0, 11.0);
    draw_rectangle(mouth.x, mouth.y, mouth.w, mouth.h, Color::from_rgba(36, 28, 26, 255));
    if lit {
        let glow = 0.75 + (time * 9.0).sin() * 0.15;
        draw_rectangle(
            mouth.x + 2.0,
            mouth.y + 4.0,
            mouth.w - 4.0,
            mouth.h - 4.0,
            Color::new(1.0, 0.5 * glow, 0.1, 1.0),
        );
        draw_circle(
            r.right() - 7.0,
            r.y - WALL_H - 12.0 - (time * 20.0) % 10.0,
            2.5,
            Color::new(0.6, 0.6, 0.6, 0.5),
        );
    }
    draw_rectangle_lines(r.x + 2.0, r.y - WALL_H + 4.0, r.w - 4.0, r.h + WALL_H - 6.0, 1.0, line);
}

fn draw_campfire(r: Rect, lit: bool, time: f32) {
    let c = vec2(r.center().x, r.bottom() - 10.0);
    let stone = Color::from_rgba(120, 118, 122, 255);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        draw_circle(c.x + a.cos() * 11.0, c.y + a.sin() * 5.0, 3.0, stone);
    }
    let wood = Color::from_rgba(110, 72, 40, 255);
    draw_line(c.x - 8.0, c.y + 2.0, c.x + 8.0, c.y - 2.0, 3.0, wood);
    draw_line(c.x - 8.0, c.y - 2.0, c.x + 8.0, c.y + 2.0, 3.0, wood);
    if lit {
        let flick = (time * 14.0).sin() * 2.0 + (time * 23.0).sin();
        draw_triangle(
            vec2(c.x - 7.0, c.y),
            vec2(c.x + 7.0, c.y),
            vec2(c.x + flick * 0.5, c.y - 20.0 - flick),
            Color::from_rgba(240, 110, 30, 255),
        );
        draw_triangle(
            vec2(c.x - 4.0, c.y),
            vec2(c.x + 4.0, c.y),
            vec2(c.x - flick * 0.3, c.y - 11.0 + flick * 0.5),
            Color::from_rgba(255, 215, 90, 255),
        );
    }
}

/// Prévia translúcida da estrutura a posicionar, verde se válida e vermelha se não.
pub fn draw_ghost(kind: StructureKind, tile: (i32, i32), vertical: bool, valid: bool) {
    let r = building::footprint(kind, tile, vertical && kind.rotates());
    let color = if valid {
        Color::new(0.4, 1.0, 0.5, 0.35)
    } else {
        Color::new(1.0, 0.35, 0.3, 0.35)
    };
    draw_rectangle(r.x, r.y, r.w, r.h, color);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, color.with_alpha(0.9));
    let icon = building::tile_rect(tile);
    crate::ui::draw_item_icon(
        kind.item(),
        Rect::new(icon.x + 4.0, icon.y + 4.0, icon.w - 8.0, icon.h - 8.0),
    );
}

/// Escuridão noturna como malha com cor por vértice: gradientes suaves em volta das luzes.
/// `lights`: (posição, raio, quente) — luzes quentes tingem de laranja.
pub fn draw_lighting(view: Rect, darkness: f32, lights: &[(Vec2, f32, bool)]) {
    if darkness <= 0.01 {
        return;
    }
    const CELL: f32 = 8.0;
    let cols = (view.w / CELL).ceil() as usize + 1;
    let rows = (view.h / CELL).ceil() as usize + 1;
    let (x0, y0) = ((view.x / CELL).floor() * CELL, (view.y / CELL).floor() * CELL);
    let night = Color::new(0.02, 0.03, 0.10, 1.0);
    let fire = Color::new(1.0, 0.55, 0.15, 1.0);

    let vertex_color = |p: Vec2| {
        let (mut lit, mut warm) = (0.0f32, 0.0f32);
        for &(pos, radius, hot) in lights {
            let t = (1.0 - p.distance(pos) / radius).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            lit = lit.max(t);
            if hot {
                warm = warm.max(t);
            }
        }
        let alpha = darkness * (1.0 - lit) + warm * 0.18 * darkness;
        let c = Color::new(
            night.r + (fire.r - night.r) * warm,
            night.g + (fire.g - night.g) * warm,
            night.b + (fire.b - night.b) * warm,
            alpha,
        );
        Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, c)
    };

    // Uma faixa por linha para respeitar o limite de índices por desenho do macroquad.
    let mut top: Vec<Vertex> = (0..=cols)
        .map(|c| vertex_color(vec2(x0 + c as f32 * CELL, y0)))
        .collect();
    for row in 1..=rows {
        let y = y0 + row as f32 * CELL;
        let bottom: Vec<Vertex> = (0..=cols)
            .map(|c| vertex_color(vec2(x0 + c as f32 * CELL, y)))
            .collect();
        let mut vertices = Vec::with_capacity((cols + 1) * 2);
        vertices.extend_from_slice(&top);
        vertices.extend_from_slice(&bottom);
        let n = cols as u16 + 1;
        let indices = (0..cols as u16)
            .flat_map(|c| [c, c + 1, n + c, c + 1, n + c + 1, n + c])
            .collect();
        draw_mesh(&Mesh {
            vertices,
            indices,
            texture: None,
        });
        top = bottom;
    }
}

/// Tom alaranjado do nascer/pôr do sol, sobre a tela toda.
pub fn draw_golden_hour(view: Rect, intensity: f32) {
    if intensity > 0.01 {
        draw_rectangle(
            view.x,
            view.y,
            view.w,
            view.h,
            Color::new(1.0, 0.45, 0.15, 0.16 * intensity),
        );
    }
}

fn draw_rock(pos: Vec2, variant: u8) {
    let (x, y) = (pos.x, pos.y);
    let s = 1.0 + variant as f32 * 0.08;
    draw_ellipse(x, y, 20.0 * s, 7.0, 0.0, SHADOW);
    draw_ellipse(
        x,
        y - 8.0 * s,
        18.0 * s,
        11.0 * s,
        0.0,
        Color::from_rgba(104, 104, 112, 255),
    );
    draw_ellipse(
        x - 2.0,
        y - 12.0 * s,
        14.0 * s,
        8.0 * s,
        0.0,
        Color::from_rgba(140, 140, 148, 255),
    );
    draw_ellipse(
        x - 6.0,
        y - 15.0 * s,
        5.0,
        2.5,
        0.0,
        Color::from_rgba(180, 180, 186, 255),
    );
}

fn draw_metal_rock(pos: Vec2, variant: u8) {
    let (x, y) = (pos.x, pos.y);
    let s = 1.0 + variant as f32 * 0.06;
    draw_ellipse(x, y, 20.0 * s, 7.0, 0.0, SHADOW);
    draw_ellipse(
        x,
        y - 8.0 * s,
        18.0 * s,
        12.0 * s,
        0.0,
        Color::from_rgba(78, 74, 80, 255),
    );
    draw_ellipse(
        x - 2.0,
        y - 12.0 * s,
        14.0 * s,
        8.0 * s,
        0.0,
        Color::from_rgba(104, 98, 104, 255),
    );
    // Veios de metal.
    let vein = Color::from_rgba(196, 120, 70, 255);
    draw_line(x - 10.0, y - 10.0, x - 2.0, y - 15.0, 2.0, vein);
    draw_line(x + 2.0, y - 6.0, x + 10.0, y - 12.0, 2.0, vein);
    draw_circle(x - 6.0, y - 16.0 * s, 1.6, Color::from_rgba(235, 235, 245, 255));
    draw_circle(x + 7.0, y - 9.0, 1.2, Color::from_rgba(235, 235, 245, 255));
}

fn draw_pebbles(pos: Vec2, variant: u8) {
    let c = Color::from_rgba(130, 128, 132, 255);
    let hi = Color::from_rgba(170, 168, 172, 255);
    let offsets: &[(f32, f32, f32)] = match variant % 2 {
        0 => &[(-4.0, 0.0, 4.0), (3.0, -2.0, 3.0), (2.0, 3.0, 2.5)],
        _ => &[(0.0, 0.0, 4.5), (-5.0, 2.0, 2.5)],
    };
    for &(dx, dy, r) in offsets {
        draw_ellipse(pos.x + dx, pos.y + dy + 1.0, r, r * 0.5, 0.0, SHADOW);
        draw_ellipse(pos.x + dx, pos.y + dy - 1.0, r, r * 0.7, 0.0, c);
        draw_ellipse(pos.x + dx - 1.0, pos.y + dy - 2.0, r * 0.5, r * 0.3, 0.0, hi);
    }
}

/// Nomes dos outros jogadores (espaço de tela, acima da cabeça).
pub fn draw_names(avatars: &[Avatar], creatures: &[Creature], camera: &Camera2D) {
    for a in avatars {
        let Some(name) = a.name else { continue };
        let mount = a.mount.and_then(|id| creatures.iter().find(|c| c.id == id));
        let lift = match mount {
            Some(c) => 42.0 + saddle_height(c.species) + lift(c),
            None => 42.0,
        };
        let at = camera.world_to_screen(a.player.pos - vec2(0.0, lift));
        let w = measure_text(name, None, 18, 1.0).width;
        draw_rectangle(
            at.x - w / 2.0 - 4.0,
            at.y - 15.0,
            w + 8.0,
            20.0,
            Color::new(0.0, 0.0, 0.0, 0.45),
        );
        let (text, bar) = if a.hostile {
            (Color::new(1.0, 0.5, 0.45, 1.0), Color::new(0.9, 0.3, 0.25, 1.0))
        } else {
            (WHITE, Color::new(0.4, 0.85, 0.4, 1.0))
        };
        draw_text(name, at.x - w / 2.0, at.y, 18.0, text);
        if let Some(frac) = a.health {
            let bw = 40.0;
            draw_rectangle(at.x - bw / 2.0, at.y + 6.0, bw, 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
            draw_rectangle(at.x - bw / 2.0, at.y + 6.0, bw * frac, 4.0, bar);
        }
    }
}

/// `seat`: altura da sela quando montado (0 = a pé). `band`: faixa da tribo na cabeça.
fn draw_player(p: &Player, held: Option<ItemId>, shirt: Color, hair: Color, band: Option<Color>, seat: f32) {
    let s = p.facing.sign();
    let mounted = seat > 0.0;
    let (x, y) = (p.pos.x, p.pos.y - seat);
    let step = if p.moving && !mounted {
        (p.anim_time * 12.0).sin()
    } else {
        0.0
    };
    let bob = step.abs() * -1.5;

    let skin = Color::from_rgba(232, 186, 146, 255);
    let pants = Color::from_rgba(70, 64, 92, 255);

    if mounted {
        // Pernas abertas sobre a sela.
        draw_rectangle(x - 7.0, y - 9.0, 4.0, 10.0, pants);
        draw_rectangle(x + 3.0, y - 9.0, 4.0, 10.0, pants);
    } else {
        draw_ellipse(x, y, 9.0, 3.5, 0.0, SHADOW);
        draw_rectangle(x - 5.0 + step * 2.0, y - 9.0, 4.0, 9.0, pants);
        draw_rectangle(x + 1.0 - step * 2.0, y - 9.0, 4.0, 9.0, pants);
    }
    // Tronco
    draw_rectangle(x - 6.0, y - 22.0 + bob, 12.0, 14.0, shirt);
    // Cabeça
    let hy = y - 28.0 + bob;
    draw_circle(x, hy, 7.0, skin);
    draw_rectangle(x - 7.0, hy - 7.0, 14.0, 4.0, hair);
    draw_rectangle(x - 7.0 * s, hy - 5.0, 4.0 * s, 6.0, hair);
    if let Some(band) = band {
        draw_rectangle(x - 7.0, hy - 4.5, 14.0, 2.5, band);
    }
    draw_rectangle(x + 3.0 * s - 1.0, hy - 1.0, 2.0, 3.0, Color::from_rgba(40, 30, 30, 255));

    // Braço + item: ângulo 0 = para frente; negativo = para cima.
    let angle = if p.swing_timer > 0.0 {
        let t = 1.0 - p.swing_timer / SWING_TIME;
        (-110.0 + 160.0 * (t * t * (3.0 - 2.0 * t))).to_radians()
    } else {
        60.0_f32.to_radians()
    };
    let dir = vec2(angle.cos() * s, angle.sin());
    let shoulder = vec2(x + 2.0 * s, y - 19.0 + bob);
    let hand = shoulder + dir * 8.0;
    draw_line(shoulder.x, shoulder.y, hand.x, hand.y, 3.0, skin);
    if let Some(item) = held {
        draw_held_item(item, hand, dir);
    }
}

fn draw_held_item(item: ItemId, hand: Vec2, dir: Vec2) {
    let wood = Color::from_rgba(150, 104, 60, 255);
    let stone = Color::from_rgba(150, 150, 158, 255);
    let perp = vec2(-dir.y, dir.x);
    match item {
        ItemId::StonePickaxe => {
            let tip = hand + dir * 14.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.0, wood);
            let (a, b) = (tip + perp * 6.0 - dir * 2.0, tip - perp * 6.0 - dir * 2.0);
            draw_line(a.x, a.y, b.x, b.y, 3.0, stone);
        }
        ItemId::StoneHatchet => {
            let tip = hand + dir * 13.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.0, wood);
            let head = tip - dir * 2.0 + perp * 3.0;
            draw_circle(head.x, head.y, 4.0, stone);
        }
        ItemId::MetalPickaxe => {
            let tip = hand + dir * 14.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.0, wood);
            let (a, b) = (tip + perp * 7.0 - dir * 2.0, tip - perp * 7.0 - dir * 2.0);
            draw_line(a.x, a.y, b.x, b.y, 3.0, Color::from_rgba(200, 206, 216, 255));
        }
        ItemId::MetalHatchet => {
            let tip = hand + dir * 13.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.0, wood);
            let head = tip - dir * 2.0 + perp * 3.0;
            draw_circle(head.x, head.y, 4.5, Color::from_rgba(200, 206, 216, 255));
        }
        ItemId::MetalSword => {
            let tip = hand + dir * 22.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.5, Color::from_rgba(210, 216, 226, 255));
            let (a, b) = (hand + dir * 3.0 + perp * 4.0, hand + dir * 3.0 - perp * 4.0);
            draw_line(a.x, a.y, b.x, b.y, 2.0, Color::from_rgba(150, 120, 60, 255));
        }
        ItemId::WoodenClub => {
            let tip = hand + dir * 12.0;
            draw_line(hand.x, hand.y, tip.x, tip.y, 2.5, wood);
            draw_circle(tip.x, tip.y, 3.5, wood);
        }
        ItemId::Slingshot => {
            let fork = hand + dir * 6.0;
            draw_line(hand.x, hand.y, fork.x, fork.y, 2.0, wood);
            for side in [-1.0, 1.0] {
                let prong = fork + dir * 4.0 + perp * 3.0 * side;
                draw_line(fork.x, fork.y, prong.x, prong.y, 1.5, wood);
            }
        }
        ItemId::WoodenSpear => {
            let (back, tip) = (hand - dir * 8.0, hand + dir * 20.0);
            draw_line(back.x, back.y, tip.x, tip.y, 2.0, wood);
            draw_triangle(
                tip + dir * 6.0,
                tip + perp * 2.5,
                tip - perp * 2.5,
                Color::from_rgba(60, 60, 70, 255),
            );
        }
        _ => {}
    }
}

/// Números flutuantes (“+3 Madeira”, “-22”). Desenhados em espaço de tela para texto nítido.
pub struct Popup {
    pub pos: Vec2,
    pub text: String,
    pub style: PopupStyle,
    pub age: f32,
}

pub const POPUP_LIFETIME: f32 = 1.2;

pub fn draw_popups(popups: &[Popup], camera: &Camera2D) {
    for p in popups {
        let t = p.age / POPUP_LIFETIME;
        let screen = camera.world_to_screen(p.pos - vec2(0.0, t * 24.0));
        let alpha = 1.0 - t * t;
        let size = 22.0;
        let w = measure_text(&p.text, None, size as u16, 1.0).width;
        draw_text(
            &p.text,
            screen.x - w / 2.0 + 1.0,
            screen.y + 1.0,
            size,
            Color::new(0.0, 0.0, 0.0, alpha * 0.6),
        );
        let color = match p.style {
            PopupStyle::Gain => Color::new(1.0, 0.95, 0.7, alpha),
            PopupStyle::Damage => Color::new(1.0, 1.0, 1.0, alpha),
            PopupStyle::Hurt => Color::new(1.0, 0.35, 0.3, alpha),
        };
        draw_text(&p.text, screen.x - w / 2.0, screen.y, size, color);
    }
}

/// Mistura `c` com vermelho/branco enquanto a criatura pisca por dano.
fn hurt_tint(c: Color, hurt: f32) -> Color {
    let t = (hurt / 0.2).clamp(0.0, 1.0) * 0.7;
    Color::new(c.r + (1.0 - c.r) * t, c.g * (1.0 - t), c.b * (1.0 - t), c.a)
}

/// Sombra no chão, menor quanto mais alto.
fn draw_shadow(c: &Creature) {
    let k = 1.0 - c.altitude * 0.35;
    let r = c.species.radius() * if c.species.flies() { 1.6 } else { 1.0 };
    draw_ellipse(c.pos.x, c.pos.y, (r + 3.0) * k, 4.0 * k, 0.0, SHADOW);
}

/// `shadow`: desenhar a sombra junto (voadores bem no alto já tiveram a sombra desenhada antes).
/// `water`: cor da água em que a criatura está (desenha a linha d'água por cima).
fn draw_creature(c: &Creature, time: f32, shadow: bool, water: Option<Color>) {
    let s = if c.facing_left { -1.0 } else { 1.0 };
    let lunge = (c.lunge / 0.25) * 6.0 * s;
    let step = if c.moving { (c.anim_time * 9.0).sin() } else { 0.0 };
    let r = c.species.radius();
    if shadow {
        draw_shadow(c);
    }
    let pos = c.pos + vec2(lunge, -lift(c));
    if !c.is_alive() || c.knocked_out {
        draw_lying(c.species, pos, s, c.knocked_out);
        if c.knocked_out {
            // "z" subindo devagar.
            for i in 0..3 {
                let t = (time * 0.6 + i as f32 / 3.0).fract();
                let zx = pos.x + s * (4.0 + t * 8.0);
                let zy = pos.y - 12.0 - t * 18.0;
                let size = 10.0 + t * 6.0;
                draw_text("z", zx, zy, size, Color::new(1.0, 1.0, 1.0, 1.0 - t));
            }
        }
        return;
    }
    if c.is_tamed() {
        // Marcador de domesticado: losango verde acima da cabeça.
        let top = vec2(pos.x, c.pos.y - head_height(c) - 2.0 - if c.flying { 8.0 } else { 0.0 });
        draw_poly(top.x, top.y, 4, 3.0, 45.0, Color::new(0.4, 0.95, 0.5, 0.9));
    }
    let tint = |col| hurt_tint(col, c.hurt);
    // Batida de asa: rápida subindo/descendo, devagar planando parado no ar.
    let flap = (c.altitude > 0.05).then(|| {
        let speed = if c.moving { 9.0 } else { 5.0 };
        ((time + c.id as f32 * 0.37) * speed).sin() * c.altitude.min(1.0)
    });
    match c.species {
        Species::Dodo => draw_dodo(pos, s, step, &tint),
        Species::Trilobite => draw_trilobite(pos, step, &tint),
        Species::Dilophosaur => draw_theropod(
            pos,
            s,
            step,
            1.0,
            true,
            tint(Color::from_rgba(92, 140, 70, 255)),
            tint(Color::from_rgba(230, 196, 70, 255)),
        ),
        Species::Raptor => draw_theropod(
            pos,
            s,
            step,
            1.1,
            false,
            tint(Color::from_rgba(176, 110, 60, 255)),
            tint(Color::from_rgba(110, 64, 34, 255)),
        ),
        Species::Pteranodon => draw_pteranodon(pos, s, step, flap, &tint),
        Species::Argentavis => draw_argentavis(pos, s, step, flap, &tint),
        Species::Mammoth => draw_mammoth(pos, s, step, &tint),
        Species::Smilodon => draw_smilodon(pos, s, step, &tint),
        Species::Wolf => draw_wolf(pos, s, step, &tint),
        Species::Beelzebufo => draw_beelzebufo(pos, s, step, &tint),
        Species::Sarcosuchus => draw_sarcosuchus(pos, s, step, &tint),
        Species::Titanoboa => {
            let wave = if c.moving { c.anim_time } else { time * 0.4 };
            draw_titanoboa(pos, s, wave, &tint);
        }
        Species::Piranha => draw_piranha(pos, s, step, &tint),
        Species::Megalodon => draw_megalodon(pos, s, step, &tint),
        Species::Ichthyosaur => draw_ichthyosaur(pos, s, step, &tint),
        Species::Mosasaur => draw_mosasaur(pos, s, step, &tint),
        Species::Kraken => {
            let wave = if c.moving { c.anim_time } else { time * 0.6 };
            draw_kraken(pos, s, wave, &tint);
        }
        // Golem selvagem parado é só mais uma rocha no deserto.
        Species::StoneGolem if c.is_wild() && !c.moving && !c.chasing() && c.hurt <= 0.0 => {
            draw_rock(pos, (c.id % 4) as u8)
        }
        Species::StoneGolem => draw_golem(pos, s, step, &tint),
        Species::Kangaroo => draw_kangaroo(pos, s, step, &tint),
        Species::Deathworm => draw_deathworm(pos, time + c.id as f32, c.buried, &tint),
        Species::Mantis => draw_mantis(pos, s, step, &tint),
    }
    if let Some(color) = water {
        draw_waterline(pos, r, color, time);
    }
    if c.can_ride() {
        let seat = saddle_height(c.species);
        let (x, y) = (pos.x, pos.y);
        let leather = match c.species {
            Species::Pteranodon => Color::from_rgba(88, 70, 56, 255),
            _ => Color::from_rgba(96, 60, 32, 255),
        };
        let w = match c.species {
            Species::Mammoth => 20.0,
            Species::Argentavis | Species::Sarcosuchus | Species::Megalodon | Species::Kraken => 16.0,
            Species::Mosasaur => 20.0,
            _ => 13.0,
        };
        draw_rectangle(x - w / 2.0, y - seat - 5.0, w, 5.5, leather);
        draw_line(x, y - seat, x, y - seat + 6.0, 1.5, Color::from_rgba(70, 44, 24, 255));
    }
}

/// Asa vista de lado (3/4): quadrilátero do ombro até a ponta, que sobe na tela (`rise` > 0) ou desce.
/// `chord`: largura junto ao corpo; `sweep`: quanto a ponta vai para trás. Retorna a ponta de trás.
fn draw_wing(shoulder: Vec2, chord: f32, rise: f32, sweep: f32, s: f32, color: Color) -> Vec2 {
    let front = shoulder + vec2(chord * 0.5 * s, 0.0);
    let back = shoulder - vec2(chord * 0.5 * s, 0.0);
    let tip_back = shoulder + vec2(-(sweep + chord * 0.5) * s, -rise);
    let tip_front = shoulder + vec2((chord * 0.15 - sweep) * s, -rise * 1.08);
    draw_triangle(front, back, tip_back, color);
    draw_triangle(front, tip_back, tip_front, color);
    tip_back
}

/// Pteranodonte: corpo pequeno, bico comprido, crista para trás e asas de membrana.
/// `flap`: posição da batida de asa (-1 a 1) quando no ar; `None` = pousado sobre as asas dobradas.
fn draw_pteranodon(pos: Vec2, s: f32, step: f32, flap: Option<f32>, tint: &dyn Fn(Color) -> Color) {
    const K: f32 = 1.35;
    let (x, y) = (pos.x, pos.y);
    let body = tint(Color::from_rgba(132, 116, 102, 255));
    let wing = tint(Color::from_rgba(158, 102, 80, 255));
    let wing_far = tint(Color::from_rgba(116, 74, 60, 255));
    let crest = tint(Color::from_rgba(200, 84, 52, 255));
    let beak = tint(Color::from_rgba(222, 198, 142, 255));
    let yb = y - if flap.is_some() { 14.0 } else { 12.0 } * K;
    // Ponto relativo ao centro do corpo, já virado para o lado certo e na escala da espécie.
    let at = |dx: f32, dy: f32| vec2(x + dx * K * s, yb + dy * K);
    let shoulder = at(-1.0, -1.0);
    match flap {
        // Asa de trás sobe na tela e a da frente desce (vista 3/4); `f` bate as duas juntas.
        Some(f) => {
            draw_wing(shoulder, 12.0 * K, (20.0 + 10.0 * f) * K, 6.0 * K, s, wing_far);
            draw_line(
                at(-5.0, 1.0).x,
                at(-5.0, 1.0).y,
                at(-10.0, 3.0).x,
                at(-10.0, 3.0).y,
                1.5,
                body,
            );
        }
        None => {
            // De pé sobre os "punhos" das asas.
            let (hand, foot) = (at(4.0, 0.0), vec2(x + (7.0 + step * 2.0) * K * s, y));
            draw_line(hand.x, hand.y, foot.x, foot.y, 2.5, wing_far);
            let (hip, back_foot) = (at(-4.0, 2.0), vec2(x - (4.0 + step * 2.0) * K * s, y));
            draw_line(hip.x, hip.y, back_foot.x, back_foot.y, 2.0, body);
        }
    }
    draw_ellipse(x, yb, 9.0 * K, 4.5 * K, 0.0, body);
    let (neck_top, head) = match flap {
        Some(_) => (at(10.0, -4.0), at(11.0, -5.0)),
        None => (at(9.0, -10.0), at(10.0, -11.0)),
    };
    let neck = at(6.0, -1.0);
    draw_line(neck.x, neck.y, neck_top.x, neck_top.y, 3.0 * K, body);
    let rel = |dx: f32, dy: f32| head + vec2(dx * K * s, dy * K);
    draw_triangle(rel(1.0, -1.0), rel(-1.0, -2.5), rel(-10.0, -8.0), crest);
    draw_circle(head.x, head.y, 3.5 * K, body);
    draw_triangle(rel(2.0, -1.5), rel(2.0, 1.5), rel(15.0, 1.0), beak);
    let eye = rel(1.0, -0.5);
    draw_circle(eye.x, eye.y, 1.2, BLACK);
    match flap {
        Some(f) => {
            draw_wing(shoulder, 12.0 * K, (-14.0 + 22.0 * f) * K, 6.0 * K, s, wing);
        }
        None => draw_triangle(at(5.0, -2.0), at(-6.0, -1.0), at(6.0, 6.0), wing),
    }
}

/// Argentavis: ave enorme, escura, de cabeça pelada, colar branco e bico em gancho.
fn draw_argentavis(pos: Vec2, s: f32, step: f32, flap: Option<f32>, tint: &dyn Fn(Color) -> Color) {
    const K: f32 = 1.15;
    let (x, y) = (pos.x, pos.y);
    let body = tint(Color::from_rgba(78, 60, 48, 255));
    let wing = tint(Color::from_rgba(64, 48, 38, 255));
    let wing_far = tint(Color::from_rgba(46, 34, 28, 255));
    let tips = tint(Color::from_rgba(32, 24, 20, 255));
    let ruff = tint(Color::from_rgba(228, 222, 208, 255));
    let skin = tint(Color::from_rgba(206, 150, 122, 255));
    let beak = tint(Color::from_rgba(228, 196, 108, 255));
    let yb = y - 16.0 * K;
    let at = |dx: f32, dy: f32| vec2(x + dx * K * s, yb + dy * K);
    // Penas compridas ("dedos") na ponta da asa.
    let fingers = |tip: Vec2, color: Color| {
        for i in 0..3 {
            let end = tip + vec2((-3.0 - i as f32 * 3.0) * K * s, (4.0 + i as f32 * 2.0) * K);
            draw_line(tip.x, tip.y, end.x, end.y, 2.0, color);
        }
    };
    let head = |center: Vec2| {
        let rel = |dx: f32, dy: f32| center + vec2(dx * K * s, dy * K);
        let neck = rel(-4.0, 2.0);
        draw_circle(neck.x, neck.y, 5.0 * K, ruff);
        draw_circle(center.x, center.y, 4.2 * K, skin);
        draw_triangle(rel(3.0, -2.0), rel(3.0, 2.0), rel(8.0, 3.0), beak);
        let eye = rel(1.0, -1.0);
        draw_circle(eye.x, eye.y, 1.2, BLACK);
    };
    match flap {
        Some(f) => {
            let shoulder = at(-1.0, -2.0);
            let far_tip = draw_wing(shoulder, 16.0 * K, (26.0 + 10.0 * f) * K, 6.0 * K, s, wing_far);
            fingers(far_tip, tips);
            draw_triangle(at(-10.0, -2.0), at(-10.0, 3.0), at(-21.0, 1.0), wing);
            draw_ellipse(x, yb, 12.0 * K, 6.0 * K, 0.0, body);
            head(at(13.0, -5.0));
            let near_tip = draw_wing(shoulder, 16.0 * K, (-16.0 + 26.0 * f) * K, 6.0 * K, s, wing);
            fingers(near_tip, tips);
        }
        None => {
            let legs = tint(Color::from_rgba(200, 170, 90, 255));
            for (side, swing) in [(-3.0, step), (3.0, -step)] {
                let (hip, foot) = (vec2(x + side * K, y - 8.0 * K), vec2(x + (side + swing * 2.0) * K, y));
                draw_line(hip.x, hip.y, foot.x, foot.y, 2.5, legs);
            }
            draw_triangle(at(-8.0, -4.0), at(-6.0, 6.0), at(-18.0, 8.0), wing_far);
            draw_ellipse(x, yb, 11.0 * K, 9.0 * K, 0.0, body);
            let folded = at(-2.0, 1.0);
            draw_ellipse(folded.x, folded.y, 9.0 * K, 6.5 * K, 0.0, wing);
            head(at(9.0, -12.0));
        }
    }
}

fn draw_dodo(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let body = tint(Color::from_rgba(150, 128, 110, 255));
    let legs = tint(Color::from_rgba(214, 170, 60, 255));
    draw_line(x - 2.0, y - 5.0, x - 2.0 + step * 2.0, y, 2.0, legs);
    draw_line(x + 2.0, y - 5.0, x + 2.0 - step * 2.0, y, 2.0, legs);
    draw_ellipse(x, y - 10.0, 10.0, 8.0, 0.0, body);
    draw_ellipse(
        x - 7.0 * s,
        y - 12.0,
        4.0,
        3.0,
        0.0,
        tint(Color::from_rgba(230, 226, 220, 255)),
    );
    draw_circle(x + 7.0 * s, y - 18.0, 5.0, body);
    draw_triangle(
        vec2(x + 10.0 * s, y - 20.0),
        vec2(x + 10.0 * s, y - 15.0),
        vec2(x + 17.0 * s, y - 16.0),
        legs,
    );
    draw_circle(x + 8.0 * s, y - 19.0, 1.2, BLACK);
}

fn draw_trilobite(pos: Vec2, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let shell = tint(Color::from_rgba(96, 116, 140, 255));
    let ridge = tint(Color::from_rgba(70, 86, 108, 255));
    for i in -2..=2 {
        let lx = x + i as f32 * 3.5;
        draw_line(lx, y - 3.0, lx + step, y + 1.0, 1.2, ridge);
    }
    draw_ellipse(x, y - 4.0, 10.0, 6.0, 0.0, shell);
    for i in -1..=1 {
        let lx = x + i as f32 * 4.0;
        draw_line(lx, y - 9.0, lx, y + 1.0, 1.0, ridge);
    }
    draw_ellipse(x, y - 6.0, 4.0, 3.0, 0.0, tint(Color::from_rgba(130, 150, 172, 255)));
}

/// Dinossauro bípede genérico (dilofossauro, raptor): corpo, cauda, pescoço e cabeça.
#[allow(clippy::too_many_arguments)]
fn draw_theropod(pos: Vec2, s: f32, step: f32, scale: f32, crest: bool, body: Color, accent: Color) {
    let (x, y) = (pos.x, pos.y);
    let k = scale;
    let dark = Color::new(body.r * 0.7, body.g * 0.7, body.b * 0.7, 1.0);
    // Pernas
    draw_line(x - 3.0 * k, y - 10.0 * k, x - 3.0 * k + step * 3.0, y, 3.0, dark);
    draw_line(x + 3.0 * k, y - 10.0 * k, x + 3.0 * k - step * 3.0, y, 3.0, dark);
    // Cauda
    draw_triangle(
        vec2(x - 4.0 * k * s, y - 16.0 * k),
        vec2(x - 4.0 * k * s, y - 10.0 * k),
        vec2(x - 22.0 * k * s, y - 15.0 * k),
        body,
    );
    // Corpo
    draw_ellipse(x, y - 13.0 * k, 10.0 * k, 6.5 * k, 0.0, body);
    // Pescoço e cabeça
    draw_line(
        x + 6.0 * k * s,
        y - 15.0 * k,
        x + 10.0 * k * s,
        y - 24.0 * k,
        4.0 * k,
        body,
    );
    draw_ellipse(x + 13.0 * k * s, y - 25.0 * k, 6.0 * k, 3.5 * k, 0.0, body);
    draw_circle(x + 14.0 * k * s, y - 26.0 * k, 1.2, BLACK);
    // Detalhe: crista (dilo) ou listras (raptor)
    if crest {
        draw_triangle(
            vec2(x + 9.0 * s, y - 27.0),
            vec2(x + 15.0 * s, y - 27.0),
            vec2(x + 11.0 * s, y - 33.0),
            accent,
        );
    } else {
        for i in 0..3 {
            let sx = x + (i as f32 * 4.0 - 4.0) * k;
            draw_line(sx, y - 18.0 * k, sx - 1.0 * s, y - 13.0 * k, 1.5, accent);
        }
    }
    // Braços
    draw_line(x + 7.0 * k * s, y - 13.0 * k, x + 10.0 * k * s, y - 10.0 * k, 1.5, dark);
}

/// Pernas de quadrúpede: `(deslocamento x, fase)` de cada uma, de `top` até o chão.
fn draw_legs(x: f32, y: f32, s: f32, top: f32, width: f32, legs: &[(f32, f32)], color: Color) {
    for &(dx, swing) in legs {
        let hip = x + dx * s;
        draw_line(hip, y - top, hip + swing * 2.5 * s, y, width, color);
    }
}

/// Mamute: corpanzil peludo com franja, cabeça alta, tromba e presas curvas.
fn draw_mammoth(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let fur = tint(Color::from_rgba(124, 86, 56, 255));
    let dark = tint(Color::from_rgba(90, 60, 40, 255));
    let tusk = tint(Color::from_rgba(238, 230, 210, 255));
    draw_legs(x, y, s, 16.0, 6.0, &[(-11.0, step), (9.0, -step)], dark);
    draw_line(x - 21.0 * s, y - 28.0, x - 25.0 * s, y - 18.0, 2.0, dark);
    draw_ellipse(x, y - 25.0, 22.0, 14.0, 0.0, fur);
    // Franja de pelos pendurada na barriga.
    for i in -4..=4 {
        let fx = x + i as f32 * 5.0;
        draw_triangle(
            vec2(fx - 3.5, y - 17.0),
            vec2(fx + 3.5, y - 17.0),
            vec2(fx, y - 10.0),
            dark,
        );
    }
    draw_legs(x, y, s, 14.0, 6.5, &[(-7.0, -step), (13.0, step)], fur);
    draw_circle(x + 17.0 * s, y - 31.0, 10.0, fur);
    draw_circle(x + 15.0 * s, y - 38.0, 7.0, fur);
    draw_ellipse(x + 11.0 * s, y - 30.0, 4.0, 6.5, 0.0, dark);
    draw_circle(x + 21.0 * s, y - 33.0, 1.4, BLACK);
    // Tromba
    draw_line(x + 24.0 * s, y - 29.0, x + 27.0 * s, y - 19.0, 4.5, fur);
    draw_line(x + 27.0 * s, y - 19.0, x + 25.0 * s, y - 11.0, 3.5, fur);
    // Presas
    draw_line(x + 22.0 * s, y - 25.0, x + 31.0 * s, y - 21.0, 2.5, tusk);
    draw_line(x + 31.0 * s, y - 21.0, x + 34.0 * s, y - 28.0, 2.5, tusk);
}

/// Smilodon: felino robusto, cauda curta e dentes-de-sabre.
fn draw_smilodon(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let coat = tint(Color::from_rgba(194, 148, 90, 255));
    let dark = tint(Color::from_rgba(150, 106, 62, 255));
    let light = tint(Color::from_rgba(226, 200, 160, 255));
    draw_legs(x, y, s, 11.0, 3.0, &[(-8.0, step), (7.0, -step)], dark);
    draw_line(x - 13.0 * s, y - 17.0, x - 19.0 * s, y - 12.0, 2.5, coat);
    draw_ellipse(x, y - 14.0, 14.0, 7.0, 0.0, coat);
    draw_ellipse(x, y - 10.5, 9.0, 3.0, 0.0, light);
    draw_legs(x, y, s, 10.0, 3.5, &[(-5.0, -step), (10.0, step)], coat);
    draw_triangle(
        vec2(x + 10.0 * s, y - 22.0),
        vec2(x + 13.0 * s, y - 22.0),
        vec2(x + 10.5 * s, y - 27.0),
        dark,
    );
    draw_circle(x + 14.0 * s, y - 19.0, 6.0, coat);
    draw_ellipse(x + 19.0 * s, y - 17.0, 3.5, 2.5, 0.0, light);
    draw_circle(x + 15.0 * s, y - 21.0, 1.2, BLACK);
    // Dentes-de-sabre
    for dx in [17.0, 19.5] {
        draw_triangle(
            vec2(x + dx * s, y - 15.0),
            vec2(x + (dx + 1.5) * s, y - 15.0),
            vec2(x + (dx + 0.7) * s, y - 8.5),
            WHITE,
        );
    }
}

/// Lobo-cinzento: corpo esguio, focinho comprido, orelhas em pé e cauda peluda.
fn draw_wolf(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let coat = tint(Color::from_rgba(132, 134, 140, 255));
    let dark = tint(Color::from_rgba(92, 94, 102, 255));
    let light = tint(Color::from_rgba(196, 196, 200, 255));
    draw_legs(x, y, s, 9.0, 2.5, &[(-7.0, step), (6.0, -step)], dark);
    draw_triangle(
        vec2(x - 9.0 * s, y - 14.0),
        vec2(x - 9.0 * s, y - 9.0),
        vec2(x - 20.0 * s, y - 15.0),
        coat,
    );
    draw_ellipse(x, y - 11.0, 11.0, 5.5, 0.0, coat);
    draw_ellipse(x + 1.0 * s, y - 8.5, 7.0, 2.0, 0.0, light);
    draw_legs(x, y, s, 8.0, 2.5, &[(-4.0, -step), (8.0, step)], coat);
    draw_triangle(
        vec2(x + 8.0 * s, y - 18.0),
        vec2(x + 11.0 * s, y - 18.0),
        vec2(x + 8.5 * s, y - 24.0),
        dark,
    );
    draw_circle(x + 11.0 * s, y - 15.0, 5.0, coat);
    draw_triangle(
        vec2(x + 13.0 * s, y - 17.0),
        vec2(x + 13.0 * s, y - 12.0),
        vec2(x + 21.0 * s, y - 13.5),
        light,
    );
    draw_circle(x + 20.5 * s, y - 14.0, 1.3, BLACK);
    draw_circle(x + 12.0 * s, y - 17.0, 1.1, BLACK);
}

/// Beelzebufo: sapo gigante, achatado, de olhos saltados e pintas.
fn draw_beelzebufo(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let skin = tint(Color::from_rgba(122, 142, 62, 255));
    let dark = tint(Color::from_rgba(84, 100, 40, 255));
    let belly = tint(Color::from_rgba(222, 204, 128, 255));
    let spot = tint(Color::from_rgba(70, 86, 34, 255));
    // Pata traseira dobrada
    draw_ellipse(x - 8.0 * s, y - 6.0, 9.0, 6.0, 0.0, dark);
    draw_line(x - 13.0 * s, y - 1.0, x - (3.0 - step * 2.0) * s, y, 3.0, dark);
    draw_ellipse(x, y - 11.0, 15.0, 9.0, 0.0, skin);
    draw_ellipse(x + 3.0 * s, y - 7.0, 9.0, 4.0, 0.0, belly);
    for (dx, dy, r) in [
        (-5.0, -15.0, 2.2),
        (2.0, -17.0, 1.7),
        (-10.0, -11.0, 1.6),
        (-1.0, -12.0, 1.4),
    ] {
        draw_circle(x + dx * s, y + dy, r, spot);
    }
    draw_line(x + 8.0 * s, y - 8.0, x + (11.0 + step * 2.0) * s, y, 3.0, dark);
    draw_line(x + 4.0 * s, y - 10.0, x + 15.0 * s, y - 12.0, 1.5, dark);
    for dx in [6.0, 11.0] {
        draw_circle(x + dx * s, y - 19.0, 3.4, skin);
        draw_circle(x + (dx + 0.8) * s, y - 19.5, 1.5, BLACK);
    }
}

/// Sarcosuchus: crocodilo enorme, baixo, de focinho comprido e fino e placas nas costas.
fn draw_sarcosuchus(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let hide = tint(Color::from_rgba(96, 110, 70, 255));
    let dark = tint(Color::from_rgba(64, 76, 46, 255));
    let belly = tint(Color::from_rgba(180, 172, 120, 255));
    draw_legs(x, y, s, 6.0, 3.0, &[(-10.0, step), (7.0, -step)], dark);
    draw_triangle(
        vec2(x - 12.0 * s, y - 11.0),
        vec2(x - 12.0 * s, y - 4.0),
        vec2(x - 34.0 * s, y - 6.0),
        hide,
    );
    draw_ellipse(x, y - 8.0, 16.0, 5.5, 0.0, hide);
    draw_ellipse(x, y - 5.0, 12.0, 2.0, 0.0, belly);
    for i in -3..=2 {
        let sx = x + i as f32 * 5.0 * s;
        draw_triangle(
            vec2(sx - 1.5, y - 12.5),
            vec2(sx + 1.5, y - 12.5),
            vec2(sx, y - 16.0),
            dark,
        );
    }
    draw_legs(x, y, s, 6.0, 3.0, &[(-6.0, -step), (11.0, step)], hide);
    draw_ellipse(x + 17.0 * s, y - 9.0, 6.0, 4.0, 0.0, hide);
    draw_triangle(
        vec2(x + 20.0 * s, y - 11.0),
        vec2(x + 20.0 * s, y - 6.0),
        vec2(x + 37.0 * s, y - 8.0),
        hide,
    );
    draw_line(x + 20.0 * s, y - 8.0, x + 36.0 * s, y - 8.0, 1.0, dark);
    for i in 0..4 {
        let tx = x + (23.0 + i as f32 * 3.5) * s;
        draw_line(tx, y - 8.0, tx, y - 6.8, 1.0, WHITE);
    }
    draw_circle(x + 16.0 * s, y - 13.0, 2.4, hide);
    draw_circle(x + 16.5 * s, y - 13.5, 1.1, BLACK);
}

/// Titanoboa: cobra gigante em zigue-zague, cabeça erguida e língua bifurcada.
/// `wave`: fase da ondulação do corpo.
fn draw_titanoboa(pos: Vec2, s: f32, wave: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let scale = tint(Color::from_rgba(78, 106, 54, 255));
    let dark = tint(Color::from_rgba(46, 64, 32, 255));
    const SEGMENTS: usize = 20;
    for i in 0..=SEGMENTS {
        // Da ponta da cauda (t = 0) até o pescoço (t = 1).
        let t = i as f32 / SEGMENTS as f32;
        let dx = -40.0 + t * 54.0;
        let dy = (t * 10.0 - wave * 3.0).sin() * 4.5 * (1.0 - t * 0.6);
        let r = 2.0 + (t * std::f32::consts::PI * 0.9).sin() * 5.0;
        let c = vec2(x + dx * s, y - 6.0 + dy);
        draw_circle(c.x, c.y, r, scale);
        if i % 3 == 1 {
            draw_circle(c.x, c.y - r * 0.35, r * 0.45, dark);
        }
    }
    // Pescoço erguido e cabeça
    for i in 0..4 {
        let t = i as f32 / 3.0;
        draw_circle(x + (14.0 + t * 5.0) * s, y - 7.0 - t * 9.0, 4.2, scale);
    }
    draw_ellipse(x + 22.0 * s, y - 17.0, 6.5, 4.0, 0.0, scale);
    draw_circle(x + 23.5 * s, y - 18.5, 1.2, BLACK);
    let flick = ((wave * 5.0).sin() > 0.0) as u8 as f32;
    let (tx, ty) = (x + 28.0 * s, y - 16.0);
    let tongue = Color::from_rgba(200, 50, 60, 255);
    draw_line(tx, ty, tx + (3.0 + flick * 3.0) * s, ty, 1.0, tongue);
    let tip = tx + (3.0 + flick * 3.0) * s;
    draw_line(tip, ty, tip + 2.0 * s, ty - 1.5, 1.0, tongue);
    draw_line(tip, ty, tip + 2.0 * s, ty + 1.5, 1.0, tongue);
}

/// Piranha: peixinho prateado de barriga vermelha (fica metade dentro d'água).
fn draw_piranha(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y - 5.0);
    let body = tint(Color::from_rgba(150, 158, 166, 255));
    let belly = tint(Color::from_rgba(206, 74, 56, 255));
    let fin = tint(Color::from_rgba(106, 114, 124, 255));
    let wag = step * 1.5;
    draw_triangle(
        vec2(x - 5.0 * s, y),
        vec2(x - 10.0 * s, y - 4.0 + wag),
        vec2(x - 10.0 * s, y + 4.0 + wag),
        fin,
    );
    draw_ellipse(x, y, 6.5, 4.0, 0.0, body);
    draw_ellipse(x + 1.0 * s, y + 2.0, 4.5, 2.0, 0.0, belly);
    draw_triangle(
        vec2(x - 2.0 * s, y - 3.0),
        vec2(x + 2.0 * s, y - 3.0),
        vec2(x - 1.0 * s, y - 8.0),
        fin,
    );
    draw_circle(x + 3.5 * s, y - 1.0, 1.0, BLACK);
    draw_line(x + 5.0 * s, y + 1.0, x + 6.5 * s, y + 1.0, 1.0, WHITE);
}

/// Linha d'água sobre uma criatura nadando: cobre a parte de baixo com a cor da água e marca as ondinhas.
fn draw_waterline(pos: Vec2, radius: f32, water: Color, time: f32) {
    let depth = (radius * 0.2).max(4.0);
    let w = radius * 2.3 + 4.0;
    draw_ellipse(pos.x, pos.y - depth * 0.3, w, depth * 0.8 + 2.0, 0.0, water);
    let ripple = 1.0 + (time * 3.0 + pos.x * 0.05).sin() * 0.08;
    let foam = Color::new(1.0, 1.0, 1.0, 0.35);
    draw_ellipse_lines(pos.x, pos.y - depth, w * ripple, 3.0, 0.0, 1.2, foam);
}

/// Jangada: toras amarradas com um mastro e uma vela pequena; balança um pouco na água.
fn draw_raft(r: &Raft, time: f32) {
    let s = if r.facing_left { -1.0 } else { 1.0 };
    let bob = (time * 2.2 + r.id as f32).sin();
    let (x, y) = (r.pos.x, r.pos.y + bob);
    let ripple = 1.0 + (time * 3.0 + r.id as f32).sin() * 0.05;
    draw_ellipse_lines(
        x,
        y + 3.0,
        25.0 * ripple,
        9.0 * ripple,
        0.0,
        1.2,
        Color::new(1.0, 1.0, 1.0, 0.4),
    );
    for i in 0..5 {
        let ly = y - 10.0 + i as f32 * 4.6;
        let log = if i % 2 == 0 {
            WOOD_FRONT
        } else {
            shade(WOOD_FRONT, 1.15)
        };
        draw_rectangle(x - 20.0, ly, 40.0, 4.2, log);
        for end in [-20.0, 20.0] {
            draw_circle(x + end, ly + 2.1, 2.1, WOOD_TOP);
        }
    }
    for rx in [-12.0, 12.0] {
        draw_line(
            x + rx,
            y - 11.0,
            x + rx,
            y + 12.0,
            1.5,
            Color::from_rgba(210, 190, 140, 255),
        );
    }
    let mast = x - 9.0 * s;
    draw_line(mast, y - 4.0, mast, y - 42.0, 2.5, WOOD_LINE);
    draw_triangle(
        vec2(mast + 1.5 * s, y - 40.0),
        vec2(mast + 1.5 * s, y - 14.0),
        vec2(mast - 17.0 * s, y - 18.0),
        Color::from_rgba(236, 226, 196, 255),
    );
}

/// Megalodonte: tubarão gigante; a barbatana das costas fica bem fora d'água.
fn draw_megalodon(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let grey = tint(Color::from_rgba(112, 122, 136, 255));
    let dark = tint(Color::from_rgba(78, 86, 100, 255));
    let belly = tint(Color::from_rgba(222, 226, 230, 255));
    let wag = step * 2.0;
    draw_triangle(
        vec2(x - 22.0 * s, y - 8.0),
        vec2(x - 34.0 * s, y - 22.0 + wag),
        vec2(x - 29.0 * s, y - 7.0),
        dark,
    );
    draw_triangle(
        vec2(x - 22.0 * s, y - 6.0),
        vec2(x - 32.0 * s, y + 3.0 + wag),
        vec2(x - 29.0 * s, y - 7.0),
        dark,
    );
    draw_ellipse(x, y - 7.0, 26.0, 8.0, 0.0, grey);
    draw_ellipse(x + 3.0 * s, y - 3.0, 19.0, 3.5, 0.0, belly);
    draw_triangle(
        vec2(x - 5.0 * s, y - 13.0),
        vec2(x + 6.0 * s, y - 13.0),
        vec2(x - 7.0 * s, y - 30.0),
        dark,
    );
    draw_triangle(
        vec2(x + 5.0 * s, y - 4.0),
        vec2(x + 12.0 * s, y - 4.0),
        vec2(x + 1.0 * s, y + 4.0),
        dark,
    );
    for i in 0..3 {
        let gx = x + (9.0 + i as f32 * 2.5) * s;
        draw_line(gx, y - 10.0, gx - 1.0 * s, y - 5.0, 1.0, dark);
    }
    draw_line(x + 15.0 * s, y - 4.0, x + 25.0 * s, y - 6.0, 1.5, dark);
    for i in 0..3 {
        let tx = x + (17.0 + i as f32 * 2.8) * s;
        draw_triangle(
            vec2(tx, y - 4.6),
            vec2(tx + 1.6 * s, y - 4.8),
            vec2(tx + 0.8 * s, y - 2.6),
            WHITE,
        );
    }
    draw_circle(x + 19.0 * s, y - 10.0, 1.5, BLACK);
}

/// Ictiossauro: réptil marinho com cara de golfinho, focinho fino e olho grande.
fn draw_ichthyosaur(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let blue = tint(Color::from_rgba(92, 130, 170, 255));
    let dark = tint(Color::from_rgba(62, 92, 128, 255));
    let belly = tint(Color::from_rgba(208, 222, 234, 255));
    let wag = step * 2.0;
    draw_triangle(
        vec2(x - 15.0 * s, y - 8.0),
        vec2(x - 25.0 * s, y - 19.0 + wag),
        vec2(x - 21.0 * s, y - 7.0),
        dark,
    );
    draw_triangle(
        vec2(x - 15.0 * s, y - 6.0),
        vec2(x - 25.0 * s, y + 3.0 + wag),
        vec2(x - 21.0 * s, y - 7.0),
        dark,
    );
    draw_ellipse(x, y - 8.0, 18.0, 6.5, 0.0, blue);
    draw_ellipse(x + 2.0 * s, y - 4.5, 13.0, 3.0, 0.0, belly);
    draw_triangle(
        vec2(x - 4.0 * s, y - 13.0),
        vec2(x + 3.0 * s, y - 13.0),
        vec2(x - 6.0 * s, y - 22.0),
        dark,
    );
    draw_triangle(
        vec2(x + 15.0 * s, y - 11.0),
        vec2(x + 15.0 * s, y - 6.0),
        vec2(x + 31.0 * s, y - 7.5),
        blue,
    );
    draw_triangle(
        vec2(x + 4.0 * s, y - 4.0),
        vec2(x + 9.0 * s, y - 4.0),
        vec2(x + 1.0 * s, y + 3.0),
        dark,
    );
    draw_circle(x + 12.0 * s, y - 10.0, 2.6, WHITE);
    draw_circle(x + 12.6 * s, y - 10.0, 1.5, BLACK);
}

/// Mosassauro: lagartão marinho enorme, com nadadeiras, cauda com barbatana e mandíbula cheia de dentes.
fn draw_mosasaur(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let teal = tint(Color::from_rgba(62, 98, 108, 255));
    let dark = tint(Color::from_rgba(40, 66, 74, 255));
    let belly = tint(Color::from_rgba(192, 196, 170, 255));
    let wag = step * 3.0;
    draw_triangle(
        vec2(x - 22.0 * s, y - 13.0),
        vec2(x - 22.0 * s, y - 4.0),
        vec2(x - 50.0 * s, y - 8.0 + wag),
        teal,
    );
    draw_triangle(
        vec2(x - 44.0 * s, y - 8.0 + wag),
        vec2(x - 54.0 * s, y - 22.0 + wag),
        vec2(x - 52.0 * s, y - 8.0 + wag),
        dark,
    );
    draw_triangle(
        vec2(x - 44.0 * s, y - 8.0 + wag),
        vec2(x - 56.0 * s, y + 2.0 + wag),
        vec2(x - 52.0 * s, y - 8.0 + wag),
        dark,
    );
    draw_triangle(
        vec2(x - 14.0 * s, y - 5.0),
        vec2(x - 6.0 * s, y - 5.0),
        vec2(x - 20.0 * s, y + 5.0),
        dark,
    );
    draw_ellipse(x, y - 10.0, 28.0, 10.0, 0.0, teal);
    draw_ellipse(x + 2.0 * s, y - 4.5, 22.0, 4.0, 0.0, belly);
    for i in -4..=3 {
        let rx = x + i as f32 * 6.0 * s;
        draw_triangle(
            vec2(rx - 2.0, y - 18.5),
            vec2(rx + 2.0, y - 18.5),
            vec2(rx, y - 22.5),
            dark,
        );
    }
    draw_triangle(
        vec2(x + 10.0 * s, y - 5.0),
        vec2(x + 18.0 * s, y - 5.0),
        vec2(x + 6.0 * s, y + 6.0),
        dark,
    );
    draw_ellipse(x + 30.0 * s, y - 12.0, 12.0, 6.0, 0.0, teal);
    draw_triangle(
        vec2(x + 30.0 * s, y - 10.0),
        vec2(x + 46.0 * s, y - 11.0),
        vec2(x + 30.0 * s, y - 5.0),
        dark,
    );
    for i in 0..5 {
        let tx = x + (32.0 + i as f32 * 2.8) * s;
        draw_line(tx, y - 10.5, tx, y - 8.8, 1.0, WHITE);
    }
    draw_circle(x + 31.0 * s, y - 15.0, 2.0, BLACK);
}

/// Kraken (lula gigante): manto comprido para trás, olho enorme e tentáculos ondulando para a frente.
/// `wave`: fase da ondulação dos tentáculos.
fn draw_kraken(pos: Vec2, s: f32, wave: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let red = tint(Color::from_rgba(178, 62, 84, 255));
    let dark = tint(Color::from_rgba(126, 36, 58, 255));
    let light = tint(Color::from_rgba(222, 134, 144, 255));
    // Tentáculos (dois mais compridos, com "clava" na ponta).
    for i in 0..8 {
        let long = i == 2 || i == 5;
        let reach = if long { 46.0 } else { 28.0 + (i % 3) as f32 * 3.0 };
        let base = vec2(x + 8.0 * s, y - 9.0 + (i as f32 - 3.5) * 1.6);
        let mut prev = base;
        for k in 1..=6 {
            let t = k as f32 / 6.0;
            let wiggle = ((wave * 3.0 + i as f32 * 0.9) + t * 4.0).sin() * 3.0 * t;
            let p = base + vec2(reach * t * s, (i as f32 - 3.5) * 1.8 * t + wiggle);
            draw_line(prev.x, prev.y, p.x, p.y, 3.0 - t * 1.8, dark);
            prev = p;
        }
        if long {
            draw_ellipse(prev.x, prev.y, 4.0, 2.5, 0.0, dark);
        }
    }
    draw_triangle(
        vec2(x - 30.0 * s, y - 12.0),
        vec2(x - 42.0 * s, y - 24.0),
        vec2(x - 44.0 * s, y - 10.0),
        dark,
    );
    draw_triangle(
        vec2(x - 30.0 * s, y - 12.0),
        vec2(x - 42.0 * s, y - 1.0),
        vec2(x - 44.0 * s, y - 10.0),
        dark,
    );
    draw_triangle(
        vec2(x - 6.0 * s, y - 20.0),
        vec2(x - 6.0 * s, y - 4.0),
        vec2(x - 40.0 * s, y - 12.0),
        red,
    );
    draw_ellipse(x - 8.0 * s, y - 12.0, 16.0, 8.5, 0.0, red);
    for (dx, dy) in [(-14.0, -14.0), (-22.0, -11.0), (-6.0, -16.0), (-28.0, -13.0)] {
        draw_circle(x + dx * s, y + dy, 1.5, light);
    }
    draw_circle(x + 6.0 * s, y - 10.0, 8.0, red);
    draw_circle(x + 7.0 * s, y - 11.0, 4.0, Color::from_rgba(242, 222, 120, 255));
    draw_circle(x + 8.0 * s, y - 11.0, 2.0, BLACK);
}

/// Golem de pedra: corpanzil de rochas com olhos acesos. Parado e selvagem, é desenhado como uma rocha.
fn draw_golem(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let stone = tint(Color::from_rgba(130, 124, 114, 255));
    let dark = tint(Color::from_rgba(94, 90, 82, 255));
    let glow = Color::from_rgba(255, 170, 60, 255);
    for (lx, swing) in [(-9.0, step), (3.0, -step)] {
        draw_rectangle(x + lx + swing * 2.0, y - 14.0, 6.0, 14.0, dark);
    }
    // Braço de trás
    draw_line(x - 13.0 * s, y - 31.0, x - 17.0 * s, y - 14.0, 7.0, dark);
    draw_circle(x - 17.0 * s, y - 13.0, 5.5, dark);
    draw_ellipse(x, y - 26.0, 15.0, 13.0, 0.0, stone);
    draw_circle(x - 12.0 * s, y - 33.0, 7.0, stone);
    draw_circle(x + 12.0 * s, y - 33.0, 7.5, stone);
    // Rachaduras com brilho de lava
    draw_line(
        x - 4.0,
        y - 32.0,
        x + 2.0,
        y - 24.0,
        1.2,
        Color::new(1.0, 0.6, 0.2, 0.7),
    );
    draw_line(
        x + 2.0,
        y - 24.0,
        x - 1.0,
        y - 17.0,
        1.2,
        Color::new(1.0, 0.6, 0.2, 0.7),
    );
    draw_circle(x + 3.0 * s, y - 42.0, 7.0, stone);
    for dx in [1.0, 5.0] {
        draw_rectangle(x + dx * s - 1.0, y - 44.0, 2.5, 2.0, glow);
    }
    // Braço da frente
    draw_line(x + 15.0 * s, y - 31.0, x + 19.0 * s, y - 13.0, 7.5, stone);
    draw_circle(x + 19.0 * s, y - 12.0, 6.0, stone);
}

/// Canguru: de pé, cauda grossa no chão, patas compridas e orelhas em pé.
fn draw_kangaroo(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let fur = tint(Color::from_rgba(178, 112, 66, 255));
    let belly = tint(Color::from_rgba(224, 186, 142, 255));
    let dark = tint(Color::from_rgba(130, 80, 46, 255));
    draw_triangle(
        vec2(x - 5.0 * s, y - 11.0),
        vec2(x - 5.0 * s, y - 4.0),
        vec2(x - 27.0 * s, y - 1.0),
        fur,
    );
    draw_line(x - 2.0 * s, y - 6.0, x + (11.0 + step * 2.0) * s, y, 3.5, dark);
    draw_ellipse(x - 2.0 * s, y - 12.0, 8.0, 7.0, 0.0, fur);
    draw_ellipse(x + 1.0 * s, y - 21.0, 8.0, 12.0, 0.0, fur);
    draw_ellipse(x + 3.0 * s, y - 19.0, 4.5, 8.0, 0.0, belly);
    draw_line(x + 6.0 * s, y - 25.0, x + 10.0 * s, y - 19.0, 2.0, dark);
    draw_triangle(
        vec2(x + 3.0 * s, y - 37.0),
        vec2(x + 6.0 * s, y - 37.0),
        vec2(x + 2.5 * s, y - 46.0),
        dark,
    );
    draw_ellipse(x + 6.0 * s, y - 34.0, 5.0, 4.5, 0.0, fur);
    draw_ellipse(x + 10.5 * s, y - 33.0, 3.5, 2.5, 0.0, fur);
    draw_circle(x + 7.0 * s, y - 35.0, 1.2, BLACK);
    draw_circle(x + 13.5 * s, y - 33.5, 1.0, BLACK);
}

/// Verme da morte: enterrado, só um montinho de areia andando; exposto, um corpo segmentado com boca de dentes.
fn draw_deathworm(pos: Vec2, time: f32, buried: bool, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let sand = shade(DESERT, 0.82);
    draw_ellipse(x, y - 1.0, 16.0, 5.0, 0.0, sand);
    if buried {
        for i in 0..4 {
            let t = (time * 1.5 + i as f32 * 0.25).fract();
            let a = i as f32 * 1.7;
            let p = vec2(x + a.cos() * (6.0 + t * 8.0), y - 2.0 - t * 6.0);
            draw_circle(p.x, p.y, 1.6 * (1.0 - t), Color::new(sand.r, sand.g, sand.b, 1.0 - t));
        }
        return;
    }
    let flesh = tint(Color::from_rgba(206, 158, 126, 255));
    let ring = tint(Color::from_rgba(156, 106, 86, 255));
    let mut top = vec2(x, y);
    for i in 0..7 {
        let t = i as f32 / 6.0;
        let sway = (time * 3.0 + t * 2.0).sin() * 4.0 * t;
        top = vec2(x + sway, y - 4.0 - t * 40.0);
        let r = 11.0 - t * 3.0;
        draw_circle(top.x, top.y, r, flesh);
        draw_line(top.x - r, top.y + 2.0, top.x + r, top.y + 2.0, 1.2, ring);
    }
    draw_ellipse(top.x, top.y - 5.0, 8.5, 4.0, 0.0, Color::from_rgba(96, 30, 34, 255));
    for i in 0..6 {
        let a = i as f32 / 6.0 * std::f32::consts::TAU;
        let p = vec2(top.x + a.cos() * 7.0, top.y - 5.0 + a.sin() * 3.0);
        draw_triangle(p, p + vec2(1.5, 0.0), vec2(top.x, top.y - 5.0) * 0.25 + p * 0.75, WHITE);
    }
}

/// Louva-deus gigante: verde, cabeça triangular de olhos grandes e patas da frente em foice.
fn draw_mantis(pos: Vec2, s: f32, step: f32, tint: &dyn Fn(Color) -> Color) {
    let (x, y) = (pos.x, pos.y);
    let green = tint(Color::from_rgba(112, 172, 72, 255));
    let dark = tint(Color::from_rgba(72, 122, 46, 255));
    let light = Color::new(0.8, 0.95, 0.7, 0.6);
    for (hip, foot, swing) in [(-6.0, -12.0, step), (-2.0, 4.0, -step), (-9.0, -4.0, -step)] {
        draw_line(x + hip * s, y - 11.0, x + (foot + swing * 2.0) * s, y, 1.5, dark);
    }
    draw_ellipse(x - 9.0 * s, y - 12.0, 11.0, 5.0, 0.0, green);
    draw_ellipse(x - 8.0 * s, y - 15.0, 10.0, 3.0, 0.0, light);
    draw_line(x - 1.0 * s, y - 13.0, x + 6.0 * s, y - 25.0, 4.0, green);
    draw_line(x + 2.0 * s, y - 14.0, x + 5.0 * s, y, 1.5, dark);
    draw_triangle(
        vec2(x + 4.0 * s, y - 28.0),
        vec2(x + 12.0 * s, y - 28.0),
        vec2(x + 8.0 * s, y - 21.0),
        green,
    );
    for dx in [4.5, 11.5] {
        draw_circle(x + dx * s, y - 28.0, 2.2, dark);
    }
    draw_line(x + 6.0 * s, y - 29.0, x + 2.0 * s, y - 38.0, 1.0, dark);
    draw_line(x + 10.0 * s, y - 29.0, x + 14.0 * s, y - 38.0, 1.0, dark);
    // Foices
    let (elbow, tip) = (vec2(x + 13.0 * s, y - 17.0), vec2(x + 10.0 * s, y - 27.0));
    draw_line(x + 6.0 * s, y - 22.0, elbow.x, elbow.y, 2.5, dark);
    draw_line(elbow.x, elbow.y, tip.x, tip.y, 2.0, dark);
    for i in 1..3 {
        let p = elbow.lerp(tip, i as f32 / 3.0);
        draw_line(p.x, p.y, p.x - 2.0 * s, p.y + 1.0, 1.0, dark);
    }
}

/// Cacto (saguaro): tronco com dois braços e espinhos; colhido, sobra um toco.
fn draw_cactus(pos: Vec2, variant: u8, alive: bool) {
    let (x, y) = (pos.x, pos.y);
    let green = Color::from_rgba(76, 142, 76, 255);
    let dark = Color::from_rgba(52, 108, 56, 255);
    draw_ellipse(x, y, 10.0, 4.0, 0.0, SHADOW);
    if !alive {
        draw_rectangle(x - 4.0, y - 6.0, 8.0, 6.0, dark);
        return;
    }
    let h = 30.0 + variant as f32 * 3.0;
    draw_rectangle(x - 4.5, y - h, 9.0, h, green);
    draw_circle(x, y - h, 4.5, green);
    let (left, right) = (14.0 + (variant % 2) as f32 * 4.0, 20.0 - (variant % 3) as f32 * 3.0);
    draw_rectangle(x - 12.0, y - left, 8.0, 4.0, green);
    draw_rectangle(x - 12.0, y - left - 9.0, 4.0, 10.0, green);
    draw_circle(x - 10.0, y - left - 9.0, 2.0, green);
    draw_rectangle(x + 4.0, y - right, 8.0, 4.0, green);
    draw_rectangle(x + 8.0, y - right - 8.0, 4.0, 9.0, green);
    draw_circle(x + 10.0, y - right - 8.0, 2.0, green);
    draw_line(x - 1.5, y - h + 2.0, x - 1.5, y - 2.0, 1.0, dark);
    for i in 0..5 {
        let sy = y - 6.0 - i as f32 * 5.5;
        draw_line(
            x + 4.5,
            sy,
            x + 6.5,
            sy - 1.0,
            0.8,
            Color::from_rgba(240, 236, 200, 255),
        );
    }
    if variant == 3 {
        draw_circle(x, y - h - 3.0, 2.2, Color::from_rgba(236, 110, 150, 255));
    }
}

/// Tenda de couro do acampamento: duas faces em 3/4, abertura escura e varas cruzadas no topo.
fn draw_tent(pos: Vec2, variant: usize) {
    let (x, y) = (pos.x, pos.y);
    const HIDES: [(u8, u8, u8); 3] = [(196, 168, 120), (176, 132, 92), (206, 186, 150)];
    let (r, g, b) = HIDES[variant % HIDES.len()];
    let hide = Color::from_rgba(r, g, b, 255);
    draw_ellipse(x, y + 2.0, 26.0, 7.0, 0.0, SHADOW);
    let top = vec2(x, y - 40.0);
    draw_triangle(top, vec2(x - 26.0, y + 4.0), vec2(x, y + 6.0), shade(hide, 0.85));
    draw_triangle(top, vec2(x, y + 6.0), vec2(x + 26.0, y + 4.0), hide);
    draw_triangle(
        vec2(x, y - 20.0),
        vec2(x - 7.0, y + 5.0),
        vec2(x + 7.0, y + 5.0),
        Color::from_rgba(52, 38, 28, 255),
    );
    let pole = Color::from_rgba(96, 66, 40, 255);
    draw_line(top.x, top.y, top.x - 6.0, top.y - 10.0, 2.0, pole);
    draw_line(top.x, top.y, top.x + 6.0, top.y - 9.0, 2.0, pole);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let p = top.lerp(vec2(x + 26.0, y + 4.0), t);
        draw_line(p.x - 3.0, p.y, p.x + 3.0, p.y + 1.0, 1.0, shade(hide, 0.7));
    }
}

/// Camisa e cabelo de um NPC (pela variação `look`).
pub fn npc_look(look: u8) -> (Color, Color) {
    const SHIRTS: [(u8, u8, u8); 6] = [
        (150, 112, 70),
        (112, 132, 80),
        (168, 92, 70),
        (96, 110, 130),
        (186, 160, 110),
        (120, 86, 110),
    ];
    const HAIRS: [(u8, u8, u8); 4] = [(30, 24, 20), (78, 50, 34), (150, 104, 60), (60, 40, 30)];
    let (r, g, b) = SHIRTS[look as usize % SHIRTS.len()];
    let (hr, hg, hb) = HAIRS[(look as usize / 2) % HAIRS.len()];
    (Color::from_rgba(r, g, b, 255), Color::from_rgba(hr, hg, hb, 255))
}

/// Criatura deitada: morta (olho em X) ou desmaiada (olho fechado).
fn draw_lying(species: Species, pos: Vec2, s: f32, asleep: bool) {
    let (x, y) = (pos.x, pos.y);
    let col = match species {
        Species::Dodo => Color::from_rgba(120, 104, 92, 255),
        Species::Trilobite => Color::from_rgba(80, 96, 116, 255),
        Species::Dilophosaur => Color::from_rgba(74, 110, 58, 255),
        Species::Raptor => Color::from_rgba(140, 90, 52, 255),
        Species::Pteranodon => Color::from_rgba(112, 98, 88, 255),
        Species::Argentavis => Color::from_rgba(62, 48, 40, 255),
        Species::Mammoth => Color::from_rgba(108, 76, 50, 255),
        Species::Smilodon => Color::from_rgba(170, 128, 80, 255),
        Species::Wolf => Color::from_rgba(118, 120, 126, 255),
        Species::Beelzebufo => Color::from_rgba(110, 128, 56, 255),
        Species::Sarcosuchus => Color::from_rgba(90, 100, 66, 255),
        Species::Titanoboa => Color::from_rgba(70, 96, 48, 255),
        Species::Piranha => Color::from_rgba(140, 146, 152, 255),
        Species::Megalodon => Color::from_rgba(104, 114, 128, 255),
        Species::Ichthyosaur => Color::from_rgba(84, 120, 160, 255),
        Species::Mosasaur => Color::from_rgba(58, 92, 100, 255),
        Species::Kraken => Color::from_rgba(160, 56, 78, 255),
        Species::StoneGolem => Color::from_rgba(118, 112, 104, 255),
        Species::Kangaroo => Color::from_rgba(160, 102, 60, 255),
        Species::Deathworm => Color::from_rgba(190, 146, 118, 255),
        Species::Mantis => Color::from_rgba(100, 156, 66, 255),
    };
    let r = species.radius();
    draw_ellipse(x, y - 4.0, r + 2.0, r * 0.45, 0.0, col);
    // Patas para cima
    for i in [-1.0, 1.0] {
        let lx = x + i * r * 0.4;
        draw_line(lx, y - 6.0, lx + 1.5 * i, y - 6.0 - r * 0.6, 1.5, col);
    }
    let ex = x + r * 0.7 * s;
    if asleep {
        draw_line(ex - 1.5, y - 5.5, ex + 1.5, y - 5.5, 1.0, BLACK);
    } else {
        draw_line(ex - 1.5, y - 7.0, ex + 1.5, y - 4.0, 1.0, BLACK);
        draw_line(ex - 1.5, y - 4.0, ex + 1.5, y - 7.0, 1.0, BLACK);
    }
}

/// Barras (espaço de tela) de criaturas feridas, atordoadas, perseguindo ou domesticadas.
/// `skip`: id da montaria (a barra dela fica no HUD).
pub fn draw_creature_bars(creatures: &[Creature], camera: &Camera2D, skip: Option<u32>) {
    for c in creatures.iter().filter(|c| c.is_alive() && Some(c.id) != skip) {
        let hurt = c.health_frac() < 1.0;
        let dazed = c.torpor > 0.0;
        let chasing = c.is_wild() && c.state == State::Chase;
        if !hurt && !dazed && !chasing && !c.is_tamed() {
            continue;
        }
        let top = c.pos - vec2(0.0, head_height(c) + 10.0);
        let at = camera.world_to_screen(top);
        let (w, h) = (44.0, 5.0);
        draw_rectangle(at.x - w / 2.0, at.y, w, h, Color::new(0.0, 0.0, 0.0, 0.6));
        let color = match (c.is_tamed(), c.species.temperament()) {
            (true, _) => Color::new(0.4, 0.85, 0.4, 1.0),
            (false, Temperament::Aggressive) => Color::new(0.9, 0.25, 0.2, 1.0),
            (false, Temperament::Passive) => Color::new(0.85, 0.75, 0.35, 1.0),
            (false, Temperament::Territorial) => Color::new(0.95, 0.55, 0.2, 1.0),
        };
        draw_rectangle(at.x - w / 2.0, at.y, w * c.health_frac(), h, color);
        if dazed {
            draw_rectangle(at.x - w / 2.0, at.y + h + 1.0, w, 3.0, Color::new(0.0, 0.0, 0.0, 0.6));
            let purple = Color::new(0.65, 0.4, 0.95, 1.0);
            draw_rectangle(at.x - w / 2.0, at.y + h + 1.0, w * c.torpor_frac(), 3.0, purple);
        }
        let mut name = format!("{} Nv {}", c.species.name(), c.level);
        if c.knocked_out {
            name.push_str(" (desacordado)");
        }
        let name_color = if c.is_tamed() {
            Color::new(0.6, 1.0, 0.65, 1.0)
        } else {
            Color::new(1.0, 0.85, 0.8, 1.0)
        };
        let tw = measure_text(&name, None, 16, 1.0).width;
        draw_text(
            &name,
            at.x - tw / 2.0 + 1.0,
            at.y - 2.0,
            16.0,
            Color::new(0.0, 0.0, 0.0, 0.7),
        );
        draw_text(&name, at.x - tw / 2.0, at.y - 3.0, 16.0, name_color);
    }
}
