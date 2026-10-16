//! Fauna: espécies, níveis, IA simples (parado, vagando, fugindo, perseguindo), torpor/desmaio,
//! domesticação e comportamento de dinos domesticados (seguir, ficar, montaria).
//! Criaturas mortas viram carcaças que o jogador coleta como um nó de recurso.

use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

use crate::inventory::{Inventory, Stack};
use crate::items::ItemId;
use crate::rng::Rng;
use crate::taming::{self, Diet, Taming};
use crate::world::{TILE, Tile, World};

/// Criaturas longe do jogador ficam congeladas para economizar processamento.
const SIMULATION_RADIUS: f32 = 900.0;
/// Agressivos desistem da perseguição se o jogador se afastar além de `aggro * LEASH`.
const LEASH: f32 = 1.8;
const CORPSE_SECS: f32 = 120.0;
const FLEE_SECS: f32 = 4.0;
/// Agressivos não nascem perto do spawn da praia nem de construções.
const SAFE_ZONE: f32 = TILE * 20.0;
const BASE_SAFE_ZONE: f32 = TILE * 8.0;
pub const MAX_WILD_LEVEL: u32 = 30;
/// Torpor perdido por segundo enquanto acordado (fração do máximo).
const TORPOR_DECAY: f32 = 0.03;
/// Sem narcóticos, um dino desmaiado acorda depois deste tempo.
const KNOCKOUT_SECS: f32 = 150.0;
/// Slots de comida de um dino desmaiado (antes de virar inventário de carga).
pub const TAMING_SLOTS: usize = 6;
/// Domesticados seguindo: distância confortável e distância para teletransportar.
const FOLLOW_DISTANCE: f32 = 48.0;
const FOLLOW_TELEPORT: f32 = 700.0;
/// Regeneração de vida de domesticados (fração do máximo por segundo).
const TAMED_REGEN: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Species {
    Dodo,
    Trilobite,
    Dilophosaur,
    Raptor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Temperament {
    Passive,
    Aggressive,
}

impl Species {
    pub const ALL: [Species; 4] = [Species::Dodo, Species::Trilobite, Species::Dilophosaur, Species::Raptor];

    pub fn name(self) -> &'static str {
        match self {
            Species::Dodo => "Dodô",
            Species::Trilobite => "Trilobita",
            Species::Dilophosaur => "Dilofossauro",
            Species::Raptor => "Raptor",
        }
    }

    pub fn temperament(self) -> Temperament {
        match self {
            Species::Dodo | Species::Trilobite => Temperament::Passive,
            Species::Dilophosaur | Species::Raptor => Temperament::Aggressive,
        }
    }

    /// Vida no nível 1.
    pub fn max_health(self) -> f32 {
        match self {
            Species::Dodo => 30.0,
            Species::Trilobite => 25.0,
            Species::Dilophosaur => 60.0,
            Species::Raptor => 90.0,
        }
    }

    /// Torpor necessário para desmaiar, no nível 1.
    pub fn base_torpor(self) -> f32 {
        match self {
            Species::Dodo => 30.0,
            Species::Trilobite => 40.0,
            Species::Dilophosaur => 80.0,
            Species::Raptor => 120.0,
        }
    }

    /// (andando, correndo) em pixels/s. O jogador anda a 130 e corre a 208.
    pub fn speeds(self) -> (f32, f32) {
        match self {
            Species::Dodo => (40.0, 120.0),
            Species::Trilobite => (20.0, 45.0),
            Species::Dilophosaur => (50.0, 115.0),
            Species::Raptor => (60.0, 170.0),
        }
    }

    pub fn radius(self) -> f32 {
        match self {
            Species::Dodo => 8.0,
            Species::Trilobite => 8.0,
            Species::Dilophosaur => 11.0,
            Species::Raptor => 12.0,
        }
    }

    /// (dano no nível 1, intervalo entre ataques em s, alcance da borda até o alvo, distância de aggro).
    pub fn attack(self) -> Option<(f32, f32, f32, f32)> {
        match self {
            Species::Dilophosaur => Some((8.0, 1.4, 12.0, 160.0)),
            Species::Raptor => Some((12.0, 1.1, 12.0, 220.0)),
            _ => None,
        }
    }

    /// Golpes de coleta que a carcaça aguenta.
    pub fn harvest_hits(self) -> u32 {
        match self {
            Species::Dodo | Species::Trilobite => 3,
            Species::Dilophosaur => 5,
            Species::Raptor => 6,
        }
    }

    /// Animais maiores rendem mais carne/couro.
    pub fn is_big(self) -> bool {
        matches!(self, Species::Dilophosaur | Species::Raptor)
    }

    pub fn diet(self) -> Diet {
        match self {
            Species::Dodo | Species::Trilobite => Diet::Herbivore,
            Species::Dilophosaur | Species::Raptor => Diet::Carnivore,
        }
    }

    /// Pontos de comida para domesticar no nível 1 (`None` = não domesticável).
    pub fn taming_food(self) -> Option<f32> {
        match self {
            Species::Dodo => Some(30.0),
            Species::Trilobite => None,
            Species::Dilophosaur => Some(100.0),
            Species::Raptor => Some(160.0),
        }
    }

    /// Sela necessária para montar (`None` = não dá para montar).
    pub fn saddle(self) -> Option<ItemId> {
        match self {
            Species::Raptor => Some(ItemId::RaptorSaddle),
            _ => None,
        }
    }

    /// Slots do inventário de carga quando domesticado.
    pub fn pack_slots(self) -> usize {
        match self {
            Species::Dodo => 6,
            Species::Trilobite => 4,
            Species::Dilophosaur => 8,
            Species::Raptor => 12,
        }
    }

    /// Limite de população selvagem viva na ilha.
    pub fn population_cap(self) -> usize {
        match self {
            Species::Dodo => 25,
            Species::Trilobite => 15,
            Species::Dilophosaur => 10,
            Species::Raptor => 6,
        }
    }

    /// Bioma onde a espécie nasce.
    fn habitat(self, world: &World, x: i32, y: i32) -> bool {
        let tile = world.tile(x, y);
        match self {
            Species::Trilobite => tile == Tile::Sand,
            Species::Dodo => tile == Tile::Grass,
            Species::Dilophosaur => tile == Tile::Grass && world.forest(x, y) > 0.45,
            Species::Raptor => tile == Tile::Grass && world.forest(x, y) > 0.55,
        }
    }

    /// Onde a criatura pode pisar: terra firme (trilobitas também andam na água rasa).
    pub fn can_stand(self, world: &World, pos: Vec2) -> bool {
        let tile_ok = match world.tile_at(pos) {
            Tile::ShallowWater => self == Species::Trilobite,
            tile => tile.walkable(),
        };
        tile_ok && !world.blocked(pos, self.radius())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Idle,
    Wander,
    Flee,
    Chase,
}

/// Ordem dada a um dino domesticado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    Follow,
    Stay,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corpse {
    pub harvest_left: u32,
    pub ttl: f32,
}

/// Algo relevante que aconteceu com a criatura neste frame (o jogo reage).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Mordeu um jogador (índice em `AiInput::players`) ou a montaria dele.
    Bite { target: usize, damage: f32 },
    /// Acordou do desmaio sem ser domesticada.
    Woke,
    /// Domesticação concluída, com os níveis bônus ganhos.
    Tamed { bonus_levels: u32 },
}

/// Resultado de um golpe recebido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Hurt,
    KnockedOut,
    Died,
}

#[derive(Clone, Debug)]
pub struct Creature {
    /// Identificador estável (índices no vetor mudam quando carcaças somem).
    pub id: u32,
    pub species: Species,
    pub level: u32,
    pub pos: Vec2,
    pub health: f32,
    pub torpor: f32,
    pub knocked_out: bool,
    pub taming: Option<Taming>,
    /// Comida (desmaiado) ou carga (domesticado).
    pub inventory: Option<Inventory>,
    /// `Some` quando domesticado.
    pub command: Option<Command>,
    /// Nome do jogador dono (domesticados seguem o dono).
    pub owner: Option<String>,
    /// Controlado pelo jogador montado: a IA não age.
    pub mounted: bool,
    pub facing_left: bool,
    pub state: State,
    state_timer: f32,
    wander_target: Vec2,
    attack_cooldown: f32,
    /// Animação de bote (s restantes).
    pub lunge: f32,
    /// Piscar vermelho ao levar dano (s restantes).
    pub hurt: f32,
    pub moving: bool,
    pub anim_time: f32,
    pub corpse: Option<Corpse>,
}

/// O que a IA sabe sobre um jogador neste frame.
#[derive(Clone, Copy, Debug)]
pub struct AiPlayer<'a> {
    pub pos: Vec2,
    pub alive: bool,
    pub name: &'a str,
}

/// O que a IA sabe sobre os jogadores neste frame.
pub struct AiInput<'a> {
    pub players: &'a [AiPlayer<'a>],
}

impl AiInput<'_> {
    /// Jogador vivo mais próximo: (índice, vetor até ele).
    fn nearest_alive(&self, from: Vec2) -> Option<(usize, Vec2)> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.alive)
            .map(|(i, p)| (i, p.pos - from))
            .min_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()))
    }

    /// Distância até o jogador mais próximo (vivo ou não).
    fn nearest_distance(&self, from: Vec2) -> f32 {
        self.players
            .iter()
            .map(|p| p.pos.distance(from))
            .fold(f32::INFINITY, f32::min)
    }
}

impl Creature {
    pub fn new(species: Species, pos: Vec2) -> Self {
        Self {
            id: 0,
            species,
            level: 1,
            pos,
            health: species.max_health(),
            torpor: 0.0,
            knocked_out: false,
            taming: None,
            inventory: None,
            command: None,
            owner: None,
            mounted: false,
            facing_left: false,
            state: State::Idle,
            state_timer: 1.0,
            wander_target: pos,
            attack_cooldown: 0.0,
            lunge: 0.0,
            hurt: 0.0,
            moving: false,
            anim_time: 0.0,
            corpse: None,
        }
    }

    /// Define o nível e enche a vida de acordo.
    pub fn with_level(mut self, level: u32) -> Self {
        self.level = level.max(1);
        self.health = self.max_health();
        self
    }

    fn level_mult(&self, per_level: f32) -> f32 {
        1.0 + per_level * (self.level.max(1) - 1) as f32
    }

    pub fn max_health(&self) -> f32 {
        self.species.max_health() * self.level_mult(0.1)
    }

    pub fn max_torpor(&self) -> f32 {
        self.species.base_torpor() * self.level_mult(0.06)
    }

    pub fn bite_damage(&self) -> f32 {
        self.species.attack().map_or(0.0, |a| a.0) * self.level_mult(0.04)
    }

    pub fn is_alive(&self) -> bool {
        self.corpse.is_none()
    }

    pub fn is_tamed(&self) -> bool {
        self.command.is_some()
    }

    pub fn is_wild(&self) -> bool {
        !self.is_tamed()
    }

    pub fn is_conscious(&self) -> bool {
        self.is_alive() && !self.knocked_out
    }

    pub fn health_frac(&self) -> f32 {
        (self.health / self.max_health()).clamp(0.0, 1.0)
    }

    pub fn torpor_frac(&self) -> f32 {
        (self.torpor / self.max_torpor()).clamp(0.0, 1.0)
    }

    /// Domesticado com a sela da espécie no inventário.
    pub fn can_ride(&self) -> bool {
        let saddle = self.species.saddle();
        self.is_tamed() && saddle.is_some_and(|s| self.inventory.as_ref().is_some_and(|inv| inv.count(s) > 0))
    }

    /// Aplica dano e torpor vindos de `from`.
    pub fn take_hit(&mut self, damage: f32, torpor: f32, from: Vec2) -> Hit {
        if !self.is_alive() {
            return Hit::Hurt;
        }
        self.health -= damage;
        self.hurt = 0.2;
        if !self.knocked_out {
            self.facing_left = from.x < self.pos.x;
        }
        if self.health <= 0.0 {
            self.health = 0.0;
            self.knocked_out = false;
            self.taming = None;
            self.state = State::Idle;
            self.corpse = Some(Corpse {
                harvest_left: self.species.harvest_hits(),
                ttl: CORPSE_SECS,
            });
            return Hit::Died;
        }
        if self.knocked_out {
            self.torpor = (self.torpor + torpor).min(self.max_torpor());
            if damage > 0.0
                && let Some(t) = self.taming.as_mut()
            {
                t.on_hit();
            }
            return Hit::Hurt;
        }
        if self.is_tamed() {
            return Hit::Hurt;
        }
        self.torpor += torpor;
        if self.torpor >= self.max_torpor() {
            self.knock_out();
            return Hit::KnockedOut;
        }
        match self.species.temperament() {
            Temperament::Passive => {
                self.state = State::Flee;
                self.state_timer = FLEE_SECS;
            }
            Temperament::Aggressive => self.state = State::Chase,
        }
        Hit::Hurt
    }

    fn knock_out(&mut self) {
        self.torpor = self.max_torpor();
        self.knocked_out = true;
        self.state = State::Idle;
        self.moving = false;
        if self.inventory.is_none() {
            self.inventory = Some(Inventory::new(TAMING_SLOTS));
        }
        if self.species.taming_food().is_some() {
            self.taming.get_or_insert_with(Taming::default);
        }
    }

    /// Um golpe de coleta na carcaça. Retorna false se não há mais o que coletar.
    pub fn harvest(&mut self) -> bool {
        let Some(corpse) = self.corpse.as_mut() else {
            return false;
        };
        if corpse.harvest_left == 0 {
            return false;
        }
        corpse.harvest_left -= 1;
        self.hurt = 0.15;
        true
    }

    /// Carcaça esgotada ou expirada: pode ser removida.
    pub fn is_gone(&self) -> bool {
        self.corpse.is_some_and(|c| c.harvest_left == 0 || c.ttl <= 0.0)
    }

    /// Esvazia o inventário da criatura (ao acordar ou morrer).
    pub fn take_inventory(&mut self) -> Vec<Stack> {
        self.inventory.as_mut().map(Inventory::drain_all).unwrap_or_default()
    }

    /// Avança IA, torpor e domesticação.
    pub fn update(&mut self, world: &World, input: &AiInput, rng: &mut Rng, dt: f32) -> Option<Event> {
        self.hurt = (self.hurt - dt).max(0.0);
        self.lunge = (self.lunge - dt).max(0.0);
        self.attack_cooldown = (self.attack_cooldown - dt).max(0.0);
        if let Some(corpse) = self.corpse.as_mut() {
            corpse.ttl -= dt;
            return None;
        }
        if self.knocked_out {
            return self.update_knocked_out(dt);
        }
        self.torpor = (self.torpor - self.max_torpor() * TORPOR_DECAY * dt).max(0.0);
        if self.mounted {
            return None;
        }
        if self.is_tamed() {
            self.update_tamed(world, input, dt);
            return None;
        }

        if input.nearest_distance(self.pos) > SIMULATION_RADIUS {
            self.moving = false;
            return None;
        }
        let (target, to_player, dist) = match input.nearest_alive(self.pos) {
            Some((i, v)) => (Some(i), v, v.length()),
            None => (None, Vec2::ZERO, f32::INFINITY),
        };
        self.state_timer -= dt;
        self.moving = false;

        if let Some((_, _, _, aggro)) = self.species.attack() {
            let keep_chasing = self.state == State::Chase && dist < aggro * LEASH;
            if target.is_some() && (dist < aggro || keep_chasing) {
                self.state = State::Chase;
            } else if self.state == State::Chase {
                self.start_idle(rng);
            }
        }

        let (walk, run) = self.species.speeds();
        match self.state {
            State::Idle => {
                if self.state_timer <= 0.0 {
                    let offset = vec2(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * 120.0;
                    self.wander_target = self.pos + offset;
                    self.state = State::Wander;
                    self.state_timer = rng.range(3.0, 6.0);
                }
            }
            State::Wander => {
                let to_target = self.wander_target - self.pos;
                let stuck = !self.step(world, to_target, walk, dt);
                if to_target.length() < 4.0 || self.state_timer <= 0.0 || stuck {
                    self.start_idle(rng);
                }
            }
            State::Flee => {
                self.step(world, -to_player, run, dt);
                if self.state_timer <= 0.0 {
                    self.start_idle(rng);
                }
            }
            State::Chase => {
                let (Some((_, cooldown, range, _)), Some(target)) = (self.species.attack(), target) else {
                    self.start_idle(rng);
                    return None;
                };
                self.facing_left = to_player.x < 0.0;
                if dist - self.species.radius() <= range {
                    if self.attack_cooldown <= 0.0 {
                        self.attack_cooldown = cooldown;
                        self.lunge = 0.25;
                        return Some(Event::Bite {
                            target,
                            damage: self.bite_damage(),
                        });
                    }
                } else {
                    self.step(world, to_player, run, dt);
                }
            }
        }
        None
    }

    fn update_knocked_out(&mut self, dt: f32) -> Option<Event> {
        self.moving = false;
        let max = self.max_torpor();
        self.torpor -= max / KNOCKOUT_SECS * dt;

        // Narcóticos no inventário mantêm o dino desacordado.
        if self.torpor < max * taming::NARCOTIC_THRESHOLD
            && let Some(inv) = self.inventory.as_mut()
            && inv.remove(ItemId::Narcotic, 1)
        {
            self.torpor = (self.torpor + max * taming::NARCOTIC_TORPOR).min(max);
        }

        if self.torpor <= 0.0 {
            self.torpor = 0.0;
            self.knocked_out = false;
            self.taming = None;
            self.state = match self.species.temperament() {
                Temperament::Aggressive => State::Chase,
                Temperament::Passive => State::Flee,
            };
            self.state_timer = FLEE_SECS;
            return Some(Event::Woke);
        }

        let diet = self.species.diet();
        let required = taming::required_food(self.species.taming_food()?, self.level);
        let (taming, inv) = (self.taming.as_mut()?, self.inventory.as_mut()?);
        taming.eat_timer -= dt;
        if taming.eat_timer > 0.0 {
            return None;
        }
        taming.eat_timer = taming::EAT_INTERVAL;
        // Come a melhor comida disponível.
        let best = taming::preferred_foods(diet)
            .iter()
            .copied()
            .find(|&food| inv.count(food) > 0)?;
        inv.remove(best, 1);
        taming.feed(taming::food_value(diet, best).unwrap_or(0.0), required);
        if taming.is_done() {
            let bonus_levels = taming::bonus_levels(self.level, taming.effectiveness);
            self.become_tamed(bonus_levels);
            return Some(Event::Tamed { bonus_levels });
        }
        None
    }

    fn become_tamed(&mut self, bonus_levels: u32) {
        self.level += bonus_levels;
        self.health = self.max_health();
        self.torpor = 0.0;
        self.knocked_out = false;
        self.taming = None;
        self.command = Some(Command::Follow);
        self.state = State::Idle;
        // A comida que sobrou vai para o inventário de carga.
        let mut pack = Inventory::new(self.species.pack_slots());
        let mut leftovers = self.take_inventory();
        pack.take_all(&mut leftovers);
        self.inventory = Some(pack);
    }

    fn update_tamed(&mut self, world: &World, input: &AiInput, dt: f32) {
        self.health = (self.health + self.max_health() * TAMED_REGEN * dt).min(self.max_health());
        self.moving = false;
        if self.command != Some(Command::Follow) {
            return;
        }
        // Segue o dono (se ele estiver no jogo e vivo).
        let owner = self.owner.as_deref();
        let Some(owner_pos) = input
            .players
            .iter()
            .find(|p| p.alive && Some(p.name) == owner)
            .map(|p| p.pos)
        else {
            return;
        };
        let to_player = owner_pos - self.pos;
        let dist = to_player.length();
        if dist > FOLLOW_TELEPORT {
            // Ficou para trás (preso atrás de algo): aparece perto do dono.
            let behind = owner_pos - to_player.normalize_or_zero() * FOLLOW_DISTANCE;
            if self.species.can_stand(world, behind) {
                self.pos = behind;
            }
            return;
        }
        if dist > FOLLOW_DISTANCE {
            let (walk, run) = self.species.speeds();
            let speed = if dist > FOLLOW_DISTANCE * 3.0 {
                run.max(150.0)
            } else {
                walk.max(90.0)
            };
            self.step(world, to_player, speed, dt);
        }
    }

    /// Movimento controlado pelo jogador montado.
    pub fn ride(&mut self, world: &World, dir: Vec2, speed: f32, dt: f32) {
        self.moving = false;
        self.step(world, dir, speed, dt);
    }

    /// Bote da montaria: pode atacar de novo?
    pub fn ready_to_bite(&self) -> bool {
        self.attack_cooldown <= 0.0 && self.species.attack().is_some()
    }

    /// Registra um bote da montaria e devolve o dano.
    pub fn bite(&mut self) -> f32 {
        let cooldown = self.species.attack().map_or(1.0, |a| a.1);
        self.attack_cooldown = cooldown * 0.8;
        self.lunge = 0.25;
        self.bite_damage()
    }

    pub fn toggle_command(&mut self) {
        self.command = match self.command {
            Some(Command::Follow) => Some(Command::Stay),
            Some(Command::Stay) => Some(Command::Follow),
            None => None,
        };
    }

    fn start_idle(&mut self, rng: &mut Rng) {
        self.state = State::Idle;
        self.state_timer = rng.range(1.0, 4.0);
    }

    /// Move na direção `dir`, deslizando em obstáculos. Retorna false se não saiu do lugar.
    fn step(&mut self, world: &World, dir: Vec2, speed: f32, dt: f32) -> bool {
        let dir = dir.normalize_or_zero();
        if dir == Vec2::ZERO {
            return false;
        }
        if dir.x.abs() > 0.05 {
            self.facing_left = dir.x < 0.0;
        }
        let start = self.pos;
        let delta = dir * speed * dt;
        for axis in [vec2(delta.x, 0.0), vec2(0.0, delta.y)] {
            let next = self.pos + axis;
            if self.species.can_stand(world, next) {
                self.pos = next;
            }
        }
        self.moving = self.pos != start;
        if self.moving {
            self.anim_time += dt * speed / 60.0;
        }
        self.moving
    }
}

/// Nível de uma criatura selvagem: níveis baixos são mais comuns.
fn roll_level(rng: &mut Rng) -> u32 {
    let t = rng.next_f32();
    (1 + (t * t * MAX_WILD_LEVEL as f32) as u32).min(MAX_WILD_LEVEL)
}

/// Tenta repor a população selvagem de cada espécie até o limite.
/// `attempts` controla quantos tiles aleatórios testar por espécie (alto no início do mundo).
pub fn populate(
    creatures: &mut Vec<Creature>,
    world: &World,
    players: &[Vec2],
    rng: &mut Rng,
    attempts: usize,
    next_id: &mut u32,
) {
    for species in Species::ALL {
        let mut alive = creatures
            .iter()
            .filter(|c| c.species == species && c.is_alive() && c.is_wild())
            .count();
        for _ in 0..attempts {
            if alive >= species.population_cap() {
                break;
            }
            let x = rng.range_u32(1, world.width as u32 - 2) as i32;
            let y = rng.range_u32(1, world.height as u32 - 2) as i32;
            let pos = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
            if spawn_ok(species, world, x, y, pos, players) {
                let mut c = Creature::new(species, pos).with_level(roll_level(rng));
                c.id = *next_id;
                *next_id += 1;
                creatures.push(c);
                alive += 1;
            }
        }
    }
}

fn spawn_ok(species: Species, world: &World, x: i32, y: i32, pos: Vec2, players: &[Vec2]) -> bool {
    // Checagens baratas primeiro; colisão (mais cara) por último. Nada nasce na frente de ninguém.
    if !species.habitat(world, x, y) || players.iter().any(|p| pos.distance(*p) < 420.0) {
        return false;
    }
    if species.temperament() == Temperament::Aggressive {
        let near_spawn = pos.distance(world.spawn) < SAFE_ZONE;
        let near_base = world
            .structures
            .iter()
            .any(|s| s.rect().center().distance(pos) < BASE_SAFE_ZONE);
        if near_spawn || near_base {
            return false;
        }
    }
    species.can_stand(world, pos)
}

/// Estado salvo de uma criatura (a IA recomeça do zero ao carregar).
/// Campos novos com `default` para saves da v0.4 continuarem válidos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatureSave {
    #[serde(default)]
    pub id: u32,
    pub species: Species,
    #[serde(default = "one")]
    pub level: u32,
    pub pos: [f32; 2],
    pub health: f32,
    pub corpse: Option<Corpse>,
    #[serde(default)]
    pub torpor: f32,
    #[serde(default)]
    pub knocked_out: bool,
    #[serde(default)]
    pub taming: Option<Taming>,
    #[serde(default)]
    pub inventory: Option<Inventory>,
    #[serde(default)]
    pub command: Option<Command>,
    #[serde(default)]
    pub owner: Option<String>,
}

fn one() -> u32 {
    1
}

impl From<&Creature> for CreatureSave {
    fn from(c: &Creature) -> Self {
        Self {
            id: c.id,
            species: c.species,
            level: c.level,
            pos: c.pos.into(),
            health: c.health,
            corpse: c.corpse,
            torpor: c.torpor,
            knocked_out: c.knocked_out,
            taming: c.taming,
            inventory: c.inventory.clone(),
            command: c.command,
            owner: c.owner.clone(),
        }
    }
}

impl From<CreatureSave> for Creature {
    fn from(s: CreatureSave) -> Self {
        let mut c = Creature::new(s.species, Vec2::from(s.pos)).with_level(s.level);
        c.id = s.id;
        c.health = s.health.min(c.max_health());
        c.corpse = s.corpse;
        c.torpor = s.torpor;
        c.knocked_out = s.knocked_out;
        c.taming = s.taming;
        c.inventory = s.inventory;
        c.command = s.command;
        c.owner = s.owner;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        World::generate(1337, 160, 160)
    }

    /// Um ponto de grama livre, com espaço em volta para a criatura andar.
    fn open_grass(world: &World, species: Species) -> Vec2 {
        for y in 0..world.height as i32 {
            for x in 0..world.width as i32 {
                let pos = vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
                let free_around = [
                    vec2(0.0, 0.0),
                    vec2(40.0, 0.0),
                    vec2(-40.0, 0.0),
                    vec2(0.0, 40.0),
                    vec2(0.0, -40.0),
                ]
                .iter()
                .all(|&o| species.can_stand(world, pos + o));
                if world.tile(x, y) == Tile::Grass && free_around {
                    return pos;
                }
            }
        }
        panic!("sem grama livre");
    }

    fn run(c: &mut Creature, world: &World, player: Vec2, secs: f32) -> Vec<Event> {
        let mut rng = Rng::new(9);
        let dt = 1.0 / 30.0;
        let players = [AiPlayer {
            pos: player,
            alive: true,
            name: "p",
        }];
        let input = AiInput { players: &players };
        (0..(secs / dt) as usize)
            .filter_map(|_| c.update(world, &input, &mut rng, dt))
            .collect()
    }

    #[test]
    fn raptor_chases_and_bites_player() {
        let world = world();
        let pos = open_grass(&world, Species::Raptor);
        let mut raptor = Creature::new(Species::Raptor, pos);
        let events = run(&mut raptor, &world, pos + vec2(30.0, 0.0), 4.0);
        assert_eq!(raptor.state, State::Chase);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Bite { target: 0, damage } if *damage > 0.0))
        );
    }

    #[test]
    fn dodo_flees_when_hit() {
        let world = world();
        let pos = open_grass(&world, Species::Dodo);
        let mut dodo = Creature::new(Species::Dodo, pos);
        let player = pos - vec2(20.0, 0.0);
        assert_eq!(dodo.take_hit(5.0, 0.0, player), Hit::Hurt);
        assert_eq!(dodo.state, State::Flee);
        let before = dodo.pos.distance(player);
        assert!(run(&mut dodo, &world, player, 1.0).is_empty());
        assert!(dodo.pos.distance(player) > before);
    }

    #[test]
    fn death_leaves_a_harvestable_corpse() {
        let mut dilo = Creature::new(Species::Dilophosaur, Vec2::ZERO);
        assert_eq!(dilo.take_hit(30.0, 0.0, Vec2::ONE), Hit::Hurt);
        assert_eq!(dilo.take_hit(30.0, 0.0, Vec2::ONE), Hit::Died);
        assert!(!dilo.is_alive());
        for _ in 0..Species::Dilophosaur.harvest_hits() {
            assert!(dilo.harvest());
        }
        assert!(!dilo.harvest());
        assert!(dilo.is_gone());
    }

    #[test]
    fn levels_scale_stats() {
        let low = Creature::new(Species::Raptor, Vec2::ZERO);
        let high = Creature::new(Species::Raptor, Vec2::ZERO).with_level(21);
        assert!(high.max_health() > low.max_health());
        assert!(high.max_torpor() > low.max_torpor());
        assert!(high.bite_damage() > low.bite_damage());
        assert_eq!(high.health, high.max_health());
    }

    #[test]
    fn torpor_knocks_out_and_wears_off() {
        let world = world();
        let mut raptor = Creature::new(Species::Raptor, open_grass(&world, Species::Raptor));
        let hits = (raptor.max_torpor() / 15.0).ceil() as usize;
        for i in 0..hits {
            let hit = raptor.take_hit(3.0, 15.0, Vec2::ZERO);
            assert_eq!(hit == Hit::KnockedOut, i == hits - 1);
        }
        assert!(raptor.knocked_out && raptor.taming.is_some() && raptor.inventory.is_some());
        let far = raptor.pos + vec2(5000.0, 0.0);
        let events = run(&mut raptor, &world, far, KNOCKOUT_SECS + 1.0);
        assert_eq!(events, vec![Event::Woke]);
        assert!(!raptor.knocked_out && raptor.taming.is_none());
    }

    #[test]
    fn narcotics_keep_it_asleep() {
        let world = world();
        let mut dodo = Creature::new(Species::Dodo, open_grass(&world, Species::Dodo));
        dodo.take_hit(0.0, 999.0, Vec2::ZERO);
        dodo.inventory.as_mut().unwrap().add(ItemId::Narcotic, 3);
        let here = dodo.pos;
        let events = run(&mut dodo, &world, here, KNOCKOUT_SECS + 1.0);
        assert!(events.is_empty(), "ainda dormindo");
        assert!(dodo.inventory.as_ref().unwrap().count(ItemId::Narcotic) < 3);
    }

    #[test]
    fn feeding_tames_with_bonus_levels() {
        let world = world();
        let mut raptor = Creature::new(Species::Raptor, open_grass(&world, Species::Raptor)).with_level(10);
        raptor.take_hit(0.0, 999.0, Vec2::ZERO);
        raptor.take_hit(1.0, 0.0, Vec2::ZERO); // um golpe: efetividade 90%
        raptor.inventory.as_mut().unwrap().add(ItemId::RawMeat, 40);
        let here = raptor.pos;
        let events = run(&mut raptor, &world, here, 120.0);
        let Some(Event::Tamed { bonus_levels }) = events.last().copied() else {
            panic!("deveria ter domesticado: {events:?}");
        };
        assert_eq!(bonus_levels, 4); // 10 * 0.5 * 0.9
        assert_eq!(raptor.level, 14);
        assert!(raptor.is_tamed() && raptor.is_conscious());
        assert_eq!(
            raptor.inventory.as_ref().unwrap().slots.len(),
            Species::Raptor.pack_slots()
        );
        assert!(
            raptor.inventory.as_ref().unwrap().count(ItemId::RawMeat) > 0,
            "sobra vai para a carga"
        );
    }

    #[test]
    fn wrong_food_does_not_tame() {
        let world = world();
        let mut dodo = Creature::new(Species::Dodo, open_grass(&world, Species::Dodo));
        dodo.take_hit(0.0, 999.0, Vec2::ZERO);
        dodo.inventory.as_mut().unwrap().add(ItemId::RawMeat, 10);
        let here = dodo.pos;
        let events = run(&mut dodo, &world, here, 30.0);
        assert!(events.is_empty());
        assert_eq!(dodo.taming.unwrap().progress, 0.0);
    }

    #[test]
    fn tamed_follows_and_stays() {
        let world = world();
        let pos = open_grass(&world, Species::Dodo);
        let mut dodo = Creature::new(Species::Dodo, pos);
        dodo.command = Some(Command::Follow);
        dodo.owner = Some("p".to_owned());
        let player = pos + vec2(35.0, 35.0);
        run(&mut dodo, &world, player, 3.0);
        assert!(dodo.pos.distance(player) <= FOLLOW_DISTANCE + 2.0);

        dodo.toggle_command();
        let start = dodo.pos;
        run(&mut dodo, &world, player + vec2(-35.0, -70.0), 2.0);
        assert_eq!(dodo.pos, start, "parado quando mandado ficar");
    }

    #[test]
    fn ride_needs_saddle() {
        let mut raptor = Creature::new(Species::Raptor, Vec2::ZERO);
        raptor.become_tamed(0);
        assert!(!raptor.can_ride());
        raptor.inventory.as_mut().unwrap().add(ItemId::RaptorSaddle, 1);
        assert!(raptor.can_ride());
        let mut dodo = Creature::new(Species::Dodo, Vec2::ZERO);
        dodo.become_tamed(0);
        assert!(!dodo.can_ride(), "dodô não tem sela");
    }

    #[test]
    fn populate_respects_caps_and_safe_zones() {
        let world = world();
        let mut rng = Rng::new(5);
        let mut creatures = Vec::new();
        let mut next_id = 1;
        populate(&mut creatures, &world, &[world.spawn], &mut rng, 5000, &mut next_id);
        for species in Species::ALL {
            let n = creatures.iter().filter(|c| c.species == species).count();
            assert!(n <= species.population_cap(), "{species:?}");
            assert!(n > 0, "{species:?} deveria existir");
        }
        for c in &creatures {
            assert!(c.pos.distance(world.spawn) >= 420.0);
            assert!((1..=MAX_WILD_LEVEL).contains(&c.level));
            if c.species.temperament() == Temperament::Aggressive {
                assert!(c.pos.distance(world.spawn) >= SAFE_ZONE);
            }
        }
        let mut ids: Vec<u32> = creatures.iter().map(|c| c.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), creatures.len(), "ids únicos");

        // Domesticados não contam para o limite selvagem.
        creatures[0].command = Some(Command::Follow);
        let before = creatures.len();
        populate(&mut creatures, &world, &[world.spawn], &mut rng, 5000, &mut next_id);
        assert!(creatures.len() <= before + 1);
    }

    #[test]
    fn save_round_trip() {
        let mut c = Creature::new(Species::Raptor, vec2(10.0, 20.0)).with_level(7);
        c.id = 42;
        c.take_hit(0.0, 999.0, Vec2::ZERO);
        c.inventory.as_mut().unwrap().add(ItemId::RawMeat, 3);
        let restored = Creature::from(CreatureSave::from(&c));
        assert_eq!(CreatureSave::from(&restored), CreatureSave::from(&c));
    }

    #[test]
    fn v04_creature_save_still_loads() {
        let json = r#"{"species":"Dodo","pos":[1.0,2.0],"health":10.0,"corpse":null}"#;
        let c = Creature::from(serde_json::from_str::<CreatureSave>(json).unwrap());
        assert_eq!(c.level, 1);
        assert!(c.is_wild() && !c.knocked_out);
    }
}
