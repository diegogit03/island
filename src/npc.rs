//! NPCs: moradores dos acampamentos, que podem ser recrutados para a tribo de um jogador,
//! e saqueadores que atacam bases à noite.
//! Aqui ficam o estado de cada NPC e as regras puras (ferramenta certa, custo de recrutar, ordens);
//! o comportamento a cada tick (andar, coletar, lutar) fica em `sim/tribe.rs` e `sim/raid.rs`.

use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

use crate::creatures::Foe;
use crate::inventory::{Inventory, Stack};
use crate::items::{ItemId, ToolKind};
use crate::player::{Facing, Player};
use crate::world::NodeKind;

/// Slots do inventário de um NPC.
pub const NPC_SLOTS: usize = 12;
pub const MAX_HEALTH: f32 = 100.0;
/// Membros por tribo (por jogador).
pub const MAX_TRIBE: usize = 8;
/// Coletando, leva tudo ao baú quando junta esta quantidade de itens.
pub const HAUL_AT: u32 = 40;
/// Moradores que cada acampamento mantém (recrutados são repostos com o tempo).
pub const CAMP_POPULATION: usize = 3;
/// Vida de um saqueador.
pub const RAIDER_HEALTH: f32 = 80.0;
/// `camp` de quem não mora em acampamento nenhum (saqueadores).
pub const NO_CAMP: usize = usize::MAX;

const NAMES: [&str; 20] = [
    "Iara", "Caio", "Bento", "Luna", "Tainá", "Raoni", "Jaci", "Ravi", "Maya", "Kauê", "Aruana", "Cauã", "Moema",
    "Potira", "Iberê", "Jurema", "Tupã", "Anahí", "Ubiratã", "Yara",
];

/// Itens que um morador pede para entrar na tribo (escolhido pelo `id`).
const RECRUIT_COSTS: [(ItemId, u32); 5] = [
    (ItemId::CookedMeat, 6),
    (ItemId::Hide, 12),
    (ItemId::Berry, 30),
    (ItemId::Flint, 15),
    (ItemId::Wood, 60),
];

/// O que um NPC coleta quando mandado trabalhar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatherKind {
    Wood,
    Stone,
    Metal,
}

impl GatherKind {
    pub fn node(self) -> NodeKind {
        match self {
            GatherKind::Wood => NodeKind::Tree,
            GatherKind::Stone => NodeKind::Rock,
            GatherKind::Metal => NodeKind::MetalRock,
        }
    }

    /// Ferramenta que rende mais (machado: madeira e pedra; picareta: metal).
    pub fn tool(self) -> ToolKind {
        match self {
            GatherKind::Wood | GatherKind::Stone => ToolKind::Hatchet,
            GatherKind::Metal => ToolKind::Pickaxe,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GatherKind::Wood => "madeira",
            GatherKind::Stone => "pedra",
            GatherKind::Metal => "metal",
        }
    }
}

/// Ordem atual de um membro da tribo.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Segue o dono e defende ele de quem o estiver caçando.
    Follow,
    Stay {
        at: [f32; 2],
    },
    /// Coleta perto de `area` e guarda no baú mais próximo quando enche.
    Gather {
        kind: GatherKind,
        area: [f32; 2],
    },
    /// Guarda o posto: ataca criaturas perigosas que chegam perto.
    Defend {
        post: [f32; 2],
    },
}

impl Order {
    pub fn label(&self) -> String {
        match self {
            Order::Follow => "Seguindo você".to_owned(),
            Order::Stay { .. } => "Parado".to_owned(),
            Order::Gather { kind, .. } => format!("Coletando {}", kind.name()),
            Order::Defend { .. } => "Defendendo o posto".to_owned(),
        }
    }
}

pub fn name_for(id: u32) -> &'static str {
    NAMES[(id as usize).wrapping_mul(7) % NAMES.len()]
}

pub struct Npc {
    pub id: u32,
    pub name: String,
    /// Corpo: posição, direção, animação e timers de golpe (anda como um jogador).
    pub body: Player,
    pub health: f32,
    pub inventory: Inventory,
    /// Nome do jogador dono da tribo (`None` = morador de acampamento).
    pub owner: Option<String>,
    /// Acampamento de origem (índice em `world.camps`).
    pub camp: usize,
    pub order: Order,
    /// Variação de aparência (camisa e cabelo).
    pub look: u8,
    /// Quem está atacando (membros da tribo).
    pub target: Option<Foe>,
    /// Item na mão agora (depende do que está fazendo).
    pub held: Option<ItemId>,
    /// Nó de recurso em que está trabalhando e um a evitar (ficou preso tentando chegar nele).
    pub work: Option<usize>,
    pub skip: Option<usize>,
    /// Indo esvaziar o inventário no baú.
    pub hauling: bool,
    /// Tempo sem sair do lugar enquanto tenta andar.
    pub stuck: f32,
    /// Tempo desde o último dano (regenera depois de um tempo).
    pub calm: f32,
    /// Destino de passeio dos moradores do acampamento (e desvio dos saqueadores presos).
    pub stroll: Vec2,
    pub stroll_timer: f32,
    /// Saqueador: ataca bases, jogadores e a tribo. Não vai para o save.
    pub hostile: bool,
    /// Base que o saqueador veio atacar.
    pub raid: Vec2,
    /// Saqueador indo embora (amanheceu ou não sobrou nada para quebrar).
    pub retreat: bool,
    /// Segundos desde que o saqueador apareceu.
    pub life: f32,
    /// Jogador que deu o último golpe (XP de quem abateu um saqueador).
    pub last_hit: Option<usize>,
}

impl Npc {
    pub fn new(id: u32, pos: Vec2, camp: usize, look: u8) -> Self {
        Self {
            id,
            name: name_for(id).to_owned(),
            body: Player::new(pos),
            health: MAX_HEALTH,
            inventory: Inventory::new(NPC_SLOTS),
            owner: None,
            camp,
            order: Order::Stay { at: pos.into() },
            look,
            target: None,
            held: None,
            work: None,
            skip: None,
            hauling: false,
            stuck: 0.0,
            calm: 0.0,
            stroll: pos,
            stroll_timer: 0.0,
            hostile: false,
            raid: pos,
            retreat: false,
            life: 0.0,
            last_hit: None,
        }
    }

    /// Saqueador vindo atacar a base em `raid`, com uma arma e o que vai deixar cair ao morrer.
    pub fn raider(id: u32, pos: Vec2, raid: Vec2, weapon: ItemId, loot: &[(ItemId, u32)]) -> Self {
        let mut n = Npc::new(id, pos, NO_CAMP, (id % 6) as u8);
        n.name = "Saqueador".to_owned();
        n.hostile = true;
        n.raid = raid;
        n.health = RAIDER_HEALTH;
        n.inventory.add(weapon, 1);
        for &(item, count) in loot {
            n.inventory.add(item, count);
        }
        n.held = Some(weapon);
        n
    }

    pub fn is_member(&self) -> bool {
        self.owner.is_some()
    }

    pub fn is_villager(&self) -> bool {
        self.owner.is_none() && !self.hostile
    }

    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }

    pub fn pos(&self) -> Vec2 {
        self.body.pos
    }

    pub fn recruit_cost(&self) -> (ItemId, u32) {
        RECRUIT_COSTS[self.id as usize % RECRUIT_COSTS.len()]
    }

    /// Melhor ferramenta do tipo no inventário (metal antes de pedra).
    pub fn best_tool(&self, kind: ToolKind) -> Option<ItemId> {
        self.inventory
            .slots
            .iter()
            .flatten()
            .map(|s| s.item)
            .filter(|i| i.tool() == Some(kind))
            .max_by_key(|&i| ItemId::tool_tier(Some(i)))
    }

    /// Item que dá mais dano corpo a corpo (ou mãos vazias).
    pub fn best_weapon(&self) -> Option<ItemId> {
        self.inventory
            .slots
            .iter()
            .flatten()
            .map(|s| s.item)
            .filter(|i| i.max_durability().is_some() && *i != ItemId::Slingshot)
            .max_by(|a, b| ItemId::melee_damage(Some(*a)).total_cmp(&ItemId::melee_damage(Some(*b))))
            .filter(|&i| ItemId::melee_damage(Some(i)) > ItemId::melee_damage(None))
    }

    /// Gasta durabilidade do item. Retorna true se ele quebrou.
    pub fn wear(&mut self, item: ItemId) -> bool {
        let Some(slot) = self
            .inventory
            .slots
            .iter()
            .position(|s| s.is_some_and(|s| s.item == item))
        else {
            return false;
        };
        self.inventory.wear(slot).is_some()
    }

    /// Juntou o bastante (ou não cabe mais nada): hora de levar ao baú.
    pub fn wants_to_haul(&self) -> bool {
        let carried: u32 = self
            .inventory
            .slots
            .iter()
            .flatten()
            .filter(|s| s.item.max_durability().is_none())
            .map(|s| s.count)
            .sum();
        carried >= HAUL_AT || self.inventory.slots.iter().all(Option::is_some)
    }

    /// Tira tudo o que não é ferramenta/arma (o que ele coletou), para guardar no baú.
    pub fn take_haul(&mut self) -> Vec<Stack> {
        self.inventory
            .slots
            .iter_mut()
            .filter(|s| s.is_some_and(|s| s.item.max_durability().is_none()))
            .filter_map(Option::take)
            .collect()
    }

    pub fn has_haul(&self) -> bool {
        self.inventory
            .slots
            .iter()
            .flatten()
            .any(|s| s.item.max_durability().is_none())
    }

    pub fn face(&mut self, x: f32) {
        self.body.face_towards(x);
    }
}

/// NPC salvo no save (o que ele estava fazendo no momento recomeça).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcSave {
    pub id: u32,
    pub name: String,
    pub pos: [f32; 2],
    pub health: f32,
    pub inventory: Inventory,
    pub owner: Option<String>,
    pub camp: usize,
    pub order: Order,
    pub look: u8,
}

impl From<&Npc> for NpcSave {
    fn from(n: &Npc) -> Self {
        Self {
            id: n.id,
            name: n.name.clone(),
            pos: n.body.pos.into(),
            health: n.health,
            inventory: n.inventory.clone(),
            owner: n.owner.clone(),
            camp: n.camp,
            order: n.order,
            look: n.look,
        }
    }
}

impl From<NpcSave> for Npc {
    fn from(s: NpcSave) -> Self {
        let mut n = Npc::new(s.id, Vec2::from(s.pos), s.camp, s.look);
        n.name = s.name;
        n.health = s.health.clamp(1.0, MAX_HEALTH);
        n.inventory = s.inventory;
        n.owner = s.owner;
        n.order = s.order;
        n
    }
}

/// NPC como o cliente precisa para desenhar e para o painel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcView {
    pub id: u32,
    pub name: String,
    pub pos: [f32; 2],
    pub facing_left: bool,
    pub moving: bool,
    pub anim_time: f32,
    pub swing_timer: f32,
    pub held: Option<ItemId>,
    pub health: f32,
    pub owner: Option<String>,
    pub look: u8,
    pub order: Order,
    pub hauling: bool,
    pub hostile: bool,
    /// Só para quem está com o painel aberto (dono, ou qualquer um num morador).
    pub inventory: Option<Inventory>,
}

impl NpcView {
    pub fn new(n: &Npc, with_inventory: bool) -> Self {
        Self {
            id: n.id,
            name: n.name.clone(),
            pos: n.body.pos.into(),
            facing_left: n.body.facing == Facing::Left,
            moving: n.body.moving,
            anim_time: n.body.anim_time,
            swing_timer: n.body.swing_timer,
            held: n.held,
            health: n.health,
            owner: n.owner.clone(),
            look: n.look,
            order: n.order,
            hauling: n.hauling,
            hostile: n.hostile,
            // Morador: painel sem slots (só o pedido de recrutamento).
            inventory: with_inventory.then(|| match n.owner {
                Some(_) => n.inventory.clone(),
                None => Inventory::new(0),
            }),
        }
    }

    pub fn health_frac(&self) -> f32 {
        let max = if self.hostile { RAIDER_HEALTH } else { MAX_HEALTH };
        (self.health / max).clamp(0.0, 1.0)
    }

    /// O que um morador pede para entrar na tribo.
    pub fn recruit_cost(&self) -> (ItemId, u32) {
        RECRUIT_COSTS[self.id as usize % RECRUIT_COSTS.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_right_tools() {
        let mut npc = Npc::new(1, Vec2::ZERO, 0, 0);
        assert_eq!(npc.best_weapon(), None, "de mãos vazias");
        npc.inventory.add(ItemId::StoneHatchet, 1);
        npc.inventory.add(ItemId::MetalHatchet, 1);
        npc.inventory.add(ItemId::WoodenSpear, 1);
        npc.inventory.add(ItemId::StonePickaxe, 1);
        assert_eq!(npc.best_tool(ToolKind::Hatchet), Some(ItemId::MetalHatchet));
        assert_eq!(npc.best_tool(GatherKind::Metal.tool()), Some(ItemId::StonePickaxe));
        assert_eq!(npc.best_weapon(), Some(ItemId::WoodenSpear));
        npc.inventory.add(ItemId::MetalSword, 1);
        assert_eq!(npc.best_weapon(), Some(ItemId::MetalSword));
    }

    #[test]
    fn hauls_only_what_it_gathered() {
        let mut npc = Npc::new(1, Vec2::ZERO, 0, 0);
        npc.inventory.add(ItemId::StoneHatchet, 1);
        npc.inventory.add(ItemId::Wood, 40);
        npc.inventory.add(ItemId::Thatch, 5);
        assert!(npc.has_haul());
        let haul = npc.take_haul();
        assert_eq!(haul.iter().map(|s| s.count).sum::<u32>(), 45);
        assert_eq!(npc.inventory.count(ItemId::StoneHatchet), 1, "a ferramenta fica");
        assert!(!npc.has_haul());
    }

    #[test]
    fn save_round_trip_and_costs() {
        let mut npc = Npc::new(9, vec2(3.0, 4.0), 2, 1);
        npc.owner = Some("Ana".to_owned());
        npc.order = Order::Gather {
            kind: GatherKind::Wood,
            area: [10.0, 20.0],
        };
        npc.inventory.add(ItemId::Wood, 3);
        let back = Npc::from(NpcSave::from(&npc));
        assert_eq!(NpcSave::from(&back), NpcSave::from(&npc));
        assert_eq!(npc.recruit_cost(), NpcView::new(&npc, false).recruit_cost());
        assert_ne!(name_for(1), name_for(2));
    }
}
