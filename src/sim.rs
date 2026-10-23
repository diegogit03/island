//! Simulação autoritativa: mundo, criaturas e todos os jogadores.
//! Roda no servidor (thread do host ou processo dedicado). Não lê teclado nem desenha nada:
//! recebe `Input`/`Command` de cada jogador e produz snapshots e eventos.

use macroquad::math::{Vec2, vec2};

use crate::building::{self, Layer, PlaceError, Structure, StructureKind};
use crate::combat::{self, Projectile};
use crate::crafting::{self, CraftError, CraftQueue, RECIPES, Recipe};
use crate::creatures::{
    self, AiInput, AiPlayer, Command as Order, Creature, CreatureSave, Event, Hit, MountHarvest, Species,
};
use crate::daynight;
use crate::gathering;
use crate::inventory::{HOTBAR_SIZE, INVENTORY_SIZE, Inventory, Stack};
use crate::items::{ItemId, ToolKind};
use crate::npc::{Npc, NpcSave, NpcView};
use crate::player::{self, Facing, Player};
use crate::progression::{self, LearnError, Progression};
use crate::protocol::{
    Command, ContainerRef, CreatureView, DebugCmd, Input, MAX_CHAT_LEN, NetEvent, PlayerId, PlayerView, PopupStyle,
    PrivateState, SlotTarget, Snapshot,
};
use crate::raft::{self, Raft, RaftSave};
use crate::rng::Rng;
use crate::save::{BagSave, LEGACY_PLAYER, PlayerSave, SaveData};
use crate::slots;
use crate::vitals::{self, Vitals};
use crate::world::{BagKind, GroundBag, NodeKind, TILE, Tile, World};

mod raid;
mod tribe;

pub const MAP_SIZE: usize = 160;
pub const PICKUP_REACH: f32 = 28.0;
/// Água que cada golpe num cacto dá.
const CACTUS_WATER: f32 = 6.0;
/// Intervalo entre tentativas de repor a fauna.
const RESPAWN_SECS: f32 = 10.0;
/// Montado, a velocidade é a corrida da espécie vezes isto (Shift multiplica de novo).
const RIDE_MULT: f32 = 1.4;
const RIDE_SPRINT: f32 = 1.3;

/// Alvo de um golpe (E / clique).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Node(usize),
    Creature(usize),
    /// Com o martelo na mão: estrutura a reparar.
    Structure(usize),
    /// Saqueador, pelo `id`.
    Npc(u32),
}

/// Raio de acerto de um NPC (o corpo é do tamanho de um jogador).
pub const NPC_HIT_RADIUS: f32 = 8.0;

/// Algo com que o jogador pode interagir com F.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Bag(usize),
    Structure(usize),
    /// Dino desacordado (domesticação) ou domesticado, pelo `id`.
    Creature(u32),
    /// Jangada livre, pelo `id`.
    Raft(u32),
    /// NPC (morador ou membro da tribo), pelo `id`.
    Npc(u32),
}

// ---------- Consultas compartilhadas com o cliente (anel de alvo, dicas "[F]") ----------

/// Alvo mais próximo ao alcance, preferindo o que está à frente do personagem.
/// Criaturas vivas e saqueadores (`foes`: `id` e posição) têm prioridade sobre recursos; carcaças contam
/// como recurso. Com o martelo, só estruturas (as danificadas primeiro).
pub fn find_target(
    world: &World,
    creatures: &[Creature],
    foes: &[(u32, Vec2)],
    p: &Player,
    held: Option<ItemId>,
) -> Option<Target> {
    let behind = |pos: Vec2| (pos.x - p.pos.x) * p.facing.sign() < -4.0;
    if held == Some(ItemId::Hammer) {
        return world
            .structures
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind.layer() != Layer::Roof)
            .filter_map(|(i, s)| {
                let gap = building::distance_to_rect(p.pos, s.rect());
                let mut score = gap + if behind(s.rect().center()) { 20.0 } else { 0.0 };
                if s.health_frac() >= 1.0 {
                    score += 15.0;
                }
                if s.kind.layer() == Layer::Floor {
                    score += 10.0;
                }
                (gap <= player::REACH).then_some((score, Target::Structure(i)))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, t)| t);
    }
    let weapon_reach = player::REACH + ItemId::reach_bonus(held);
    let score = |pos: Vec2, radius: f32, reach: f32| {
        let gap = pos.distance(p.pos) - radius;
        (gap <= reach).then_some(gap + if behind(pos) { 20.0 } else { 0.0 })
    };
    let nodes = world
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_alive())
        .filter_map(|(i, n)| score(n.pos, n.kind.collision_radius(), player::REACH).map(|s| (s, Target::Node(i))));
    // Domesticados não são alvo; desmaiados também não (bater reduz a efetividade).
    // Voadores no ar só são alcançados pelo estilingue.
    let creatures = creatures
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_wild() && !c.knocked_out && !c.flying && !c.buried)
        .filter_map(|(i, c)| {
            let (reach, bonus) = if c.is_alive() {
                (weapon_reach, 10.0)
            } else {
                (player::REACH, 0.0)
            };
            score(c.pos, c.species.radius(), reach).map(|s| (s - bonus, Target::Creature(i)))
        });
    let foes = foes
        .iter()
        .filter_map(|&(id, pos)| score(pos, NPC_HIT_RADIUS, weapon_reach).map(|s| (s - 10.0, Target::Npc(id))));
    nodes
        .chain(creatures)
        .chain(foes)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, t)| t)
}

/// Posição e raio do alvo, para o anel de destaque.
pub fn target_ring(world: &World, creatures: &[Creature], foes: &[(u32, Vec2)], target: Target) -> (Vec2, f32) {
    match target {
        Target::Structure(i) => {
            let r = world.structures[i].rect();
            (vec2(r.center().x, r.bottom() - 4.0), r.w.max(r.h) / 2.0 + 4.0)
        }
        Target::Npc(id) => {
            let pos = foes.iter().find(|f| f.0 == id).map_or(Vec2::ZERO, |f| f.1);
            (pos, NPC_HIT_RADIUS + 6.0)
        }
        Target::Node(i) => {
            let n = &world.nodes[i];
            (n.pos, n.kind.collision_radius().max(8.0) + 6.0)
        }
        Target::Creature(i) => {
            let c = &creatures[i];
            (c.pos, c.species.radius() + 6.0)
        }
    }
}

/// `npcs`: (`id`, posição) de cada NPC.
pub fn nearest_interaction(
    world: &World,
    creatures: &[Creature],
    rafts: &[Raft],
    npcs: &[(u32, Vec2)],
    p: Vec2,
) -> Option<Interaction> {
    let bag = world
        .nearest_bag(p, PICKUP_REACH)
        .map(|i| (world.bags[i].pos.distance(p), Interaction::Bag(i)));
    let structure = world
        .structures
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind.interactive())
        .map(|(i, s)| (building::distance_to_rect(p, s.rect()), Interaction::Structure(i)))
        .filter(|(d, _)| *d <= building::INTERACT_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let creature = creatures
        .iter()
        .filter(|c| c.is_alive() && !c.mounted && (c.is_tamed() || (c.knocked_out && c.inventory.is_some())))
        .map(|c| (c.pos.distance(p) - c.species.radius(), Interaction::Creature(c.id)))
        .filter(|(d, _)| *d <= building::INTERACT_REACH + 6.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let raft = rafts
        .iter()
        .filter(|r| r.rider.is_none())
        .map(|r| (r.pos.distance(p) - raft::RADIUS, Interaction::Raft(r.id)))
        .filter(|(d, _)| *d <= raft::STEP_REACH - raft::RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let npc = npcs
        .iter()
        .map(|&(id, pos)| (pos.distance(p), Interaction::Npc(id)))
        .filter(|(d, _)| *d <= building::INTERACT_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    [bag, structure, creature, raft, npc]
        .into_iter()
        .flatten()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, target)| target)
}

/// Inventário do contêiner aberto (baú, fogueira, forja ou dino).
fn container_inventory<'a>(
    world: &'a mut World,
    creatures: &'a mut [Creature],
    npcs: &'a mut [Npc],
    container: Option<ContainerRef>,
) -> Option<&'a mut Inventory> {
    match container? {
        ContainerRef::Structure(i) => world.structures.get_mut(i)?.storage.as_mut(),
        ContainerRef::Creature(id) => creatures.iter_mut().find(|c| c.id == id)?.inventory.as_mut(),
        // Só membros da tribo (o painel só abre para o dono): moradores não recebem itens.
        ContainerRef::Npc(id) => Some(&mut npcs.iter_mut().find(|n| n.id == id && n.owner.is_some())?.inventory),
    }
}

// ---------- Jogador ----------

pub struct PlayerState {
    pub id: PlayerId,
    pub name: String,
    pub player: Player,
    pub vitals: Vitals,
    pub progression: Progression,
    pub inventory: Inventory,
    pub hotbar_selected: usize,
    pub queue: CraftQueue,
    /// Pilha presa no cursor (arrastando no inventário).
    pub held: Option<Stack>,
    pub container: Option<ContainerRef>,
    /// `id` da criatura montada.
    pub mount: Option<u32>,
    /// `id` da jangada em que está remando.
    pub raft: Option<u32>,
    pub dead: bool,
    pub input: Input,
}

impl PlayerState {
    fn new(id: PlayerId, name: String, spawn: Vec2) -> Self {
        Self {
            id,
            name,
            player: Player::new(spawn),
            vitals: Vitals::full(),
            progression: Progression::default(),
            inventory: Inventory::new(INVENTORY_SIZE),
            hotbar_selected: 0,
            queue: CraftQueue::default(),
            held: None,
            container: None,
            mount: None,
            raft: None,
            dead: false,
            input: Input::default(),
        }
    }

    fn from_save(id: PlayerId, name: String, save: PlayerSave) -> Self {
        let mut p = Self::new(id, name, Vec2::from(save.pos));
        p.player.facing = if save.facing_left { Facing::Left } else { Facing::Right };
        p.vitals = save.vitals;
        p.dead = save.vitals.is_dead();
        // Saves anteriores à v0.6 não têm progressão: liberam as receitas que já existiam.
        p.progression = save.progression.unwrap_or_else(Progression::legacy);
        p.vitals.set_max(p.progression.maxima());
        for (slot, saved) in p.inventory.slots.iter_mut().zip(save.inventory) {
            *slot = saved;
        }
        p.hotbar_selected = save.hotbar_selected.min(HOTBAR_SIZE - 1);
        p.queue = save.craft_queue;
        p
    }

    fn to_save(&self) -> PlayerSave {
        // Uma pilha presa no cursor também conta como inventário.
        let mut inventory = self.inventory.clone();
        if let Some(held) = self.held {
            inventory.insert_stack(held);
        }
        PlayerSave {
            name: self.name.clone(),
            pos: self.player.pos.into(),
            facing_left: self.player.facing == Facing::Left,
            vitals: self.vitals,
            inventory: inventory.slots,
            hotbar_selected: self.hotbar_selected,
            craft_queue: self.queue.clone(),
            progression: Some(self.progression.clone()),
        }
    }

    pub fn equipped(&self) -> Option<ItemId> {
        self.inventory.slots[self.hotbar_selected].map(|s| s.item)
    }

    fn equipped_tool(&self) -> Option<ToolKind> {
        self.equipped().and_then(ItemId::tool)
    }

    fn view(&self) -> PlayerView {
        PlayerView {
            id: self.id,
            name: self.name.clone(),
            pos: self.player.pos.into(),
            facing_left: self.player.facing == Facing::Left,
            moving: self.player.moving,
            anim_time: self.player.anim_time,
            swing_timer: self.player.swing_timer,
            held: self.equipped(),
            dead: self.dead,
            mount: self.mount,
        }
    }

    fn private(&self) -> PrivateState {
        PrivateState {
            vitals: self.vitals,
            inventory: self.inventory.clone(),
            hotbar_selected: self.hotbar_selected,
            progression: self.progression.clone(),
            queue: self.queue.clone(),
            held: self.held,
            container: self.container,
            dead: self.dead,
            mount: self.mount,
            raft: self.raft,
        }
    }
}

// ---------- Simulação ----------

pub struct Sim {
    pub world: World,
    pub creatures: Vec<Creature>,
    pub projectiles: Vec<Projectile>,
    pub rafts: Vec<Raft>,
    /// Moradores dos acampamentos e membros das tribos.
    pub npcs: Vec<Npc>,
    pub players: Vec<PlayerState>,
    pub time: f32,
    /// Jogadores que saíram (voltam com o mesmo estado ao entrar com o mesmo nome).
    offline: Vec<PlayerSave>,
    next_creature_id: u32,
    next_raft_id: u32,
    next_npc_id: u32,
    /// Tempo até os acampamentos repovoarem.
    camp_timer: f32,
    /// Tempo até a próxima tentativa de ataque de saqueadores.
    raid_timer: f32,
    next_player_id: PlayerId,
    rng: Rng,
    spawn_timer: f32,
    /// Eventos a enviar: `None` = para todos.
    outbox: Vec<(Option<PlayerId>, NetEvent)>,
    /// Mensagens de chat (de, texto).
    chat: Vec<(String, String)>,
}

impl Sim {
    pub fn new(seed: u64) -> Self {
        let world = World::generate(seed, MAP_SIZE, MAP_SIZE);
        let mut rng = Rng::new(seed ^ 0xA11CE);
        let mut creatures = Vec::new();
        let mut next_creature_id = 1;
        creatures::populate(
            &mut creatures,
            &world,
            &[world.spawn],
            &mut rng,
            4000,
            &mut next_creature_id,
        );
        let mut sim = Self {
            world,
            creatures,
            projectiles: Vec::new(),
            rafts: Vec::new(),
            players: Vec::new(),
            time: 0.0,
            offline: Vec::new(),
            next_creature_id,
            next_raft_id: 1,
            npcs: Vec::new(),
            next_npc_id: 1,
            camp_timer: 0.0,
            raid_timer: raid::FIRST_RAID,
            next_player_id: 1,
            rng,
            spawn_timer: RESPAWN_SECS,
            outbox: Vec::new(),
            chat: Vec::new(),
        };
        sim.populate_camps();
        sim.camp_timer = 300.0;
        sim
    }

    /// NPCs com quem dá para falar (F): moradores e membros.
    fn npc_spots(&self) -> Vec<(u32, Vec2)> {
        self.npcs
            .iter()
            .filter(|n| !n.hostile)
            .map(|n| (n.id, n.pos()))
            .collect()
    }

    /// Saqueadores vivos (alvos de golpes e pedras).
    fn raider_spots(&self) -> Vec<(u32, Vec2)> {
        self.npcs
            .iter()
            .filter(|n| n.hostile && n.is_alive())
            .map(|n| (n.id, n.pos()))
            .collect()
    }

    pub fn seed(&self) -> u64 {
        self.world.seed
    }

    pub fn from_save(data: SaveData) -> Self {
        let mut sim = Self::new(data.seed);
        sim.time = data.time;
        for (i, hp, timer) in data.nodes {
            if let Some(node) = sim.world.nodes.get_mut(i) {
                node.hp = hp.min(node.kind.max_hp());
                node.respawn_timer = timer;
            }
        }
        sim.world.bags = data
            .bags
            .into_iter()
            .map(|b| GroundBag {
                pos: Vec2::from(b.pos),
                stacks: b.stacks,
                kind: b.kind,
                ttl: b.ttl,
            })
            .collect();
        sim.world.structures = data.structures;
        for s in &mut sim.world.structures {
            s.normalize();
        }
        // Saves anteriores à v0.4 não têm fauna: mantém a população recém-gerada.
        if let Some(saved) = data.creatures {
            sim.creatures = saved.into_iter().map(Creature::from).collect();
            // Saves da v0.4 não têm ids: numera de novo.
            sim.next_creature_id = sim.creatures.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            for c in sim.creatures.iter_mut().filter(|c| c.id == 0) {
                c.id = sim.next_creature_id;
                sim.next_creature_id += 1;
            }
            // O save não guarda quem estava no ar: voadores salvos sobre água ou árvores pousam perto.
            for c in sim.creatures.iter_mut().filter(|c| c.species.flies()) {
                if !c.species.can_stand(&sim.world, c.pos)
                    && let Some(spot) = c.landing_spot(&sim.world)
                {
                    c.pos = spot;
                }
            }
        }
        sim.rafts = data.rafts.iter().map(Raft::from).collect();
        for r in &mut sim.rafts {
            r.rider = None;
        }
        sim.next_raft_id = sim.rafts.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        // Saves anteriores à v0.9 não têm NPCs: os acampamentos ganham moradores no primeiro tick.
        if let Some(npcs) = data.npcs {
            sim.npcs = npcs.into_iter().map(Npc::from).collect();
            sim.camp_timer = 60.0;
        }
        sim.next_npc_id = sim.npcs.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        sim.offline = data.players;
        sim
    }

    pub fn to_save(&self) -> SaveData {
        let mut data = SaveData::new(self.world.seed, self.time);
        data.nodes = self
            .world
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.hp != n.kind.max_hp())
            .map(|(i, n)| (i, n.hp, n.respawn_timer))
            .collect();
        data.bags = self.bag_saves();
        data.structures = self.world.structures.clone();
        data.creatures = Some(self.creatures.iter().map(CreatureSave::from).collect());
        data.players = self.players.iter().map(PlayerState::to_save).collect();
        data.players.extend(self.offline.iter().cloned());
        // Saqueadores não vão para o save (o ataque acaba ao fechar o jogo).
        data.npcs = Some(self.npcs.iter().filter(|n| !n.hostile).map(NpcSave::from).collect());
        data.rafts = self
            .rafts
            .iter()
            .map(|r| RaftSave {
                rider: None,
                ..RaftSave::from(r)
            })
            .collect();
        data
    }

    fn bag_saves(&self) -> Vec<BagSave> {
        self.world
            .bags
            .iter()
            .map(|b| BagSave {
                pos: b.pos.into(),
                stacks: b.stacks.clone(),
                kind: b.kind,
                ttl: b.ttl,
            })
            .collect()
    }

    // ---------- Entrar / sair ----------

    /// Entra no jogo. Quem já jogou neste mundo (mesmo nome) volta como estava.
    /// Um save antigo de um jogador só fica com o primeiro que entrar sem save próprio (o host).
    pub fn join(&mut self, name: &str) -> PlayerId {
        let mut name = name.to_owned();
        let base = name.clone();
        let mut n = 2;
        while self.players.iter().any(|p| p.name == name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        let id = self.next_player_id;
        self.next_player_id += 1;
        let saved = self
            .offline
            .iter()
            .position(|p| p.name == name)
            .or_else(|| self.offline.iter().position(|p| p.name == LEGACY_PLAYER))
            .map(|i| self.offline.remove(i));
        let state = match saved {
            Some(save) => PlayerState::from_save(id, name.clone(), save),
            None => PlayerState::new(id, name.clone(), self.world.spawn),
        };
        self.players.push(state);
        // Salvo voando sobre a água ou dentro de algo: volta em terra firme.
        self.rescue_player(self.players.len() - 1);
        // Domesticados de saves antigos (sem dono) passam a seguir quem entrou.
        for c in self.creatures.iter_mut().filter(|c| c.is_tamed() && c.owner.is_none()) {
            c.owner = Some(name.clone());
        }
        self.broadcast(format!("{name} entrou no jogo"));
        id
    }

    pub fn leave(&mut self, id: PlayerId) {
        let Some(pi) = self.index(id) else { return };
        self.dismount(pi);
        self.abandon_raft(pi);
        self.close_container(pi);
        let p = self.players.remove(pi);
        self.offline.retain(|s| s.name != p.name);
        self.offline.push(p.to_save());
        self.broadcast(format!("{} saiu", p.name));
    }

    /// Tira o jogador de um lugar onde ele não pode ficar (ex.: a montaria voadora sumiu no mar).
    fn rescue_player(&mut self, pi: usize) {
        let pos = self.players[pi].player.pos;
        if !self.world.blocked(pos, player::RADIUS) {
            return;
        }
        let world = &self.world;
        let spot = world.find_near(pos, |p| !world.blocked(p, player::RADIUS));
        self.players[pi].player.pos = spot.unwrap_or(world.spawn);
    }

    pub fn index(&self, id: PlayerId) -> Option<usize> {
        self.players.iter().position(|p| p.id == id)
    }

    pub fn set_input(&mut self, id: PlayerId, input: Input) {
        if let Some(pi) = self.index(id) {
            let mut input = input;
            let dir = Vec2::from(input.dir);
            // Nada de andar mais rápido mandando um vetor grande.
            input.dir = if dir.length() > 1.0 { dir.normalize() } else { dir }.into();
            self.players[pi].input = input;
        }
    }

    pub fn take_events(&mut self) -> Vec<(Option<PlayerId>, NetEvent)> {
        std::mem::take(&mut self.outbox)
    }

    pub fn take_chat(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.chat)
    }

    // ---------- Mensagens ----------

    fn notify(&mut self, pi: usize, msg: impl Into<String>) {
        let id = self.players[pi].id;
        self.outbox.push((Some(id), NetEvent::Notify(msg.into())));
    }

    /// Mensagem para todos os jogadores.
    pub fn broadcast(&mut self, msg: impl Into<String>) {
        self.outbox.push((None, NetEvent::Notify(msg.into())));
    }

    fn popup(&mut self, pos: Vec2, text: String) {
        self.popup_styled(pos, text, PopupStyle::Gain);
    }

    fn popup_styled(&mut self, pos: Vec2, text: String, style: PopupStyle) {
        self.outbox.push((
            None,
            NetEvent::Popup {
                pos: pos.into(),
                text,
                style,
            },
        ));
    }

    /// Soma XP e anuncia subidas de nível.
    fn gain_xp(&mut self, pi: usize, amount: u32) {
        let gained = self.players[pi].progression.add_xp(amount);
        if gained == 0 {
            return;
        }
        let level = self.players[pi].progression.level;
        self.notify(
            pi,
            format!(
                "Nível {level}! +{gained} ponto(s) de atributo, +{} de engrama (Tab)",
                gained * progression::ENGRAM_POINTS_PER_LEVEL
            ),
        );
        let at = self.players[pi].player.pos - vec2(0.0, 48.0);
        self.popup(at, format!("Nível {level}!"));
    }

    // ---------- Helpers ----------

    fn creature(&self, id: u32) -> Option<&Creature> {
        self.creatures.iter().find(|c| c.id == id)
    }

    fn creature_mut(&mut self, id: u32) -> Option<&mut Creature> {
        self.creatures.iter_mut().find(|c| c.id == id)
    }

    /// Adiciona uma criatura com um `id` novo.
    fn add_creature(&mut self, mut c: Creature) -> u32 {
        c.id = self.next_creature_id;
        self.next_creature_id += 1;
        let id = c.id;
        self.creatures.push(c);
        id
    }

    fn station_ok(&self, pi: usize, recipe: &Recipe) -> bool {
        let pos = self.players[pi].player.pos;
        recipe.station.is_none_or(|kind| self.world.station_near(kind, pos))
    }

    fn drop_stack(&mut self, pi: usize, stack: Stack) {
        let p = &self.players[pi].player;
        let ahead = p.pos + vec2(16.0 * p.facing.sign(), 4.0);
        let pos = if self.world.tile_at(ahead).walkable() {
            ahead
        } else {
            p.pos
        };
        self.world.drop_bag(pos, vec![stack], BagKind::Dropped);
    }

    /// Fecha o painel: a pilha do cursor volta ao inventário (ou ao contêiner, ou ao chão).
    fn close_container(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        if let Some(held) = p.held.take() {
            let left = p.inventory.insert_stack(held);
            let left = match (
                left,
                container_inventory(&mut self.world, &mut self.creatures, &mut self.npcs, p.container),
            ) {
                (Some(stack), Some(chest)) => chest.insert_stack(stack),
                (left, _) => left,
            };
            if let Some(stack) = left {
                self.drop_stack(pi, stack);
            }
        }
        self.players[pi].container = None;
    }

    // ---------- Tick ----------

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        for pi in 0..self.players.len() {
            self.update_player(pi, dt);
        }
        for pi in 0..self.players.len() {
            self.deliver_crafts(pi, dt);
        }
        let positions: Vec<Vec2> = self.players.iter().map(|p| p.player.pos).collect();
        self.world.update(dt, &positions);
        self.update_creatures(dt);
        self.update_npcs(dt);
        self.update_tamed_combat(dt);
        self.update_raids(dt);
        self.reap_npcs();
        self.remove_destroyed_structures();
        for pi in 0..self.players.len() {
            self.validate_container(pi);
        }
    }

    fn update_player(&mut self, pi: usize, dt: f32) {
        if self.players[pi].dead {
            self.players[pi].player.moving = false;
            return;
        }
        let input = self.players[pi].input;
        let dir = Vec2::from(input.dir);
        if self.players[pi].mount.is_some() {
            self.update_riding(pi, dir, input, dt);
        } else if self.players[pi].raft.is_some() {
            self.update_rafting(pi, dir, input, dt);
        } else {
            let p = &mut self.players[pi];
            let sprinting = dir != Vec2::ZERO && input.sprint && p.vitals.can_sprint();
            let mult = if sprinting { player::SPRINT_MULT } else { 1.0 };
            p.player.update(dir, mult, &self.world, dt);
            p.vitals.tick(dt, sprinting && p.player.moving);
            if input.attack && p.player.cooldown <= 0.0 {
                self.swing(pi);
            }
        }
        if self.players[pi].vitals.is_dead() {
            self.die(pi);
        }
    }

    /// Entrega o que a fila de crafting terminou; se não couber, cai no chão.
    fn deliver_crafts(&mut self, pi: usize, dt: f32) {
        for stack in self.players[pi].queue.tick(dt) {
            let secs = crafting::recipe_for(stack.item).map_or(0.0, |r| r.secs);
            self.gain_xp(pi, (secs * progression::XP_CRAFT_PER_SEC).round() as u32);
            let name = stack.item.name();
            match self.players[pi].inventory.insert_stack(stack) {
                None => self.notify(pi, format!("Fabricado: {name}")),
                Some(left) => {
                    self.drop_stack(pi, left);
                    self.notify(pi, format!("Inventário cheio: {name} caiu no chão"));
                }
            }
        }
    }

    /// Fecha o painel se o contêiner sumiu ou se o jogador se afastou.
    fn validate_container(&mut self, pi: usize) {
        let p = &self.players[pi];
        let Some(container) = p.container else { return };
        let reach = building::INTERACT_REACH + 16.0;
        let pos = p.player.pos;
        let still_valid = match container {
            ContainerRef::Structure(i) => self
                .world
                .structures
                .get(i)
                .is_some_and(|s| s.storage.is_some() && building::distance_to_rect(pos, s.rect()) <= reach),
            ContainerRef::Creature(id) => self.creature(id).is_some_and(|c| {
                let usable = c.is_tamed() || (c.knocked_out && c.inventory.is_some());
                usable && c.is_alive() && !c.mounted && c.pos.distance(pos) - c.species.radius() <= reach + 8.0
            }),
            ContainerRef::Npc(id) => self.npcs.iter().any(|n| {
                n.id == id && n.pos().distance(pos) <= reach + 8.0 && n.owner.as_ref().is_none_or(|o| *o == p.name)
            }),
        };
        if !still_valid || p.dead {
            self.close_container(pi);
        }
    }

    // ---------- Fauna ----------

    fn update_creatures(&mut self, dt: f32) {
        let flying_mounts: Vec<u32> = self.creatures.iter().filter(|c| c.flying).map(|c| c.id).collect();
        // Alvos da IA: jogadores e, depois deles, os membros das tribos (moradores ficam em paz).
        let members = self.member_ids();
        let mut ai_players: Vec<AiPlayer> = self
            .players
            .iter()
            .map(|p| AiPlayer {
                pos: p.player.pos,
                alive: !p.dead,
                name: &p.name,
                airborne: p.mount.is_some_and(|id| flying_mounts.contains(&id)),
                sheltered: self.world.sheltered(p.player.pos),
            })
            .collect();
        ai_players.extend(self.npcs.iter().filter(|n| n.is_member()).map(|n| AiPlayer {
            pos: n.pos(),
            alive: n.is_alive(),
            name: tribe::NPC_AI_NAME,
            airborne: false,
            sheltered: self.world.sheltered(n.pos()),
        }));
        let input = AiInput { players: &ai_players };
        let mut bites = Vec::new();
        let mut events = Vec::new();
        for c in &mut self.creatures {
            match c.update(&self.world, &input, &mut self.rng, dt) {
                Some(Event::Bite { target, damage }) => bites.push((target, damage)),
                Some(event) => events.push((c.id, event)),
                None => {}
            }
        }
        drop(ai_players);
        for (id, event) in events {
            self.on_creature_event(id, event);
        }
        self.update_projectiles(dt);
        self.creatures.retain(|c| !c.is_gone());
        for pi in 0..self.players.len() {
            let gone = self.players[pi]
                .mount
                .is_some_and(|id| !self.creatures.iter().any(|c| c.id == id && c.is_alive()));
            if gone {
                self.players[pi].mount = None;
                self.rescue_player(pi);
            }
        }
        let players = self.players.len();
        for (target, damage) in bites {
            if target < players {
                if !self.players[target].dead {
                    self.take_bite(target, damage);
                }
            } else if let Some(&id) = members.get(target - players) {
                self.hurt_npc(id, damage, None);
            }
        }

        self.spawn_timer -= dt;
        if self.spawn_timer <= 0.0 {
            self.spawn_timer = RESPAWN_SECS;
            let positions: Vec<Vec2> = self.players.iter().map(|p| p.player.pos).collect();
            creatures::populate(
                &mut self.creatures,
                &self.world,
                &positions,
                &mut self.rng,
                40,
                &mut self.next_creature_id,
            );
        }
    }

    /// Mordidas de selvagens: montado, quem apanha é a montaria.
    fn take_bite(&mut self, pi: usize, damage: f32) {
        let player_pos = self.players[pi].player.pos;
        if let Some(id) = self.players[pi].mount
            && let Some(mount) = self.creature_mut(id)
        {
            let died = mount.take_hit(damage, 0.0, player_pos) == Hit::Died;
            let (pos, name) = (mount.pos, mount.species.name());
            self.popup_styled(pos - vec2(0.0, 30.0), format!("-{damage:.0}"), PopupStyle::Hurt);
            if died {
                self.dismount(pi);
                self.on_tamed_death(id);
                self.notify(pi, format!("Seu {name} morreu!"));
            }
            return;
        }
        self.players[pi].vitals.damage(damage);
        let id = self.players[pi].id;
        self.outbox.push((Some(id), NetEvent::Hurt));
        self.popup_styled(player_pos - vec2(0.0, 36.0), format!("-{damage:.0}"), PopupStyle::Hurt);
        if self.players[pi].vitals.is_dead() {
            self.die(pi);
        }
    }

    /// Inventário de um domesticado morto cai no chão.
    fn on_tamed_death(&mut self, id: u32) {
        if let Some(c) = self.creature_mut(id) {
            let (pos, stacks) = (c.pos, c.take_inventory());
            self.world.drop_bag(pos, stacks, BagKind::Dropped);
        }
    }

    /// Jogador vivo mais perto de um ponto.
    fn nearest_player(&self, pos: Vec2) -> Option<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.dead)
            .min_by(|a, b| a.1.player.pos.distance(pos).total_cmp(&b.1.player.pos.distance(pos)))
            .map(|(i, _)| i)
    }

    fn on_creature_event(&mut self, id: u32, event: Event) {
        let Some(c) = self.creature(id) else { return };
        let (name, pos, level) = (c.species.name(), c.pos, c.level);
        let tamer = self.nearest_player(pos);
        match event {
            Event::Woke => {
                // A comida que sobrou cai no chão.
                let Some(c) = self.creature_mut(id) else { return };
                let stacks = c.take_inventory();
                c.inventory = None;
                self.world.drop_bag(pos, stacks, BagKind::Dropped);
                if let Some(pi) = tamer {
                    self.notify(pi, format!("{name} acordou!"));
                }
            }
            Event::Tamed { bonus_levels } => {
                // O dono é quem estiver mais perto (normalmente quem alimentou).
                if let Some(pi) = tamer {
                    let owner = self.players[pi].name.clone();
                    if let Some(c) = self.creature_mut(id) {
                        c.owner = Some(owner);
                    }
                    self.gain_xp(pi, progression::xp_tame(level));
                    self.notify(pi, format!("{name} domesticado! Nv {level} (+{bonus_levels} de bônus)"));
                }
                self.popup(pos - vec2(0.0, 30.0), "Domesticado!".to_owned());
            }
            Event::Smash { structure, damage } => self.damage_structure(structure, damage),
            Event::Bite { .. } => {}
        }
    }

    // ---------- Estruturas atacadas ----------

    /// Dano de criatura ou saqueador numa estrutura (o material reduz). Ela sai do mundo no fim do tick.
    pub(super) fn damage_structure(&mut self, i: usize, damage: f32) {
        let Some(s) = self.world.structures.get_mut(i) else {
            return;
        };
        if s.health <= 0.0 {
            return;
        }
        let before = s.health;
        s.take_damage(damage);
        let (lost, r) = (before - s.health.max(0.0), s.rect());
        if lost >= 1.0 {
            let at = vec2(r.center().x, r.y - 30.0);
            self.popup_styled(at, format!("-{lost:.0}"), PopupStyle::Hurt);
        }
    }

    /// Tira do mundo as estruturas que chegaram a 0 de vida (o conteúdo cai numa bolsa).
    fn remove_destroyed_structures(&mut self) {
        if self.world.structures.iter().all(|s| s.health > 0.0) {
            return;
        }
        // Painéis de estruturas abertos fecham (os índices mudam).
        for pj in 0..self.players.len() {
            if matches!(self.players[pj].container, Some(ContainerRef::Structure(_))) {
                self.close_container(pj);
            }
        }
        let mut destroyed = Vec::new();
        self.world.structures.retain_mut(|s| {
            if s.health > 0.0 {
                return true;
            }
            let stacks: Vec<Stack> = s
                .storage
                .take()
                .map(|inv| inv.slots.into_iter().flatten().collect())
                .unwrap_or_default();
            destroyed.push((s.rect().center(), s.kind.item().name(), stacks));
            false
        });
        for (center, name, stacks) in destroyed {
            self.world.drop_bag(center, stacks, BagKind::Dropped);
            for pi in 0..self.players.len() {
                if self.players[pi].player.pos.distance(center) < 900.0 {
                    self.notify(pi, format!("Estrutura destruída: {name}"));
                }
            }
        }
    }

    fn update_projectiles(&mut self, dt: f32) {
        let mut hits = Vec::new();
        let mut raider_hits = Vec::new();
        // Subpassos curtos para a pedra não atravessar alvos quando o frame demora.
        let substeps = (dt * combat::SLINGSHOT_SPEED / 4.0).ceil().max(1.0) as usize;
        let sub_dt = dt / substeps as f32;
        self.projectiles.retain_mut(|stone| {
            for _ in 0..substeps {
                if !stone.update(&self.world, sub_dt) {
                    return false;
                }
                let target = self.creatures.iter().position(|c| {
                    c.is_wild() && c.is_conscious() && !c.buried && stone.hits(c.pos, c.species.radius())
                });
                if let Some(i) = target {
                    hits.push((i, stone.damage, stone.torpor, stone.owner, stone.pos - stone.vel));
                    return false;
                }
                let raider = self
                    .npcs
                    .iter()
                    .find(|n| n.hostile && n.is_alive() && stone.hits(n.pos(), NPC_HIT_RADIUS));
                if let Some(n) = raider {
                    raider_hits.push((n.id, stone.damage * 3.0, stone.owner));
                    return false;
                }
            }
            true
        });
        for (id, damage, owner) in raider_hits {
            let shooter = self.index(owner);
            self.hurt_npc(id, damage, shooter);
        }
        for (i, damage, torpor, owner, from) in hits {
            let hit = self.creatures[i].take_hit(damage, torpor, from);
            let shooter = self.index(owner);
            self.report_hit(shooter, i, damage, hit);
        }
    }

    /// Mensagens, números flutuantes e XP de um golpe numa criatura.
    fn report_hit(&mut self, pi: Option<usize>, i: usize, damage: f32, hit: Hit) {
        let c = &self.creatures[i];
        let (pos, species, level) = (c.pos, c.species, c.level);
        if damage > 0.0 {
            self.popup_styled(pos - vec2(0.0, 30.0), format!("-{damage:.0}"), PopupStyle::Damage);
        }
        let Some(pi) = pi else { return };
        if hit == Hit::Died {
            self.gain_xp(pi, progression::xp_kill(level));
        }
        match hit {
            Hit::Died => self.notify(pi, format!("{} abatido", species.name())),
            Hit::KnockedOut if species.taming_food().is_some() => {
                self.notify(
                    pi,
                    format!("{} desmaiou! Aperte F nele e coloque comida.", species.name()),
                );
            }
            Hit::KnockedOut => self.notify(pi, format!("{} desmaiou", species.name())),
            Hit::Hurt => {}
        }
    }

    // ---------- Montaria ----------

    fn mount_creature(&mut self, pi: usize, id: u32) {
        let Some(c) = self.creatures.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if !c.can_ride() || c.mounted {
            return;
        }
        c.mounted = true;
        let pos = c.pos;
        self.release_raft(pi);
        let p = &mut self.players[pi];
        p.mount = Some(id);
        p.player.pos = pos;
    }

    /// Onde o jogador pisa ao descer de uma montaria em `pos`: ao lado dela ou na margem perto.
    fn step_off_spot(&self, pos: Vec2, radius: f32, facing_left: bool) -> Option<Vec2> {
        let side = if facing_left { 1.0 } else { -1.0 };
        let spots = [vec2(side, 0.0), vec2(-side, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)];
        spots
            .iter()
            .map(|d| pos + *d * (radius + player::RADIUS + 4.0))
            .find(|&p| !self.world.blocked(p, player::RADIUS))
            .or_else(|| raft::shore_spot(&self.world, pos))
    }

    /// Jangada livre perto de `pos` (para descer de uma montaria no meio do mar).
    fn free_raft_near(&self, pos: Vec2) -> Option<usize> {
        self.rafts
            .iter()
            .enumerate()
            .filter(|(_, r)| r.rider.is_none() && r.pos.distance(pos) <= raft::STEP_REACH * 1.5)
            .min_by(|a, b| a.1.pos.distance(pos).total_cmp(&b.1.pos.distance(pos)))
            .map(|(i, _)| i)
    }

    /// Desce da montaria para um lugar livre ao lado dela. No ar, a montaria pousa no chão livre
    /// mais próximo antes (morte, saída do jogo); para descer por vontade própria, ver `Command::Interact`.
    fn dismount(&mut self, pi: usize) {
        let Some(id) = self.players[pi].mount.take() else {
            return;
        };
        let world = &self.world;
        let Some(c) = self.creatures.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if c.flying && !c.try_land(world) {
            if let Some(spot) = c.landing_spot(world) {
                c.pos = spot;
            }
            c.flying = false;
        }
        c.mounted = false;
        let (pos, r, left) = (c.pos, c.species.radius(), c.facing_left);
        // No mar: ao lado, na margem perto, numa jangada perto ou, em último caso, na terra mais próxima.
        if let Some(spot) = self.step_off_spot(pos, r, left) {
            self.players[pi].player.pos = spot;
        } else if let Some(i) = self.free_raft_near(pos) {
            self.board_raft(pi, i);
        } else {
            self.players[pi].player.pos = pos;
            self.rescue_player(pi);
        }
    }

    // ---------- Jangada ----------

    /// Põe a jangada da mão na água perto do jogador e sobe nela.
    fn launch_raft(&mut self, pi: usize) {
        let p = &self.players[pi];
        if p.mount.is_some() || p.raft.is_some() {
            return;
        }
        let Some(spot) = raft::launch_spot(&self.world, p.player.pos) else {
            self.notify(pi, "Chegue na beira da água para pôr a jangada");
            return;
        };
        let p = &mut self.players[pi];
        p.inventory.take_from_slot(p.hotbar_selected, 1);
        let id = self.next_raft_id;
        self.next_raft_id += 1;
        self.rafts.push(Raft::new(id, spot));
        self.board_raft(pi, self.rafts.len() - 1);
    }

    fn board_raft(&mut self, pi: usize, i: usize) {
        if self.rafts[i].rider.is_some() || self.players[pi].mount.is_some() {
            return;
        }
        self.release_raft(pi);
        self.close_container(pi);
        let raft = &mut self.rafts[i];
        raft.rider = Some(self.players[pi].id);
        let (id, pos) = (raft.id, raft.pos);
        let p = &mut self.players[pi];
        p.raft = Some(id);
        p.player.pos = pos;
    }

    /// F na jangada sem nada por perto: desce para a margem (só perto dela).
    fn leave_raft(&mut self, pi: usize) {
        let pos = self.players[pi].player.pos;
        let Some(spot) = raft::shore_spot(&self.world, pos) else {
            self.notify(pi, "Chegue perto da margem para descer da jangada");
            return;
        };
        self.release_raft(pi);
        self.players[pi].player.pos = spot;
    }

    /// Larga a jangada sem mover o jogador.
    fn release_raft(&mut self, pi: usize) {
        if let Some(id) = self.players[pi].raft.take()
            && let Some(r) = self.rafts.iter_mut().find(|r| r.id == id)
        {
            r.rider = None;
        }
    }

    /// Morte ou saída do jogo na jangada: ela fica na água e o jogador vai para a terra mais próxima.
    fn abandon_raft(&mut self, pi: usize) {
        if self.players[pi].raft.is_some() {
            self.release_raft(pi);
            self.rescue_player(pi);
        }
    }

    fn update_rafting(&mut self, pi: usize, dir: Vec2, input: Input, dt: f32) {
        let Some(id) = self.players[pi].raft else { return };
        let world = &self.world;
        let Some(raft) = self.rafts.iter_mut().find(|r| r.id == id) else {
            self.players[pi].raft = None;
            return;
        };
        let p = &mut self.players[pi];
        // Shift rema mais rápido gastando stamina.
        let rowing = dir != Vec2::ZERO && input.sprint && p.vitals.can_sprint();
        let speed = raft::SPEED * if rowing { raft::SPRINT_MULT } else { 1.0 };
        raft.steer(world, dir, speed, dt);
        p.player.pos = raft.pos;
        p.player.update(Vec2::ZERO, 1.0, world, dt);
        if dir.x.abs() > 0.05 {
            p.player.facing = if dir.x < 0.0 { Facing::Left } else { Facing::Right };
        }
        p.vitals.tick(dt, rowing && raft.moving);
        // Na jangada dá para bater, atirar e beber.
        if input.attack && p.player.cooldown <= 0.0 {
            self.swing(pi);
        }
    }

    fn update_riding(&mut self, pi: usize, dir: Vec2, input: Input, dt: f32) {
        let Some(id) = self.players[pi].mount else { return };
        let world = &self.world;
        let Some(mount) = self.creatures.iter_mut().find(|c| c.id == id) else {
            self.players[pi].mount = None;
            return;
        };
        let sprint = if input.sprint { RIDE_SPRINT } else { 1.0 };
        let speed = mount.ride_speed(RIDE_MULT, sprint);
        mount.ride(world, dir, speed, dt);
        let p = &mut self.players[pi];
        p.player.pos = mount.pos;
        p.player.facing = if mount.facing_left { Facing::Left } else { Facing::Right };
        p.player.update(Vec2::ZERO, 1.0, world, dt);
        p.player.moving = mount.moving;
        p.vitals.tick(dt, false);
        if input.attack {
            self.mount_bite(pi);
        }
    }

    /// Espaço montado: voador decola ou pousa; beelzebufo pula.
    fn mount_action(&mut self, pi: usize) {
        let Some(id) = self.players[pi].mount else { return };
        let world = &self.world;
        let Some(mount) = self.creatures.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if mount.species.leaps() {
            if !mount.start_leap() && mount.overweight() {
                self.notify(pi, "Carga pesada demais para pular".to_owned());
            }
            return;
        }
        if !mount.species.flies() {
            return;
        }
        let message = if mount.flying {
            (!mount.try_land(world)).then_some("Sem lugar para pousar aqui: procure chão livre")
        } else if mount.overweight() {
            Some("Carga pesada demais para voar")
        } else {
            mount.flying = true;
            None
        };
        if let Some(text) = message {
            self.notify(pi, text.to_owned());
        }
    }

    /// A montaria morde o selvagem mais próximo à frente dela (no chão, não alcança quem voa).
    fn mount_bite(&mut self, pi: usize) {
        let Some(id) = self.players[pi].mount else { return };
        let Some(mount) = self.creature(id) else { return };
        if !mount.ready_to_bite() {
            return;
        }
        let (pos, reach, s, airborne) = (
            mount.pos,
            mount.species.radius() + 18.0,
            if mount.facing_left { -1.0 } else { 1.0 },
            mount.flying,
        );
        let target = self
            .creatures
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_wild() && c.is_conscious() && !c.buried && (airborne || !c.flying))
            .map(|(i, c)| {
                (
                    i,
                    c.pos.distance(pos) - c.species.radius(),
                    (c.pos.x - pos.x) * s >= -4.0,
                )
            })
            .filter(|&(_, gap, ahead)| gap <= reach && ahead)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _, _)| i);
        let damage = match self.creature_mut(id) {
            Some(m) => m.bite(),
            None => return,
        };
        if let Some(i) = target {
            let hit = self.creatures[i].take_hit(damage, 0.0, pos);
            self.report_hit(Some(pi), i, damage, hit);
        } else if let Some(harvest) = self.creature(id).and_then(|m| m.species.mount_harvest()) {
            self.mount_gather(pi, id, harvest, pos, reach, s);
        }
    }

    /// Montaria coletora sem inimigo à frente: mamute derruba árvores, smilodon tira couro de carcaças.
    /// O que sai vai para a carga da montaria (o que não couber, para o jogador; depois, o chão).
    fn mount_gather(&mut self, pi: usize, id: u32, harvest: MountHarvest, pos: Vec2, reach: f32, s: f32) {
        let ahead = |p: Vec2| (p.x - pos.x) * s >= -4.0;
        let nearest = |items: &mut dyn Iterator<Item = (usize, f32)>| {
            items
                .filter(|&(_, gap)| gap <= reach)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let (at, drops) = match harvest {
            MountHarvest::Wood => {
                let mut trees = self
                    .world
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.kind == NodeKind::Tree && n.is_alive() && ahead(n.pos))
                    .map(|(i, n)| (i, n.pos.distance(pos) - n.kind.collision_radius()));
                let Some(i) = nearest(&mut trees) else { return };
                let node = &mut self.world.nodes[i];
                node.hp -= 1;
                node.shake = 0.2;
                if node.hp == 0 {
                    node.respawn_timer = node.kind.respawn_secs();
                }
                (node.pos, gathering::roll_mammoth_tree(&mut self.rng))
            }
            MountHarvest::Hide => {
                let mut corpses = self
                    .creatures
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| !c.is_alive() && ahead(c.pos))
                    .map(|(i, c)| (i, c.pos.distance(pos) - c.species.radius()));
                let Some(i) = nearest(&mut corpses) else { return };
                let corpse = &mut self.creatures[i];
                if !corpse.harvest() {
                    return;
                }
                let (at, species) = (corpse.pos, corpse.species);
                (at, gathering::roll_smilodon_corpse(species, &mut self.rng))
            }
        };
        self.gain_xp(pi, progression::XP_GATHER);
        let mut spilled = Vec::new();
        for (row, (item, n)) in drops.into_iter().enumerate() {
            let in_mount = self
                .creature_mut(id)
                .and_then(|m| m.inventory.as_mut())
                .map_or(n, |inv| inv.add(item, n));
            let left = self.players[pi].inventory.add(item, in_mount);
            if left > 0 {
                spilled.push(Stack::new(item, left));
            }
            let popup = at - vec2(0.0, 30.0 + row as f32 * 14.0);
            self.popup(popup, format!("+{n} {}", item.name()));
        }
        if !spilled.is_empty() {
            self.world.drop_bag(pos, spilled, BagKind::Dropped);
            self.notify(pi, "Montaria e inventário cheios: itens caíram no chão");
        }
    }

    // ---------- Coleta, combate, comida, água ----------

    /// Água ao alcance: no tile do jogador, à frente dele ou logo acima/abaixo.
    fn near_water(&self, pi: usize) -> bool {
        let p = &self.players[pi].player;
        let s = p.facing.sign();
        [
            p.pos,
            p.pos + vec2(16.0 * s, 0.0),
            p.pos + vec2(0.0, 16.0),
            p.pos + vec2(0.0, -16.0),
        ]
        .iter()
        .any(|&q| self.world.tile_at(q).is_water())
    }

    fn swing(&mut self, pi: usize) {
        self.players[pi].player.cooldown = player::SWING_COOLDOWN;
        if self.players[pi].equipped() == Some(ItemId::Slingshot) {
            self.fire_slingshot(pi);
            return;
        }
        let foes = self.raider_spots();
        let p = &self.players[pi];
        let target = find_target(&self.world, &self.creatures, &foes, &p.player, p.equipped());
        if target.is_none() && self.near_water(pi) {
            self.drink(pi);
            return;
        }
        if !self.players[pi].vitals.spend_stamina(vitals::SWING_STAMINA) {
            self.notify(pi, "Sem stamina!");
            return;
        }
        self.players[pi].player.swing_timer = player::SWING_TIME;
        match target {
            Some(Target::Node(i)) => self.hit_node(pi, i),
            Some(Target::Creature(i)) => self.hit_creature(pi, i),
            Some(Target::Structure(i)) => self.repair_structure(pi, i),
            Some(Target::Npc(id)) => self.hit_npc(pi, id),
            None => {}
        }
    }

    /// Golpe num saqueador.
    fn hit_npc(&mut self, pi: usize, id: u32) {
        let Some(ni) = self.npc_index(id) else { return };
        let p = &self.players[pi];
        let held = p.equipped();
        let damage = ItemId::melee_damage(held) * p.progression.melee_mult();
        let pos = self.npcs[ni].pos();
        self.players[pi].player.face_towards(pos.x);
        if held.is_some_and(|item| item.max_durability().is_some()) {
            self.wear_equipped(pi);
        }
        self.popup_styled(pos - vec2(0.0, 36.0), format!("-{damage:.0}"), PopupStyle::Damage);
        self.hurt_npc(id, damage, Some(pi));
    }

    /// Martelada: repara 1/4 da vida gastando 1/4 dos materiais da receita.
    fn repair_structure(&mut self, pi: usize, i: usize) {
        let s = &self.world.structures[i];
        let (kind, r) = (s.kind, s.rect());
        self.players[pi].player.face_towards(r.center().x);
        if s.health >= kind.max_health() {
            self.notify(pi, format!("{} já está inteira", kind.item().name()));
            return;
        }
        let cost = kind.repair_cost();
        let inv = &self.players[pi].inventory;
        if let Some(&(item, n)) = cost.iter().find(|&&(item, n)| inv.count(item) < n) {
            self.notify(pi, format!("Para reparar: {n} {}", item.name()));
            return;
        }
        for &(item, n) in &cost {
            self.players[pi].inventory.remove(item, n);
        }
        let healed = self.world.structures[i].repair();
        self.wear_equipped(pi);
        self.gain_xp(pi, 1);
        let at = vec2(r.center().x, r.y - 30.0);
        self.popup(at, format!("+{healed:.0} reparo"));
    }

    /// Dispara uma pedra na direção do cursor (clique) ou para a frente (E).
    fn fire_slingshot(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        p.player.cooldown = combat::SLINGSHOT_COOLDOWN;
        if p.inventory.count(ItemId::Stone) == 0 {
            self.notify(pi, "Sem pedras para o estilingue");
            return;
        }
        if !p.vitals.spend_stamina(vitals::SWING_STAMINA) {
            self.notify(pi, "Sem stamina!");
            return;
        }
        let from = p.player.pos;
        let dir = match p.input.aim {
            Some(aim) => Vec2::from(aim) + vec2(0.0, 14.0) - from,
            None => vec2(p.player.facing.sign(), 0.0),
        };
        p.player.face_towards(from.x + dir.x);
        p.inventory.remove(ItemId::Stone, 1);
        p.player.swing_timer = player::SWING_TIME;
        let owner = p.id;
        self.projectiles.push(Projectile::stone(from, dir, owner));
        self.wear_equipped(pi);
    }

    /// Gasta durabilidade do item na mão (se ele tiver) e avisa se quebrou.
    fn wear_equipped(&mut self, pi: usize) {
        let p = &mut self.players[pi];
        if let Some(broken) = p.inventory.wear(p.hotbar_selected) {
            self.notify(pi, format!("{} quebrou!", broken.name()));
        }
    }

    fn hit_creature(&mut self, pi: usize, i: usize) {
        let p = &self.players[pi];
        let held = p.equipped();
        let tool = p.equipped_tool();
        let damage = ItemId::melee_damage(held) * p.progression.melee_mult();
        let torpor = ItemId::melee_torpor(held);
        let player_pos = p.player.pos;
        let c = &mut self.creatures[i];
        let pos = c.pos;
        let species = c.species;
        if c.is_alive() {
            let hit = c.take_hit(damage, torpor, player_pos);
            self.players[pi].player.face_towards(pos.x);
            self.report_hit(Some(pi), i, damage, hit);
            if held.is_some_and(|item| item.max_durability().is_some()) {
                self.wear_equipped(pi);
            }
            return;
        }
        if !c.harvest() {
            return;
        }
        self.players[pi].player.face_towards(pos.x);
        if matches!(tool, Some(ToolKind::Pickaxe | ToolKind::Hatchet)) {
            self.wear_equipped(pi);
        }
        let tier = ItemId::tool_tier(held);
        let drops = gathering::roll_corpse_yield(species, tool, tier, &mut self.rng);
        self.give_drops(pi, pos, drops);
        self.gain_xp(pi, progression::XP_GATHER);
    }

    fn hit_node(&mut self, pi: usize, idx: usize) {
        let tool = self.players[pi].equipped_tool();
        let node = &mut self.world.nodes[idx];
        node.hp -= 1;
        node.shake = 0.2;
        let (kind, pos) = (node.kind, node.pos);
        if node.hp == 0 {
            node.respawn_timer = kind.respawn_secs();
        }
        self.players[pi].player.face_towards(pos.x);

        // Só ferramentas de coleta desgastam ao coletar.
        if matches!(tool, Some(ToolKind::Pickaxe | ToolKind::Hatchet)) {
            self.wear_equipped(pi);
        }
        let tier = ItemId::tool_tier(self.players[pi].equipped());
        let drops = gathering::roll_yield(kind, tool, tier, &mut self.rng);
        if !drops.is_empty() {
            self.gain_xp(pi, progression::XP_GATHER);
        }
        self.give_drops(pi, pos, drops);
        if kind == NodeKind::Cactus {
            // A seiva do cacto mata um pouco a sede.
            self.players[pi].vitals.drink(CACTUS_WATER);
            let at = self.players[pi].player.pos - vec2(0.0, 40.0);
            self.popup(at, format!("+{CACTUS_WATER} Água"));
        }
    }

    /// Coloca os itens coletados no inventário, com números flutuantes sobre `pos`.
    fn give_drops(&mut self, pi: usize, pos: Vec2, drops: Vec<(ItemId, u32)>) {
        let mut full = false;
        for (i, (item, n)) in drops.into_iter().enumerate() {
            let leftover = self.players[pi].inventory.add(item, n);
            if leftover < n {
                let at = pos - vec2(0.0, 30.0 + i as f32 * 14.0);
                self.popup(at, format!("+{} {}", n - leftover, item.name()));
            }
            full |= leftover > 0;
        }
        if full {
            self.notify(pi, "Inventário cheio!");
        }
    }

    fn drink(&mut self, pi: usize) {
        let v = &mut self.players[pi].vitals;
        if v.water >= v.max.water - 0.5 {
            self.notify(pi, "Você não está com sede");
            return;
        }
        v.drink(vitals::DRINK_WATER);
        let at = self.players[pi].player.pos - vec2(0.0, 40.0);
        self.popup(at, format!("+{} Água", vitals::DRINK_WATER));
    }

    /// Usa o item da hotbar selecionado (por enquanto: comer).
    fn use_selected(&mut self, pi: usize) {
        if self.players[pi].equipped() == Some(ItemId::Raft) {
            self.launch_raft(pi);
            return;
        }
        let p = &mut self.players[pi];
        let Some(item) = p.equipped() else { return };
        let Some(effect) = item.food_effect() else { return };
        if p.vitals.food >= p.vitals.max.food - 0.5 {
            self.notify(pi, "Você está satisfeito");
            return;
        }
        p.inventory.take_from_slot(p.hotbar_selected, 1);
        p.vitals.eat(effect);
        let at = p.player.pos - vec2(0.0, 40.0);
        self.popup(at, format!("+{} Comida", effect.food));
        if effect.health < 0.0 {
            self.notify(pi, "Carne crua faz mal. Cozinhe na fogueira!");
        }
    }

    // ---------- Construção ----------

    fn place_selected_at(&mut self, pi: usize, tile: (i32, i32), vertical: bool) -> Result<(), PlaceError> {
        let p = &self.players[pi];
        let Some(kind) = p.equipped().and_then(ItemId::structure) else {
            return Ok(());
        };
        let vertical = vertical && kind.rotates();
        // Ninguém (jogador, NPC ou criatura) pode estar no lugar de algo que bloqueia passagem.
        let bodies = self
            .players
            .iter()
            .map(|o| (o.player.pos, player::RADIUS))
            .chain(self.npcs.iter().map(|n| (n.pos(), player::RADIUS)))
            .chain(
                self.creatures
                    .iter()
                    .filter(|c| !c.flying && c.corpse.is_none())
                    .map(|c| (c.pos, c.species.radius())),
            );
        for (pos, radius) in bodies {
            self.world
                .can_place_oriented(kind, tile, vertical, pos, radius)
                .or_else(|e| if e == PlaceError::TooFar { Ok(()) } else { Err(e) })?;
        }
        self.world
            .can_place_oriented(kind, tile, vertical, p.player.pos, player::RADIUS)?;
        self.world.place_structure(Structure {
            vertical,
            owner: Some(p.name.clone()),
            ..Structure::new(kind, tile)
        });
        let p = &mut self.players[pi];
        p.inventory.take_from_slot(p.hotbar_selected, 1);
        if kind == StructureKind::Bed {
            self.notify(pi, "Cama posicionada: você pode renascer nela");
        }
        Ok(())
    }

    /// Remove a estrutura no tile (objeto antes do piso) e devolve o item; conteúdo de baú cai no chão.
    fn demolish_at(&mut self, pi: usize, tile: (i32, i32)) -> bool {
        let pos = self.players[pi].player.pos;
        if building::tile_rect(tile).center().distance(pos) > building::PLACE_REACH {
            return false;
        }
        let Some(i) = self
            .world
            .structure_at(tile, Layer::Object)
            .or_else(|| self.world.structure_at(tile, Layer::Roof))
            .or_else(|| self.world.structure_at(tile, Layer::Floor))
        else {
            return false;
        };
        // Painéis de estruturas abertos por qualquer jogador fecham (os índices mudam).
        for pj in 0..self.players.len() {
            if matches!(self.players[pj].container, Some(ContainerRef::Structure(_))) {
                self.close_container(pj);
            }
        }
        let mut s = self.world.structures.remove(i);
        let center = s.rect().center();
        if let Some(storage) = s.storage.take() {
            let stacks: Vec<Stack> = storage.slots.into_iter().flatten().collect();
            self.world.drop_bag(center, stacks, BagKind::Dropped);
        }
        let item = s.kind.item();
        // Muito danificada, não sobra nada para reaproveitar.
        if s.health_frac() < 0.5 {
            self.notify(
                pi,
                format!("Desmontado: {} (danificado demais, sem devolver)", item.name()),
            );
            return true;
        }
        if let Some(left) = self.players[pi].inventory.insert_stack(Stack::new(item, 1)) {
            self.world.drop_bag(center, vec![left], BagKind::Dropped);
        }
        self.notify(pi, format!("Desmontado: {}", item.name()));
        true
    }

    // ---------- Interação (F) ----------

    fn interact(&mut self, pi: usize, target: Interaction) {
        match target {
            Interaction::Bag(i) => self.pick_up(pi, i),
            Interaction::Structure(i) => match self.world.structures[i].kind {
                kind if kind.opens() => self.toggle_door(pi, i),
                StructureKind::StorageBox | StructureKind::Campfire | StructureKind::Forge => {
                    self.close_container(pi);
                    self.players[pi].container = Some(ContainerRef::Structure(i));
                }
                StructureKind::Bed => {
                    let name = self.players[pi].name.clone();
                    self.world.structures[i].owner = Some(name);
                    self.notify(pi, "Esta cama agora é seu ponto de renascer");
                }
                _ => {}
            },
            Interaction::Creature(id) => {
                self.close_container(pi);
                self.players[pi].container = Some(ContainerRef::Creature(id));
            }
            Interaction::Npc(id) => self.talk_to(pi, id),
            Interaction::Raft(id) => {
                if let Some(i) = self.rafts.iter().position(|r| r.id == id) {
                    self.board_raft(pi, i);
                }
            }
        }
    }

    fn toggle_door(&mut self, pi: usize, i: usize) {
        let door = &self.world.structures[i];
        let mut closed = door.clone();
        closed.open = false;
        let blocked = self.players.iter().any(|p| closed.blocks(p.player.pos, player::RADIUS))
            || self.npcs.iter().any(|n| closed.blocks(n.pos(), player::RADIUS))
            || self
                .creatures
                .iter()
                .any(|c| !c.flying && closed.blocks(c.pos, c.species.radius()));
        if door.open && blocked {
            self.notify(pi, "Tem alguém na passagem");
            return;
        }
        let door = &mut self.world.structures[i];
        door.open = !door.open;
    }

    /// Acende/apaga a fogueira ou forja aberta no painel.
    fn toggle_fire(&mut self, pi: usize) {
        let Some(ContainerRef::Structure(i)) = self.players[pi].container else {
            return;
        };
        let Some(fire) = self.world.structures.get_mut(i) else {
            return;
        };
        if !fire.toggle_fire() {
            self.notify(pi, "Coloque madeira ou palha no fogo");
        }
    }

    fn pick_up(&mut self, pi: usize, i: usize) {
        let bag = &mut self.world.bags[i];
        let before: u32 = bag.stacks.iter().map(|s| s.count).sum();
        self.players[pi].inventory.take_all(&mut bag.stacks);
        let after: u32 = bag.stacks.iter().map(|s| s.count).sum();
        let pos = bag.pos;
        if after < before {
            self.popup(pos - vec2(0.0, 20.0), format!("+{} itens", before - after));
        }
        if after > 0 {
            self.notify(pi, "Inventário cheio!");
        }
    }

    fn slot(&mut self, pi: usize, target: SlotTarget, left: bool, shift: bool) {
        let p = &mut self.players[pi];
        let chest = container_inventory(&mut self.world, &mut self.creatures, &mut self.npcs, p.container);
        match target {
            SlotTarget::Inventory(i) => {
                let Some(slot) = p.inventory.slots.get_mut(i) else {
                    return;
                };
                match chest {
                    Some(chest) if left && shift && p.held.is_none() => slots::transfer(slot, chest),
                    _ if left => slots::click(&mut p.held, slot),
                    _ => slots::split(&mut p.held, slot),
                }
            }
            SlotTarget::Container(i) => {
                let Some(chest) = chest else { return };
                let Some(slot) = chest.slots.get_mut(i) else { return };
                if left && shift && p.held.is_none() {
                    slots::transfer(slot, &mut p.inventory);
                } else if left {
                    slots::click(&mut p.held, slot);
                } else {
                    slots::split(&mut p.held, slot);
                }
            }
        }
    }

    // ---------- Morte ----------

    fn die(&mut self, pi: usize) {
        if self.players[pi].dead {
            return;
        }
        self.players[pi].dead = true;
        self.dismount(pi);
        self.abandon_raft(pi);
        self.close_container(pi);
        let p = &mut self.players[pi];
        // A fila é cancelada: os materiais voltam e vão junto para a bolsa.
        let mut leftovers = Vec::new();
        while !p.queue.jobs.is_empty() {
            leftovers.extend(p.queue.cancel(0, &mut p.inventory));
        }
        let mut stacks = p.inventory.drain_all();
        stacks.extend(leftovers);
        p.player.moving = false;
        let pos = p.player.pos;
        let name = p.name.clone();
        self.world.drop_bag(pos, stacks, BagKind::Death);
        self.broadcast(format!("{name} morreu"));
    }

    /// Cama do jogador mais perto de onde ele morreu.
    fn bed_of(&self, pi: usize) -> Option<usize> {
        let p = &self.players[pi];
        self.world
            .structures
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == StructureKind::Bed && s.owner.as_ref() == Some(&p.name))
            .min_by(|a, b| {
                let (da, db) = (
                    a.1.rect().center().distance(p.player.pos),
                    b.1.rect().center().distance(p.player.pos),
                );
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
    }

    /// Renasce na praia ou, com `bed`, ao lado da sua cama mais perto de onde morreu.
    fn respawn(&mut self, pi: usize, bed: bool) {
        if !self.players[pi].dead {
            return;
        }
        let at_bed = bed.then(|| self.bed_of(pi)).flatten().and_then(|i| {
            let r = self.world.structures[i].rect();
            self.world.find_near(vec2(r.center().x, r.bottom() + 10.0), |q| {
                !self.world.blocked(q, player::RADIUS)
            })
        });
        let p = &mut self.players[pi];
        p.player = Player::new(at_bed.unwrap_or(self.world.spawn));
        p.vitals = Vitals::full_with(p.progression.maxima());
        p.dead = false;
        match at_bed {
            Some(_) => self.notify(pi, "Você renasceu na sua cama. Recupere sua bolsa!"),
            None if bed => self.notify(pi, "Você não tem cama: renasceu na praia"),
            None => self.notify(pi, "Você renasceu na praia. Recupere sua bolsa!"),
        }
    }

    fn craft(&mut self, pi: usize, index: usize) {
        let Some(recipe) = RECIPES.get(index) else { return };
        let p = &mut self.players[pi];
        if !p.progression.knows(recipe.output) {
            match p.progression.learn(recipe) {
                Ok(()) => self.notify(
                    pi,
                    format!(
                        "Engrama aprendido: {} (-{} pontos)",
                        recipe.output.name(),
                        recipe.engram_cost
                    ),
                ),
                Err(LearnError::LevelTooLow(level)) => self.notify(pi, format!("Requer nível {level}")),
                Err(LearnError::NotEnoughPoints(cost)) => {
                    self.notify(pi, format!("Faltam pontos de engrama (custa {cost})"));
                }
                Err(LearnError::AlreadyKnown) => {}
            }
            return;
        }
        let station_near = self.station_ok(pi, recipe);
        let p = &mut self.players[pi];
        match p.queue.enqueue(&mut p.inventory, recipe, station_near) {
            Ok(()) => {}
            Err(CraftError::MissingIngredients) => self.notify(pi, "Faltam materiais"),
            Err(CraftError::NeedsStation(kind)) => {
                self.notify(pi, format!("Requer {} por perto", kind.item().name()));
            }
            Err(CraftError::QueueFull) => self.notify(pi, "Fila de crafting cheia"),
        }
    }

    // ---------- Comandos ----------

    pub fn command(&mut self, id: PlayerId, cmd: Command) {
        let Some(pi) = self.index(id) else { return };
        if self.players[pi].dead && !matches!(cmd, Command::Respawn { .. } | Command::Chat(_)) {
            return;
        }
        match cmd {
            Command::SelectHotbar(i) => {
                if i < HOTBAR_SIZE {
                    self.players[pi].hotbar_selected = i;
                }
            }
            Command::Use => self.use_selected(pi),
            Command::Drop { all } => {
                let p = &mut self.players[pi];
                let count = if all { u32::MAX } else { 1 };
                if let Some(stack) = p.inventory.take_from_slot(p.hotbar_selected, count) {
                    self.drop_stack(pi, stack);
                }
            }
            Command::Interact => {
                let world = &self.world;
                let mount = self.players[pi]
                    .mount
                    .and_then(|id| self.creatures.iter_mut().find(|c| c.id == id));
                if let Some(mount) = mount {
                    if !mount.try_land(world) {
                        self.notify(pi, "Pouse antes de descer (Espaço)".to_owned());
                        return;
                    }
                    let (pos, r, left) = (mount.pos, mount.species.radius(), mount.facing_left);
                    if self.step_off_spot(pos, r, left).is_none() && self.free_raft_near(pos).is_none() {
                        self.notify(pi, "Chegue perto da margem ou de uma jangada para descer");
                    } else {
                        self.dismount(pi);
                    }
                } else if let Some(target) = nearest_interaction(
                    &self.world,
                    &self.creatures,
                    &self.rafts,
                    &self.npc_spots(),
                    self.players[pi].player.pos,
                ) {
                    self.interact(pi, target);
                } else if self.players[pi].raft.is_some() {
                    self.leave_raft(pi);
                }
            }
            Command::Place { tile, vertical } => {
                if self.players[pi].mount.is_none()
                    && self.players[pi].raft.is_none()
                    && let Err(e) = self.place_selected_at(pi, tile, vertical)
                {
                    self.notify(pi, e.message());
                }
            }
            Command::Demolish { tile } => {
                if self.players[pi].mount.is_none() {
                    self.demolish_at(pi, tile);
                }
            }
            Command::Craft(i) => self.craft(pi, i),
            Command::CancelCraft(i) => {
                let p = &mut self.players[pi];
                for stack in p.queue.cancel(i, &mut p.inventory) {
                    self.drop_stack(pi, stack);
                }
            }
            Command::Slot { target, left, shift } => self.slot(pi, target, left, shift),
            Command::DropHeld => {
                if let Some(stack) = self.players[pi].held.take() {
                    self.drop_stack(pi, stack);
                }
            }
            Command::CloseContainer => self.close_container(pi),
            Command::ToggleFire => self.toggle_fire(pi),
            Command::ToggleOrder => {
                let name = self.players[pi].name.clone();
                if let Some(ContainerRef::Creature(id)) = self.players[pi].container
                    && let Some(c) = self.creature_mut(id)
                {
                    c.toggle_command();
                    // Mandar seguir faz o dino seguir quem deu a ordem.
                    if c.command == Some(Order::Follow) {
                        c.owner = Some(name);
                    }
                }
            }
            Command::MountAction => self.mount_action(pi),
            Command::Mount => {
                if let Some(ContainerRef::Creature(id)) = self.players[pi].container {
                    self.close_container(pi);
                    self.mount_creature(pi, id);
                }
            }
            Command::SpendAttribute(attr) => {
                let p = &mut self.players[pi];
                if p.progression.spend(attr) {
                    p.vitals.set_max(p.progression.maxima());
                }
            }
            Command::Respawn { bed } => self.respawn(pi, bed),
            Command::Whistle(whistle) => self.whistle(pi, whistle),
            Command::NpcOrder(cmd) => self.npc_order(pi, cmd),
            Command::Chat(text) => {
                let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_CHAT_LEN).collect();
                if !text.trim().is_empty() {
                    self.chat.push((self.players[pi].name.clone(), text));
                }
            }
            // Salvar é tratado pelo servidor (ele sabe onde gravar).
            Command::Save => {}
            Command::Debug(debug) => {
                if cfg!(debug_assertions) {
                    self.debug(pi, debug);
                }
            }
        }
    }

    fn debug(&mut self, pi: usize, cmd: DebugCmd) {
        match cmd {
            DebugCmd::Items => self.give_debug_items(pi),
            DebugCmd::Damage => self.players[pi].vitals.damage(25.0),
            DebugCmd::SkipHours => {
                self.time += daynight::DAY_LENGTH / 8.0;
                self.broadcast("+3 horas");
            }
            DebugCmd::Xp => self.gain_xp(pi, 200),
        }
    }

    // ---------- Snapshot ----------

    /// O que o jogador `id` precisa para desenhar o próximo frame.
    pub fn snapshot(&self, id: PlayerId) -> Option<Snapshot> {
        let pi = self.index(id)?;
        let me = &self.players[pi];
        let (open_structure, open_creature) = match me.container {
            Some(ContainerRef::Structure(i)) => (Some(i), None),
            Some(ContainerRef::Creature(c)) => (None, Some(c)),
            Some(ContainerRef::Npc(_)) | None => (None, None),
        };
        let open_npc = match me.container {
            Some(ContainerRef::Npc(id)) => Some(id),
            _ => None,
        };
        Some(Snapshot {
            time: self.time,
            node_hp: self.world.nodes.iter().map(|n| n.hp.min(255) as u8).collect(),
            bags: self.bag_saves(),
            structures: self
                .world
                .structures
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mut s = s.clone();
                    if Some(i) != open_structure {
                        s.storage = None;
                    }
                    s
                })
                .collect(),
            creatures: self
                .creatures
                .iter()
                .map(|c| CreatureView::new(c, Some(c.id) == open_creature || Some(c.id) == me.mount))
                .collect(),
            projectiles: self.projectiles.iter().map(|p| p.pos.into()).collect(),
            rafts: self.rafts.iter().map(RaftSave::from).collect(),
            npcs: self
                .npcs
                .iter()
                .map(|n| {
                    let mine = n.owner.as_ref().is_none_or(|o| *o == me.name);
                    NpcView::new(n, Some(n.id) == open_npc && mine)
                })
                .collect(),
            players: self.players.iter().map(PlayerState::view).collect(),
            me: me.private(),
        })
    }

    // ---------- Atalhos de debug / smoke test (agem sobre o primeiro jogador) ----------

    pub fn give_debug_items(&mut self, pi: usize) {
        for (item, n) in [
            (ItemId::Wood, 100),
            (ItemId::Stone, 60),
            (ItemId::Thatch, 80),
            (ItemId::Flint, 10),
            (ItemId::Berry, 20),
            (ItemId::RawMeat, 20),
            (ItemId::Hide, 30),
            (ItemId::Narcotic, 5),
            (ItemId::WoodenClub, 1),
            (ItemId::Slingshot, 1),
            (ItemId::MetalOre, 20),
            (ItemId::MetalIngot, 10),
        ] {
            self.players[pi].inventory.add(item, n);
        }
        self.notify(pi, "Itens de debug adicionados");
    }

    pub fn debug_xp(&mut self, amount: u32) {
        self.gain_xp(0, amount);
    }

    pub fn kill(&mut self) {
        self.players[0].vitals.damage(f32::MAX);
        self.die(0);
    }

    /// Avança o relógio até a próxima ocorrência de `hour`.
    pub fn skip_to_hour(&mut self, hour: f32) {
        self.time += daynight::secs_until(self.time, hour);
    }

    /// Monta uma cabana de exemplo ao norte do spawn.
    pub fn build_demo_base(&mut self) {
        let (sx, sy) = building::tile_of(self.world.spawn);
        let (x0, x1, y0, y1) = (sx - 2, sx + 2, sy - 5, sy - 2);
        let mut place = |kind: StructureKind, tile: (i32, i32)| {
            if self.world.tile(tile.0, tile.1).is_land() && self.world.structure_at(tile, kind.layer()).is_none() {
                self.world.place(kind, tile);
            }
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                place(StructureKind::WoodFoundation, (x, y));
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    let door = y == y1 && x == sx;
                    place(
                        if door {
                            StructureKind::WoodDoor
                        } else {
                            StructureKind::WoodWall
                        },
                        (x, y),
                    );
                }
            }
        }
        place(StructureKind::StorageBox, (sx - 1, y0 + 1));
        place(StructureKind::Workbench, (sx + 1, y0 + 1));
        place(StructureKind::Campfire, (sx + 2, sy));
        if let Some(i) = self.world.structure_at((sx + 2, sy), Layer::Object) {
            let fire = &mut self.world.structures[i];
            if let Some(storage) = fire.storage.as_mut() {
                storage.add(ItemId::Wood, 5);
                storage.add(ItemId::RawMeat, 3);
            }
            fire.toggle_fire();
        }
    }

    /// Base de pedra ao norte do spawn: portão grande, janelas, porta de metal, teto em metade,
    /// móveis, tochas, cerca e uma parede danificada. O jogador fica dentro, com o martelo.
    pub fn demo_fortress(&mut self) {
        self.creatures.clear();
        let (sx, sy) = building::tile_of(self.world.spawn);
        let (x0, x1, y0, y1) = (sx - 4, sx + 4, sy - 9, sy - 4);
        let name = self.players[0].name.clone();
        let world = &mut self.world;
        // Limpa o terreno (árvores e pedras) para a base caber.
        let area = building::tile_rect((x0 - 1, y0 - 1));
        let area = macroquad::math::Rect::new(area.x, area.y, (x1 - x0 + 3) as f32 * TILE, (y1 - y0 + 6) as f32 * TILE);
        world.nodes.retain(|n| !area.contains(n.pos));
        let mut place = |kind: StructureKind, tile: (i32, i32), vertical: bool| {
            let free = building::footprint_tiles(kind, tile, vertical)
                .iter()
                .all(|t| world.tile(t.0, t.1).is_land() && world.structure_at(*t, kind.layer()).is_none());
            if free {
                world.place_structure(Structure {
                    vertical,
                    owner: Some(name.clone()),
                    ..Structure::new(kind, tile)
                });
            }
        };
        place(StructureKind::StoneGate, (sx, y1), false);
        for y in y0..=y1 {
            for x in x0..=x1 {
                place(StructureKind::StoneFoundation, (x, y), false);
                let edge = x == x0 || x == x1 || y == y0 || y == y1;
                if !edge {
                    continue;
                }
                let kind = if (x == x0 || x == x1) && y == y0 + 2 {
                    StructureKind::StoneWindow
                } else if y == y0 && x == sx + 2 {
                    StructureKind::MetalDoor
                } else if y == y0 && x == sx - 2 {
                    StructureKind::MetalWall
                } else {
                    StructureKind::StoneWall
                };
                place(kind, (x, y), false);
            }
        }
        for y in y0..=y1 {
            for x in x0..sx {
                place(StructureKind::StoneRoof, (x, y), false);
            }
        }
        for (kind, tile) in [
            (StructureKind::Bed, (x1 - 1, y0 + 1)),
            (StructureKind::Rug, (sx + 1, y0 + 3)),
            (StructureKind::Table, (sx + 2, y0 + 2)),
            (StructureKind::Chair, (sx + 1, y0 + 2)),
            (StructureKind::FlowerPot, (x1 - 1, y1 - 1)),
            (StructureKind::StorageBox, (sx + 1, y0 + 1)),
            (StructureKind::Torch, (sx - 2, y1 + 1)),
            (StructureKind::Torch, (sx + 2, y1 + 1)),
        ] {
            place(kind, tile, false);
        }
        for x in x0 - 1..=x1 + 1 {
            if (x - sx).abs() > 2 {
                place(StructureKind::WoodFence, (x, y1 + 2), false);
            }
        }
        place(StructureKind::WoodFence, (x0 - 1, y1 + 1), false);
        place(StructureKind::WoodFence, (x1 + 1, y1 + 1), false);
        if let Some(i) = self.world.structure_at((x1, y0 + 4), Layer::Object) {
            self.world.structures[i].health *= 0.4;
        }
        let p = &mut self.players[0];
        p.player.pos = building::tile_rect((sx + 1, y0 + 4)).center() + vec2(8.0, 0.0);
        p.inventory.add(ItemId::Hammer, 1);
        p.inventory.add(ItemId::Stone, 40);
        p.inventory.add(ItemId::Wood, 20);
    }

    /// Abre o painel da primeira fogueira do mundo.
    pub fn open_first_campfire(&mut self) {
        if let Some(i) = self
            .world
            .structures
            .iter()
            .position(|s| s.kind == StructureKind::Campfire)
        {
            self.players[0].player.pos = self.world.structures[i].rect().center() - vec2(28.0, 0.0);
            self.players[0].container = Some(ContainerRef::Structure(i));
        }
    }

    /// Raptor desacordado com carne, com o painel de domesticação aberto.
    pub fn demo_taming(&mut self) {
        self.creatures.clear();
        let base = self.players[0].player.pos;
        let mut raptor = Creature::new(Species::Raptor, base + vec2(30.0, 0.0)).with_level(12);
        raptor.take_hit(4.0, 9999.0, base);
        if let Some(inv) = raptor.inventory.as_mut() {
            inv.add(ItemId::RawMeat, 12);
            inv.add(ItemId::Narcotic, 2);
        }
        if let Some(t) = raptor.taming.as_mut() {
            t.progress = 0.35;
        }
        let id = self.add_creature(raptor);
        self.players[0].container = Some(ContainerRef::Creature(id));
    }

    /// Jogador montado num raptor, um dodô seguindo e um dilo desacordado.
    pub fn demo_riding(&mut self) {
        self.creatures.clear();
        let base = self.players[0].player.pos;
        let owner = Some(self.players[0].name.clone());
        let mut raptor = Creature::new(Species::Raptor, base).with_level(18);
        raptor.command = Some(Order::Follow);
        raptor.owner = owner.clone();
        let mut pack = Inventory::new(Species::Raptor.pack_slots());
        pack.add(ItemId::RaptorSaddle, 1);
        raptor.inventory = Some(pack);
        let id = self.add_creature(raptor);
        let mut dodo = Creature::new(Species::Dodo, base + vec2(-50.0, 30.0)).with_level(5);
        dodo.command = Some(Order::Follow);
        dodo.owner = owner;
        self.add_creature(dodo);
        let mut dilo = Creature::new(Species::Dilophosaur, base + vec2(90.0, 50.0)).with_level(9);
        dilo.take_hit(0.0, 9999.0, base);
        self.add_creature(dilo);
        self.mount_creature(0, id);
        let owner_id = self.players[0].id;
        self.projectiles
            .push(Projectile::stone(base + vec2(20.0, -10.0), vec2(1.0, 0.3), owner_id));
    }

    /// Painel de um raptor domesticado (desmontado).
    pub fn demo_pet_panel(&mut self) {
        self.demo_riding();
        let Some(id) = self.players[0].mount else { return };
        self.dismount(0);
        self.players[0].container = Some(ContainerRef::Creature(id));
    }

    /// Uma criatura de cada espécie perto do jogador, uma carcaça e um dilo ferido.
    pub fn spawn_demo_creatures(&mut self) {
        let base = self.players[0].player.pos;
        let offsets = [
            vec2(-90.0, 40.0),
            vec2(-40.0, 80.0),
            vec2(70.0, 70.0),
            vec2(120.0, 10.0),
            vec2(-130.0, -30.0),
            vec2(150.0, 90.0),
            vec2(-60.0, -90.0),
            vec2(30.0, 130.0),
            vec2(-150.0, 110.0),
            vec2(90.0, -100.0),
            vec2(-190.0, 30.0),
            vec2(190.0, -40.0),
            vec2(-20.0, 170.0),
        ];
        for (species, offset) in Species::ALL.into_iter().zip(offsets) {
            let mut c = Creature::new(species, base + offset);
            c.facing_left = offset.x > 0.0;
            self.add_creature(c);
        }
        let mut corpse = Creature::new(Species::Dodo, base + vec2(40.0, -50.0));
        corpse.take_hit(999.0, 0.0, base);
        self.add_creature(corpse);
        let dilo = self.creatures.len() - 12;
        self.creatures[dilo].take_hit(30.0, 20.0, base);
    }

    /// Domesticado da espécie, com sela e dono = primeiro jogador.
    fn demo_tamed(&mut self, species: Species, pos: Vec2, level: u32) -> u32 {
        let mut c = Creature::new(species, pos).with_level(level);
        c.command = Some(Order::Follow);
        c.owner = Some(self.players[0].name.clone());
        let mut pack = Inventory::new(species.pack_slots());
        if let Some(saddle) = species.saddle() {
            pack.add(saddle, 1);
        }
        c.inventory = Some(pack);
        self.add_creature(c)
    }

    /// Jogador voando num pteranodonte, um argentavis domesticado voando junto e um selvagem no ar.
    pub fn demo_flying(&mut self) {
        self.creatures.clear();
        let base = self.players[0].player.pos;
        let ptera = self.demo_tamed(Species::Pteranodon, base, 15);
        let argy = self.demo_tamed(Species::Argentavis, base + vec2(-70.0, 40.0), 20);
        let mut wild = Creature::new(Species::Pteranodon, base + vec2(140.0, -60.0)).with_level(8);
        wild.facing_left = true;
        self.add_creature(wild);
        self.mount_creature(0, ptera);
        for c in &mut self.creatures {
            if c.id == ptera || c.id == argy || c.is_wild() {
                c.flying = true;
                c.altitude = 1.0;
                c.moving = true;
            }
        }
    }

    /// Primeiro jogador no bioma de gelo, montado num mamute de frente para uma árvore, com um smilodon
    /// domesticado, uma alcateia de lobos e um mamute selvagem por perto.
    pub fn demo_ice(&mut self) {
        let world = &self.world;
        let Some(spot) = world
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Tree && world.tile_at(n.pos) == Tile::Snow)
            .map(|n| n.pos - vec2(40.0, 4.0))
            .find(|&p| Species::Mammoth.can_stand(world, p) && world.tile_at(p - vec2(160.0, 0.0)) == Tile::Snow)
        else {
            return;
        };
        self.creatures.clear();
        self.players[0].player.pos = spot;
        let mammoth = self.demo_tamed(Species::Mammoth, spot, 22);
        self.demo_tamed(Species::Smilodon, spot + vec2(-70.0, 46.0), 14);
        let wild = [
            (Species::Wolf, vec2(150.0, -70.0), 6),
            (Species::Wolf, vec2(178.0, -40.0), 6),
            (Species::Wolf, vec2(160.0, -100.0), 6),
            (Species::Mammoth, vec2(-150.0, -40.0), 12),
        ];
        for (species, offset, level) in wild {
            let world = &self.world;
            let Some(at) = world.find_near(spot + offset, |p| species.can_stand(world, p)) else {
                continue;
            };
            let mut c = Creature::new(species, at).with_level(level);
            c.facing_left = offset.x > 0.0;
            self.add_creature(c);
        }
        self.mount_creature(0, mammoth);
    }

    /// Primeiro jogador no pântano montado num sarcosuchus dentro de uma lagoa, com piranhas em volta,
    /// um beelzebufo e uma titanoboa domesticados na lama.
    pub fn demo_swamp(&mut self) {
        let world = &self.world;
        let mud_near = |p: Vec2| {
            world
                .find_near(p, |q| world.tile_at(q) == Tile::Swamp)
                .is_some_and(|m| m.distance(p) < 90.0)
        };
        let Some(spot) = (0..world.width as i32 * world.height as i32)
            .map(|i| {
                let (x, y) = (i % world.width as i32, i / world.width as i32);
                vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE)
            })
            .filter(|&p| world.tile_at(p) == Tile::SwampWater && Species::Sarcosuchus.can_stand(world, p))
            .find(|&p| {
                let fish = [vec2(60.0, -30.0), vec2(80.0, 10.0)];
                mud_near(p) && fish.iter().all(|&o| world.tile_at(p + o) == Tile::SwampWater)
            })
        else {
            return;
        };
        self.creatures.clear();
        self.players[0].player.pos = spot;
        let sarco = self.demo_tamed(Species::Sarcosuchus, spot, 16);
        let place = |sim: &Sim, species: Species, offset: Vec2| {
            sim.world.find_near(spot + offset, |p| species.can_stand(&sim.world, p))
        };
        if let Some(at) = place(self, Species::Beelzebufo, vec2(-60.0, 60.0)) {
            self.demo_tamed(Species::Beelzebufo, at, 9);
        }
        if let Some(at) = place(self, Species::Titanoboa, vec2(-130.0, -10.0)) {
            self.demo_tamed(Species::Titanoboa, at, 12);
        }
        for offset in [vec2(60.0, -30.0), vec2(80.0, 10.0), vec2(95.0, -15.0), vec2(70.0, 30.0)] {
            if let Some(at) = place(self, Species::Piranha, offset) {
                let mut fish = Creature::new(Species::Piranha, at).with_level(4);
                fish.facing_left = true;
                self.add_creature(fish);
            }
        }
        self.mount_creature(0, sarco);
    }

    /// Primeiro jogador numa jangada no mar, com um ictiossauro domesticado ao lado, um megalodonte
    /// chegando e um mosassauro e um kraken mais longe.
    pub fn demo_sea(&mut self) {
        let world = &self.world;
        let deep = |p: Vec2| world.tile_at(p) == Tile::DeepWater;
        let Some(spot) = (0..120)
            .map(|i| world.spawn + vec2(0.0, i as f32 * 16.0))
            .find(|&p| raft::can_float(world, p) && deep(p) && deep(p + vec2(0.0, 220.0)))
        else {
            return;
        };
        self.creatures.clear();
        self.rafts.clear();
        self.players[0].player.pos = spot;
        self.players[0].inventory.add(ItemId::Raft, 1);
        self.players[0].hotbar_selected = 0;
        self.launch_raft(0);
        self.players[0].player.facing = Facing::Right;
        self.demo_tamed(Species::Ichthyosaur, spot + vec2(-70.0, 34.0), 12);
        let wild = [
            (Species::Megalodon, vec2(110.0, 50.0), 14),
            (Species::Mosasaur, vec2(-150.0, 150.0), 20),
            (Species::Kraken, vec2(170.0, 170.0), 18),
        ];
        for (species, offset, level) in wild {
            let mut c = Creature::new(species, spot + offset).with_level(level);
            c.facing_left = offset.x > 0.0;
            self.add_creature(c);
        }
    }

    /// Montado num mosassauro em mar aberto, com uma jangada vazia por perto.
    pub fn demo_sea_ride(&mut self) {
        self.demo_sea();
        let mosasaur = self.creatures.iter().position(|c| c.species == Species::Mosasaur);
        let Some(i) = mosasaur else { return };
        let pos = self.creatures[i].pos;
        self.creatures.remove(i);
        let id = self.demo_tamed(Species::Mosasaur, pos, 24);
        self.mount_creature(0, id);
    }

    /// Primeiro jogador no deserto montado num canguru perto de cactos, com um golem e um louva-deus
    /// domesticados, um verme da morte saindo da areia e um golem selvagem disfarçado de rocha.
    pub fn demo_desert(&mut self) {
        let world = &self.world;
        let open = |p: Vec2| {
            [vec2(0.0, 0.0), vec2(-70.0, 30.0), vec2(80.0, 10.0), vec2(150.0, -40.0)]
                .iter()
                .all(|&o| world.tile_at(p + o) == Tile::Desert && !world.blocked(p + o, 16.0))
        };
        let Some(spot) = world
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Cactus)
            .map(|n| n.pos + vec2(-36.0, 18.0))
            .find(|&p| open(p))
        else {
            return;
        };
        self.creatures.clear();
        self.players[0].player.pos = spot;
        let roo = self.demo_tamed(Species::Kangaroo, spot, 12);
        self.demo_tamed(Species::StoneGolem, spot + vec2(-70.0, 30.0), 15);
        self.demo_tamed(Species::Mantis, spot + vec2(-40.0, 60.0), 10);
        let mut worm = Creature::new(Species::Deathworm, spot + vec2(80.0, 10.0)).with_level(14);
        worm.buried = false;
        worm.facing_left = true;
        self.add_creature(worm);
        let golem = Creature::new(Species::StoneGolem, spot + vec2(150.0, -40.0)).with_level(9);
        self.add_creature(golem);
        self.mount_creature(0, roo);
    }

    /// Painel de carga de um argentavis domesticado.
    pub fn demo_cargo(&mut self) {
        self.creatures.clear();
        let base = self.players[0].player.pos;
        let id = self.demo_tamed(Species::Argentavis, base + vec2(34.0, 0.0), 20);
        if let Some(inv) = self.creature_mut(id).and_then(|c| c.inventory.as_mut()) {
            inv.add(ItemId::Stone, 100);
            inv.add(ItemId::Wood, 100);
            inv.add(ItemId::MetalOre, 60);
        }
        self.demo_tamed(Species::Pteranodon, base + vec2(-40.0, 30.0), 10);
        self.players[0].container = Some(ContainerRef::Creature(id));
    }

    /// Uma segunda jogadora ("Bia") ao lado do primeiro, com picareta e uma mensagem no chat.
    pub fn demo_coop(&mut self) {
        let id = self.join("Bia");
        let Some(pi) = self.index(id) else { return };
        let base = self.players[0].player.pos;
        self.players[pi].player.pos = base + vec2(40.0, 16.0);
        self.players[pi].player.facing = Facing::Left;
        self.players[pi].inventory.add(ItemId::StonePickaxe, 1);
        self.command(id, Command::Chat("Vamos construir a base perto da praia!".to_owned()));
    }

    /// Leva o primeiro jogador para a montanha, com uma forja acesa fundindo metal.
    pub fn demo_mountain(&mut self) {
        let Some(target) = self
            .world
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::MetalRock)
            .map(|n| n.pos)
        else {
            return;
        };
        let spot = (1..6)
            .flat_map(|r| {
                [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)].map(|d| target + d * 24.0 * r as f32)
            })
            .find(|&p| !self.world.blocked(p, player::RADIUS));
        let Some(spot) = spot else { return };
        self.players[0].player.pos = spot;
        self.creatures.clear();
        let (tx, ty) = building::tile_of(spot);
        let candidates = [(tx + 2, ty), (tx - 2, ty), (tx, ty + 2), (tx, ty - 2)];
        if let Some(&tile) = candidates.iter().find(|&&t| {
            self.world
                .can_place(StructureKind::Forge, t, spot, player::RADIUS)
                .is_ok()
        }) {
            self.world.place(StructureKind::Forge, tile);
            let i = self.world.structures.len() - 1;
            let forge = &mut self.world.structures[i];
            if let Some(storage) = forge.storage.as_mut() {
                storage.add(ItemId::Wood, 5);
                storage.add(ItemId::MetalOre, 6);
            }
            forge.toggle_fire();
            forge.cook = 4.0;
        }
        self.players[0].inventory.add(ItemId::MetalPickaxe, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::Attribute;

    fn sim_with(names: &[&str]) -> (Sim, Vec<PlayerId>) {
        let mut sim = Sim::new(1337);
        let ids = names.iter().map(|n| sim.join(n)).collect();
        (sim, ids)
    }

    #[test]
    fn players_join_leave_and_come_back() {
        let (mut sim, ids) = sim_with(&["Ana", "Bia"]);
        assert_eq!(sim.players.len(), 2);
        sim.players[1].inventory.add(ItemId::Flint, 7);
        sim.leave(ids[1]);
        assert_eq!(sim.players.len(), 1);
        let again = sim.join("Bia");
        let bia = sim.index(again).unwrap();
        assert_eq!(
            sim.players[bia].inventory.count(ItemId::Flint),
            7,
            "volta com o que tinha"
        );
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let (sim, _) = sim_with(&["Ana", "Ana"]);
        assert_ne!(sim.players[0].name, sim.players[1].name);
    }

    #[test]
    fn save_round_trip_keeps_every_player() {
        let (mut sim, ids) = sim_with(&["Ana", "Bia"]);
        sim.players[0].inventory.add(ItemId::StonePickaxe, 1);
        sim.players[0].inventory.wear(0);
        sim.players[1].vitals.food = 42.0;
        sim.world.nodes[5].hp = 0;
        sim.world.place(StructureKind::StorageBox, (3, 4));
        sim.world.structures[0].storage.as_mut().unwrap().add(ItemId::Flint, 9);
        sim.leave(ids[1]);

        let saved = sim.to_save();
        let json = serde_json::to_string(&saved).unwrap();
        let mut loaded = Sim::from_save(serde_json::from_str(&json).unwrap());
        assert_eq!(loaded.to_save(), saved);
        let bia = loaded.join("Bia");
        let pi = loaded.index(bia).unwrap();
        assert_eq!(loaded.players[pi].vitals.food, 42.0);
    }

    #[test]
    fn legacy_single_player_save_goes_to_the_host() {
        let (mut sim, _) = sim_with(&["Velho"]);
        sim.players[0].inventory.add(ItemId::Hide, 3);
        let mut save = sim.to_save();
        save.players[0].name = LEGACY_PLAYER.to_owned();
        let mut loaded = Sim::from_save(save);
        let host = loaded.join("Diego");
        let pi = loaded.index(host).unwrap();
        assert_eq!(loaded.players[pi].inventory.count(ItemId::Hide), 3);
        assert_eq!(loaded.players[pi].name, "Diego");
    }

    #[test]
    fn wild_creatures_hunt_the_nearest_player() {
        let (mut sim, ids) = sim_with(&["Ana", "Bia"]);
        sim.creatures.clear();
        sim.players[1].player.pos = sim.players[0].player.pos + vec2(600.0, 0.0);
        let raptor_pos = sim.players[1].player.pos + vec2(22.0, 0.0);
        sim.add_creature(Creature::new(Species::Raptor, raptor_pos));
        let before = (sim.players[0].vitals.health, sim.players[1].vitals.health);
        for _ in 0..120 {
            sim.tick(1.0 / 60.0);
        }
        assert_eq!(sim.players[0].vitals.health, before.0, "longe, Ana não apanha");
        assert!(sim.players[1].vitals.health < before.1, "Bia está do lado do raptor");
        let events = sim.take_events();
        assert!(events.contains(&(Some(ids[1]), NetEvent::Hurt)));
    }

    #[test]
    fn slot_commands_move_items_between_inventory_and_chest() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let here = building::tile_of(sim.players[0].player.pos);
        sim.world.place(StructureKind::StorageBox, (here.0 + 1, here.1));
        sim.players[0].inventory.add(ItemId::Wood, 10);
        sim.command(ids[0], Command::Interact);
        assert_eq!(sim.players[0].container, Some(ContainerRef::Structure(0)));
        let slot = |target, left, shift| Command::Slot { target, left, shift };
        sim.command(ids[0], slot(SlotTarget::Inventory(0), true, true));
        assert_eq!(
            sim.world.structures[0].storage.as_ref().unwrap().count(ItemId::Wood),
            10
        );
        sim.command(ids[0], slot(SlotTarget::Container(0), true, false));
        assert_eq!(sim.players[0].held.unwrap().count, 10, "pilha no cursor");
        sim.command(ids[0], Command::CloseContainer);
        assert!(sim.players[0].held.is_none());
        assert_eq!(sim.players[0].inventory.count(ItemId::Wood), 10, "volta ao inventário");
    }

    #[test]
    fn gathering_gives_xp_and_attributes_raise_max_health() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let tree = sim.world.nodes.iter().position(|n| n.kind == NodeKind::Tree).unwrap();
        sim.hit_node(0, tree);
        assert_eq!(sim.players[0].progression.xp, progression::XP_GATHER);
        sim.players[0].progression.attribute_points = 1;
        sim.command(ids[0], Command::SpendAttribute(Attribute::Health));
        assert_eq!(sim.players[0].vitals.max.health, 110.0);
    }

    #[test]
    fn locked_recipes_are_learned_before_crafting() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let wall = RECIPES.iter().position(|r| r.output == ItemId::WoodWall).unwrap();
        sim.command(ids[0], Command::Craft(wall));
        assert!(!sim.players[0].progression.knows(ItemId::WoodWall), "nível baixo");
        sim.gain_xp(0, 5_000);
        sim.command(ids[0], Command::Craft(wall));
        assert!(sim.players[0].progression.knows(ItemId::WoodWall));
        assert!(sim.players[0].queue.jobs.is_empty(), "aprender não fabrica");
    }

    #[test]
    fn club_knocks_out_feeding_tames_and_owner_is_set() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.creatures.clear();
        sim.players[0].inventory.add(ItemId::WoodenClub, 1);
        let pos = sim.players[0].player.pos + vec2(20.0, 0.0);
        let id = sim.add_creature(Creature::new(Species::Dodo, pos));
        let mut swings = 0;
        while !sim.creature(id).unwrap().knocked_out {
            sim.hit_creature(0, 0);
            swings += 1;
            assert!(swings < 10, "a clava deveria derrubar o dodô");
        }
        sim.command(ids[0], Command::Interact);
        assert_eq!(sim.players[0].container, Some(ContainerRef::Creature(id)));
        sim.creature_mut(id)
            .unwrap()
            .inventory
            .as_mut()
            .unwrap()
            .add(ItemId::Berry, 10);
        for _ in 0..60 {
            sim.update_creatures(1.0);
        }
        let dodo = sim.creature(id).unwrap();
        assert!(dodo.is_tamed());
        assert_eq!(dodo.owner.as_deref(), Some("Ana"));
    }

    #[test]
    fn riding_and_mount_takes_bites() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.creatures.clear();
        let mut raptor = Creature::new(Species::Raptor, sim.players[0].player.pos + vec2(20.0, 0.0));
        raptor.command = Some(Order::Follow);
        let mut pack = Inventory::new(Species::Raptor.pack_slots());
        pack.add(ItemId::RaptorSaddle, 1);
        raptor.inventory = Some(pack);
        let id = sim.add_creature(raptor);
        sim.mount_creature(0, id);
        assert_eq!(sim.players[0].mount, Some(id));
        let health = sim.players[0].vitals.health;
        sim.take_bite(0, 10.0);
        assert_eq!(sim.players[0].vitals.health, health, "a montaria protege");
        sim.command(ids[0], Command::Interact);
        assert_eq!(sim.players[0].mount, None, "F desmonta");
    }

    /// Primeiro jogador montado num voador domesticado com sela, ao lado dele.
    fn mount_flyer(sim: &mut Sim, species: Species) -> u32 {
        sim.creatures.clear();
        let id = sim.demo_tamed(species, sim.players[0].player.pos, 1);
        sim.mount_creature(0, id);
        assert_eq!(sim.players[0].mount, Some(id));
        id
    }

    #[test]
    fn flying_mount_crosses_the_sea_and_lands_on_the_way_out() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let id = mount_flyer(&mut sim, Species::Pteranodon);
        sim.command(ids[0], Command::MountAction);
        assert!(sim.creature(id).unwrap().flying);
        // Voa do centro da ilha para fora (o spawn é na praia) até estar sobre o mar aberto.
        let out = (sim.players[0].player.pos - sim.world.size_px() / 2.0).normalize();
        sim.set_input(
            ids[0],
            Input {
                dir: out.into(),
                ..Input::default()
            },
        );
        for _ in 0..60 * 30 {
            sim.tick(1.0 / 60.0);
            if sim.world.tile_at(sim.players[0].player.pos) == Tile::DeepWater {
                break;
            }
        }
        let pos = sim.players[0].player.pos;
        assert_eq!(sim.world.tile_at(pos), Tile::DeepWater, "chegou ao mar: {pos}");

        // F e Espaço não descem nem pousam na água.
        sim.command(ids[0], Command::Interact);
        sim.command(ids[0], Command::MountAction);
        assert_eq!(sim.players[0].mount, Some(id));
        assert!(sim.creature(id).unwrap().flying);

        // Sair do jogo voando: a montaria pousa em terra e o jogador fica ao lado dela.
        sim.leave(ids[0]);
        let ptera = sim.creature(id).unwrap();
        assert!(!ptera.flying && Species::Pteranodon.can_stand(&sim.world, ptera.pos));
        let back = sim.join("Ana");
        let pi = sim.index(back).unwrap();
        assert!(!sim.world.blocked(sim.players[pi].player.pos, player::RADIUS));
    }

    #[test]
    fn ground_predators_ignore_riders_in_the_air() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let id = mount_flyer(&mut sim, Species::Pteranodon);
        sim.command(ids[0], Command::MountAction);
        let pos = sim.players[0].player.pos;
        let raptor = sim.add_creature(Creature::new(Species::Raptor, pos + vec2(20.0, 0.0)));
        for _ in 0..120 {
            sim.tick(1.0 / 60.0);
        }
        assert_eq!(sim.creature(id).unwrap().health_frac(), 1.0, "raptor não alcança");
        assert!(sim.creature(raptor).is_some());
        // Bate só no chão: ninguém no ar é alvo de golpe corpo a corpo.
        let mut wild = Creature::new(Species::Pteranodon, pos + vec2(10.0, 0.0));
        wild.flying = true;
        let p = Player::new(pos);
        assert_eq!(find_target(&sim.world, &[wild], &[], &p, None), None);
    }

    #[test]
    fn piranhas_bite_whoever_wades_into_the_pond() {
        let (mut sim, _) = sim_with(&["Ana"]);
        sim.creatures.clear();
        let world = &sim.world;
        let pond = (0..world.width as i32 * world.height as i32)
            .map(|i| {
                let (x, y) = (i % world.width as i32, i / world.width as i32);
                vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE)
            })
            .find(|&p| {
                [vec2(0.0, 0.0), vec2(20.0, 0.0)]
                    .iter()
                    .all(|&o| Species::Piranha.can_stand(world, p + o) && !world.blocked(p + o, player::RADIUS))
            })
            .expect("lagoa com espaço");
        sim.players[0].player.pos = pond;
        sim.add_creature(Creature::new(Species::Piranha, pond + vec2(20.0, 0.0)));
        let health = sim.players[0].vitals.health;
        for _ in 0..120 {
            sim.tick(1.0 / 60.0);
        }
        assert!(sim.players[0].vitals.health < health, "piranha mordeu");
        assert!(sim.near_water(0), "dá para beber na lagoa");
    }

    /// Primeiro jogador na beira da água ao sul do spawn, com uma jangada na mão.
    fn at_the_shore(sim: &mut Sim) -> Vec2 {
        let shore = (0..60)
            .map(|i| sim.world.spawn + vec2(0.0, i as f32 * 16.0))
            .find(|&p| raft::launch_spot(&sim.world, p).is_some() && !sim.world.blocked(p, player::RADIUS))
            .expect("praia com água");
        sim.players[0].player.pos = shore;
        sim.players[0].inventory.add(ItemId::Raft, 1);
        sim.players[0].hotbar_selected = 0;
        shore
    }

    #[test]
    fn raft_trip_out_to_sea_and_back() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.creatures.clear();
        at_the_shore(&mut sim);
        sim.command(ids[0], Command::Use);
        assert_eq!(sim.rafts.len(), 1, "jangada na água");
        assert_eq!(sim.players[0].raft, Some(sim.rafts[0].id), "já embarcado");
        assert_eq!(sim.players[0].inventory.count(ItemId::Raft), 0);

        let row = |sim: &mut Sim, dir: [f32; 2], secs: f32| {
            sim.set_input(
                ids[0],
                Input {
                    dir,
                    ..Input::default()
                },
            );
            for _ in 0..(secs * 60.0) as usize {
                sim.tick(1.0 / 60.0);
            }
        };
        row(&mut sim, [0.0, 1.0], 5.0);
        let pos = sim.players[0].player.pos;
        assert_eq!(sim.world.tile_at(pos), Tile::DeepWater, "no mar: {pos}");
        sim.command(ids[0], Command::Interact);
        assert!(sim.players[0].raft.is_some(), "não desce no meio do mar");

        // Salva com a jangada no mar e volta: a jangada continua lá e o jogador vai para a terra.
        let saved = sim.to_save();
        assert_eq!(saved.rafts.len(), 1);
        let mut loaded = Sim::from_save(saved);
        loaded.join("Ana");
        assert_eq!(loaded.rafts.len(), 1);
        assert!(!loaded.world.blocked(loaded.players[0].player.pos, player::RADIUS));

        row(&mut sim, [0.0, -1.0], 12.0);
        sim.set_input(ids[0], Input::default());
        sim.command(ids[0], Command::Interact);
        assert_eq!(sim.players[0].raft, None, "desceu na margem");
        assert!(!sim.world.blocked(sim.players[0].player.pos, player::RADIUS));
        assert_eq!(sim.rafts[0].rider, None);
        sim.command(ids[0], Command::Interact);
        assert!(sim.players[0].raft.is_some(), "F perto da jangada sobe de novo");
    }

    #[test]
    fn sea_mount_drops_its_rider_on_a_nearby_raft() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.creatures.clear();
        at_the_shore(&mut sim);
        sim.command(ids[0], Command::Use);
        sim.set_input(
            ids[0],
            Input {
                dir: [0.0, 1.0],
                ..Input::default()
            },
        );
        for _ in 0..150 {
            sim.tick(1.0 / 60.0);
        }
        sim.set_input(ids[0], Input::default());
        let raft_pos = sim.rafts[0].pos;
        let shark = sim.demo_tamed(Species::Megalodon, raft_pos + vec2(45.0, 0.0), 1);
        assert!(Species::Megalodon.can_stand(&sim.world, raft_pos + vec2(45.0, 0.0)));
        sim.mount_creature(0, shark);
        assert_eq!(sim.players[0].mount, Some(shark));
        assert_eq!(sim.players[0].raft, None, "saiu da jangada para montar");
        sim.command(ids[0], Command::Interact);
        assert_eq!(sim.players[0].mount, None);
        assert_eq!(sim.players[0].raft, Some(sim.rafts[0].id), "desceu na jangada");
    }

    #[test]
    fn cactus_quenches_thirst_and_buried_worms_cannot_be_hit() {
        let (mut sim, _) = sim_with(&["Ana"]);
        sim.creatures.clear();
        let cactus = sim.world.nodes.iter().position(|n| n.kind == NodeKind::Cactus).unwrap();
        let pos = sim.world.nodes[cactus].pos;
        sim.players[0].player.pos = pos - vec2(20.0, 0.0);
        sim.players[0].vitals.water = 10.0;
        sim.hit_node(0, cactus);
        assert!(sim.players[0].vitals.water > 10.0, "cacto dá água");
        assert!(sim.players[0].inventory.count(ItemId::Thatch) > 0);

        let mut worm = Creature::new(Species::Deathworm, pos + vec2(10.0, 0.0));
        assert!(worm.buried);
        let p = &sim.players[0].player;
        let target = find_target(&sim.world, std::slice::from_ref(&worm), &[], p, None);
        assert!(!matches!(target, Some(Target::Creature(_))), "enterrado não é alvo");
        worm.buried = false;
        let target = find_target(&sim.world, std::slice::from_ref(&worm), &[], p, None);
        assert_eq!(target, Some(Target::Creature(0)), "exposto, vira alvo");
    }

    #[test]
    fn mounted_mammoth_farms_wood_into_its_cargo() {
        let (mut sim, _) = sim_with(&["Ana"]);
        sim.creatures.clear();
        let tree = sim.world.nodes.iter().find(|n| n.kind == NodeKind::Tree).unwrap().pos;
        let id = sim.demo_tamed(Species::Mammoth, tree - vec2(30.0, 0.0), 1);
        sim.mount_creature(0, id);
        sim.creature_mut(id).unwrap().facing_left = false;
        let tree_hp = |sim: &Sim| -> u32 { sim.world.nodes.iter().map(|n| n.hp).sum() };
        let before = tree_hp(&sim);
        sim.mount_bite(0);
        assert_eq!(tree_hp(&sim), before - 1, "um golpe numa árvore");
        let cargo = sim
            .creature(id)
            .unwrap()
            .inventory
            .as_ref()
            .unwrap()
            .count(ItemId::Wood);
        assert!(cargo >= 8, "madeira na carga do mamute: {cargo}");
        assert_eq!(sim.players[0].inventory.count(ItemId::Wood), 0);
    }

    #[test]
    fn mounted_smilodon_skins_corpses() {
        let (mut sim, _) = sim_with(&["Ana"]);
        sim.creatures.clear();
        let pos = sim.players[0].player.pos;
        let mut raptor = Creature::new(Species::Raptor, pos + vec2(28.0, 0.0));
        raptor.take_hit(999.0, 0.0, pos);
        let corpse = sim.add_creature(raptor);
        let id = sim.demo_tamed(Species::Smilodon, pos, 1);
        sim.mount_creature(0, id);
        sim.creature_mut(id).unwrap().facing_left = false;
        sim.mount_bite(0);
        let hide = sim
            .creature(id)
            .unwrap()
            .inventory
            .as_ref()
            .unwrap()
            .count(ItemId::Hide);
        assert!(hide >= 6, "couro de raptor: {hide}");
        let left = sim.creature(corpse).unwrap().corpse.unwrap().harvest_left;
        assert_eq!(left, Species::Raptor.harvest_hits() - 1);
    }

    #[test]
    fn rider_sees_the_mount_cargo() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let id = mount_flyer(&mut sim, Species::Argentavis);
        let snap = sim.snapshot(ids[0]).unwrap();
        let view = snap.creatures.iter().find(|c| c.save.id == id).unwrap();
        assert!(view.save.inventory.is_some(), "carga e sobrecarga no HUD de quem monta");
    }

    #[test]
    fn overloaded_argentavis_cannot_take_off() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let id = mount_flyer(&mut sim, Species::Argentavis);
        let argy = sim.creature_mut(id).unwrap();
        let light = argy.ride_speed(RIDE_MULT, 1.0);
        argy.inventory.as_mut().unwrap().add(ItemId::Stone, 100);
        argy.inventory.as_mut().unwrap().add(ItemId::Stone, 100);
        assert!(!argy.overweight(), "argentavis aguenta 200 pedras");
        for _ in 0..3 {
            argy.inventory.as_mut().unwrap().add(ItemId::MetalIngot, 100);
        }
        assert!(argy.overweight());
        assert!(argy.ride_speed(RIDE_MULT, 1.0) < light, "fica lento");
        sim.command(ids[0], Command::MountAction);
        assert!(!sim.creature(id).unwrap().flying);
    }

    #[test]
    fn death_drops_everything_and_respawn_works() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.players[0].inventory.add(ItemId::Stone, 10);
        sim.kill();
        assert!(sim.players[0].dead);
        let bag = sim.world.bags.iter().find(|b| b.kind == BagKind::Death).unwrap();
        assert_eq!(bag.stacks[0].count, 10);
        sim.command(ids[0], Command::Respawn { bed: false });
        assert!(!sim.players[0].dead);
    }

    #[test]
    fn bed_is_a_respawn_point() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        sim.demo_fortress();
        let bed = sim
            .world
            .structures
            .iter()
            .find(|s| s.kind == StructureKind::Bed)
            .unwrap();
        assert_eq!(bed.owner.as_deref(), Some("Ana"));
        let bed_pos = bed.rect().center();
        sim.players[0].player.pos = sim.world.spawn + vec2(0.0, 300.0);
        sim.kill();
        sim.command(ids[0], Command::Respawn { bed: true });
        assert!(sim.players[0].player.pos.distance(bed_pos) < 60.0, "renasceu na cama");
        assert!(!sim.world.blocked(sim.players[0].player.pos, player::RADIUS));
    }

    #[test]
    fn hammer_repairs_with_materials_and_ruins_are_not_refunded() {
        let (mut sim, ids) = sim_with(&["Ana"]);
        let here = building::tile_of(sim.players[0].player.pos);
        let tile = (here.0 + 1, here.1);
        sim.world.place(StructureKind::StoneWall, tile);
        sim.world.structures[0].health = 100.0;
        sim.players[0].inventory.add(ItemId::Hammer, 1);
        sim.players[0].hotbar_selected = 0;
        sim.players[0].player.facing = Facing::Right;
        sim.players[0].input.attack = true;
        sim.tick(1.0 / 60.0);
        assert_eq!(sim.world.structures[0].health, 100.0, "sem pedra não repara");
        sim.players[0].inventory.add(ItemId::Stone, 40);
        sim.players[0].inventory.add(ItemId::Wood, 10);
        for _ in 0..60 {
            sim.tick(1.0 / 60.0);
        }
        sim.players[0].input.attack = false;
        assert!(sim.world.structures[0].health > 100.0, "o martelo reparou");
        assert!(sim.players[0].inventory.count(ItemId::Stone) < 40, "gastou pedra");

        // Danificada demais, desmontar não devolve.
        sim.world.structures[0].health = 50.0;
        let walls = sim.players[0].inventory.count(ItemId::StoneWall);
        sim.command(ids[0], Command::Demolish { tile });
        assert!(sim.world.structures.is_empty());
        assert_eq!(sim.players[0].inventory.count(ItemId::StoneWall), walls);
    }

    #[test]
    fn destroyed_box_drops_its_contents() {
        let (mut sim, _) = sim_with(&["Ana"]);
        let here = building::tile_of(sim.players[0].player.pos);
        sim.world.place(StructureKind::StorageBox, (here.0 + 2, here.1));
        sim.world.structures[0].storage.as_mut().unwrap().add(ItemId::Flint, 7);
        sim.damage_structure(0, 10_000.0);
        sim.tick(1.0 / 60.0);
        assert!(sim.world.structures.is_empty());
        assert!(
            sim.world
                .bags
                .iter()
                .any(|b| b.stacks.iter().any(|s| s.item == ItemId::Flint && s.count == 7))
        );
    }

    #[test]
    fn snapshot_hides_other_inventories() {
        let (mut sim, ids) = sim_with(&["Ana", "Bia"]);
        let here = building::tile_of(sim.players[0].player.pos);
        sim.world.place(StructureKind::StorageBox, (here.0 + 1, here.1));
        sim.command(ids[0], Command::Interact);
        let ana = sim.snapshot(ids[0]).unwrap();
        let bia = sim.snapshot(ids[1]).unwrap();
        assert!(ana.structures[0].storage.is_some(), "Ana está com o baú aberto");
        assert!(bia.structures[0].storage.is_none());
        assert_eq!(ana.players.len(), 2);
        assert_eq!(ana.node_hp.len(), sim.world.nodes.len());
    }
}
