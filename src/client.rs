//! Cliente: recebe snapshots do servidor, suaviza posições, transforma teclado/mouse em
//! `Input`/`Command` e desenha. Não decide nada sobre o jogo: quem manda é o servidor.

use std::collections::HashMap;

use macroquad::prelude::*;

use crate::building;
use crate::creatures::Creature;
use crate::daynight;
use crate::inventory::HOTBAR_SIZE;
use crate::items::ItemId;
use crate::net::Connection;
use crate::player::{self, Facing, Player};
use crate::protocol::{
    ClientMsg, Command, ContainerRef, Input, MAX_CHAT_LEN, NetEvent, PlayerId, PlayerView, ServerMsg, Snapshot,
};
use crate::raft::Raft;
use crate::render::{self, Avatar, POPUP_LIFETIME, Popup};
use crate::sim::{self, Interaction, MAP_SIZE};
use crate::ui::{self, ChatLine, ContainerView, UiAction, UiState};
use crate::world::{GroundBag, World};

/// Pixels de tela por pixel de mundo.
const ZOOM: f32 = 2.0;
const MESSAGE_TTL: f32 = 3.0;
/// Luz fraca em volta de cada jogador para não ficar cego à noite.
const PLAYER_LIGHT: f32 = 56.0;
/// Reenvia o input mesmo sem mudança (o servidor sabe que o cliente segue vivo).
const INPUT_KEEPALIVE: f32 = 0.1;
/// Suavização das posições entre snapshots.
const SMOOTHING: f32 = 18.0;
/// Acima disso, a posição pula direto (renascer, montar, teletransporte do dino).
const SNAP_DISTANCE: f32 = 160.0;
const MAX_CHAT_LINES: usize = 50;

pub enum Status {
    Connecting,
    Playing,
    /// A partida acabou (motivo para mostrar no menu; vazio = saída normal).
    Closed(String),
}

/// Outro jogador (ou o próprio) como desenhado: posição suavizada.
struct Remote {
    view: PlayerView,
    player: Player,
    target: Vec2,
}

pub struct Client {
    conn: Connection,
    me: Option<PlayerId>,
    world: Option<World>,
    snap: Option<Snapshot>,
    creatures: Vec<Creature>,
    creature_targets: HashMap<u32, Vec2>,
    rafts: Vec<Raft>,
    raft_targets: HashMap<u32, Vec2>,
    remotes: Vec<Remote>,
    projectiles: Vec<Vec2>,
    pub ui: UiState,
    /// Menu de pausa (Esc) aberto.
    paused: bool,
    popups: Vec<Popup>,
    messages: Vec<(String, f32)>,
    chat: Vec<ChatLine>,
    hurt_flash: f32,
    camera_pos: Vec2,
    time: f32,
    last_input: Input,
    input_timer: f32,
    last_container: Option<ContainerRef>,
    /// Frames desenhados desde o primeiro snapshot (screenshots esperam alguns).
    pub frames_ready: u32,
    pub status: Status,
}

impl Client {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            me: None,
            world: None,
            snap: None,
            creatures: Vec::new(),
            creature_targets: HashMap::new(),
            rafts: Vec::new(),
            raft_targets: HashMap::new(),
            remotes: Vec::new(),
            projectiles: Vec::new(),
            ui: UiState::default(),
            paused: false,
            popups: Vec::new(),
            messages: Vec::new(),
            chat: Vec::new(),
            hurt_flash: 0.0,
            camera_pos: Vec2::ZERO,
            time: 0.0,
            last_input: Input::default(),
            input_timer: 0.0,
            last_container: None,
            frames_ready: 0,
            status: Status::Connecting,
        }
    }

    /// Sai da partida (o servidor salva este jogador).
    pub fn quit(&mut self) {
        self.conn.send(ClientMsg::Bye);
        self.status = Status::Closed(String::new());
    }

    fn send(&self, cmd: Command) {
        self.conn.send(ClientMsg::Command(cmd));
    }

    /// Mensagens iguais seguidas não se acumulam: só renovam o tempo.
    fn notify(&mut self, msg: String) {
        match self.messages.last_mut() {
            Some(last) if last.0 == msg => last.1 = MESSAGE_TTL,
            _ => self.messages.push((msg, MESSAGE_TTL)),
        }
    }

    fn my_view(&self) -> Option<&Remote> {
        let me = self.me?;
        self.remotes.iter().find(|r| r.view.id == me)
    }

    fn creature(&self, id: u32) -> Option<&Creature> {
        self.creatures.iter().find(|c| c.id == id)
    }

    // ---------- Rede ----------

    fn poll(&mut self) {
        while let Ok(msg) = self.conn.rx.try_recv() {
            match msg {
                ServerMsg::Welcome { you, seed } => {
                    self.me = Some(you);
                    self.world = Some(World::generate(seed, MAP_SIZE, MAP_SIZE));
                    self.status = Status::Playing;
                }
                ServerMsg::Snapshot(snapshot) => self.apply(*snapshot),
                ServerMsg::Events(events) => {
                    for event in events {
                        match event {
                            NetEvent::Popup { pos, text, style } => self.popups.push(Popup {
                                pos: Vec2::from(pos),
                                text,
                                style,
                                age: 0.0,
                            }),
                            NetEvent::Notify(msg) => self.notify(msg),
                            NetEvent::Hurt => self.hurt_flash = 0.35,
                        }
                    }
                }
                ServerMsg::Chat { from, text } => {
                    self.chat.push((from, text, 0.0));
                    if self.chat.len() > MAX_CHAT_LINES {
                        self.chat.remove(0);
                    }
                }
                ServerMsg::Rejected(reason) | ServerMsg::Shutdown(reason) => {
                    self.status = Status::Closed(reason);
                }
            }
        }
    }

    fn apply(&mut self, s: Snapshot) {
        let Some(world) = self.world.as_mut() else { return };
        for (node, &hp) in world.nodes.iter_mut().zip(&s.node_hp) {
            if u32::from(hp) < node.hp {
                node.shake = 0.2;
            }
            node.hp = u32::from(hp);
        }
        world.bags = s
            .bags
            .iter()
            .map(|b| GroundBag {
                pos: Vec2::from(b.pos),
                stacks: b.stacks.clone(),
                kind: b.kind,
                ttl: b.ttl,
            })
            .collect();
        world.structures = s.structures.clone();

        // Criaturas: posição desenhada continua de onde estava e desliza até a do servidor.
        let mut creatures: Vec<Creature> = s.creatures.iter().map(|v| v.to_creature()).collect();
        self.creature_targets.clear();
        for c in &mut creatures {
            self.creature_targets.insert(c.id, c.pos);
            if let Some(old) = self.creatures.iter().find(|o| o.id == c.id)
                && old.pos.distance(c.pos) < SNAP_DISTANCE
            {
                c.pos = old.pos;
            }
        }
        self.creatures = creatures;

        let mut rafts: Vec<Raft> = s.rafts.iter().map(Raft::from).collect();
        self.raft_targets.clear();
        for r in &mut rafts {
            self.raft_targets.insert(r.id, r.pos);
            if let Some(old) = self.rafts.iter().find(|o| o.id == r.id)
                && old.pos.distance(r.pos) < SNAP_DISTANCE
            {
                r.pos = old.pos;
            }
        }
        self.rafts = rafts;

        self.remotes = s
            .players
            .iter()
            .map(|view| {
                let target = Vec2::from(view.pos);
                let pos = self
                    .remotes
                    .iter()
                    .find(|r| r.view.id == view.id)
                    .map(|r| r.player.pos)
                    .filter(|p| p.distance(target) < SNAP_DISTANCE)
                    .unwrap_or(target);
                let mut player = Player::new(pos);
                player.facing = if view.facing_left { Facing::Left } else { Facing::Right };
                player.moving = view.moving;
                player.anim_time = view.anim_time;
                player.swing_timer = view.swing_timer;
                Remote {
                    view: view.clone(),
                    player,
                    target,
                }
            })
            .collect();

        self.projectiles = s.projectiles.iter().map(|&p| Vec2::from(p)).collect();
        self.time = s.time;

        // O servidor abriu um contêiner (F num baú, dino...): abre o painel junto.
        if s.me.container.is_some() && s.me.container != self.last_container {
            self.ui.inventory_open = true;
        }
        self.last_container = s.me.container;
        if s.me.dead {
            self.ui.inventory_open = false;
            self.ui.chat_open = false;
        }
        if self.snap.is_none()
            && let Some(me) = self.me
            && let Some(view) = s.players.iter().find(|p| p.id == me)
        {
            self.camera_pos = Vec2::from(view.pos);
        }
        self.snap = Some(s);
    }

    // ---------- Update ----------

    pub fn update(&mut self, dt: f32) {
        self.poll();
        self.time += dt;
        for p in &mut self.popups {
            p.age += dt;
        }
        self.popups.retain(|p| p.age < POPUP_LIFETIME);
        for m in &mut self.messages {
            m.1 -= dt;
        }
        self.messages.retain(|m| m.1 > 0.0);
        for line in &mut self.chat {
            line.2 += dt;
        }
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        if let Some(world) = self.world.as_mut() {
            for node in &mut world.nodes {
                node.shake = (node.shake - dt).max(0.0);
            }
        }

        if matches!(self.status, Status::Playing) && self.snap.is_some() {
            self.handle_input(dt);
        }

        let k = 1.0 - (-SMOOTHING * dt).exp();
        for c in &mut self.creatures {
            if let Some(target) = self.creature_targets.get(&c.id) {
                c.pos = c.pos.lerp(*target, k);
            }
        }
        for r in &mut self.rafts {
            if let Some(target) = self.raft_targets.get(&r.id) {
                r.pos = r.pos.lerp(*target, k);
            }
        }
        for r in &mut self.remotes {
            r.player.pos = r.player.pos.lerp(r.target, k);
        }
        if let Some(pos) = self.my_view().map(|r| r.player.pos) {
            let follow = 1.0 - (-10.0 * dt).exp();
            self.camera_pos = self.camera_pos.lerp(pos, follow);
        }
        if self.snap.is_some() {
            self.frames_ready += 1;
        }
    }

    fn handle_input(&mut self, dt: f32) {
        let Some(snap) = self.snap.as_ref() else { return };
        let me = &snap.me;

        // Chat aberto: o teclado é só do chat.
        if self.ui.chat_open {
            while let Some(c) = get_char_pressed() {
                if !c.is_control() && self.ui.chat_input.chars().count() < MAX_CHAT_LEN {
                    self.ui.chat_input.push(c);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.ui.chat_input.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                let text = std::mem::take(&mut self.ui.chat_input);
                if !text.trim().is_empty() {
                    self.send(Command::Chat(text));
                }
                self.ui.chat_open = false;
            }
            if is_key_pressed(KeyCode::Escape) {
                self.ui.chat_open = false;
                self.ui.chat_input.clear();
            }
            self.send_input(Input::default(), dt);
            return;
        }

        if me.dead {
            if is_key_pressed(KeyCode::Enter) || is_mouse_button_pressed(MouseButton::Left) {
                self.send(Command::Respawn);
            }
            self.send_input(Input::default(), dt);
            return;
        }

        if self.paused {
            self.send_input(Input::default(), dt);
            if is_key_pressed(KeyCode::Escape) {
                self.paused = false;
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                let (resume, leave) = pause_buttons();
                let m: Vec2 = mouse_position().into();
                if resume.contains(m) {
                    self.paused = false;
                } else if leave.contains(m) {
                    self.quit();
                }
            }
            return;
        }

        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
            clear_input_queue();
            self.ui.chat_open = true;
            return;
        }
        if is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::I) {
            self.ui.inventory_open = !self.ui.inventory_open;
            if !self.ui.inventory_open {
                self.send(Command::CloseContainer);
            }
        }
        if is_key_pressed(KeyCode::Escape) {
            if self.ui.inventory_open {
                self.ui.inventory_open = false;
                self.send(Command::CloseContainer);
            } else {
                self.paused = true;
            }
        }
        if is_key_pressed(KeyCode::H) {
            self.ui.hide_help = !self.ui.hide_help;
        }
        if is_key_pressed(KeyCode::F5) {
            self.send(Command::Save);
        }
        #[cfg(debug_assertions)]
        for (key, cmd) in [
            (KeyCode::F1, crate::protocol::DebugCmd::Items),
            (KeyCode::F2, crate::protocol::DebugCmd::Damage),
            (KeyCode::F3, crate::protocol::DebugCmd::SkipHours),
            (KeyCode::F4, crate::protocol::DebugCmd::Xp),
        ] {
            if is_key_pressed(key) {
                self.send(Command::Debug(cmd));
            }
        }

        // Hotbar
        let keys = [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
        ];
        let mut selected = keys.iter().position(|&k| is_key_pressed(k));
        let wheel = mouse_wheel().1;
        if !self.ui.inventory_open && wheel != 0.0 {
            let step = if wheel < 0.0 { 1 } else { HOTBAR_SIZE - 1 };
            selected = Some((me.hotbar_selected + step) % HOTBAR_SIZE);
        }
        if let Some(i) = selected.filter(|&i| i != me.hotbar_selected) {
            self.send(Command::SelectHotbar(i));
        }

        // Painéis
        let panel = self
            .container_view()
            .map(|v| (v.kind(), v.inventory().map_or(0, |i| i.slots.len())));
        let action = ui::handle_input(
            &self.ui,
            me.inventory.slots.len(),
            panel,
            me.queue.jobs.len(),
            me.held.is_some(),
        );
        if let Some(action) = action {
            let cmd = match action {
                UiAction::Craft(i) => Command::Craft(i),
                UiAction::CancelCraft(i) => Command::CancelCraft(i),
                UiAction::SelectHotbar(i) => Command::SelectHotbar(i),
                UiAction::Slot { target, left, shift } => Command::Slot { target, left, shift },
                UiAction::DropHeld => Command::DropHeld,
                UiAction::ToggleFire => Command::ToggleFire,
                UiAction::ToggleCommand => Command::ToggleOrder,
                UiAction::Mount => {
                    self.ui.inventory_open = false;
                    Command::Mount
                }
                UiAction::SpendAttribute(attr) => Command::SpendAttribute(attr),
            };
            self.send(cmd);
        }

        // Mundo
        let free = !self.ui.inventory_open;
        let over_ui = ui::pointer_over_ui(&self.ui);
        let mounted = me.mount.is_some();
        let placing = !mounted && self.placing_kind().is_some();
        let mouse_attack = free && is_mouse_button_down(MouseButton::Left) && !over_ui && !placing;
        let key_attack = free && is_key_down(KeyCode::E);
        let input = Input {
            dir: if free { movement_input().into() } else { [0.0, 0.0] },
            sprint: free && is_key_down(KeyCode::LeftShift),
            attack: key_attack || mouse_attack,
            aim: (mouse_attack && !key_attack).then(|| self.mouse_world().into()),
        };
        self.send_input(input, dt);

        if free {
            if placing && is_mouse_button_pressed(MouseButton::Left) && !over_ui {
                self.send(Command::Place {
                    tile: building::tile_of(self.mouse_world()),
                });
            }
            if is_key_pressed(KeyCode::X) {
                self.send(Command::Demolish {
                    tile: building::tile_of(self.mouse_world()),
                });
            }
            if is_key_pressed(KeyCode::R) || (is_mouse_button_pressed(MouseButton::Right) && !over_ui) {
                self.send(Command::Use);
            }
            if is_key_pressed(KeyCode::Q) {
                self.send(Command::Drop {
                    all: is_key_down(KeyCode::LeftControl),
                });
            }
            if is_key_pressed(KeyCode::F) {
                self.send(Command::Interact);
            }
            if mounted && is_key_pressed(KeyCode::Space) {
                self.send(Command::MountAction);
            }
        }
    }

    fn send_input(&mut self, input: Input, dt: f32) {
        self.input_timer -= dt;
        if input != self.last_input || self.input_timer <= 0.0 {
            self.conn.send(ClientMsg::Input(input));
            self.last_input = input;
            self.input_timer = INPUT_KEEPALIVE;
        }
    }

    // ---------- Leituras do snapshot ----------

    fn equipped(&self) -> Option<ItemId> {
        let me = &self.snap.as_ref()?.me;
        me.inventory.slots[me.hotbar_selected].map(|s| s.item)
    }

    fn placing_kind(&self) -> Option<building::StructureKind> {
        self.equipped().and_then(ItemId::structure)
    }

    fn container_view(&self) -> Option<ContainerView<'_>> {
        let snap = self.snap.as_ref()?;
        match snap.me.container? {
            ContainerRef::Structure(i) => self.world.as_ref()?.structures.get(i).map(ContainerView::Structure),
            ContainerRef::Creature(id) => self.creature(id).map(ContainerView::Creature),
        }
    }

    // ---------- Desenho ----------

    fn camera(&self) -> Camera2D {
        let half = vec2(screen_width(), screen_height()) / (2.0 * ZOOM);
        let world_size = self
            .world
            .as_ref()
            .map_or(Vec2::splat(MAP_SIZE as f32 * crate::world::TILE), World::size_px);
        let target = vec2(
            self.camera_pos.x.clamp(half.x, (world_size.x - half.x).max(half.x)),
            self.camera_pos.y.clamp(half.y, (world_size.y - half.y).max(half.y)),
        );
        // zoom.y positivo => Y cresce para baixo (sem render target no macroquad 0.4).
        Camera2D {
            target,
            zoom: vec2(2.0 * ZOOM / screen_width(), 2.0 * ZOOM / screen_height()),
            ..Default::default()
        }
    }

    fn mouse_world(&self) -> Vec2 {
        self.camera().screen_to_world(mouse_position().into())
    }

    pub fn draw(&self) {
        clear_background(render::DEEP_WATER);
        let (Some(world), Some(snap)) = (self.world.as_ref(), self.snap.as_ref()) else {
            draw_centered("Conectando...", screen_height() / 2.0, 32.0);
            return;
        };
        let me = &snap.me;
        let my = self.my_view();
        let camera = self.camera();
        set_camera(&camera);

        let half = vec2(screen_width(), screen_height()) / (2.0 * ZOOM);
        let view = Rect::new(
            camera.target.x - half.x,
            camera.target.y - half.y,
            half.x * 2.0,
            half.y * 2.0,
        );
        render::draw_ground(world, view, self.time);

        let interactive = !self.ui.inventory_open && !me.dead && !self.paused;
        let on_foot = interactive && me.mount.is_none();
        let ring = my
            .filter(|_| on_foot)
            .and_then(|r| sim::find_target(world, &self.creatures, &r.player, self.equipped()))
            .map(|t| sim::target_ring(world, &self.creatures, t));
        let avatars: Vec<Avatar> = self
            .remotes
            .iter()
            .filter(|r| !r.view.dead)
            .map(|r| Avatar {
                player: &r.player,
                held: r.view.held,
                mount: r.view.mount,
                shirt: render::shirt_color(r.view.id),
                name: (Some(r.view.id) != self.me).then_some(r.view.name.as_str()),
            })
            .collect();
        render::draw_entities(&render::Scene {
            world,
            creatures: &self.creatures,
            rafts: &self.rafts,
            projectiles: &self.projectiles,
            avatars: &avatars,
            local: my.map(|r| r.player.pos),
            ring,
            view,
            time: self.time,
        });

        let hour = daynight::hour(self.time);
        let flicker = 1.0 + (self.time * 9.0).sin() * 0.03 + (self.time * 23.0).sin() * 0.02;
        let mut lights: Vec<(Vec2, f32, bool)> = world.lights().map(|(p, r)| (p, r * flicker, true)).collect();
        lights.extend(
            avatars
                .iter()
                .map(|a| (a.player.pos - vec2(0.0, 12.0), PLAYER_LIGHT, false)),
        );
        render::draw_golden_hour(view, daynight::golden_hour(hour));
        render::draw_lighting(view, daynight::darkness(hour), &lights);

        if on_foot && let (Some(kind), Some(r)) = (self.placing_kind(), my) {
            let tile = building::tile_of(self.mouse_world());
            let valid = world.can_place(kind, tile, r.player.pos, player::RADIUS).is_ok();
            render::draw_ghost(kind, tile, valid);
        }

        set_default_camera();
        render::draw_creature_bars(&self.creatures, &camera, me.mount);
        render::draw_names(&avatars, &self.creatures, &camera);
        render::draw_popups(&self.popups, &camera);
        if on_foot
            && let Some(r) = my
            && let Some(target) = sim::nearest_interaction(world, &self.creatures, &self.rafts, r.player.pos)
            && let Some((label, pos)) = self.interaction_label(world, target)
        {
            render::draw_prompt(&label, pos, &camera);
        }

        let my_pos = my.map_or(Vec2::ZERO, |r| r.player.pos);
        ui::draw(&ui::Hud {
            ui: &self.ui,
            inventory: &me.inventory,
            held: me.held,
            chat: &self.chat,
            online: snap.players.len(),
            container: self.container_view(),
            mount: me.mount.and_then(|id| self.creature(id)),
            on_raft: me.raft.is_some(),
            progression: &me.progression,
            hurt: self.hurt_flash,
            queue: &me.queue,
            station_ok: &|recipe| recipe.station.is_none_or(|kind| world.station_near(kind, my_pos)),
            selected: me.hotbar_selected,
            messages: &self.messages,
            vitals: &me.vitals,
            dead: me.dead,
            time: self.time,
        });
        if self.paused {
            draw_pause();
        }
    }

    fn interaction_label(&self, world: &World, target: Interaction) -> Option<(String, Vec2)> {
        Some(match target {
            Interaction::Bag(i) => {
                let bag = world.bags.get(i)?;
                (render::bag_label(bag), bag.pos - vec2(0.0, 26.0))
            }
            Interaction::Structure(i) => {
                let s = world.structures.get(i)?;
                let r = s.rect();
                (render::structure_label(s), vec2(r.center().x, r.y - 30.0))
            }
            Interaction::Raft(id) => {
                let raft = self.rafts.iter().find(|r| r.id == id)?;
                ("[F] Subir na jangada".to_owned(), raft.pos - vec2(0.0, 52.0))
            }
            Interaction::Creature(id) => {
                let c = self.creature(id)?;
                if c.knocked_out {
                    (
                        format!("[F] Alimentar {} (desacordado)", c.species.name()),
                        c.pos - vec2(0.0, 44.0),
                    )
                } else {
                    (
                        format!("[F] {} Nv {}", c.species.name(), c.level),
                        c.pos - vec2(0.0, render::head_height(c) + 24.0),
                    )
                }
            }
        })
    }
}

fn movement_input() -> Vec2 {
    let mut v = Vec2::ZERO;
    if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
        v.y -= 1.0;
    }
    if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
        v.y += 1.0;
    }
    if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
        v.x -= 1.0;
    }
    if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
        v.x += 1.0;
    }
    v.normalize_or_zero()
}

fn draw_centered(text: &str, y: f32, size: f32) {
    let w = measure_text(text, None, size as u16, 1.0).width;
    draw_text(text, (screen_width() - w) / 2.0, y, size, WHITE);
}

/// Botões do menu de pausa: (voltar, sair para o menu).
fn pause_buttons() -> (Rect, Rect) {
    let (w, h) = (260.0, 44.0);
    let x = (screen_width() - w) / 2.0;
    let y = screen_height() / 2.0;
    (Rect::new(x, y - 10.0, w, h), Rect::new(x, y + 50.0, w, h))
}

fn draw_pause() {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    draw_centered("Pausa", screen_height() / 2.0 - 50.0, 48.0);
    let (resume, leave) = pause_buttons();
    let m: Vec2 = mouse_position().into();
    for (rect, label) in [(resume, "Voltar ao jogo"), (leave, "Sair para o menu")] {
        let bg = if rect.contains(m) {
            Color::new(1.0, 0.8, 0.35, 1.0)
        } else {
            Color::new(0.85, 0.65, 0.25, 1.0)
        };
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
        let w = measure_text(label, None, 24, 1.0).width;
        draw_text(
            label,
            rect.x + (rect.w - w) / 2.0,
            rect.y + 29.0,
            24.0,
            Color::new(0.1, 0.08, 0.05, 1.0),
        );
    }
    draw_centered(
        "O jogo continua rodando para os outros jogadores",
        screen_height() / 2.0 + 130.0,
        18.0,
    );
}
