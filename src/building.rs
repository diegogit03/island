//! Estruturas construídas pelo jogador, alinhadas ao grid de tiles.
//! Cada tile comporta um piso (`Layer::Floor`), um objeto (`Layer::Object`) e um teto (`Layer::Roof`).
//! Peças de construção têm material (madeira, pedra, metal): define a vida e quanto do dano elas sentem.

use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

use crate::inventory::{Inventory, Stack};
use crate::items::ItemId;
use crate::world::TILE;

pub const STORAGE_SLOTS: usize = 18;
pub const CAMPFIRE_SLOTS: usize = 6;
pub const FORGE_SLOTS: usize = 6;
/// Segundos para a fogueira cozinhar uma carne.
pub const COOK_SECS: f32 = 10.0;
/// Segundos para a forja fundir minério em lingote.
pub const SMELT_SECS: f32 = 8.0;
pub const CAMPFIRE_LIGHT: f32 = 150.0;
pub const FORGE_LIGHT: f32 = 100.0;
pub const TORCH_LIGHT: f32 = 120.0;
/// Tiles de comprimento de um portão grande (passa dino grande).
pub const GATE_SPAN: i32 = 3;
/// Espessura dos batentes de um portão aberto (o vão é o resto).
const GATE_POST: f32 = 6.0;
/// O teto precisa de uma fundação ou parede a até esta distância (em tiles).
pub const ROOF_SUPPORT: i32 = 2;
/// Cada martelada repara esta fração da vida máxima.
pub const REPAIR_FRACTION: f32 = 0.25;

/// O que uma estrutura de fogo transforma enquanto acesa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Process {
    pub input: ItemId,
    pub amount: u32,
    pub output: ItemId,
    pub secs: f32,
    pub label: &'static str,
}
/// Distância máxima (jogador → centro do tile) para posicionar.
pub const PLACE_REACH: f32 = TILE * 3.5;
/// Distância máxima até uma estação (ex.: bancada) para usar receitas que dependem dela.
pub const STATION_RANGE: f32 = TILE * 3.0;
/// Distância máxima (jogador → borda da estrutura) para interagir.
pub const INTERACT_REACH: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureKind {
    Campfire,
    Forge,
    Workbench,
    StorageBox,
    WoodFoundation,
    WoodWall,
    WoodDoor,
    StoneFoundation,
    StoneWall,
    StoneDoor,
    MetalFoundation,
    MetalWall,
    MetalDoor,
    WoodWindow,
    StoneWindow,
    MetalWindow,
    WoodGate,
    StoneGate,
    MetalGate,
    WoodRoof,
    StoneRoof,
    MetalRoof,
    WoodFence,
    Bed,
    Torch,
    Table,
    Chair,
    Rug,
    FlowerPot,
}

/// Material de uma peça de construção.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Wood,
    Stone,
    Metal,
}

impl Material {
    /// Vida de uma parede deste material (as outras peças são proporcionais).
    fn wall_health(self) -> f32 {
        match self {
            Material::Wood => 500.0,
            Material::Stone => 1500.0,
            Material::Metal => 4000.0,
        }
    }

    /// Fração do dano de criaturas e saqueadores que a peça sente.
    pub fn damage_factor(self) -> f32 {
        match self {
            Material::Wood => 1.0,
            Material::Stone => 0.4,
            Material::Metal => 0.15,
        }
    }
}

/// Papel da estrutura (colisão, desenho, interação).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Foundation,
    Wall,
    Door,
    Window,
    /// Portão grande (3 tiles): passa dino grande quando aberto.
    Gate,
    Roof,
    Fence,
    /// Fogueira, forja, bancada e caixa.
    Station,
    Furniture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Floor,
    Object,
    Roof,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlaceError {
    TooFar,
    NotLand,
    Occupied,
    Blocked,
    NoSupport,
}

impl PlaceError {
    pub fn message(&self) -> &'static str {
        match self {
            PlaceError::TooFar => "Longe demais",
            PlaceError::NotLand => "Só é possível construir em terra firme",
            PlaceError::Occupied => "Já existe algo aqui",
            PlaceError::Blocked => "Algo está bloqueando o lugar",
            PlaceError::NoSupport => "O teto precisa de uma parede ou fundação por perto",
        }
    }
}

impl StructureKind {
    pub fn item(self) -> ItemId {
        match self {
            StructureKind::Campfire => ItemId::Campfire,
            StructureKind::Forge => ItemId::Forge,
            StructureKind::Workbench => ItemId::Workbench,
            StructureKind::StorageBox => ItemId::StorageBox,
            StructureKind::WoodFoundation => ItemId::WoodFoundation,
            StructureKind::WoodWall => ItemId::WoodWall,
            StructureKind::WoodDoor => ItemId::WoodDoor,
            StructureKind::StoneFoundation => ItemId::StoneFoundation,
            StructureKind::StoneWall => ItemId::StoneWall,
            StructureKind::StoneDoor => ItemId::StoneDoor,
            StructureKind::MetalFoundation => ItemId::MetalFoundation,
            StructureKind::MetalWall => ItemId::MetalWall,
            StructureKind::MetalDoor => ItemId::MetalDoor,
            StructureKind::WoodWindow => ItemId::WoodWindow,
            StructureKind::StoneWindow => ItemId::StoneWindow,
            StructureKind::MetalWindow => ItemId::MetalWindow,
            StructureKind::WoodGate => ItemId::WoodGate,
            StructureKind::StoneGate => ItemId::StoneGate,
            StructureKind::MetalGate => ItemId::MetalGate,
            StructureKind::WoodRoof => ItemId::WoodRoof,
            StructureKind::StoneRoof => ItemId::StoneRoof,
            StructureKind::MetalRoof => ItemId::MetalRoof,
            StructureKind::WoodFence => ItemId::WoodFence,
            StructureKind::Bed => ItemId::Bed,
            StructureKind::Torch => ItemId::Torch,
            StructureKind::Table => ItemId::Table,
            StructureKind::Chair => ItemId::Chair,
            StructureKind::Rug => ItemId::Rug,
            StructureKind::FlowerPot => ItemId::FlowerPot,
        }
    }

    pub fn piece(self) -> Piece {
        use StructureKind::*;
        match self {
            WoodFoundation | StoneFoundation | MetalFoundation => Piece::Foundation,
            WoodWall | StoneWall | MetalWall => Piece::Wall,
            WoodDoor | StoneDoor | MetalDoor => Piece::Door,
            WoodWindow | StoneWindow | MetalWindow => Piece::Window,
            WoodGate | StoneGate | MetalGate => Piece::Gate,
            WoodRoof | StoneRoof | MetalRoof => Piece::Roof,
            WoodFence => Piece::Fence,
            Campfire | Forge | Workbench | StorageBox => Piece::Station,
            Bed | Torch | Table | Chair | Rug | FlowerPot => Piece::Furniture,
        }
    }

    /// Material das peças de construção (estações e móveis não têm).
    pub fn material(self) -> Option<Material> {
        use StructureKind::*;
        match self {
            WoodFoundation | WoodWall | WoodDoor | WoodWindow | WoodGate | WoodRoof | WoodFence => Some(Material::Wood),
            StoneFoundation | StoneWall | StoneDoor | StoneWindow | StoneGate | StoneRoof => Some(Material::Stone),
            MetalFoundation | MetalWall | MetalDoor | MetalWindow | MetalGate | MetalRoof => Some(Material::Metal),
            _ => None,
        }
    }

    pub fn layer(self) -> Layer {
        match self.piece() {
            Piece::Foundation => Layer::Floor,
            Piece::Roof => Layer::Roof,
            _ => Layer::Object,
        }
    }

    /// Desenhado rente ao chão, antes de tudo (pisos e tapete).
    pub fn flat(self) -> bool {
        self.piece() == Piece::Foundation || self == StructureKind::Rug
    }

    /// Comprimento em tiles (o portão ocupa 3; o resto, 1).
    pub fn span(self) -> i32 {
        if self.piece() == Piece::Gate { GATE_SPAN } else { 1 }
    }

    /// Tem orientação (o jogador gira com T antes de posicionar).
    pub fn rotates(self) -> bool {
        self.span() > 1
    }

    /// Vida cheia.
    pub fn max_health(self) -> f32 {
        let Some(material) = self.material() else {
            return match self {
                StructureKind::Campfire => 200.0,
                StructureKind::Forge => 1200.0,
                StructureKind::Workbench => 300.0,
                StructureKind::StorageBox => 350.0,
                StructureKind::Bed => 250.0,
                StructureKind::Table => 200.0,
                StructureKind::Chair | StructureKind::FlowerPot => 120.0,
                StructureKind::Torch => 100.0,
                _ => 80.0,
            };
        };
        let mult = match self.piece() {
            Piece::Foundation => 1.5,
            Piece::Door | Piece::Roof => 0.8,
            Piece::Gate => 2.0,
            Piece::Fence => 0.6,
            _ => 1.0,
        };
        material.wall_health() * mult
    }

    /// Fração do dano que a estrutura sente (pedra e metal resistem; a forja é de pedra).
    pub fn damage_factor(self) -> f32 {
        match (self.material(), self) {
            (Some(m), _) => m.damage_factor(),
            (None, StructureKind::Forge) => Material::Stone.damage_factor(),
            (None, _) => 1.0,
        }
    }

    /// Materiais de uma martelada: `REPAIR_FRACTION` de cada ingrediente da receita (ao menos 1 do primeiro).
    pub fn repair_cost(self) -> Vec<(ItemId, u32)> {
        let Some(recipe) = crate::crafting::recipe_for(self.item()) else {
            return Vec::new();
        };
        recipe
            .inputs
            .iter()
            .enumerate()
            .filter_map(|(i, &(item, n))| {
                let need = (n as f32 * REPAIR_FRACTION).round() as u32;
                let need = if i == 0 { need.max(1) } else { need };
                (need > 0).then_some((item, need))
            })
            .collect()
    }

    /// Estruturas de fogo (queimam combustível e transformam itens).
    pub fn process(self) -> Option<Process> {
        match self {
            StructureKind::Campfire => Some(Process {
                input: ItemId::RawMeat,
                amount: 1,
                output: ItemId::CookedMeat,
                secs: COOK_SECS,
                label: "Cozinhando carne",
            }),
            StructureKind::Forge => Some(Process {
                input: ItemId::MetalOre,
                amount: 2,
                output: ItemId::MetalIngot,
                secs: SMELT_SECS,
                label: "Fundindo metal",
            }),
            _ => None,
        }
    }

    pub fn interactive(self) -> bool {
        matches!(
            self,
            StructureKind::Campfire
                | StructureKind::Forge
                | StructureKind::Workbench
                | StructureKind::StorageBox
                | StructureKind::Bed
        ) || self.opens()
    }

    /// Porta ou portão (F abre/fecha).
    pub fn opens(self) -> bool {
        matches!(self.piece(), Piece::Door | Piece::Gate)
    }

    /// Estrutura alta: segura pedras do estilingue (janela, cerca e móveis deixam passar).
    pub fn stops_shots(self) -> bool {
        matches!(self.piece(), Piece::Wall | Piece::Door | Piece::Gate)
            || matches!(
                self,
                StructureKind::Forge | StructureKind::Workbench | StructureKind::StorageBox
            )
    }

    /// Conta como parte de uma base (para saqueadores e para o teto se apoiar).
    pub fn supports_roof(self) -> bool {
        matches!(
            self.piece(),
            Piece::Foundation | Piece::Wall | Piece::Door | Piece::Window | Piece::Gate
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Structure {
    pub kind: StructureKind,
    /// Tile de referência (o do meio, no portão).
    pub tile: (i32, i32),
    /// Porta aberta.
    #[serde(default)]
    pub open: bool,
    /// Segundos restantes do combustível que está queimando agora (fogueira).
    #[serde(default)]
    pub fuel: f32,
    /// Fogueira ligada: queima o combustível do próprio inventário enquanto houver.
    #[serde(default)]
    pub active: bool,
    /// Progresso (segundos) da carne que está cozinhando.
    #[serde(default)]
    pub cook: f32,
    /// Conteúdo (caixa de armazenamento, fogueira).
    #[serde(default)]
    pub storage: Option<Inventory>,
    /// Vida (0 em saves anteriores à v0.10: `normalize` enche).
    #[serde(default)]
    pub health: f32,
    /// Quem posicionou (a cama é o ponto de renascer dessa pessoa).
    #[serde(default)]
    pub owner: Option<String>,
    /// Peças compridas (portão) na vertical.
    #[serde(default)]
    pub vertical: bool,
}

/// Segundos de fogo que cada item rende como combustível.
pub fn fuel_value(item: ItemId) -> Option<f32> {
    match item {
        ItemId::Wood => Some(60.0),
        ItemId::Thatch => Some(15.0),
        _ => None,
    }
}

impl Structure {
    pub fn new(kind: StructureKind, tile: (i32, i32)) -> Self {
        Self {
            kind,
            tile,
            open: false,
            fuel: 0.0,
            active: false,
            cook: 0.0,
            storage: storage_slots(kind).map(Inventory::new),
            health: kind.max_health(),
            owner: None,
            vertical: false,
        }
    }

    /// Ajusta estruturas vindas de saves antigos (v0.3: fogueira sem inventário; v0.10: vida).
    pub fn normalize(&mut self) {
        if self.storage.is_none() {
            self.storage = storage_slots(self.kind).map(Inventory::new);
        }
        if self.kind == StructureKind::Campfire && self.fuel > 0.0 {
            self.active = true;
        }
        if self.health <= 0.0 {
            self.health = self.kind.max_health();
        }
        self.health = self.health.min(self.kind.max_health());
        if !self.kind.rotates() {
            self.vertical = false;
        }
    }

    pub fn rect(&self) -> Rect {
        footprint(self.kind, self.tile, self.vertical)
    }

    /// Ocupa este tile?
    pub fn covers(&self, tile: (i32, i32)) -> bool {
        self.rect().contains(tile_rect(tile).center())
    }

    pub fn health_frac(&self) -> f32 {
        (self.health / self.kind.max_health()).clamp(0.0, 1.0)
    }

    pub fn is_lit(&self) -> bool {
        self.kind == StructureKind::Torch || (self.kind.process().is_some() && self.active && self.fuel > 0.0)
    }

    pub fn light_radius(&self) -> f32 {
        match self.kind {
            StructureKind::Forge => FORGE_LIGHT,
            StructureKind::Torch => TORCH_LIGHT,
            _ => CAMPFIRE_LIGHT,
        }
    }

    /// Y usada no y-sort: a base da estrutura.
    pub fn sort_y(&self) -> f32 {
        self.rect().bottom() - 1.0
    }

    /// Um círculo (jogador, criatura) colide com esta estrutura?
    pub fn blocks(&self, pos: Vec2, radius: f32) -> bool {
        let r = self.rect();
        match self.kind.piece() {
            Piece::Foundation | Piece::Roof => false,
            Piece::Door if self.open => false,
            // Aberto, só os batentes das pontas seguram.
            Piece::Gate if self.open => gate_posts(r, self.vertical)
                .iter()
                .any(|post| distance_to_rect(pos, *post) < radius),
            _ => match self.kind {
                StructureKind::Rug => false,
                StructureKind::Campfire => r.center().distance(pos) < radius + 10.0,
                StructureKind::Torch => r.center().distance(pos) < radius + 4.0,
                StructureKind::Chair | StructureKind::FlowerPot => r.center().distance(pos) < radius + 8.0,
                _ => distance_to_rect(pos, r) < radius,
            },
        }
    }

    /// Segura uma pedra do estilingue neste ponto?
    pub fn blocks_shot(&self, pos: Vec2, radius: f32) -> bool {
        self.kind.stops_shots() && self.blocks(pos, radius)
    }

    /// Recebe dano (já reduzido pelo material). Retorna true se foi destruída.
    pub fn take_damage(&mut self, damage: f32) -> bool {
        self.health -= damage * self.kind.damage_factor();
        self.health <= 0.0
    }

    /// Repara uma martelada. Retorna a vida recuperada.
    pub fn repair(&mut self) -> f32 {
        let max = self.kind.max_health();
        let before = self.health;
        self.health = (self.health + max * REPAIR_FRACTION).min(max);
        self.health - before
    }

    /// Pega a próxima unidade de combustível do inventário, se houver.
    fn refuel(&mut self) -> bool {
        let Some(storage) = self.storage.as_mut() else {
            return false;
        };
        for item in [ItemId::Thatch, ItemId::Wood] {
            if storage.remove(item, 1) {
                self.fuel += fuel_value(item).unwrap_or(0.0);
                return true;
            }
        }
        false
    }

    /// Acende/apaga a fogueira. Acender sem combustível falha.
    pub fn toggle_fire(&mut self) -> bool {
        if self.active {
            self.active = false;
            return true;
        }
        if self.fuel <= 0.0 && !self.refuel() {
            return false;
        }
        self.active = true;
        true
    }

    /// Avança fogueira/forja: queima combustível e transforma itens (carne, minério).
    pub fn tick(&mut self, dt: f32) {
        let Some(process) = self.kind.process() else { return };
        if !self.active {
            return;
        }
        self.fuel -= dt;
        if self.fuel <= 0.0 {
            self.fuel = 0.0;
            if !self.refuel() {
                self.active = false;
                return;
            }
        }
        let Some(storage) = self.storage.as_mut() else { return };
        if storage.count(process.input) < process.amount {
            self.cook = 0.0;
            return;
        }
        self.cook += dt;
        if self.cook < process.secs {
            return;
        }
        self.cook = 0.0;
        storage.remove(process.input, process.amount);
        if storage.insert_stack(Stack::new(process.output, 1)).is_some() {
            // Sem espaço para o resultado: devolve a matéria-prima e espera.
            storage.add(process.input, process.amount);
        }
    }
}

fn storage_slots(kind: StructureKind) -> Option<usize> {
    match kind {
        StructureKind::StorageBox => Some(STORAGE_SLOTS),
        StructureKind::Campfire => Some(CAMPFIRE_SLOTS),
        StructureKind::Forge => Some(FORGE_SLOTS),
        _ => None,
    }
}

/// Retângulo ocupado por `kind` no `tile` (o portão se estende para os lados, ou para cima e para baixo).
pub fn footprint(kind: StructureKind, tile: (i32, i32), vertical: bool) -> Rect {
    let r = tile_rect(tile);
    let span = kind.span();
    if span == 1 {
        return r;
    }
    let extra = (span - 1) as f32 / 2.0 * TILE;
    if vertical {
        Rect::new(r.x, r.y - extra, TILE, TILE * span as f32)
    } else {
        Rect::new(r.x - extra, r.y, TILE * span as f32, TILE)
    }
}

/// Tiles cobertos por `kind` no `tile`.
pub fn footprint_tiles(kind: StructureKind, tile: (i32, i32), vertical: bool) -> Vec<(i32, i32)> {
    let half = (kind.span() - 1) / 2;
    (-half..=half)
        .map(|d| {
            if vertical {
                (tile.0, tile.1 + d)
            } else {
                (tile.0 + d, tile.1)
            }
        })
        .collect()
}

/// Batentes de um portão aberto (uma ponta de cada lado do vão).
pub fn gate_posts(r: Rect, vertical: bool) -> [Rect; 2] {
    if vertical {
        [
            Rect::new(r.x, r.y, r.w, GATE_POST),
            Rect::new(r.x, r.bottom() - GATE_POST, r.w, GATE_POST),
        ]
    } else {
        [
            Rect::new(r.x, r.y, GATE_POST, r.h),
            Rect::new(r.right() - GATE_POST, r.y, GATE_POST, r.h),
        ]
    }
}

pub fn tile_of(pos: Vec2) -> (i32, i32) {
    ((pos.x / TILE).floor() as i32, (pos.y / TILE).floor() as i32)
}

pub fn tile_rect(tile: (i32, i32)) -> Rect {
    Rect::new(tile.0 as f32 * TILE, tile.1 as f32 * TILE, TILE, TILE)
}

/// Distância de um ponto até um retângulo (0 se estiver dentro).
pub fn distance_to_rect(p: Vec2, r: Rect) -> f32 {
    let closest = vec2(p.x.clamp(r.x, r.right()), p.y.clamp(r.y, r.bottom()));
    closest.distance(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_block_and_open_doors_do_not() {
        let wall = Structure::new(StructureKind::WoodWall, (2, 2));
        let center = wall.rect().center();
        assert!(wall.blocks(center, 7.0));
        assert!(wall.blocks(center + vec2(0.0, TILE / 2.0 + 5.0), 7.0));
        assert!(!wall.blocks(center + vec2(0.0, TILE / 2.0 + 8.0), 7.0));

        let mut door = Structure::new(StructureKind::WoodDoor, (2, 2));
        assert!(door.blocks(center, 7.0));
        door.open = true;
        assert!(!door.blocks(center, 7.0));

        let floor = Structure::new(StructureKind::WoodFoundation, (2, 2));
        assert!(!floor.blocks(center, 7.0));
        let roof = Structure::new(StructureKind::StoneRoof, (2, 2));
        assert!(!roof.blocks(center, 7.0));
        assert_eq!(roof.kind.layer(), Layer::Roof);
    }

    #[test]
    fn open_gate_lets_a_mammoth_through_but_keeps_its_posts() {
        let mut gate = Structure::new(StructureKind::StoneGate, (5, 5));
        let r = gate.rect();
        assert_eq!((r.w, r.h), (TILE * 3.0, TILE));
        assert!(gate.covers((4, 5)) && gate.covers((6, 5)) && !gate.covers((5, 6)));
        let middle = r.center();
        assert!(gate.blocks(middle, 18.0));
        gate.open = true;
        assert!(!gate.blocks(middle, 18.0), "vão de quase 3 tiles");
        assert!(gate.blocks(vec2(r.x + 2.0, middle.y), 4.0), "batente");

        gate.vertical = true;
        let r = gate.rect();
        assert_eq!((r.w, r.h), (TILE, TILE * 3.0));
        assert!(gate.covers((5, 4)) && gate.covers((5, 6)));
        assert_eq!(
            footprint_tiles(StructureKind::StoneGate, (5, 5), true),
            vec![(5, 4), (5, 5), (5, 6)]
        );
    }

    #[test]
    fn windows_and_fences_let_stones_fly_but_not_people() {
        let center = tile_rect((1, 1)).center();
        for kind in [StructureKind::WoodWindow, StructureKind::WoodFence] {
            let s = Structure::new(kind, (1, 1));
            assert!(s.blocks(center, 7.0), "{kind:?} bloqueia quem anda");
            assert!(!s.blocks_shot(center, 1.0), "{kind:?} deixa pedras passarem");
        }
        assert!(Structure::new(StructureKind::MetalWall, (1, 1)).blocks_shot(center, 1.0));
        assert!(!Structure::new(StructureKind::Rug, (1, 1)).blocks(center, 7.0));
    }

    #[test]
    fn stronger_materials_last_longer_and_hammer_repairs() {
        let hits = |kind: StructureKind| {
            let mut s = Structure::new(kind, (0, 0));
            (1..).find(|_| s.take_damage(50.0)).unwrap()
        };
        let (wood, stone, metal) = (
            hits(StructureKind::WoodWall),
            hits(StructureKind::StoneWall),
            hits(StructureKind::MetalWall),
        );
        assert!(wood < stone && stone * 4 < metal, "{wood} {stone} {metal}");

        let mut wall = Structure::new(StructureKind::WoodWall, (0, 0));
        wall.take_damage(400.0);
        assert!(wall.health_frac() < 0.25);
        assert!(wall.repair() > 0.0);
        for _ in 0..4 {
            wall.repair();
        }
        assert_eq!(wall.health, StructureKind::WoodWall.max_health());
        assert_eq!(wall.repair(), 0.0, "inteira");
    }

    #[test]
    fn torch_is_always_lit() {
        let torch = Structure::new(StructureKind::Torch, (0, 0));
        assert!(torch.is_lit());
        assert_eq!(torch.light_radius(), TORCH_LIGHT);
    }

    fn fire_with(items: &[(ItemId, u32)]) -> Structure {
        let mut fire = Structure::new(StructureKind::Campfire, (0, 0));
        for &(item, n) in items {
            fire.storage.as_mut().unwrap().add(item, n);
        }
        fire
    }

    #[test]
    fn campfire_needs_fuel_and_burns_it() {
        let mut empty = fire_with(&[]);
        assert!(!empty.toggle_fire());
        assert!(!empty.is_lit());

        let mut fire = fire_with(&[(ItemId::Wood, 2)]);
        assert!(fire.toggle_fire());
        assert!(fire.is_lit());
        assert_eq!(fire.storage.as_ref().unwrap().count(ItemId::Wood), 1);
        for _ in 0..121 {
            fire.tick(1.0);
        }
        assert!(!fire.is_lit(), "apaga quando acaba a madeira");
        assert_eq!(fire.storage.as_ref().unwrap().count(ItemId::Wood), 0);
    }

    #[test]
    fn campfire_cooks_meat() {
        let mut fire = fire_with(&[(ItemId::Wood, 5), (ItemId::RawMeat, 2)]);
        fire.toggle_fire();
        for _ in 0..(COOK_SECS as usize * 2 + 1) {
            fire.tick(1.0);
        }
        let storage = fire.storage.as_ref().unwrap();
        assert_eq!(storage.count(ItemId::CookedMeat), 2);
        assert_eq!(storage.count(ItemId::RawMeat), 0);
    }

    #[test]
    fn forge_smelts_two_ore_into_an_ingot() {
        let mut forge = Structure::new(StructureKind::Forge, (0, 0));
        let storage = forge.storage.as_mut().unwrap();
        storage.add(ItemId::Wood, 3);
        storage.add(ItemId::MetalOre, 5);
        assert!(forge.toggle_fire());
        for _ in 0..(SMELT_SECS as usize * 2 + 1) {
            forge.tick(1.0);
        }
        let storage = forge.storage.as_ref().unwrap();
        assert_eq!(storage.count(ItemId::MetalIngot), 2);
        assert_eq!(storage.count(ItemId::MetalOre), 1, "1 minério sozinho não funde");
        assert_eq!(forge.light_radius(), FORGE_LIGHT);
    }

    #[test]
    fn unlit_campfire_does_not_cook() {
        let mut fire = fire_with(&[(ItemId::Wood, 5), (ItemId::RawMeat, 1)]);
        fire.tick(COOK_SECS * 2.0);
        assert_eq!(fire.storage.as_ref().unwrap().count(ItemId::CookedMeat), 0);
    }

    #[test]
    fn old_structures_are_fixed_on_normalize() {
        let mut old = Structure::new(StructureKind::Campfire, (0, 0));
        old.storage = None;
        old.fuel = 30.0;
        old.health = 0.0;
        old.normalize();
        assert!(old.storage.is_some() && old.is_lit());
        assert_eq!(
            old.health,
            StructureKind::Campfire.max_health(),
            "save sem vida volta inteira"
        );
    }

    #[test]
    fn only_containers_have_storage() {
        assert!(Structure::new(StructureKind::StorageBox, (0, 0)).storage.is_some());
        assert!(Structure::new(StructureKind::Campfire, (0, 0)).storage.is_some());
        assert!(Structure::new(StructureKind::WoodWall, (0, 0)).storage.is_none());
    }
}
