//! Desenho do mundo em visão 3/4. Arte provisória feita com primitivas:
//! cada entidade tem uma função `draw_*` isolada para ser trocada por sprites depois.

use macroquad::prelude::*;

use crate::building::{self, Structure, StructureKind};
use crate::creatures::{Creature, Species, State, Temperament};
use crate::items::ItemId;
use crate::player::{Player, SWING_TIME};
use crate::protocol::PopupStyle;
use crate::rng::hash2;
use crate::ui::draw_item_icon;
use crate::world::{BagKind, GroundBag, NodeKind, ResourceNode, TILE, Tile, World};

pub const DEEP_WATER: Color = Color::new(0.09, 0.31, 0.55, 1.0);
const SHALLOW_WATER: Color = Color::new(0.20, 0.55, 0.74, 1.0);
const SAND: Color = Color::new(0.89, 0.80, 0.59, 1.0);
const GRASS: Color = Color::new(0.38, 0.63, 0.28, 1.0);
const MOUNTAIN: Color = Color::new(0.55, 0.52, 0.48, 1.0);
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
                Tile::Sand if h > 0.75 => {
                    draw_circle(px + h * 28.0, py + (h * 53.0) % 28.0, 1.2, shade(SAND, 0.82));
                }
                Tile::ShallowWater => draw_foam(world, x, y, px, py, time),
                _ => {}
            }
        }
    }

    for s in world
        .structures
        .iter()
        .filter(|s| s.kind == StructureKind::WoodFoundation)
    {
        let r = s.rect();
        if r.overlaps(&view) {
            draw_foundation(r);
        }
    }
}

const WOOD_TOP: Color = Color::new(0.80, 0.60, 0.38, 1.0);
const WOOD_FRONT: Color = Color::new(0.52, 0.36, 0.20, 1.0);
const WOOD_LINE: Color = Color::new(0.38, 0.25, 0.13, 1.0);
/// Altura visual de paredes e portas (em pixels de mundo).
const WALL_H: f32 = 24.0;

fn draw_foundation(r: Rect) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.60, 0.43, 0.25, 1.0));
    for i in 1..4 {
        let y = r.y + i as f32 * r.h / 4.0;
        draw_line(r.x, y, r.right(), y, 1.0, WOOD_LINE);
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, WOOD_LINE);
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
}

/// Um personagem a desenhar (o próprio jogador ou outro na partida).
pub struct Avatar<'a> {
    pub player: &'a Player,
    pub held: Option<ItemId>,
    /// `id` da criatura montada.
    pub mount: Option<u32>,
    pub shirt: Color,
    /// Nome acima da cabeça (`None` para o próprio jogador).
    pub name: Option<&'a str>,
}

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
        _ => 10.0,
    }
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
            .filter(|s| s.kind != StructureKind::WoodFoundation && s.rect().overlaps(&culled))
            .map(|s| (s.sort_y(), Drawable::Structure(s))),
    );
    list.extend(
        creatures
            .iter()
            .filter(|c| culled.contains(c.pos))
            .map(|c| (c.pos.y, Drawable::Creature(c))),
    );
    // Montado, o personagem é desenhado junto da montaria (logo depois dela).
    list.extend(
        avatars
            .iter()
            .filter(|a| a.mount.is_none())
            .map(|a| (a.player.pos.y, Drawable::Avatar(a))),
    );
    list.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (_, d) in list {
        match d {
            Drawable::Node(n) => draw_node(n, player_pos),
            Drawable::Bag(b) => draw_bag(b, time),
            Drawable::Structure(s) => draw_structure(s, time),
            Drawable::Creature(c) => {
                draw_creature(c, time);
                for rider in avatars.iter().filter(|a| a.mount == Some(c.id)) {
                    draw_player(rider.player, rider.held, rider.shirt, saddle_height(c.species));
                }
            }
            Drawable::Avatar(a) => draw_player(a.player, a.held, a.shirt, 0.0),
        }
    }

    for stone in scene.projectiles {
        let (x, y) = (stone.x, stone.y);
        draw_ellipse(x, y, 2.5, 1.0, 0.0, SHADOW);
        draw_circle(x, y - 14.0, 2.2, Color::from_rgba(150, 150, 158, 255));
    }
}

fn draw_node(node: &ResourceNode, player_pos: Vec2) {
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
            draw_tree(pos, node.variant, if behind { 0.45 } else { 1.0 });
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
        _ => {}
    }
}

fn draw_tree(pos: Vec2, variant: u8, alpha: f32) {
    let (x, y) = (pos.x, pos.y);
    draw_ellipse(x, y, 16.0, 6.0, 0.0, SHADOW);
    draw_rectangle(x - 4.0, y - 26.0, 8.0, 26.0, Color::from_rgba(110, 72, 40, 255));
    draw_rectangle(x - 4.0, y - 26.0, 3.0, 26.0, Color::from_rgba(132, 90, 52, 255));

    let leaf = |r, g, b| Color::from_rgba(r, g, b, 255).with_alpha(alpha);
    if variant.is_multiple_of(2) {
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

pub fn structure_label(s: &Structure) -> String {
    match s.kind {
        StructureKind::WoodDoor if s.open => "[F] Fechar porta".to_owned(),
        StructureKind::WoodDoor => "[F] Abrir porta".to_owned(),
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

fn draw_structure(s: &Structure, time: f32) {
    let r = s.rect();
    match s.kind {
        StructureKind::WoodWall => draw_block(r, WOOD_TOP, WOOD_FRONT, true),
        StructureKind::WoodDoor if s.open => {
            // Batentes nas laterais; folha da porta encostada à esquerda.
            for x in [r.x, r.right() - 5.0] {
                draw_rectangle(x, r.y - WALL_H, 5.0, r.h + WALL_H, WOOD_FRONT);
            }
            draw_rectangle(r.x + 5.0, r.y - WALL_H + 4.0, 4.0, r.h + WALL_H - 4.0, WOOD_TOP);
        }
        StructureKind::WoodDoor => {
            draw_block(r, WOOD_TOP, Color::new(0.45, 0.30, 0.17, 1.0), false);
            let front = r.bottom() - WALL_H;
            draw_rectangle_lines(r.x + 6.0, front + 2.0, r.w - 12.0, WALL_H - 2.0, 1.5, WOOD_LINE);
            draw_circle(
                r.right() - 10.0,
                front + WALL_H * 0.55,
                1.6,
                Color::new(0.85, 0.75, 0.4, 1.0),
            );
        }
        StructureKind::StorageBox => {
            let (x, w) = (r.x + 5.0, r.w - 10.0);
            let base = r.bottom() - 4.0;
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
        StructureKind::Workbench => {
            let base = r.bottom() - 4.0;
            draw_ellipse(r.center().x, base, r.w / 2.0, 4.0, 0.0, SHADOW);
            for lx in [r.x + 4.0, r.right() - 8.0] {
                draw_rectangle(lx, base - 14.0, 4.0, 14.0, WOOD_LINE);
            }
            draw_rectangle(r.x + 1.0, base - 22.0, r.w - 2.0, 6.0, WOOD_TOP);
            draw_rectangle(r.x + 1.0, base - 16.0, r.w - 2.0, 3.0, WOOD_FRONT);
            // Ferramentas em cima
            let stone = Color::from_rgba(150, 150, 158, 255);
            draw_line(r.x + 7.0, base - 24.0, r.x + 16.0, base - 27.0, 2.0, WOOD_LINE);
            draw_rectangle(r.x + 14.0, base - 30.0, 5.0, 4.0, stone);
            draw_circle(r.right() - 8.0, base - 25.0, 3.0, stone);
        }
        StructureKind::Campfire => draw_campfire(r, s.is_lit(), time),
        StructureKind::Forge => draw_forge(r, s.is_lit(), time),
        StructureKind::WoodFoundation => draw_foundation(r),
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

/// Bloco em 3/4: face de cima deslocada para cima + face frontal.
fn draw_block(r: Rect, top: Color, front: Color, planks: bool) {
    draw_rectangle(r.x, r.y - WALL_H, r.w, r.h, top);
    // Moldura interna na face de cima, para não confundir com o piso.
    draw_rectangle_lines(r.x + 4.0, r.y - WALL_H + 4.0, r.w - 8.0, r.h - 8.0, 1.0, WOOD_LINE);
    let front_y = r.bottom() - WALL_H;
    draw_rectangle(r.x, front_y, r.w, WALL_H, front);
    if planks {
        for i in 1..4 {
            let x = r.x + i as f32 * r.w / 4.0;
            draw_line(x, front_y, x, r.bottom(), 1.0, WOOD_LINE);
        }
    }
    draw_line(r.x, front_y, r.right(), front_y, 1.5, WOOD_LINE);
    draw_rectangle_lines(r.x, r.y - WALL_H, r.w, r.h + WALL_H, 1.0, WOOD_LINE);
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
pub fn draw_ghost(kind: StructureKind, tile: (i32, i32), valid: bool) {
    let r = building::tile_rect(tile);
    let color = if valid {
        Color::new(0.4, 1.0, 0.5, 0.35)
    } else {
        Color::new(1.0, 0.35, 0.3, 0.35)
    };
    draw_rectangle(r.x, r.y, r.w, r.h, color);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, color.with_alpha(0.9));
    crate::ui::draw_item_icon(kind.item(), Rect::new(r.x + 4.0, r.y + 4.0, r.w - 8.0, r.h - 8.0));
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
pub fn draw_names(avatars: &[Avatar], camera: &Camera2D) {
    for a in avatars {
        let Some(name) = a.name else { continue };
        let lift = if a.mount.is_some() { 58.0 } else { 42.0 };
        let at = camera.world_to_screen(a.player.pos - vec2(0.0, lift));
        let w = measure_text(name, None, 18, 1.0).width;
        draw_rectangle(
            at.x - w / 2.0 - 4.0,
            at.y - 15.0,
            w + 8.0,
            20.0,
            Color::new(0.0, 0.0, 0.0, 0.45),
        );
        draw_text(name, at.x - w / 2.0, at.y, 18.0, WHITE);
    }
}

/// `seat`: altura da sela quando montado (0 = a pé).
fn draw_player(p: &Player, held: Option<ItemId>, shirt: Color, seat: f32) {
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
    let hair = Color::from_rgba(78, 50, 34, 255);

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

fn draw_creature(c: &Creature, time: f32) {
    let s = if c.facing_left { -1.0 } else { 1.0 };
    let lunge = (c.lunge / 0.25) * 6.0 * s;
    let pos = c.pos + vec2(lunge, 0.0);
    let step = if c.moving { (c.anim_time * 9.0).sin() } else { 0.0 };
    let r = c.species.radius();
    draw_ellipse(pos.x, pos.y, r + 3.0, 4.0, 0.0, SHADOW);
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
        let top = vec2(pos.x, pos.y - r * 2.8 - 2.0);
        draw_poly(top.x, top.y, 4, 3.0, 45.0, Color::new(0.4, 0.95, 0.5, 0.9));
    }
    let tint = |col| hurt_tint(col, c.hurt);
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
    }
    if c.can_ride() {
        let k = 1.1;
        let (x, y) = (pos.x, pos.y);
        draw_rectangle(
            x - 6.0 * k,
            y - 21.0 * k,
            12.0 * k,
            5.0 * k,
            Color::from_rgba(96, 60, 32, 255),
        );
        draw_line(x, y - 16.0 * k, x, y - 10.0 * k, 1.5, Color::from_rgba(70, 44, 24, 255));
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

/// Criatura deitada: morta (olho em X) ou desmaiada (olho fechado).
fn draw_lying(species: Species, pos: Vec2, s: f32, asleep: bool) {
    let (x, y) = (pos.x, pos.y);
    let col = match species {
        Species::Dodo => Color::from_rgba(120, 104, 92, 255),
        Species::Trilobite => Color::from_rgba(80, 96, 116, 255),
        Species::Dilophosaur => Color::from_rgba(74, 110, 58, 255),
        Species::Raptor => Color::from_rgba(140, 90, 52, 255),
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
        let lift = if c.knocked_out { 1.4 } else { 2.8 };
        let top = c.pos - vec2(0.0, c.species.radius() * lift + 10.0);
        let at = camera.world_to_screen(top);
        let (w, h) = (44.0, 5.0);
        draw_rectangle(at.x - w / 2.0, at.y, w, h, Color::new(0.0, 0.0, 0.0, 0.6));
        let color = match (c.is_tamed(), c.species.temperament()) {
            (true, _) => Color::new(0.4, 0.85, 0.4, 1.0),
            (false, Temperament::Aggressive) => Color::new(0.9, 0.25, 0.2, 1.0),
            (false, Temperament::Passive) => Color::new(0.85, 0.75, 0.35, 1.0),
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
