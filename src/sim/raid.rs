//! Saqueadores: grupos de NPCs hostis que aparecem à noite perto da base de quem está jogando,
//! quebram o que estiver no caminho e atacam jogadores, membros da tribo e domesticados.
//! Vão embora quando amanhece ou quando não sobra nada para quebrar. Não vão para o save.

use macroquad::math::{Vec2, vec2};

use super::Sim;
use crate::building::Layer;
use crate::creatures::Hit;
use crate::daynight;
use crate::items::ItemId;
use crate::npc::Npc;
use crate::player;

/// Primeiro ataque possível (segundos de jogo) e intervalo entre ataques.
pub(super) const FIRST_RAID: f32 = 900.0;
const RAID_INTERVAL: f32 = 1200.0;
/// Sem base ou sem noite: tenta de novo depois disto.
const RAID_RETRY: f32 = 30.0;
/// Peças de construção perto de um jogador para contar como base.
const BASE_PIECES: usize = 10;
const BASE_RADIUS: f32 = 400.0;
/// Os saqueadores aparecem a esta distância da base, longe de todo mundo.
const SPAWN_DISTANCE: f32 = 620.0;
const SPAWN_CLEARANCE: f32 = 450.0;
/// Atacam quem chegar a esta distância.
const AGGRO: f32 = 230.0;
/// Depois disto (ou ao amanhecer), vão embora.
const LIFETIME: f32 = 540.0;
/// Indo embora, somem quando ninguém está vendo.
const DESPAWN_DISTANCE: f32 = 900.0;
/// Golpes em estruturas: mais lentos e mais fracos que em gente.
const SMASH_COOLDOWN: f32 = 0.9;
const SMASH_MULT: f32 = 0.6;
const HIT_MULT: f32 = 0.8;
const RUN_MULT: f32 = 1.3;
pub(super) const XP_RAIDER: u32 = 40;

/// Quem um saqueador está atacando.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Victim {
    Player(usize),
    Npc(u32),
    Creature(u32),
}

impl Sim {
    /// Noite e alguém com base por perto: chegam saqueadores.
    pub(super) fn update_raids(&mut self, dt: f32) {
        self.raid_timer -= dt;
        if self.raid_timer > 0.0 {
            return;
        }
        self.raid_timer = RAID_RETRY;
        let night = daynight::darkness(daynight::hour(self.time)) > 0.5;
        if !night || self.npcs.iter().any(|n| n.hostile) {
            return;
        }
        if let Some((center, pieces)) = self.find_base()
            && self.start_raid(center, pieces)
        {
            self.raid_timer = RAID_INTERVAL + self.rng.range(0.0, 300.0);
        }
    }

    /// Maior base perto de um jogador vivo: (centro, peças).
    fn find_base(&self) -> Option<(Vec2, usize)> {
        self.players
            .iter()
            .filter(|p| !p.dead)
            .filter_map(|p| {
                let pieces: Vec<Vec2> = self
                    .world
                    .structures
                    .iter()
                    .filter(|s| s.kind.supports_roof() && s.rect().center().distance(p.player.pos) <= BASE_RADIUS)
                    .map(|s| s.rect().center())
                    .collect();
                (pieces.len() >= BASE_PIECES)
                    .then(|| (pieces.iter().copied().sum::<Vec2>() / pieces.len() as f32, pieces.len()))
            })
            .max_by_key(|&(_, n)| n)
    }

    /// Põe um grupo de saqueadores fora de vista e avisa quem está perto da base.
    pub(super) fn start_raid(&mut self, center: Vec2, pieces: usize) -> bool {
        let count = (2 + pieces / 30).min(5);
        let mut spot = None;
        for _ in 0..40 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let dist = self.rng.range(SPAWN_DISTANCE, SPAWN_DISTANCE + 120.0);
            let p = center + vec2(angle.cos(), angle.sin()) * dist;
            let clear = self
                .players
                .iter()
                .all(|pl| pl.player.pos.distance(p) > SPAWN_CLEARANCE);
            if clear && self.world.tile_at(p).is_land() && !self.world.blocked(p, player::RADIUS) {
                spot = Some(p);
                break;
            }
        }
        let Some(spot) = spot else { return false };
        for k in 0..count {
            let offset = vec2((k as f32 - count as f32 / 2.0) * 18.0, self.rng.range(-12.0, 12.0));
            let pos = Some(spot + offset)
                .filter(|&p| !self.world.blocked(p, player::RADIUS))
                .unwrap_or(spot);
            let weapon = self.raider_weapon(pieces);
            let loot = self.raider_loot();
            let id = self.next_npc_id;
            self.next_npc_id += 1;
            self.npcs.push(Npc::raider(id, pos, center, weapon, &loot));
        }
        for pi in 0..self.players.len() {
            if self.players[pi].player.pos.distance(center) < 1500.0 {
                self.notify(pi, "Saqueadores estão vindo atacar a base!");
            }
        }
        true
    }

    /// Bases maiores atraem saqueadores mais bem armados.
    fn raider_weapon(&mut self, pieces: usize) -> ItemId {
        let roll = self.rng.range(0.0, 1.0) + (pieces as f32 / 200.0).min(0.3);
        match roll {
            r if r < 0.35 => ItemId::WoodenSpear,
            r if r < 0.65 => ItemId::StoneHatchet,
            r if r < 0.85 => ItemId::StonePickaxe,
            r if r < 1.05 => ItemId::MetalHatchet,
            _ => ItemId::MetalSword,
        }
    }

    fn raider_loot(&mut self) -> Vec<(ItemId, u32)> {
        let mut loot = vec![
            (ItemId::Flint, self.rng.range(2.0, 7.0) as u32),
            (ItemId::Hide, self.rng.range(1.0, 5.0) as u32),
        ];
        if self.rng.range(0.0, 1.0) < 0.5 {
            loot.push((ItemId::CookedMeat, self.rng.range(1.0, 4.0) as u32));
        }
        if self.rng.range(0.0, 1.0) < 0.3 {
            loot.push((ItemId::MetalIngot, self.rng.range(1.0, 3.0) as u32));
        }
        loot
    }

    /// Saqueador: ataca quem chegar perto; senão quebra o caminho até a base e o que houver nela.
    pub(super) fn think_raider(&mut self, ni: usize, dt: f32) {
        let day = daynight::darkness(daynight::hour(self.time)) < 0.3;
        let npc = &mut self.npcs[ni];
        npc.life += dt;
        npc.stroll_timer -= dt;
        if !npc.retreat && (npc.life > LIFETIME || (day && npc.life > 60.0)) {
            npc.retreat = true;
        }
        npc.held = npc.best_weapon();
        if let Some(victim) = self.raider_victim(ni) {
            self.raider_fight(ni, victim, dt);
            return;
        }
        let (pos, raid) = (self.npcs[ni].pos(), self.npcs[ni].raid);
        if self.npcs[ni].retreat {
            let far = self
                .players
                .iter()
                .all(|p| p.player.pos.distance(pos) > DESPAWN_DISTANCE);
            if far {
                // Some sem deixar nada.
                let npc = &mut self.npcs[ni];
                npc.inventory.drain_all();
                npc.health = 0.0;
                npc.last_hit = None;
                return;
            }
            let away = pos + (pos - raid).normalize_or(Vec2::X) * 200.0;
            self.raider_walk(ni, away, 1.0, dt);
            return;
        }
        // Chegou na base: quebra a estrutura mais perto (a não ser pisos e tetos).
        let goal = if pos.distance(raid) < 40.0 {
            let target = self
                .world
                .structures
                .iter()
                .filter(|s| s.kind.layer() == Layer::Object && s.health > 0.0)
                .map(|s| (s.rect(), crate::building::distance_to_rect(pos, s.rect())))
                .filter(|&(_, d)| d <= 260.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((rect, _)) = target else {
                self.npcs[ni].retreat = true;
                return;
            };
            rect.center()
        } else {
            raid
        };
        self.raider_walk(ni, goal, 1.0, dt);
    }

    /// Anda até `to`; o que estiver no caminho (parede, porta, cerca...) apanha.
    /// Preso em árvore ou pedra, desvia um pouco para o lado.
    fn raider_walk(&mut self, ni: usize, to: Vec2, mult: f32, dt: f32) {
        let pos = self.npcs[ni].pos();
        if let Some(si) = self.world.structure_in_way(pos, to - pos, player::RADIUS) {
            self.raider_smash(ni, si, dt);
            return;
        }
        let npc = &mut self.npcs[ni];
        let goal = if npc.stroll_timer > 0.0 { npc.stroll } else { to };
        let start = npc.body.pos;
        npc.body.update(goal - start, mult, &self.world, dt);
        if npc.body.pos.distance(start) < 0.01 {
            npc.stuck += dt;
        } else {
            npc.stuck = 0.0;
        }
        if npc.stuck > 1.0 {
            let dir = (to - start).normalize_or(Vec2::X);
            let side = if self.rng.range(0.0, 1.0) < 0.5 { 1.0 } else { -1.0 };
            npc.stroll = start + vec2(-dir.y, dir.x) * side * 70.0 - dir * 20.0;
            npc.stroll_timer = 1.2;
            npc.stuck = 0.0;
        }
    }

    fn raider_smash(&mut self, ni: usize, si: usize, dt: f32) {
        let rect = self.world.structures[si].rect();
        let npc = &mut self.npcs[ni];
        npc.body.update(Vec2::ZERO, 1.0, &self.world, dt);
        npc.face(rect.center().x);
        if npc.body.cooldown > 0.0 {
            return;
        }
        npc.body.cooldown = SMASH_COOLDOWN;
        npc.body.swing_timer = player::SWING_TIME;
        let damage = ItemId::melee_damage(npc.held) * SMASH_MULT;
        self.damage_structure(si, damage);
    }

    /// Alvo mais perto ao alcance: jogador (a pé, montado ou de jangada), membro da tribo ou domesticado.
    fn raider_victim(&self, ni: usize) -> Option<Victim> {
        let pos = self.npcs[ni].pos();
        let flying: Vec<u32> = self.creatures.iter().filter(|c| c.flying).map(|c| c.id).collect();
        let players = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.dead && !p.mount.is_some_and(|id| flying.contains(&id)))
            .map(|(i, p)| (Victim::Player(i), p.player.pos));
        let members = self
            .npcs
            .iter()
            .filter(|n| n.is_member() && n.is_alive())
            .map(|n| (Victim::Npc(n.id), n.pos()));
        let pets = self
            .creatures
            .iter()
            .filter(|c| c.is_tamed() && c.is_alive() && !c.mounted && !c.flying)
            .map(|c| (Victim::Creature(c.id), c.pos));
        players
            .chain(members)
            .chain(pets)
            .map(|(v, p)| (v, p.distance(pos)))
            .filter(|&(_, d)| d <= AGGRO)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(v, _)| v)
    }

    fn raider_fight(&mut self, ni: usize, victim: Victim, dt: f32) {
        let body = match victim {
            Victim::Player(pi) => Some((self.players[pi].player.pos, player::RADIUS)),
            Victim::Npc(id) => self.npcs.iter().find(|n| n.id == id).map(|n| (n.pos(), player::RADIUS)),
            Victim::Creature(id) => self.creature(id).map(|c| (c.pos, c.species.radius())),
        };
        let Some((vpos, vradius)) = body else { return };
        let npc = &self.npcs[ni];
        let weapon = npc.held;
        let reach = player::REACH + ItemId::reach_bonus(weapon);
        if vpos.distance(npc.pos()) - vradius > reach {
            self.raider_walk(ni, vpos, RUN_MULT, dt);
            return;
        }
        let npc = &mut self.npcs[ni];
        npc.body.update(Vec2::ZERO, 1.0, &self.world, dt);
        npc.face(vpos.x);
        if npc.body.cooldown > 0.0 {
            return;
        }
        npc.body.cooldown = player::SWING_COOLDOWN * 1.6;
        npc.body.swing_timer = player::SWING_TIME;
        let from = npc.body.pos;
        let damage = ItemId::melee_damage(weapon) * HIT_MULT;
        match victim {
            Victim::Player(pi) => self.take_bite(pi, damage),
            Victim::Npc(id) => self.hurt_npc(id, damage, None),
            Victim::Creature(id) => {
                let Some(ci) = self.creatures.iter().position(|c| c.id == id) else {
                    return;
                };
                let hit = self.creatures[ci].take_hit(damage, 0.0, from);
                let c = &self.creatures[ci];
                let (pos, name, owner) = (c.pos, c.species.name(), c.owner.clone());
                self.popup_styled(
                    pos - vec2(0.0, 30.0),
                    format!("-{damage:.0}"),
                    crate::protocol::PopupStyle::Hurt,
                );
                if hit == Hit::Died {
                    self.on_tamed_death(id);
                    if let Some(pi) = self.players.iter().position(|p| Some(&p.name) == owner.as_ref()) {
                        self.notify(pi, format!("Saqueadores mataram seu {name}!"));
                    }
                }
            }
        }
    }

    /// Noite, com saqueadores atacando uma cabana de madeira e dois membros da tribo defendendo.
    pub fn demo_raid(&mut self) {
        self.creatures.clear();
        self.build_demo_base();
        self.skip_to_hour(22.0);
        let (sx, sy) = crate::building::tile_of(self.world.spawn);
        let hut = crate::building::tile_rect((sx, sy - 3)).center();
        self.players[0].player.pos = hut + vec2(30.0, 80.0);
        let id = self.next_npc_id;
        self.next_npc_id += 1;
        let mut raider = Npc::raider(id, hut + vec2(-120.0, -10.0), hut, ItemId::StoneHatchet, &[]);
        raider.life = 30.0;
        self.npcs.push(raider);
        let id = self.next_npc_id;
        self.next_npc_id += 1;
        self.npcs
            .push(Npc::raider(id, hut + vec2(-64.0, 40.0), hut, ItemId::WoodenSpear, &[]));
        // Parede da esquerda já apanhando.
        if let Some(i) = self.world.structure_at((sx - 2, sy - 3), Layer::Object) {
            self.world.structures[i].health *= 0.45;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::StructureKind;

    fn run(sim: &mut Sim, secs: f32) {
        for _ in 0..(secs * 60.0) as usize {
            sim.tick(1.0 / 60.0);
        }
    }

    #[test]
    fn raid_comes_at_night_to_a_base_and_breaks_walls() {
        let mut sim = Sim::new(1337);
        sim.join("Ana");
        sim.creatures.clear();
        sim.build_demo_base();
        let (sx, sy) = crate::building::tile_of(sim.world.spawn);
        let hut = crate::building::tile_rect((sx, sy - 3)).center();
        sim.players[0].player.pos = hut;
        let pieces_before: f32 = sim.world.structures.iter().map(|s| s.health).sum();

        // De dia, nada acontece.
        sim.skip_to_hour(12.0);
        sim.raid_timer = 0.0;
        run(&mut sim, 1.0);
        assert!(!sim.npcs.iter().any(|n| n.hostile), "de dia não tem ataque");

        sim.skip_to_hour(22.0);
        sim.raid_timer = 0.0;
        run(&mut sim, 1.0);
        let raiders = sim.npcs.iter().filter(|n| n.hostile).count();
        assert!(raiders >= 2, "chegaram saqueadores: {raiders}");
        // O jogador sai para os saqueadores irem direto à cabana.
        sim.players[0].player.pos = hut + vec2(3000.0, 0.0);
        run(&mut sim, 40.0);
        let pieces_after: f32 = sim.world.structures.iter().map(|s| s.health).sum();
        assert!(pieces_after < pieces_before, "a base apanhou");

        // Saqueadores não vão para o save.
        let loaded = Sim::from_save(sim.to_save());
        assert!(!loaded.npcs.iter().any(|n| n.hostile));
    }

    #[test]
    fn players_and_members_fight_raiders_who_drop_loot() {
        let mut sim = Sim::new(1337);
        sim.join("Ana");
        sim.creatures.clear();
        let spot = sim.world.spawn;
        sim.players[0].player.pos = spot;
        sim.players[0].inventory.add(ItemId::MetalSword, 1);
        let id = sim.next_npc_id;
        sim.next_npc_id += 1;
        let raider = Npc::raider(
            id,
            spot + vec2(24.0, 0.0),
            spot,
            ItemId::WoodenSpear,
            &[(ItemId::Flint, 5)],
        );
        sim.npcs.push(raider);
        let health = sim.players[0].vitals.health;
        run(&mut sim, 1.5);
        assert!(sim.players[0].vitals.health < health, "o saqueador bate no jogador");

        // Com a espada, o jogador derruba o saqueador e pega o que ele levava.
        sim.players[0].hotbar_selected = 0;
        sim.players[0].vitals.health = sim.players[0].vitals.max.health;
        sim.players[0].input.attack = true;
        run(&mut sim, 6.0);
        sim.players[0].input.attack = false;
        assert!(!sim.npcs.iter().any(|n| n.id == id), "saqueador abatido");
        assert!(
            sim.world
                .bags
                .iter()
                .any(|b| b.stacks.iter().any(|s| s.item == ItemId::Flint)),
            "deixou o saque no chão"
        );
    }

    #[test]
    fn tribe_member_defends_against_raiders() {
        let mut sim = Sim::new(1337);
        sim.join("Ana");
        sim.creatures.clear();
        let spot = sim.world.spawn;
        sim.players[0].player.pos = spot + vec2(2000.0, 0.0);
        let member = sim.npcs.iter().position(|n| n.is_villager()).unwrap();
        sim.npcs[member].owner = Some("Ana".to_owned());
        sim.npcs[member].order = crate::npc::Order::Defend { post: spot.into() };
        sim.npcs[member].body.pos = spot;
        sim.npcs[member].inventory.add(ItemId::MetalSword, 1);
        let id = sim.next_npc_id;
        sim.next_npc_id += 1;
        sim.npcs
            .push(Npc::raider(id, spot + vec2(80.0, 0.0), spot, ItemId::WoodenSpear, &[]));
        run(&mut sim, 15.0);
        assert!(!sim.npcs.iter().any(|n| n.id == id), "o membro venceu o saqueador");
    }

    #[test]
    fn walls_block_raiders_until_they_break() {
        let mut sim = Sim::new(1337);
        sim.join("Ana");
        sim.creatures.clear();
        let spot = sim.world.spawn;
        sim.players[0].player.pos = spot + vec2(3000.0, 0.0);
        let tile = crate::building::tile_of(spot + vec2(40.0, 0.0));
        sim.world.place(StructureKind::WoodWall, tile);
        let id = sim.next_npc_id;
        sim.next_npc_id += 1;
        let target = crate::building::tile_rect(tile).center() + vec2(60.0, 0.0);
        sim.npcs.push(Npc::raider(id, spot, target, ItemId::MetalSword, &[]));
        run(&mut sim, 10.0);
        let wall = sim.world.structure_at(tile, Layer::Object);
        assert!(
            wall.is_none_or(|i| sim.world.structures[i].health < StructureKind::WoodWall.max_health()),
            "o saqueador bateu na parede"
        );
    }
}
