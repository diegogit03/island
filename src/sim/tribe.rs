//! Tribo: moradores dos acampamentos, membros recrutados (seguir, ficar, coletar e guardar no baú,
//! defender, atacar), apitos para a tribo toda e domesticados que lutam junto com o dono
//! (contra criaturas selvagens e saqueadores).

use macroquad::math::{Vec2, vec2};

use super::Sim;
use crate::building::{self, StructureKind};
use crate::creatures::{Command as Pet, Creature, Foe, Hit, State, Temperament};
use crate::gathering;
use crate::items::ItemId;
use crate::npc::{self, GatherKind, Npc, Order};
use crate::player;
use crate::protocol::{ContainerRef, NpcCmd, PopupStyle, Whistle};
use crate::world::{BagKind, World};

/// Seguindo, o membro fica a esta distância do dono e reaparece perto dele se ficar para trás.
const FOLLOW_DISTANCE: f32 = 56.0;
const FOLLOW_TELEPORT: f32 = 800.0;
/// Seguindo, defende o dono de quem o estiver caçando nesta distância.
const ESCORT_RADIUS: f32 = 220.0;
/// Defendendo, ataca criaturas perigosas a esta distância do posto.
const DEFEND_RADIUS: f32 = 300.0;
/// Coletando, trabalha nesta distância do lugar da ordem e guarda no baú mais perto dele.
const GATHER_RADIUS: f32 = 520.0;
const HAUL_RADIUS: f32 = 800.0;
/// Alcance dos apitos.
const WHISTLE_RADIUS: f32 = 900.0;
/// Acampamentos repõem moradores recrutados de tempos em tempos.
const CAMP_REFILL_SECS: f32 = 300.0;
const RUN_MULT: f32 = 1.4;
/// Vida recuperada por segundo depois de um tempo sem apanhar.
const REGEN: f32 = 2.0;
const STUCK_SECS: f32 = 1.5;
/// Nome usado na lista de alvos da IA das criaturas (nunca coincide com o de um jogador).
pub(super) const NPC_AI_NAME: &str = "\u{1}npc";

/// Ameaça mais perto de `center`: criatura selvagem perigosa (`hunters`: só quem está caçando alguém)
/// ou saqueador (`raiders`: `id` e posição; sempre contam).
fn nearest_threat(
    creatures: &[Creature],
    raiders: &[(u32, Vec2)],
    center: Vec2,
    radius: f32,
    hunters: bool,
) -> Option<Foe> {
    creatures
        .iter()
        .filter(|c| c.is_wild() && c.is_conscious() && !c.buried && !c.flying)
        .filter(|c| c.state == State::Chase || (!hunters && c.species.temperament() == Temperament::Aggressive))
        .map(|c| (Foe::Creature(c.id), c.pos.distance(center)))
        .chain(raiders.iter().map(|&(id, pos)| (Foe::Npc(id), pos.distance(center))))
        .filter(|&(_, d)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(foe, _)| foe)
}

/// Dá para continuar batendo neste alvo? (criatura desmaiada não: ela pode estar sendo domesticada.)
fn fair_target(creatures: &[Creature], npcs: &[Npc], foe: Foe) -> bool {
    match foe {
        Foe::Creature(id) => creatures
            .iter()
            .any(|c| c.id == id && c.is_wild() && c.is_conscious() && !c.buried && !c.flying),
        Foe::Npc(id) => npcs.iter().any(|n| n.id == id && n.hostile && n.is_alive()),
    }
}

/// Anda em direção a `to`. Conta o tempo preso (andando sem sair do lugar).
fn walk(npc: &mut Npc, world: &World, to: Vec2, mult: f32, dt: f32) {
    let start = npc.body.pos;
    let dir = to - start;
    npc.body.update(dir, mult, world, dt);
    if dir != Vec2::ZERO && npc.body.pos.distance(start) < 0.01 {
        npc.stuck += dt;
    } else {
        npc.stuck = 0.0;
    }
}

/// Parado: só passa o tempo dos timers de golpe.
fn idle(npc: &mut Npc, world: &World, dt: f32) {
    npc.body.update(Vec2::ZERO, 1.0, world, dt);
    npc.stuck = 0.0;
}

impl Sim {
    pub(super) fn npc_index(&self, id: u32) -> Option<usize> {
        self.npcs.iter().position(|n| n.id == id)
    }

    fn owner_index(&self, owner: Option<&str>) -> Option<usize> {
        let owner = owner?;
        self.players.iter().position(|p| p.name == owner)
    }

    /// Posição e raio de um alvo de luta.
    pub(super) fn foe_body(&self, foe: Foe) -> Option<(Vec2, f32)> {
        match foe {
            Foe::Creature(id) => self.creature(id).map(|c| (c.pos, c.species.radius())),
            Foe::Npc(id) => self
                .npcs
                .iter()
                .find(|n| n.id == id)
                .map(|n| (n.pos(), super::NPC_HIT_RADIUS)),
        }
    }

    /// Golpe (ou mordida) de alguém da tribo de `by` num alvo. Retorna true se a luta acabou
    /// (morreu ou desmaiou).
    pub(super) fn strike(&mut self, foe: Foe, damage: f32, torpor: f32, from: Vec2, by: Option<usize>) -> bool {
        match foe {
            Foe::Creature(id) => {
                let Some(ci) = self.creatures.iter().position(|c| c.id == id) else {
                    return true;
                };
                let hit = self.creatures[ci].take_hit(damage, torpor, from);
                self.report_hit(by, ci, damage, hit);
                hit != Hit::Hurt
            }
            Foe::Npc(id) => {
                if let Some(pos) = self.npcs.iter().find(|n| n.id == id).map(|n| n.pos()) {
                    self.popup_styled(pos - vec2(0.0, 36.0), format!("-{damage:.0}"), PopupStyle::Damage);
                }
                self.hurt_npc(id, damage, by);
                !self.npcs.iter().any(|n| n.id == id && n.is_alive())
            }
        }
    }

    /// Repõe os moradores de cada acampamento até `CAMP_POPULATION`.
    pub(super) fn populate_camps(&mut self) {
        for ci in 0..self.world.camps.len() {
            let center = self.world.camps[ci].center;
            let living = self.npcs.iter().filter(|n| n.is_villager() && n.camp == ci).count();
            for _ in living..npc::CAMP_POPULATION {
                let spot = (0..12)
                    .map(|_| center + vec2(self.rng.range(-60.0, 60.0), self.rng.range(-20.0, 40.0)))
                    .find(|&p| !self.world.blocked(p, player::RADIUS));
                let Some(pos) = spot else { continue };
                let id = self.next_npc_id;
                self.next_npc_id += 1;
                self.npcs.push(Npc::new(id, pos, ci, (id % 6) as u8));
            }
        }
    }

    pub(super) fn update_npcs(&mut self, dt: f32) {
        for ni in 0..self.npcs.len() {
            let npc = &mut self.npcs[ni];
            if !npc.is_alive() {
                continue;
            }
            npc.calm += dt;
            if npc.calm > 6.0 && !npc.hostile {
                npc.health = (npc.health + REGEN * dt).min(npc::MAX_HEALTH);
            }
            if npc.hostile {
                self.think_raider(ni, dt);
            } else if npc.is_member() {
                self.think_member(ni, dt);
            } else {
                self.think_villager(ni, dt);
            }
        }
        self.camp_timer -= dt;
        if self.camp_timer <= 0.0 {
            self.camp_timer = CAMP_REFILL_SECS;
            self.populate_camps();
        }
    }

    /// Morador: passeia devagar em volta da fogueira do acampamento.
    fn think_villager(&mut self, ni: usize, dt: f32) {
        let center = self.world.camps.get(self.npcs[ni].camp).map(|c| c.center);
        let npc = &mut self.npcs[ni];
        npc.held = None;
        npc.stroll_timer -= dt;
        if npc.stroll_timer <= 0.0 {
            let home = center.unwrap_or(npc.body.pos);
            npc.stroll = home + vec2(self.rng.range(-75.0, 75.0), self.rng.range(-25.0, 45.0));
            npc.stroll_timer = self.rng.range(3.0, 7.0);
        }
        if npc.body.pos.distance(npc.stroll) > 6.0 && npc.stuck < STUCK_SECS {
            walk(npc, &self.world, npc.stroll, 0.45, dt);
        } else {
            idle(npc, &self.world, dt);
        }
    }

    fn think_member(&mut self, ni: usize, dt: f32) {
        let owner = self.npcs[ni].owner.clone();
        let owner_pos = owner
            .as_deref()
            .and_then(|o| self.players.iter().find(|p| p.name == o && !p.dead))
            .map(|p| p.player.pos);

        // Luta primeiro: alvo dado pelo apito ou ameaça por perto (conforme a ordem).
        if let Some(foe) = self.npcs[ni].target
            && !fair_target(&self.creatures, &self.npcs, foe)
        {
            self.npcs[ni].target = None;
        }
        if self.npcs[ni].target.is_none() {
            let raiders = self.raider_spots();
            let npc = &self.npcs[ni];
            let threat = match npc.order {
                Order::Follow => {
                    owner_pos.and_then(|o| nearest_threat(&self.creatures, &raiders, o, ESCORT_RADIUS, true))
                }
                Order::Defend { post } => {
                    nearest_threat(&self.creatures, &raiders, Vec2::from(post), DEFEND_RADIUS, false)
                }
                _ => nearest_threat(&self.creatures, &raiders, npc.pos(), 90.0, true),
            };
            self.npcs[ni].target = threat;
        }
        if let Some(foe) = self.npcs[ni].target {
            self.npc_fight(ni, foe, dt);
            return;
        }

        match self.npcs[ni].order {
            Order::Follow => {
                let npc = &mut self.npcs[ni];
                npc.held = npc.best_weapon();
                let Some(o) = owner_pos else {
                    idle(npc, &self.world, dt);
                    return;
                };
                let dist = o.distance(npc.body.pos);
                if dist > FOLLOW_TELEPORT {
                    // Ficou para trás (preso): aparece atrás do dono.
                    let behind = o - (o - npc.body.pos).normalize_or_zero() * FOLLOW_DISTANCE;
                    if !self.world.blocked(behind, player::RADIUS) {
                        npc.body.pos = behind;
                    }
                } else if dist > FOLLOW_DISTANCE {
                    let mult = if dist > FOLLOW_DISTANCE * 3.0 { RUN_MULT } else { 1.0 };
                    walk(npc, &self.world, o, mult, dt);
                } else {
                    idle(npc, &self.world, dt);
                }
            }
            Order::Stay { at } | Order::Defend { post: at } => {
                let npc = &mut self.npcs[ni];
                npc.held = npc.best_weapon();
                let at = Vec2::from(at);
                if npc.body.pos.distance(at) > 24.0 && npc.stuck < STUCK_SECS {
                    walk(npc, &self.world, at, 1.0, dt);
                } else {
                    idle(npc, &self.world, dt);
                }
            }
            Order::Gather { kind, area } => self.npc_gather(ni, kind, Vec2::from(area), dt),
        }
    }

    /// Persegue e golpeia o alvo com a melhor arma que tiver.
    fn npc_fight(&mut self, ni: usize, foe: Foe, dt: f32) {
        let Some((cpos, radius)) = self.foe_body(foe) else {
            self.npcs[ni].target = None;
            return;
        };
        let npc = &mut self.npcs[ni];
        let weapon = npc.best_weapon();
        npc.held = weapon;
        let reach = player::REACH + ItemId::reach_bonus(weapon);
        if cpos.distance(npc.body.pos) - radius > reach {
            walk(npc, &self.world, cpos, RUN_MULT, dt);
            if npc.stuck > STUCK_SECS * 2.0 {
                npc.target = None;
                npc.stuck = 0.0;
            }
            return;
        }
        idle(npc, &self.world, dt);
        npc.face(cpos.x);
        if npc.body.cooldown > 0.0 {
            return;
        }
        npc.body.cooldown = player::SWING_COOLDOWN;
        npc.body.swing_timer = player::SWING_TIME;
        let from = npc.body.pos;
        if let Some(w) = weapon {
            npc.wear(w);
        }
        let damage = ItemId::melee_damage(weapon);
        let torpor = ItemId::melee_torpor(weapon);
        let owner = self.owner_index(self.npcs[ni].owner.as_deref());
        if self.strike(foe, damage, torpor, from, owner) {
            self.npcs[ni].target = None;
        }
    }

    /// Coleta nós do tipo pedido perto de `area`; cheio, leva tudo para o baú mais perto da área.
    fn npc_gather(&mut self, ni: usize, kind: GatherKind, area: Vec2, dt: f32) {
        if self.npcs[ni].wants_to_haul() {
            self.npcs[ni].hauling = true;
        }
        if self.npcs[ni].hauling {
            self.npc_haul(ni, area, dt);
            return;
        }
        let node_kind = kind.node();
        let tool = self.npcs[ni].best_tool(kind.tool());
        self.npcs[ni].held = tool;
        let nodes = &self.world.nodes;
        let usable = |i: usize| {
            nodes
                .get(i)
                .is_some_and(|n| n.kind == node_kind && n.is_alive() && n.pos.distance(area) <= GATHER_RADIUS)
        };
        let npc = &self.npcs[ni];
        let work = npc.work.filter(|&i| usable(i)).or_else(|| {
            (0..nodes.len())
                .filter(|&i| usable(i) && Some(i) != npc.skip)
                .min_by(|&a, &b| {
                    let (da, db) = (nodes[a].pos.distance(npc.body.pos), nodes[b].pos.distance(npc.body.pos));
                    da.total_cmp(&db)
                })
        });
        self.npcs[ni].work = work;
        let Some(wi) = work else {
            // Nada para coletar por perto: espera no lugar da ordem (e guarda o que tiver).
            if self.npcs[ni].has_haul() {
                self.npcs[ni].hauling = true;
            } else {
                idle(&mut self.npcs[ni], &self.world, dt);
            }
            return;
        };
        let (npos, nradius) = (self.world.nodes[wi].pos, self.world.nodes[wi].kind.collision_radius());
        let npc = &mut self.npcs[ni];
        if npos.distance(npc.body.pos) - nradius > player::REACH - 4.0 {
            walk(npc, &self.world, npos, 1.0, dt);
            if npc.stuck > STUCK_SECS {
                npc.skip = npc.work.take();
                npc.stuck = 0.0;
            }
            return;
        }
        idle(npc, &self.world, dt);
        npc.face(npos.x);
        if npc.body.cooldown > 0.0 {
            return;
        }
        npc.body.cooldown = player::SWING_COOLDOWN;
        npc.body.swing_timer = player::SWING_TIME;
        if let Some(t) = tool {
            npc.wear(t);
        }
        let node = &mut self.world.nodes[wi];
        node.hp -= 1;
        node.shake = 0.2;
        if node.hp == 0 {
            node.respawn_timer = node.kind.respawn_secs();
        }
        let drops = gathering::roll_yield(
            node_kind,
            tool.and_then(ItemId::tool),
            ItemId::tool_tier(tool),
            &mut self.rng,
        );
        let npc = &mut self.npcs[ni];
        for (item, n) in drops {
            if npc.inventory.add(item, n) > 0 {
                npc.hauling = true;
            }
        }
    }

    /// Leva o que coletou até o baú mais perto da área de coleta.
    fn npc_haul(&mut self, ni: usize, area: Vec2, dt: f32) {
        let pos = self.npcs[ni].body.pos;
        if !self.npcs[ni].has_haul() {
            self.npcs[ni].hauling = false;
            return;
        }
        self.npcs[ni].held = None;
        let chest = self
            .world
            .structures
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == StructureKind::StorageBox && s.rect().center().distance(area) <= HAUL_RADIUS)
            .min_by(|a, b| {
                let (da, db) = (a.1.rect().center().distance(pos), b.1.rect().center().distance(pos));
                da.total_cmp(&db)
            })
            .map(|(i, s)| (i, s.rect()));
        let npc = &mut self.npcs[ni];
        let Some((si, rect)) = chest else {
            // Sem baú perto: espera parado, com tudo no inventário.
            idle(npc, &self.world, dt);
            return;
        };
        if building::distance_to_rect(pos, rect) > building::INTERACT_REACH {
            walk(npc, &self.world, rect.center(), 1.0, dt);
            return;
        }
        idle(npc, &self.world, dt);
        let mut haul = npc.take_haul();
        if let Some(storage) = self.world.structures[si].storage.as_mut() {
            storage.take_all(&mut haul);
        }
        // Baú cheio: o que sobrou volta para o inventário e ele espera.
        let npc = &mut self.npcs[ni];
        npc.inventory.take_all(&mut haul);
        npc.hauling = !haul.is_empty();
        if npc.hauling {
            idle(npc, &self.world, dt);
        }
    }

    /// Dano num NPC (mordida, golpe de saqueador ou, num saqueador, de quem defende a base).
    /// `by`: jogador que bateu (ou dono de quem bateu). Quem morre sai no fim do tick (`reap_npcs`).
    pub(super) fn hurt_npc(&mut self, id: u32, damage: f32, by: Option<usize>) {
        let Some(ni) = self.npc_index(id) else { return };
        let npc = &mut self.npcs[ni];
        if !npc.is_alive() {
            return;
        }
        npc.health -= damage;
        npc.calm = 0.0;
        if by.is_some() {
            npc.last_hit = by;
        }
        let (pos, hostile) = (npc.body.pos, npc.hostile);
        if !hostile {
            self.popup_styled(pos - vec2(0.0, 36.0), format!("-{damage:.0}"), PopupStyle::Hurt);
        }
    }

    /// Tira do mundo os NPCs mortos: membro deixa a bolsa com o inventário; saqueador deixa o que
    /// carregava e dá XP para quem o abateu.
    pub(super) fn reap_npcs(&mut self) {
        let mut ni = 0;
        while ni < self.npcs.len() {
            if self.npcs[ni].is_alive() {
                ni += 1;
                continue;
            }
            let mut npc = self.npcs.remove(ni);
            let stacks = npc.inventory.drain_all();
            if npc.hostile {
                self.world.drop_bag(npc.body.pos, stacks, BagKind::Dropped);
                if let Some(pi) = npc.last_hit.filter(|&pi| pi < self.players.len()) {
                    self.gain_xp(pi, super::raid::XP_RAIDER);
                    self.notify(pi, "Saqueador abatido");
                }
                continue;
            }
            self.world.drop_bag(npc.body.pos, stacks, BagKind::Death);
            if let Some(pi) = self.owner_index(npc.owner.as_deref()) {
                self.notify(pi, format!("{} morreu", npc.name));
            }
        }
    }

    /// F num NPC: abre o painel (recrutar, ou ordens e inventário se for da sua tribo).
    pub(super) fn talk_to(&mut self, pi: usize, id: u32) {
        let Some(ni) = self.npc_index(id) else { return };
        let npc = &self.npcs[ni];
        if npc.hostile {
            return;
        }
        if let Some(owner) = npc.owner.as_ref().filter(|o| **o != self.players[pi].name) {
            let text = format!("{} é da tribo de {owner}", npc.name);
            self.notify(pi, text);
            return;
        }
        self.close_container(pi);
        self.players[pi].container = Some(ContainerRef::Npc(id));
    }

    pub(super) fn npc_order(&mut self, pi: usize, cmd: NpcCmd) {
        let Some(ContainerRef::Npc(id)) = self.players[pi].container else {
            return;
        };
        let Some(ni) = self.npc_index(id) else { return };
        if cmd == NpcCmd::Recruit {
            self.recruit(pi, ni);
            return;
        }
        let name = &self.players[pi].name;
        let npc = &mut self.npcs[ni];
        if npc.owner.as_ref() != Some(name) {
            return;
        }
        let here = npc.body.pos.into();
        npc.order = match cmd {
            NpcCmd::Follow => Order::Follow,
            NpcCmd::Stay => Order::Stay { at: here },
            NpcCmd::Defend => Order::Defend { post: here },
            NpcCmd::Gather(kind) => Order::Gather { kind, area: here },
            NpcCmd::Recruit => return,
        };
        npc.target = None;
        npc.work = None;
        npc.skip = None;
        npc.hauling = false;
        if let NpcCmd::Gather(kind) = cmd
            && npc.best_tool(kind.tool()).is_none()
        {
            let text = format!("{} vai coletar com as mãos (dê um machado ou picareta)", npc.name);
            self.notify(pi, text);
        }
    }

    fn recruit(&mut self, pi: usize, ni: usize) {
        if self.npcs[ni].owner.is_some() {
            return;
        }
        let name = self.players[pi].name.clone();
        let members = self.npcs.iter().filter(|n| n.owner.as_ref() == Some(&name)).count();
        if members >= npc::MAX_TRIBE {
            self.notify(pi, format!("Sua tribo já tem {} membros", npc::MAX_TRIBE));
            return;
        }
        let (item, n) = self.npcs[ni].recruit_cost();
        if !self.players[pi].inventory.remove(item, n) {
            self.notify(pi, format!("{} pede {n} {}", self.npcs[ni].name, item.name()));
            return;
        }
        let npc = &mut self.npcs[ni];
        npc.owner = Some(name);
        npc.order = Order::Follow;
        let text = format!("{} entrou na sua tribo!", npc.name);
        self.notify(pi, text);
        self.gain_xp(pi, 25);
    }

    /// Apito: ordem para todos os membros e domesticados do jogador por perto.
    pub(super) fn whistle(&mut self, pi: usize, whistle: Whistle) {
        let (name, pos) = (self.players[pi].name.clone(), self.players[pi].player.pos);
        let target = match whistle {
            Whistle::Attack { at } => {
                let at = Vec2::from(at);
                let found = self
                    .creatures
                    .iter()
                    .filter(|c| c.is_wild() && c.is_conscious() && !c.buried && !c.flying)
                    .map(|c| (Foe::Creature(c.id), c.pos.distance(at) - c.species.radius()))
                    .chain(
                        self.raider_spots()
                            .into_iter()
                            .map(|(id, p)| (Foe::Npc(id), p.distance(at) - super::NPC_HIT_RADIUS)),
                    )
                    .filter(|&(_, d)| d <= 60.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(foe, _)| foe);
                let Some(foe) = found else {
                    self.notify(pi, "Aponte o mouse para uma criatura selvagem ou um saqueador");
                    return;
                };
                Some(foe)
            }
            _ => None,
        };
        let mut count = 0;
        for npc in self
            .npcs
            .iter_mut()
            .filter(|n| n.owner.as_ref() == Some(&name) && n.body.pos.distance(pos) <= WHISTLE_RADIUS)
        {
            let here = npc.body.pos.into();
            match whistle {
                Whistle::Follow => npc.order = Order::Follow,
                Whistle::Stay => npc.order = Order::Stay { at: here },
                Whistle::Defend => npc.order = Order::Defend { post: here },
                Whistle::Attack { .. } => {}
            }
            npc.target = target;
            npc.hauling = false;
            count += 1;
        }
        for c in self.creatures.iter_mut().filter(|c| {
            c.is_tamed() && !c.mounted && c.owner.as_ref() == Some(&name) && c.pos.distance(pos) <= WHISTLE_RADIUS
        }) {
            match whistle {
                Whistle::Follow => c.command = Some(Pet::Follow),
                Whistle::Stay => c.command = Some(Pet::Stay),
                Whistle::Defend => {
                    c.command = Some(Pet::Defend);
                    c.home = Some(c.pos);
                }
                Whistle::Attack { .. } if c.species.attack().is_none() => continue,
                Whistle::Attack { .. } => {}
            }
            c.attack_target = target;
            count += 1;
        }
        let label = match whistle {
            Whistle::Follow => "Seguir",
            Whistle::Stay => "Ficar",
            Whistle::Defend => "Defender",
            Whistle::Attack { .. } => "Atacar",
        };
        if count == 0 {
            self.notify(pi, "Ninguém da tribo por perto");
        } else {
            self.notify(pi, format!("{label}: {count} da tribo"));
        }
    }

    /// Domesticados que mordem lutam: atacam o alvo do apito, defendem o dono (seguindo) ou o posto.
    pub(super) fn update_tamed_combat(&mut self, dt: f32) {
        let raiders = self.raider_spots();
        for ci in 0..self.creatures.len() {
            let c = &self.creatures[ci];
            let Some((_, _, range, _)) = c.species.attack() else {
                continue;
            };
            if !c.is_tamed() || !c.is_conscious() || c.mounted || c.flying {
                continue;
            }
            if let Some(foe) = c.attack_target
                && !fair_target(&self.creatures, &self.npcs, foe)
            {
                self.creatures[ci].attack_target = None;
            }
            let c = &self.creatures[ci];
            let owner_pos = c
                .owner
                .as_deref()
                .and_then(|o| self.players.iter().find(|p| p.name == o && !p.dead))
                .map(|p| p.player.pos);
            let home = c.home.unwrap_or(c.pos);
            if c.attack_target.is_none() {
                let threat = match c.command {
                    Some(Pet::Follow) => {
                        owner_pos.and_then(|o| nearest_threat(&self.creatures, &raiders, o, ESCORT_RADIUS, true))
                    }
                    Some(Pet::Defend) => nearest_threat(&self.creatures, &raiders, home, DEFEND_RADIUS, false),
                    _ => None,
                };
                self.creatures[ci].attack_target = threat;
            }
            let c = &self.creatures[ci];
            let Some((foe, (tpos, tradius))) = c.attack_target.and_then(|f| self.foe_body(f).map(|b| (f, b))) else {
                // Defendendo e sem inimigo: volta para o posto.
                if c.command == Some(Pet::Defend) && c.pos.distance(home) > 40.0 {
                    let (dir, speed) = (home - c.pos, c.species.speeds().0.max(90.0));
                    self.creatures[ci].step(&self.world, dir, speed, dt);
                }
                continue;
            };
            let (pos, radius) = (c.pos, c.species.radius());
            if tpos.distance(pos) - tradius - radius > range {
                let speed = c.species.speeds().1.max(150.0);
                self.creatures[ci].step(&self.world, tpos - pos, speed, dt);
                continue;
            }
            if !self.creatures[ci].ready_to_bite() {
                continue;
            }
            self.creatures[ci].facing_left = tpos.x < pos.x;
            let damage = self.creatures[ci].bite();
            let owner = self.owner_index(self.creatures[ci].owner.as_deref());
            if self.strike(foe, damage, 0.0, pos, owner) {
                self.creatures[ci].attack_target = None;
            }
        }
    }

    /// Jogador dono de cada membro da tribo (para a lista de alvos da IA).
    pub(super) fn member_ids(&self) -> Vec<u32> {
        self.npcs.iter().filter(|n| n.is_member()).map(|n| n.id).collect()
    }

    /// Primeiro jogador num acampamento com dois membros da tribo (um coletando madeira perto de um
    /// baú), moradores em volta e um raptor domesticado defendendo.
    pub fn demo_tribe(&mut self) {
        let Some(camp) = self.world.camps.first().cloned() else {
            return;
        };
        self.creatures.clear();
        let base = camp.center + vec2(0.0, 110.0);
        self.players[0].player.pos = base;
        let name = self.players[0].name.clone();
        let ids: Vec<u32> = self.npcs.iter().filter(|n| n.camp == 0).map(|n| n.id).take(2).collect();
        let offsets = [vec2(-36.0, 18.0), vec2(40.0, 30.0)];
        for (k, id) in ids.iter().enumerate() {
            let Some(ni) = self.npc_index(*id) else { continue };
            let npc = &mut self.npcs[ni];
            npc.owner = Some(name.clone());
            npc.order = Order::Follow;
            npc.body.pos = base + offsets[k];
            npc.inventory.add(
                if k == 0 {
                    ItemId::WoodenSpear
                } else {
                    ItemId::StoneHatchet
                },
                1,
            );
            npc.held = npc.best_weapon();
        }
        self.populate_camps();
        let raptor = self.demo_tamed(crate::creatures::Species::Raptor, base + vec2(90.0, -10.0), 10);
        if let Some(c) = self.creature_mut(raptor) {
            c.command = Some(Pet::Defend);
            c.home = Some(c.pos);
        }
    }

    /// Painel de ordens e inventário de um membro da tribo.
    pub fn demo_member(&mut self) {
        self.demo_tribe();
        let name = self.players[0].name.clone();
        let Some(id) = self.npcs.iter().find(|n| n.owner.as_ref() == Some(&name)).map(|n| n.id) else {
            return;
        };
        if let Some(ni) = self.npc_index(id) {
            self.npcs[ni].inventory.add(ItemId::Wood, 34);
            self.npcs[ni].order = Order::Gather {
                kind: GatherKind::Wood,
                area: self.npcs[ni].body.pos.into(),
            };
            self.players[0].player.pos = self.npcs[ni].body.pos + vec2(30.0, 0.0);
        }
        self.players[0].container = Some(ContainerRef::Npc(id));
    }

    /// Painel de recrutamento aberto num morador do primeiro acampamento.
    pub fn demo_recruit(&mut self) {
        self.demo_tribe();
        let Some(id) = self
            .npcs
            .iter()
            .find(|n| n.owner.is_none() && n.camp == 0)
            .map(|n| n.id)
        else {
            return;
        };
        if let Some(ni) = self.npc_index(id) {
            let (item, n) = self.npcs[ni].recruit_cost();
            self.players[0].inventory.add(item, n);
            self.players[0].player.pos = self.npcs[ni].body.pos + vec2(30.0, 0.0);
        }
        self.players[0].container = Some(ContainerRef::Npc(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creatures::Species;
    use crate::protocol::{Command, PlayerId};

    fn with_member() -> (Sim, PlayerId, u32) {
        let mut sim = Sim::new(1337);
        let id = sim.join("Ana");
        sim.creatures.clear();
        let npc = sim.npcs.iter().find(|n| n.owner.is_none()).map(|n| n.id).unwrap();
        let ni = sim.npc_index(npc).unwrap();
        let (item, n) = sim.npcs[ni].recruit_cost();
        sim.players[0].player.pos = sim.npcs[ni].body.pos + vec2(20.0, 0.0);
        sim.command(id, Command::Interact);
        assert_eq!(
            sim.players[0].container,
            Some(ContainerRef::Npc(npc)),
            "F abre o painel do morador"
        );
        sim.command(id, Command::NpcOrder(NpcCmd::Recruit));
        assert!(sim.npcs[ni].owner.is_none(), "sem pagar não entra");
        sim.players[0].inventory.add(item, n);
        sim.command(id, Command::NpcOrder(NpcCmd::Recruit));
        assert_eq!(sim.npcs[ni].owner.as_deref(), Some("Ana"));
        assert_eq!(sim.players[0].inventory.count(item), 0, "pagou");
        (sim, id, npc)
    }

    fn run(sim: &mut Sim, secs: f32) {
        for _ in 0..(secs * 60.0) as usize {
            sim.tick(1.0 / 60.0);
        }
    }

    #[test]
    fn camps_have_villagers_that_can_be_recruited_and_follow() {
        let (mut sim, _, npc) = with_member();
        assert!(sim.world.camps.len() >= 4);
        for ci in 0..sim.world.camps.len() {
            let n = sim.npcs.iter().filter(|n| n.camp == ci).count();
            assert!(n >= npc::CAMP_POPULATION, "acampamento {ci} com moradores");
        }
        let target = sim.players[0].player.pos + vec2(200.0, 0.0);
        let free = (0..40)
            .map(|i| target + vec2(0.0, i as f32 * 8.0))
            .find(|&p| !sim.world.blocked(p, player::RADIUS))
            .unwrap();
        sim.players[0].player.pos = free;
        run(&mut sim, 4.0);
        let ni = sim.npc_index(npc).unwrap();
        assert!(sim.npcs[ni].body.pos.distance(free) < 120.0, "seguiu o dono");

        // Salva e carrega: continua na tribo.
        let loaded = Sim::from_save(sim.to_save());
        let back = loaded.npcs.iter().find(|n| n.id == npc).unwrap();
        assert_eq!(back.owner.as_deref(), Some("Ana"));
    }

    #[test]
    fn member_gathers_wood_and_stores_it_in_the_box() {
        let (mut sim, id, npc) = with_member();
        // Um bosque longe dos acampamentos, com um baú do lado.
        let world = &sim.world;
        let tree = world
            .nodes
            .iter()
            .filter(|n| n.kind == crate::world::NodeKind::Tree)
            .map(|n| n.pos)
            .find(|&p| {
                world.camps.iter().all(|c| c.center.distance(p) > 300.0) && !world.blocked(p + vec2(-30.0, 0.0), 7.0)
            })
            .unwrap();
        let tile = building::tile_of(tree + vec2(-30.0, 40.0));
        sim.world.place(StructureKind::StorageBox, tile);
        let ni = sim.npc_index(npc).unwrap();
        sim.npcs[ni].body.pos = tree + vec2(-30.0, 0.0);
        sim.npcs[ni].inventory.add(ItemId::MetalHatchet, 1);
        sim.players[0].player.pos = tree + vec2(-40.0, 0.0);
        sim.command(id, Command::Interact);
        assert_eq!(sim.players[0].container, Some(ContainerRef::Npc(npc)));
        sim.command(id, Command::NpcOrder(NpcCmd::Gather(GatherKind::Wood)));
        sim.command(id, Command::CloseContainer);
        sim.players[0].player.pos = tree + vec2(2000.0, 0.0);
        run(&mut sim, 60.0);
        let si = sim.world.structure_at(tile, StructureKind::StorageBox.layer()).unwrap();
        let wood = sim.world.structures[si].storage.as_ref().unwrap().count(ItemId::Wood);
        assert!(wood > 20, "madeira guardada no baú: {wood}");
    }

    #[test]
    fn whistle_attack_sends_members_and_pets_after_the_target() {
        let (mut sim, id, npc) = with_member();
        let ni = sim.npc_index(npc).unwrap();
        sim.npcs[ni].inventory.add(ItemId::MetalSword, 1);
        let pos = sim.players[0].player.pos;
        let raptor = sim.demo_tamed(Species::Raptor, pos + vec2(-20.0, 20.0), 10);
        let dodo = sim.add_creature(Creature::new(Species::Dodo, pos + vec2(60.0, 0.0)));
        sim.command(
            id,
            Command::Whistle(Whistle::Attack {
                at: (pos + vec2(60.0, 0.0)).into(),
            }),
        );
        assert_eq!(sim.npcs[ni].target, Some(Foe::Creature(dodo)));
        assert_eq!(sim.creature(raptor).unwrap().attack_target, Some(Foe::Creature(dodo)));
        run(&mut sim, 6.0);
        assert!(sim.creature(dodo).is_none_or(|d| !d.is_alive()), "o dodô caiu");
    }

    #[test]
    fn wild_predators_hunt_tribe_members_too() {
        let (mut sim, _, npc) = with_member();
        let ni = sim.npc_index(npc).unwrap();
        let spot = sim.npcs[ni].body.pos;
        sim.npcs[ni].order = Order::Stay { at: spot.into() };
        sim.players[0].player.pos = spot + vec2(3000.0, 0.0);
        let raptor = Creature::new(Species::Raptor, spot + vec2(30.0, 0.0));
        sim.add_creature(raptor);
        run(&mut sim, 2.0);
        let health = sim.npc_index(npc).map_or(0.0, |i| sim.npcs[i].health);
        assert!(health < npc::MAX_HEALTH, "o raptor mordeu o membro da tribo");
    }
}
