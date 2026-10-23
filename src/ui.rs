//! HUD (hotbar, status, relógio, mensagens) e painéis de inventário, crafting e baú em modo imediato.
//! Layout é recalculado a cada frame pelas mesmas funções usadas no input e no desenho.

use macroquad::prelude::*;

use crate::building::{Material, Piece, Structure, StructureKind};
use crate::crafting::{self, Category, CraftQueue, MAX_QUEUE, RECIPES, Recipe};
use crate::creatures::{Command, Creature, MountHarvest};
use crate::daynight;
use crate::inventory::{HOTBAR_SIZE, Inventory, Stack};
use crate::items::ItemId;
use crate::npc::{GatherKind, NpcView};
use crate::progression::{self, Attribute, Progression};
use crate::protocol::{MAX_CHAT_LEN, NpcCmd, SlotTarget};
use crate::taming;
use crate::vitals::Vitals;

const SLOT: f32 = 52.0;
const SMALL_SLOT: f32 = 40.0;
const GAP: f32 = 6.0;
const PAD: f32 = 16.0;
const HEADER: f32 = 36.0;
const COLS: usize = 6;
const RIGHT_W: f32 = 400.0;
const CHAR_W: f32 = 230.0;
/// Topo da lista de atributos dentro do painel de personagem.
const ATTR_TOP: f32 = 132.0;
const ATTR_ROW: f32 = 30.0;
const DETAIL_H: f32 = 92.0;
/// Altura da linha de abas do crafting.
const TAB_H: f32 = 34.0;

const PANEL_BG: Color = Color::new(0.10, 0.11, 0.13, 0.92);
const SLOT_BG: Color = Color::new(1.0, 1.0, 1.0, 0.08);
const BORDER: Color = Color::new(1.0, 1.0, 1.0, 0.18);
const ACCENT: Color = Color::new(1.0, 0.80, 0.35, 1.0);
const TEXT: Color = Color::new(0.93, 0.93, 0.93, 1.0);
const DIM: Color = Color::new(0.65, 0.65, 0.68, 1.0);
const OK: Color = Color::new(0.55, 0.85, 0.50, 1.0);
const BAD: Color = Color::new(0.95, 0.45, 0.40, 1.0);

/// Estado só de interface (local ao cliente).
#[derive(Default)]
pub struct UiState {
    pub inventory_open: bool,
    pub hide_help: bool,
    /// Digitando uma mensagem de chat.
    pub chat_open: bool,
    pub chat_input: String,
    /// Aba aberta no painel de crafting.
    pub craft_tab: Category,
    /// Peças compridas (portão) na vertical (T gira).
    pub rotate: bool,
}

/// Linha do chat: (de, texto, idade em segundos).
pub type ChatLine = (String, String, f32);
/// Por quanto tempo uma mensagem fica visível com o chat fechado.
pub const CHAT_VISIBLE_SECS: f32 = 12.0;

/// Tipo de painel de contêiner: define layout e botões.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Storage,
    Campfire,
    /// Dino desacordado sendo domesticado.
    Taming,
    /// Dino domesticado.
    Tamed,
    /// Membro da sua tribo: ordens e inventário.
    Npc,
    /// Morador de acampamento: recrutar.
    Recruit,
}

impl PanelKind {
    /// Altura extra abaixo dos slots (status, barras, botões).
    fn extra_height(self) -> f32 {
        match self {
            PanelKind::Storage => 0.0,
            PanelKind::Campfire => 84.0,
            PanelKind::Taming | PanelKind::Tamed | PanelKind::Recruit => 112.0,
            PanelKind::Npc => 156.0,
        }
    }

    fn buttons(self) -> &'static [UiAction] {
        match self {
            PanelKind::Campfire => &[UiAction::ToggleFire],
            PanelKind::Tamed => &[UiAction::ToggleCommand, UiAction::Mount],
            PanelKind::Npc => &[
                UiAction::NpcOrder(NpcCmd::Follow),
                UiAction::NpcOrder(NpcCmd::Stay),
                UiAction::NpcOrder(NpcCmd::Defend),
                UiAction::NpcOrder(NpcCmd::Gather(GatherKind::Wood)),
                UiAction::NpcOrder(NpcCmd::Gather(GatherKind::Stone)),
                UiAction::NpcOrder(NpcCmd::Gather(GatherKind::Metal)),
            ],
            PanelKind::Recruit => &[UiAction::NpcOrder(NpcCmd::Recruit)],
            PanelKind::Storage | PanelKind::Taming => &[],
        }
    }
}

/// Leitura do contêiner aberto, para desenhar o painel.
#[derive(Clone, Copy)]
pub enum ContainerView<'a> {
    Structure(&'a Structure),
    Creature(&'a Creature),
    Npc(&'a NpcView),
}

impl<'a> ContainerView<'a> {
    pub fn kind(self) -> PanelKind {
        match self {
            ContainerView::Structure(s) if s.kind.process().is_some() => PanelKind::Campfire,
            ContainerView::Structure(_) => PanelKind::Storage,
            ContainerView::Creature(c) if c.is_tamed() => PanelKind::Tamed,
            ContainerView::Creature(_) => PanelKind::Taming,
            ContainerView::Npc(n) if n.owner.is_some() => PanelKind::Npc,
            ContainerView::Npc(_) => PanelKind::Recruit,
        }
    }

    pub fn inventory(self) -> Option<&'a Inventory> {
        match self {
            ContainerView::Structure(s) => s.storage.as_ref(),
            ContainerView::Creature(c) => c.inventory.as_ref(),
            ContainerView::Npc(n) => n.inventory.as_ref(),
        }
    }

    fn title(self) -> String {
        match self {
            ContainerView::Structure(s) => s.kind.item().name().to_owned(),
            ContainerView::Creature(c) => format!("{} Nv {}", c.species.name(), c.level),
            ContainerView::Npc(n) if n.owner.is_some() => format!("{} (sua tribo)", n.name),
            ContainerView::Npc(n) => format!("{} (acampamento)", n.name),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    Craft(usize),
    /// Trocar a aba do crafting (só muda a interface).
    CraftTab(Category),
    CancelCraft(usize),
    SelectHotbar(usize),
    /// Clique num slot (esquerdo/direito, com ou sem Shift).
    Slot {
        target: SlotTarget,
        left: bool,
        shift: bool,
    },
    /// Pilha do cursor solta fora dos painéis: vai para o chão.
    DropHeld,
    /// Acender/apagar a fogueira aberta.
    ToggleFire,
    /// Alternar seguir/ficar do dino aberto.
    ToggleCommand,
    /// Montar no dino aberto.
    Mount,
    /// Gastar um ponto de atributo.
    SpendAttribute(Attribute),
    /// Botão do painel de um NPC (ordem ou recrutar).
    NpcOrder(NpcCmd),
}

/// Tudo o que o HUD precisa ler do jogo.
pub struct Hud<'a> {
    pub ui: &'a UiState,
    pub inventory: &'a Inventory,
    /// Pilha presa no cursor (vem do servidor).
    pub held: Option<Stack>,
    pub chat: &'a [ChatLine],
    /// Jogadores conectados (para o canto da tela).
    pub online: usize,
    pub container: Option<ContainerView<'a>>,
    /// Montaria atual, para a barra de vida dela.
    pub mount: Option<&'a Creature>,
    /// Remando numa jangada.
    pub on_raft: bool,
    pub progression: &'a Progression,
    /// Flash vermelho de dano recebido (s restantes).
    pub hurt: f32,
    pub queue: &'a CraftQueue,
    /// Receitas que pedem estação estão liberadas (estação por perto)?
    pub station_ok: &'a dyn Fn(&Recipe) -> bool,
    pub selected: usize,
    pub messages: &'a [(String, f32)],
    pub vitals: &'a Vitals,
    pub dead: bool,
    /// O jogador tem cama (botão de renascer nela).
    pub has_bed: bool,
    pub time: f32,
}

// ---------- Layout ----------

enum RightLayout {
    Craft {
        tabs: Vec<Rect>,
        recipes: Vec<Rect>,
        /// Índice em `RECIPES` de cada receita da aba.
        indices: Vec<usize>,
        detail: Rect,
        queue: Vec<Rect>,
    },
    Container {
        slots: Vec<Rect>,
        /// Área abaixo dos slots para status/barras.
        info: Rect,
        /// Botões, na ordem de `PanelKind::buttons`.
        buttons: Vec<Rect>,
    },
}

struct PanelLayout {
    char_panel: Rect,
    /// Botões "+" de cada atributo, na ordem de `Attribute::ALL`.
    attr_buttons: Vec<Rect>,
    inv_panel: Rect,
    slots: Vec<Rect>,
    right_panel: Rect,
    right: RightLayout,
}

fn grid(origin: Vec2, n: usize, cols: usize, size: f32) -> Vec<Rect> {
    (0..n)
        .map(|i| {
            let (c, r) = ((i % cols) as f32, (i / cols) as f32);
            Rect::new(origin.x + c * (size + GAP), origin.y + r * (size + GAP), size, size)
        })
        .collect()
}

fn grid_height(n: usize, cols: usize, size: f32) -> f32 {
    n.div_ceil(cols) as f32 * (size + GAP) - GAP
}

/// `container`: (slots, tipo) do contêiner aberto, se houver (substitui o painel de crafting).
/// Receitas da maior aba (o painel não muda de tamanho ao trocar de aba).
fn max_tab_len() -> usize {
    Category::ALL
        .iter()
        .map(|&c| crafting::recipes_in(c).len())
        .max()
        .unwrap_or(0)
}

fn panel_layout(n_slots: usize, container: Option<(usize, PanelKind)>, queue_len: usize, tab: Category) -> PanelLayout {
    let grid_w = COLS as f32 * (SLOT + GAP) - GAP;
    let inv_w = grid_w + PAD * 2.0;
    let inv_h = HEADER + PAD * 2.0 + grid_height(n_slots, COLS, SLOT) + 8.0;
    let right_h = match container {
        Some((n, kind)) => HEADER + PAD * 2.0 + grid_height(n, COLS, SLOT) + 24.0 + kind.extra_height(),
        None => {
            HEADER
                + PAD * 2.0
                + TAB_H
                + grid_height(max_tab_len(), COLS, SLOT)
                + DETAIL_H
                + 24.0
                + 22.0
                + SMALL_SLOT
                + 48.0
        }
    };
    let total_w = CHAR_W + 16.0 + inv_w + 16.0 + RIGHT_W;
    let cx = ((screen_width() - total_w) / 2.0).max(8.0);
    let y0 = ((screen_height() - inv_h.max(right_h)) / 2.0 - 30.0).max(8.0);
    let char_panel = Rect::new(cx, y0, CHAR_W, inv_h);
    let attr_buttons = (0..Attribute::ALL.len())
        .map(|i| {
            Rect::new(
                cx + CHAR_W - PAD - 24.0,
                y0 + ATTR_TOP + i as f32 * ATTR_ROW - 18.0,
                24.0,
                24.0,
            )
        })
        .collect();
    let x0 = cx + CHAR_W + 16.0;

    let mut slots = grid(vec2(x0 + PAD, y0 + HEADER + PAD), n_slots, COLS, SLOT);
    // Primeira linha (hotbar) levemente separada do resto.
    for r in slots.iter_mut().skip(COLS) {
        r.y += 8.0;
    }

    let rx = x0 + inv_w + 16.0;
    let top = vec2(rx + PAD, y0 + HEADER + PAD);
    let right = match container {
        Some((n, kind)) => {
            let slots = grid(top, n, COLS, SLOT);
            let info_y = top.y + grid_height(n, COLS, SLOT) + 8.0;
            let info = Rect::new(top.x, info_y, RIGHT_W - PAD * 2.0, kind.extra_height());
            // Botões em linhas de três, de baixo para cima.
            let n = kind.buttons().len();
            let rows = n.div_ceil(3);
            let buttons = (0..n)
                .map(|i| {
                    let (col, row) = ((i % 3) as f32, (i / 3) as f32);
                    let y = info.bottom() - 40.0 - (rows as f32 - 1.0 - row) * 38.0;
                    Rect::new(top.x + col * 122.0, y, 112.0, 32.0)
                })
                .collect();
            RightLayout::Container { slots, info, buttons }
        }
        None => {
            let tab_w = (RIGHT_W - PAD * 2.0 - GAP * 2.0) / 3.0;
            let tabs = (0..Category::ALL.len())
                .map(|i| Rect::new(top.x + i as f32 * (tab_w + GAP), top.y, tab_w, TAB_H - 8.0))
                .collect();
            let indices = crafting::recipes_in(tab);
            let grid_top = top + vec2(0.0, TAB_H);
            let recipes = grid(grid_top, indices.len(), COLS, SLOT);
            let detail_y = grid_top.y + grid_height(max_tab_len(), COLS, SLOT) + 12.0;
            let detail = Rect::new(top.x, detail_y, RIGHT_W - PAD * 2.0, DETAIL_H);
            let queue_y = detail.bottom() + 12.0 + 22.0;
            let queue = grid(vec2(top.x, queue_y), queue_len, MAX_QUEUE, SMALL_SLOT);
            RightLayout::Craft {
                tabs,
                recipes,
                indices,
                detail,
                queue,
            }
        }
    };

    PanelLayout {
        char_panel,
        attr_buttons,
        inv_panel: Rect::new(x0, y0, inv_w, inv_h),
        slots,
        right_panel: Rect::new(rx, y0, RIGHT_W, right_h),
        right,
    }
}

fn hotbar_slots() -> Vec<Rect> {
    let w = HOTBAR_SIZE as f32 * (SLOT + GAP) - GAP;
    let x0 = (screen_width() - w) / 2.0;
    let y = screen_height() - SLOT - 16.0;
    (0..HOTBAR_SIZE)
        .map(|i| Rect::new(x0 + i as f32 * (SLOT + GAP), y, SLOT, SLOT))
        .collect()
}

fn mouse() -> Vec2 {
    mouse_position().into()
}

// ---------- Input ----------

/// O cursor está sobre algum elemento de UI (para não coletar ao clicar na hotbar)?
pub fn pointer_over_ui(ui: &UiState) -> bool {
    let m = mouse();
    ui.inventory_open || hotbar_slots().iter().any(|r| r.contains(m))
}

/// Traduz cliques nos painéis em ações. Nada é alterado aqui: o servidor aplica as ações.
/// `container`: (tipo, nº de slots) do contêiner aberto; `holding`: há pilha no cursor.
pub fn handle_input(
    ui: &UiState,
    inv_slots: usize,
    container: Option<(PanelKind, usize)>,
    queue_len: usize,
    holding: bool,
) -> Option<UiAction> {
    let kind = container.map(|c| c.0);
    let m = mouse();
    if !ui.inventory_open {
        if is_mouse_button_pressed(MouseButton::Left) {
            return hotbar_slots()
                .iter()
                .position(|r| r.contains(m))
                .map(UiAction::SelectHotbar);
        }
        return None;
    }

    let layout = panel_layout(inv_slots, container.map(|(k, n)| (n, k)), queue_len, ui.craft_tab);
    let left = is_mouse_button_pressed(MouseButton::Left);
    let right = is_mouse_button_pressed(MouseButton::Right);
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    if !left && !right {
        return None;
    }

    if let Some(i) = layout.slots.iter().position(|r| r.contains(m)) {
        return Some(UiAction::Slot {
            target: SlotTarget::Inventory(i),
            left,
            shift,
        });
    }

    match &layout.right {
        RightLayout::Container { slots, buttons, .. } => {
            if left
                && let Some(i) = buttons.iter().position(|b| b.contains(m))
                && let Some(kind) = kind
            {
                return Some(kind.buttons()[i]);
            }
            if let Some(i) = slots.iter().position(|r| r.contains(m)) {
                return Some(UiAction::Slot {
                    target: SlotTarget::Container(i),
                    left,
                    shift,
                });
            }
        }
        RightLayout::Craft {
            tabs,
            recipes,
            indices,
            queue,
            ..
        } => {
            if left && let Some(i) = tabs.iter().position(|r| r.contains(m)) {
                return Some(UiAction::CraftTab(Category::ALL[i]));
            }
            if left
                && !holding
                && let Some(i) = recipes.iter().position(|r| r.contains(m))
            {
                return Some(UiAction::Craft(indices[i]));
            }
            if right && let Some(i) = queue.iter().position(|r| r.contains(m)) {
                return Some(UiAction::CancelCraft(i));
            }
        }
    }

    if left && let Some(i) = layout.attr_buttons.iter().position(|b| b.contains(m)) {
        return Some(UiAction::SpendAttribute(Attribute::ALL[i]));
    }

    let over_panel = layout.inv_panel.contains(m) || layout.right_panel.contains(m) || layout.char_panel.contains(m);
    if left && !over_panel && holding {
        return Some(UiAction::DropHeld);
    }
    None
}

// ---------- Desenho ----------

pub fn draw(hud: &Hud) {
    if hud.dead {
        draw_death_screen(hud.has_bed);
        draw_messages(hud.messages);
        return;
    }
    if hud.ui.hide_help {
        shadowed("H: ajuda", 12.0, 22.0, 20.0, DIM);
    } else {
        draw_help();
    }
    if hud.hurt > 0.0 {
        let a = (hud.hurt / 0.35).min(1.0) * 0.25;
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.8, 0.0, 0.0, a));
    }
    draw_clock(hud.time, hud.online);
    draw_vitals(hud.vitals);
    draw_xp_bar(hud.progression);
    if hud.on_raft {
        let y = screen_height() - 30.0 - 5.0 * 24.0 - 18.0;
        shadowed(
            "Jangada   F: descer na margem   Shift: remar mais rápido",
            16.0,
            y,
            18.0,
            OK,
        );
    }
    if let Some(mount) = hud.mount {
        draw_mount_bar(mount);
    }
    draw_messages(hud.messages);
    draw_chat(hud.ui, hud.chat);
    if hud.ui.inventory_open {
        draw_panels(hud);
    } else {
        draw_queue_hud(hud.queue);
        draw_hotbar(hud.inventory, hud.selected, hud.mount.map(mount_hint));
    }
}

fn shadowed(text: &str, x: f32, y: f32, size: f32, color: Color) {
    draw_text(text, x + 1.0, y + 1.0, size, Color::new(0.0, 0.0, 0.0, 0.55 * color.a));
    draw_text(text, x, y, size, color);
}

/// Últimas mensagens de chat (esmaecem) e a caixa de digitação.
fn draw_chat(ui: &UiState, chat: &[ChatLine]) {
    let x = 16.0;
    let base = screen_height() - 190.0;
    let visible: Vec<&ChatLine> = chat
        .iter()
        .rev()
        .filter(|(_, _, age)| ui.chat_open || *age < CHAT_VISIBLE_SECS)
        .take(8)
        .collect();
    for (i, (from, text, age)) in visible.iter().enumerate() {
        let alpha = if ui.chat_open {
            1.0
        } else {
            (CHAT_VISIBLE_SECS - age).clamp(0.0, 1.0)
        };
        let y = base - i as f32 * 20.0;
        let line = format!("{from}: {text}");
        shadowed(&line, x, y, 18.0, Color::new(0.9, 0.95, 1.0, alpha));
    }
    if ui.chat_open {
        let y = base + 24.0;
        let w = 420.0;
        draw_rectangle(x - 4.0, y - 18.0, w, 26.0, Color::new(0.0, 0.0, 0.0, 0.7));
        let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "_" } else { " " };
        let text = format!("Dizer: {}{caret}", ui.chat_input);
        draw_text(&text, x + 2.0, y, 18.0, TEXT);
        let count = format!("{}/{MAX_CHAT_LEN}", ui.chat_input.chars().count());
        draw_text(&count, x + w - 60.0, y, 16.0, DIM);
    }
}

fn draw_help() {
    let lines = [
        "WASD: mover   Shift: correr   E / Clique: coletar ou beber   R / Botão dir.: comer",
        "F: interagir   Q: largar (Ctrl+Q: pilha)   Estrutura na mão: clique posiciona, T gira, X demole",
        "Martelo na mão: E / clique repara estruturas (gasta parte dos materiais)",
        "Montado: E / Clique morde   Espaço: decolar/pousar (voadores)",
        "Tribo (apito): Z seguir   C ficar   B defender   V atacar o bicho sob o mouse",
        "1-6 / Roda: hotbar   Tab / I: inventário   Enter: chat   F5: salvar   H: esconder ajuda",
    ];
    for (i, line) in lines.iter().enumerate() {
        shadowed(line, 12.0, 22.0 + i as f32 * 20.0, 20.0, TEXT);
    }
}

fn draw_clock(time: f32, online: usize) {
    let hour = daynight::hour(time);
    let label = format!(
        "Dia {}  {:02}:{:02}",
        daynight::day(time),
        hour as u32,
        ((hour.fract()) * 60.0) as u32 / 10 * 10
    );
    let size = 24.0;
    let w = measure_text(&label, None, size as u16, 1.0).width;
    let x = screen_width() - w - 16.0;
    shadowed(&label, x, 30.0, size, TEXT);
    let (ix, iy) = (x - 16.0, 22.0);
    if online > 1 {
        let players = format!("{online} jogadores");
        let pw = measure_text(&players, None, 18, 1.0).width;
        shadowed(&players, ix - 24.0 - pw, 28.0, 18.0, OK);
    }
    if daynight::darkness(hour) < 0.4 {
        draw_circle(ix, iy, 8.0, Color::new(1.0, 0.85, 0.3, 1.0));
    } else {
        draw_circle(ix, iy, 8.0, Color::new(0.85, 0.88, 1.0, 1.0));
        draw_circle(ix + 4.0, iy - 3.0, 7.0, Color::new(0.10, 0.11, 0.18, 1.0));
    }
}

fn draw_vitals(v: &Vitals) {
    let bars = [
        ("Vida", v.health / v.max.health, Color::new(0.85, 0.25, 0.25, 1.0)),
        ("Comida", v.food / v.max.food, Color::new(0.90, 0.60, 0.20, 1.0)),
        ("Água", v.water / v.max.water, Color::new(0.25, 0.60, 0.95, 1.0)),
        ("Stamina", v.stamina / v.max.stamina, Color::new(0.85, 0.85, 0.30, 1.0)),
    ];
    let (w, h, x) = (170.0, 12.0, 16.0);
    let blink = (get_time() * 6.0).sin() > 0.0;
    for (i, (label, frac, color)) in bars.into_iter().enumerate() {
        let y = screen_height() - 30.0 - (bars.len() - 1 - i) as f32 * 24.0;
        let low = frac < 0.2;
        let label_color = if low && blink { BAD } else { TEXT };
        shadowed(label, x, y + 11.0, 18.0, label_color);
        let bx = x + 64.0;
        draw_rectangle(bx, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
        let fill = if label == "Stamina" && v.exhausted { DIM } else { color };
        draw_rectangle(bx, y, w * frac.clamp(0.0, 1.0), h, fill);
        draw_rectangle_lines(bx, y, w, h, 1.0, BORDER);
    }
}

/// Botões da tela de morte: (retângulo, renascer na cama?, texto). O primeiro é o do Enter.
pub fn death_buttons(has_bed: bool) -> Vec<(Rect, bool, &'static str)> {
    let (w, h) = (260.0, 44.0);
    let x = (screen_width() - w) / 2.0;
    let y = screen_height() / 2.0 + 70.0;
    let mut buttons = Vec::new();
    if has_bed {
        buttons.push((Rect::new(x, y, w, h), true, "Renascer na cama"));
    }
    let y = y + buttons.len() as f32 * (h + 12.0);
    buttons.push((Rect::new(x, y, w, h), false, "Renascer na praia"));
    buttons
}

fn draw_death_screen(has_bed: bool) {
    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.25, 0.0, 0.0, 0.6),
    );
    let center = |text: &str, y: f32, size: f32, color: Color| {
        let w = measure_text(text, None, size as u16, 1.0).width;
        draw_text(text, (screen_width() - w) / 2.0, y, size, color);
    };
    let cy = screen_height() / 2.0;
    center("Você morreu", cy - 20.0, 64.0, Color::new(1.0, 0.85, 0.8, 1.0));
    center("Seus itens ficaram numa bolsa onde você caiu.", cy + 24.0, 24.0, TEXT);
    center("Enter: primeira opção", cy + 54.0, 18.0, DIM);
    let m = mouse();
    for (rect, _, label) in death_buttons(has_bed) {
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
}

fn draw_messages(messages: &[(String, f32)]) {
    for (i, (msg, ttl)) in messages.iter().rev().take(4).enumerate() {
        let alpha = ttl.min(1.0);
        let size = 24.0;
        let w = measure_text(msg, None, size as u16, 1.0).width;
        let x = (screen_width() - w) / 2.0;
        let y = screen_height() - 100.0 - i as f32 * 26.0;
        shadowed(msg, x, y, size, Color::new(1.0, 0.85, 0.6, alpha));
    }
}

/// Indicador compacto da fila de crafting quando o inventário está fechado.
fn draw_queue_hud(queue: &CraftQueue) {
    let (Some(job), Some(progress)) = (queue.jobs.first(), queue.current_progress()) else {
        return;
    };
    let (w, x, y) = (200.0, screen_width() - 216.0, 48.0);
    let icon = Rect::new(x, y, 32.0, 32.0);
    draw_rectangle(x - 6.0, y - 4.0, w + 12.0, 40.0, Color::new(0.0, 0.0, 0.0, 0.45));
    draw_item_icon(job.output, icon);
    let extra = queue.jobs.len() - 1;
    let label = if extra > 0 {
        format!("{} (+{extra})", job.output.name())
    } else {
        job.output.name().to_owned()
    };
    draw_text(&label, icon.right() + 6.0, y + 14.0, 18.0, TEXT);
    let bar = Rect::new(icon.right() + 6.0, y + 22.0, w - 38.0, 6.0);
    draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(1.0, 1.0, 1.0, 0.15));
    draw_rectangle(bar.x, bar.y, bar.w * progress, bar.h, ACCENT);
}

/// Vida da montaria, acima das barras do jogador.
/// Nível e barra de XP, logo acima das barras de status.
fn draw_xp_bar(p: &Progression) {
    let (x, w, h) = (16.0, 170.0, 6.0);
    let y = screen_height() - 30.0 - 4.0 * 24.0 + 4.0;
    let mut label = format!("Nv {}", p.level);
    if p.attribute_points > 0 {
        label.push_str(" +");
    }
    shadowed(&label, x, y + 7.0, 18.0, ACCENT);
    let bx = x + 64.0;
    draw_rectangle(bx, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(bx, y, w * p.xp_frac(), h, ACCENT);
}

fn draw_mount_bar(mount: &Creature) {
    let (x, w, h) = (16.0, 234.0, 10.0);
    let y = screen_height() - 30.0 - 5.0 * 24.0 - 14.0;
    let mut label = format!("{} Nv {}   F: desmontar", mount.species.name(), mount.level);
    if mount.species.leaps() {
        label.push_str("   Espaço: pular");
    }
    if mount.species.flies() {
        label.push_str(if mount.flying {
            "   Espaço: pousar"
        } else {
            "   Espaço: voar"
        });
    }
    shadowed(&label, x, y - 4.0, 18.0, OK);
    if mount.overweight() {
        shadowed("Sobrecarregado: lento e sem voar", x, y - 24.0, 18.0, BAD);
    }
    draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(x, y, w * mount.health_frac(), h, Color::new(0.4, 0.8, 0.4, 1.0));
    draw_rectangle_lines(x, y, w, h, 1.0, BORDER);
    let load = format!("Carga {:.0}/{:.0}", mount.weight(), mount.max_weight());
    let color = if mount.overweight() { BAD } else { DIM };
    // Logo depois do rótulo (que fica comprido com as dicas de Espaço).
    let label_w = measure_text(&label, None, 18, 1.0).width;
    shadowed(&load, x + label_w.max(w) + 16.0, y - 4.0, 18.0, color);
}

/// O que E / clique faz montado em `mount`.
fn mount_hint(mount: &Creature) -> &'static str {
    match (mount.species.mount_harvest(), mount.species.attack()) {
        (Some(MountHarvest::Wood), _) => "montado: E / clique morde ou derruba árvores",
        (Some(MountHarvest::Hide), _) => "montado: E / clique morde ou tira couro de carcaças",
        _ if mount.species.leaps() => "montado: Espaço pula",
        (None, Some(_)) => "montado: E / clique morde",
        (None, None) => "montado",
    }
}

/// `mount_hint`: dica de montaria no lugar da do item.
fn draw_hotbar(inv: &Inventory, selected: usize, mount_hint: Option<&str>) {
    for (i, rect) in hotbar_slots().iter().enumerate() {
        draw_slot(*rect, inv.slots[i], i == selected);
        draw_text((i + 1).to_string(), rect.x + 4.0, rect.y + 14.0, 16.0, DIM);
    }
    if let Some(stack) = inv.slots[selected] {
        let name = match stack.item {
            _ if mount_hint.is_some() => format!("{}  ({})", stack.item.name(), mount_hint.unwrap_or_default()),
            item if item.structure().is_some_and(StructureKind::rotates) => {
                format!("{}  (clique para posicionar, T gira)", item.name())
            }
            item if item.structure().is_some() => format!("{}  (clique para posicionar)", item.name()),
            ItemId::Hammer => "Martelo  (E / clique numa estrutura danificada repara)".to_owned(),
            ItemId::Slingshot => format!(
                "Estilingue  ({} pedras, clique mira no cursor)",
                inv.count(ItemId::Stone)
            ),
            ItemId::Narcotic => "Narcótico  (coloque no inventário do dino desacordado)".to_owned(),
            ItemId::Raft => "Jangada  (R / botão direito na beira da água)".to_owned(),
            item => item.name().to_owned(),
        };
        let w = measure_text(&name, None, 20, 1.0).width;
        let y = hotbar_slots()[0].y - 8.0;
        shadowed(&name, (screen_width() - w) / 2.0, y, 20.0, TEXT);
    }
}

fn draw_panel(rect: Rect, title: &str) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL_BG);
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.0, BORDER);
    draw_text(title, rect.x + PAD, rect.y + 28.0, 26.0, TEXT);
}

fn draw_panels(hud: &Hud) {
    let inv = hud.inventory;
    let storage = hud.container.and_then(|c| c.inventory());
    let layout = panel_layout(
        inv.slots.len(),
        hud.container.zip(storage).map(|(c, s)| (s.slots.len(), c.kind())),
        hud.queue.jobs.len(),
        hud.ui.craft_tab,
    );
    let m = mouse();

    draw_rectangle(
        0.0,
        0.0,
        screen_width(),
        screen_height(),
        Color::new(0.0, 0.0, 0.0, 0.35),
    );

    draw_character_panel(&layout, hud, m);

    let p = layout.inv_panel;
    draw_panel(p, "Inventário");
    draw_text("hotbar", p.right() - 70.0, p.y + 28.0, 18.0, DIM);
    for (i, rect) in layout.slots.iter().enumerate() {
        draw_slot(*rect, inv.slots[i], false);
    }
    let mut tooltip: Option<String> = layout
        .slots
        .iter()
        .position(|r| r.contains(m))
        .and_then(|i| inv.slots[i])
        .map(|s| stack_tooltip(&s));

    let rp = layout.right_panel;
    match (&layout.right, hud.container.zip(storage)) {
        (RightLayout::Container { slots, info, buttons }, Some((view, chest))) => {
            draw_panel(rp, &view.title());
            for (i, rect) in slots.iter().enumerate() {
                draw_slot(*rect, chest.slots[i], false);
            }
            if let Some(s) = slots.iter().position(|r| r.contains(m)).and_then(|i| chest.slots[i]) {
                tooltip = Some(stack_tooltip(&s));
            }
            match view {
                ContainerView::Structure(fire) if view.kind() == PanelKind::Campfire => {
                    draw_fire_controls(fire, slots, buttons[0], m);
                }
                ContainerView::Creature(c) if c.is_tamed() => draw_tamed_info(c, *info, buttons, m),
                ContainerView::Creature(c) => draw_taming_info(c, *info),
                ContainerView::Npc(n) if n.owner.is_some() => draw_npc_info(n, *info, buttons, m),
                ContainerView::Npc(n) => draw_recruit_info(n, inv, *info, buttons[0], m),
                ContainerView::Structure(_) => {}
            }
            if view.kind() != PanelKind::Recruit {
                draw_text(
                    "Shift+clique: transferir tudo",
                    rp.x + PAD,
                    rp.bottom() - 12.0,
                    18.0,
                    DIM,
                );
            }
        }
        (
            RightLayout::Craft {
                tabs,
                recipes,
                indices,
                detail,
                queue,
            },
            _,
        ) => {
            draw_panel(rp, "Crafting");
            for (rect, &tab) in tabs.iter().zip(&Category::ALL) {
                let active = tab == hud.ui.craft_tab;
                let bg = if active { ACCENT } else { SLOT_BG };
                draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
                draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, BORDER);
                let label = tab.label();
                let w = measure_text(label, None, 18, 1.0).width;
                let color = if active { Color::new(0.1, 0.08, 0.05, 1.0) } else { TEXT };
                draw_text(label, rect.x + (rect.w - w) / 2.0, rect.y + 19.0, 18.0, color);
            }
            let hovered = recipes.iter().position(|r| r.contains(m));
            for (i, (rect, recipe)) in recipes.iter().zip(indices.iter().map(|&k| &RECIPES[k])).enumerate() {
                let known = hud.progression.knows(recipe.output);
                let craftable = known && crafting::can_afford(inv, recipe) && (hud.station_ok)(recipe);
                draw_slot(*rect, None, false);
                draw_item_icon(recipe.output, *rect);
                draw_count(*rect, recipe.count);
                if !known {
                    draw_lock(*rect, recipe.level);
                    let learnable = hud.progression.can_learn(recipe).is_ok();
                    if learnable {
                        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, OK);
                    }
                } else if craftable {
                    let thick = if hovered == Some(i) { 3.0 } else { 1.5 };
                    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, thick, ACCENT);
                } else {
                    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.0, 0.0, 0.0, 0.45));
                }
            }
            draw_recipe_detail(*detail, hovered.map(|i| &RECIPES[indices[i]]), hud);
            draw_queue(*detail, queue, hud.queue);
            if let Some(i) = queue.iter().position(|r| r.contains(m)) {
                tooltip = Some(format!("{} (botão dir.: cancelar)", hud.queue.jobs[i].output.name()));
            }
            draw_text(
                "Clique: fabricar   Botão dir. na fila: cancelar",
                rp.x + PAD,
                rp.bottom() - 30.0,
                16.0,
                DIM,
            );
            draw_text(
                "Solte um item fora dos painéis para largar",
                rp.x + PAD,
                rp.bottom() - 12.0,
                16.0,
                DIM,
            );
        }
        _ => {}
    }

    if let Some(held) = hud.held {
        let r = Rect::new(m.x - SLOT / 2.0, m.y - SLOT / 2.0, SLOT, SLOT);
        draw_item_icon(held.item, r);
        draw_count(r, held.count);
    } else if let Some(text) = tooltip {
        draw_tooltip(&text, m);
    }
}

/// Status da fogueira (combustível, cozimento) e botão acender/apagar.
fn draw_fire_controls(fire: &Structure, slots: &[Rect], button: Rect, m: Vec2) {
    let y = slots.last().map_or(button.y - 40.0, |r| r.bottom()) + 24.0;
    let x = button.x;
    let status = if fire.is_lit() {
        let secs = fire.fuel.ceil() as u32;
        format!("Acesa: {}:{:02} nesta lenha", secs / 60, secs % 60)
    } else {
        "Apagada. Coloque madeira ou palha.".to_owned()
    };
    draw_text(&status, x, y, 18.0, if fire.is_lit() { ACCENT } else { DIM });

    draw_button(button, if fire.active { "Apagar" } else { "Acender" }, true, m);

    if let Some(process) = fire.kind.process()
        && fire.is_lit()
        && fire.cook > 0.0
    {
        let bar = Rect::new(button.right() + 16.0, button.y + 20.0, 200.0, 6.0);
        draw_text(process.label, bar.x, button.y + 12.0, 16.0, TEXT);
        draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(1.0, 1.0, 1.0, 0.15));
        draw_rectangle(bar.x, bar.y, bar.w * (fire.cook / process.secs).min(1.0), bar.h, OK);
    }
}

fn draw_bar(x: f32, y: f32, w: f32, label: &str, frac: f32, color: Color) {
    draw_text(label, x, y, 16.0, TEXT);
    draw_rectangle(x, y + 4.0, w, 7.0, Color::new(1.0, 1.0, 1.0, 0.12));
    draw_rectangle(x, y + 4.0, w * frac.clamp(0.0, 1.0), 7.0, color);
}

fn draw_button(rect: Rect, label: &str, enabled: bool, m: Vec2) {
    let bg = match (enabled, rect.contains(m)) {
        (true, true) => ACCENT,
        (true, false) => Color::new(0.85, 0.65, 0.25, 1.0),
        _ => Color::new(1.0, 1.0, 1.0, 0.10),
    };
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
    let w = measure_text(label, None, 20, 1.0).width;
    let fg = if enabled { Color::new(0.1, 0.08, 0.05, 1.0) } else { DIM };
    draw_text(label, rect.x + (rect.w - w) / 2.0, rect.y + 22.0, 20.0, fg);
}

/// Barras de torpor e domesticação de um dino desacordado + dica de comida.
fn draw_taming_info(c: &Creature, area: Rect) {
    let w = area.w;
    let torpor_label = format!("Torpor {:.0}/{:.0}", c.torpor, c.max_torpor());
    draw_bar(
        area.x,
        area.y + 14.0,
        w,
        &torpor_label,
        c.torpor_frac(),
        Color::new(0.65, 0.4, 0.95, 1.0),
    );
    match c.taming {
        Some(t) => {
            let label = format!(
                "Domesticação {:.0}%   Efetividade {:.0}%",
                t.progress * 100.0,
                t.effectiveness * 100.0
            );
            draw_bar(area.x, area.y + 44.0, w, &label, t.progress, OK);
            let foods: Vec<&str> = taming::preferred_foods(c.species.diet())
                .iter()
                .map(|f| f.name())
                .collect();
            let hint = format!("Come: {}", foods.join(" > "));
            draw_text(&hint, area.x, area.y + 80.0, 16.0, DIM);
            draw_text(
                "Narcótico o mantém dormindo. Não bata nele!",
                area.x,
                area.y + 98.0,
                16.0,
                DIM,
            );
        }
        None => {
            draw_text(
                "Esta espécie não pode ser domesticada.",
                area.x,
                area.y + 50.0,
                18.0,
                BAD,
            );
        }
    }
}

/// Vida, ordem atual e botões de ordem de um membro da tribo.
fn draw_npc_info(n: &NpcView, area: Rect, buttons: &[Rect], m: Vec2) {
    let label = format!("Vida {:.0}/{:.0}", n.health, crate::npc::MAX_HEALTH);
    draw_bar(
        area.x,
        area.y + 14.0,
        area.w,
        &label,
        n.health_frac(),
        Color::new(0.4, 0.8, 0.4, 1.0),
    );
    let mut order = n.order.label();
    if n.hauling {
        order.push_str(" (levando ao baú)");
    }
    draw_text(format!("Ordem: {order}"), area.x, area.y + 44.0, 18.0, TEXT);
    draw_text(
        "Dê ferramentas e armas. A coleta vai para um baú.",
        area.x,
        area.y + 64.0,
        16.0,
        DIM,
    );
    let labels = ["Seguir", "Ficar", "Defender", "Madeira", "Pedra", "Metal"];
    for (rect, label) in buttons.iter().zip(labels) {
        draw_button(*rect, label, true, m);
    }
}

/// Pedido do morador para entrar na tribo e o botão de recrutar.
fn draw_recruit_info(n: &NpcView, inv: &Inventory, area: Rect, button: Rect, m: Vec2) {
    let (item, count) = n.recruit_cost();
    let have = inv.count(item);
    draw_text(
        format!("{} vive neste acampamento.", n.name),
        area.x,
        area.y + 10.0,
        18.0,
        TEXT,
    );
    draw_text(
        format!("Para entrar na sua tribo, pede {count} {}.", item.name()),
        area.x,
        area.y + 32.0,
        18.0,
        TEXT,
    );
    let color = if have >= count { OK } else { BAD };
    draw_text(format!("Você tem {have}."), area.x, area.y + 52.0, 18.0, color);
    draw_button(button, "Recrutar", have >= count, m);
}

/// Vida, ordem atual e botões de um dino domesticado.
fn draw_tamed_info(c: &Creature, area: Rect, buttons: &[Rect], m: Vec2) {
    let label = format!("Vida {:.0}/{:.0}", c.health, c.max_health());
    draw_bar(
        area.x,
        area.y + 14.0,
        area.w,
        &label,
        c.health_frac(),
        Color::new(0.4, 0.8, 0.4, 1.0),
    );
    let (order, toggle) = match c.command {
        Some(Command::Stay) => ("Parado", "Seguir"),
        Some(Command::Defend) => ("Defendendo o lugar", "Seguir"),
        _ => ("Seguindo você", "Ficar"),
    };
    draw_text(format!("Ordem: {order}"), area.x, area.y + 44.0, 18.0, TEXT);
    let load = format!("Carga {:.0}/{:.0}", c.weight(), c.max_weight());
    let load_color = if c.overweight() { BAD } else { TEXT };
    let lw = measure_text(&load, None, 18, 1.0).width;
    draw_text(&load, area.x + area.w - lw, area.y + 44.0, 18.0, load_color);
    let ride_hint = match c.species.saddle() {
        _ if c.overweight() => "Sobrecarregado: anda devagar e não voa.".to_owned(),
        Some(_) if c.can_ride() && c.species.flies() => "Pronto para montar. Espaço decola.".to_owned(),
        Some(_) if c.can_ride() => "Pronto para montar.".to_owned(),
        Some(saddle) => format!("Coloque uma {} aqui para montar.", saddle.name()),
        None => "Esta espécie não pode ser montada.".to_owned(),
    };
    draw_text(&ride_hint, area.x, area.y + 64.0, 16.0, DIM);
    draw_button(buttons[0], toggle, true, m);
    draw_button(buttons[1], "Montar", c.can_ride(), m);
}

fn draw_character_panel(layout: &PanelLayout, hud: &Hud, m: Vec2) {
    let (p, prog) = (layout.char_panel, hud.progression);
    draw_panel(p, "Personagem");
    let x = p.x + PAD;
    let w = p.w - PAD * 2.0;
    draw_text(format!("Nível {}", prog.level), x, p.y + 64.0, 24.0, ACCENT);
    let xp_label = if prog.level >= progression::MAX_LEVEL {
        "Nível máximo".to_owned()
    } else {
        format!("XP {}/{}", prog.xp, progression::xp_to_next(prog.level))
    };
    draw_bar(x, p.y + 84.0, w, &xp_label, prog.xp_frac(), ACCENT);
    let points_color = if prog.attribute_points > 0 { OK } else { DIM };
    let points = format!("Pontos de atributo: {}", prog.attribute_points);
    draw_text(&points, x, p.y + ATTR_TOP - 26.0, 16.0, points_color);

    let max = hud.vitals.max;
    for (i, attr) in Attribute::ALL.into_iter().enumerate() {
        let y = p.y + ATTR_TOP + i as f32 * ATTR_ROW;
        let value = match attr {
            Attribute::Health => format!("{:.0}", max.health),
            Attribute::Stamina => format!("{:.0}", max.stamina),
            Attribute::Food => format!("{:.0}", max.food),
            Attribute::Water => format!("{:.0}", max.water),
            Attribute::Melee => format!("{:.0}%", prog.melee_mult() * 100.0),
        };
        let spent = prog.attributes.get(attr);
        let name = if spent > 0 {
            format!("{} (+{spent})", attr.name())
        } else {
            attr.name().to_owned()
        };
        draw_text(&name, x, y, 18.0, TEXT);
        let vw = measure_text(&value, None, 18, 1.0).width;
        draw_text(&value, p.right() - PAD - 32.0 - vw, y, 18.0, TEXT);
        draw_button(layout.attr_buttons[i], "+", prog.attribute_points > 0, m);
    }

    let engrams = format!("Pontos de engrama: {}", prog.engram_points);
    let ey = p.y + ATTR_TOP + Attribute::ALL.len() as f32 * ATTR_ROW + 8.0;
    draw_text(&engrams, x, ey, 16.0, if prog.engram_points > 0 { OK } else { DIM });
    draw_text("Receita com cadeado:", x, ey + 22.0, 16.0, DIM);
    draw_text("clique para aprender.", x, ey + 40.0, 16.0, DIM);
}

/// Cadeado pequeno no canto de uma receita bloqueada.
fn draw_lock(rect: Rect, level: u32) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.0, 0.0, 0.0, 0.6));
    let (lx, ly) = (rect.x + 6.0, rect.y + 8.0);
    draw_circle_lines(lx + 5.0, ly + 3.0, 4.0, 1.5, DIM);
    draw_rectangle(lx, ly + 3.0, 10.0, 8.0, DIM);
    let label = format!("Nv {level}");
    let w = measure_text(&label, None, 16, 1.0).width;
    draw_text(&label, rect.right() - w - 3.0, rect.bottom() - 4.0, 16.0, TEXT);
}

fn stack_tooltip(stack: &Stack) -> String {
    match stack.item.max_durability() {
        Some(max) => format!("{} ({}/{max})", stack.item.name(), stack.durability),
        None => stack.item.name().to_owned(),
    }
}

fn draw_recipe_detail(area: Rect, recipe: Option<&Recipe>, hud: &Hud) {
    draw_rectangle(area.x, area.y, area.w, area.h, SLOT_BG);
    let Some(recipe) = recipe else {
        draw_text("Passe o mouse numa receita", area.x + 10.0, area.y + 26.0, 18.0, DIM);
        return;
    };
    draw_text(recipe.output.name(), area.x + 10.0, area.y + 24.0, 22.0, TEXT);
    let prog = hud.progression;
    if !prog.knows(recipe.output) {
        let level_ok = prog.level >= recipe.level;
        let points_ok = prog.engram_points >= recipe.engram_cost;
        let level = format!("Requer nível {} (você: {})", recipe.level, prog.level);
        draw_text(
            &level,
            area.x + 10.0,
            area.y + 48.0,
            18.0,
            if level_ok { OK } else { BAD },
        );
        let cost = format!(
            "Custa {} pontos de engrama (você: {})",
            recipe.engram_cost, prog.engram_points
        );
        draw_text(
            &cost,
            area.x + 10.0,
            area.y + 68.0,
            18.0,
            if points_ok { OK } else { BAD },
        );
        if level_ok && points_ok {
            draw_text("Clique para aprender", area.x + 10.0, area.y + 86.0, 16.0, ACCENT);
        }
        return;
    }
    let secs = format!("{:.0}s", recipe.secs.ceil());
    let w = measure_text(&secs, None, 18, 1.0).width;
    draw_text(&secs, area.right() - w - 10.0, area.y + 24.0, 18.0, DIM);

    let mut x = area.x + 10.0;
    for &(item, need) in recipe.inputs {
        let have = hud.inventory.count(item);
        let chip = Rect::new(x, area.y + 34.0, 22.0, 22.0);
        draw_item_icon(item, chip);
        let label = format!("{}/{}", have.min(need), need);
        let color = if have >= need { OK } else { BAD };
        draw_text(&label, chip.right() + 2.0, chip.y + 16.0, 18.0, color);
        x = chip.right() + measure_text(&label, None, 18, 1.0).width + 12.0;
    }

    if let Some(station) = recipe.station {
        let ok = (hud.station_ok)(recipe);
        let text = format!("Requer {} por perto", station.item().name());
        draw_text(
            &text,
            area.x + 10.0,
            area.bottom() - 12.0,
            18.0,
            if ok { OK } else { BAD },
        );
    }
}

fn draw_queue(detail: Rect, slots: &[Rect], queue: &CraftQueue) {
    let label = format!("Fila ({}/{MAX_QUEUE})", queue.jobs.len());
    draw_text(&label, detail.x, detail.bottom() + 12.0 + 16.0, 18.0, DIM);
    let progress = queue.current_progress().unwrap_or(0.0);
    for (i, (rect, job)) in slots.iter().zip(&queue.jobs).enumerate() {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, SLOT_BG);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, BORDER);
        draw_item_icon(job.output, *rect);
        if i == 0 {
            draw_rectangle(rect.x, rect.bottom() - 4.0, rect.w * progress, 4.0, ACCENT);
        }
    }
}

fn draw_tooltip(text: &str, at: Vec2) {
    let w = measure_text(text, None, 20, 1.0).width + 16.0;
    let x = (at.x + 14.0).min(screen_width() - w - 4.0);
    draw_rectangle(x, at.y + 10.0, w, 28.0, Color::new(0.0, 0.0, 0.0, 0.85));
    draw_text(text, x + 8.0, at.y + 30.0, 20.0, TEXT);
}

fn draw_slot(rect: Rect, stack: Option<Stack>, selected: bool) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.08, 0.09, 0.10, 0.75));
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, SLOT_BG);
    let (thick, color) = if selected { (3.0, ACCENT) } else { (1.0, BORDER) };
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, thick, color);
    if let Some(s) = stack {
        draw_item_icon(s.item, rect);
        draw_count(rect, s.count);
        if let Some(frac) = s.durability_frac() {
            let (bw, bh) = (rect.w - 8.0, 4.0);
            let (bx, by) = (rect.x + 4.0, rect.bottom() - 7.0);
            draw_rectangle(bx, by, bw, bh, Color::new(0.0, 0.0, 0.0, 0.6));
            let color = Color::new(1.0 - frac, 0.3 + frac * 0.55, 0.3, 1.0);
            draw_rectangle(bx, by, bw * frac, bh, color);
        }
    }
}

fn draw_count(rect: Rect, count: u32) {
    if count > 1 {
        let t = count.to_string();
        let w = measure_text(&t, None, 18, 1.0).width;
        shadowed(&t, rect.right() - w - 4.0, rect.bottom() - 4.0, 18.0, TEXT);
    }
}

/// Ícones provisórios desenhados com primitivas, normalizados para o retângulo do slot.
pub fn draw_item_icon(item: ItemId, rect: Rect) {
    let c = rect.center();
    let u = rect.w / 52.0; // escala relativa ao slot padrão
    let p = |dx: f32, dy: f32| vec2(c.x + dx * u, c.y + dy * u);
    let wood = Color::from_rgba(150, 104, 60, 255);
    let wood_light = Color::from_rgba(214, 170, 112, 255);
    let wood_dark = Color::from_rgba(97, 64, 33, 255);
    let stone = Color::from_rgba(150, 150, 158, 255);
    let stone_dark = Color::from_rgba(104, 104, 112, 255);
    let line = |a: Vec2, b: Vec2, t: f32, col| draw_line(a.x, a.y, b.x, b.y, t * u, col);
    let rect_at = |dx: f32, dy: f32, w: f32, h: f32, col| draw_rectangle(c.x + dx * u, c.y + dy * u, w * u, h * u, col);

    match item {
        ItemId::Wood => {
            for dy in [-7.0, 7.0] {
                let (a, b) = (p(-14.0, dy + 4.0), p(14.0, dy - 4.0));
                line(a, b, 10.0, wood);
                draw_circle(b.x, b.y, 5.0 * u, wood_light);
            }
        }
        ItemId::Thatch => {
            let col = Color::from_rgba(200, 186, 96, 255);
            for i in -3..=3 {
                let dx = i as f32 * 3.5;
                line(p(dx * 0.4, 14.0), p(dx, -14.0), 2.5, col);
            }
            line(p(-6.0, 4.0), p(6.0, 4.0), 3.0, wood);
        }
        ItemId::Stone => {
            let a = p(0.0, 2.0);
            draw_ellipse(a.x, a.y, 15.0 * u, 11.0 * u, 0.0, stone_dark);
            let b = p(-3.0, -1.0);
            draw_ellipse(b.x, b.y, 10.0 * u, 7.0 * u, 0.0, stone);
        }
        ItemId::Flint => {
            draw_triangle(
                p(-12.0, 10.0),
                p(12.0, 8.0),
                p(2.0, -14.0),
                Color::from_rgba(60, 62, 72, 255),
            );
            draw_triangle(
                p(-4.0, 4.0),
                p(4.0, 4.0),
                p(1.0, -8.0),
                Color::from_rgba(96, 100, 116, 255),
            );
        }
        ItemId::Berry => {
            let red = Color::from_rgba(196, 40, 72, 255);
            let hi = Color::from_rgba(240, 130, 150, 255);
            for (dx, dy) in [(-6.0, 4.0), (6.0, 4.0), (0.0, -6.0)] {
                let b = p(dx, dy);
                draw_circle(b.x, b.y, 7.0 * u, red);
                draw_circle(b.x - 2.0 * u, b.y - 2.0 * u, 2.0 * u, hi);
            }
            line(p(0.0, -12.0), p(4.0, -18.0), 2.0, Color::from_rgba(70, 120, 50, 255));
        }
        ItemId::RawMeat | ItemId::CookedMeat => {
            let (meat, fat) = if item == ItemId::RawMeat {
                (Color::from_rgba(200, 70, 80, 255), Color::from_rgba(240, 190, 190, 255))
            } else {
                (Color::from_rgba(150, 82, 40, 255), Color::from_rgba(210, 150, 90, 255))
            };
            let a = p(-3.0, 2.0);
            draw_ellipse(a.x, a.y, 13.0 * u, 10.0 * u, 0.0, meat);
            draw_ellipse(a.x - 3.0 * u, a.y - 3.0 * u, 6.0 * u, 3.0 * u, 0.0, fat);
            line(p(8.0, -4.0), p(16.0, -12.0), 4.0, Color::from_rgba(235, 228, 210, 255));
            let b = p(17.0, -13.0);
            draw_circle(b.x, b.y, 3.0 * u, Color::from_rgba(235, 228, 210, 255));
        }
        ItemId::Hide => {
            let col = Color::from_rgba(160, 120, 80, 255);
            draw_ellipse(c.x, c.y, 15.0 * u, 11.0 * u, 0.3, col);
            for (dx, dy) in [(-13.0, -9.0), (13.0, -9.0), (-12.0, 10.0), (12.0, 10.0)] {
                let q = p(dx, dy);
                draw_circle(q.x, q.y, 4.0 * u, col);
            }
            draw_ellipse(c.x, c.y, 7.0 * u, 5.0 * u, 0.3, Color::from_rgba(130, 94, 60, 255));
        }
        ItemId::MetalOre => {
            let a = p(0.0, 2.0);
            draw_ellipse(a.x, a.y, 15.0 * u, 11.0 * u, 0.0, stone_dark);
            for (dx, dy) in [(-6.0, -1.0), (4.0, 3.0), (1.0, -5.0)] {
                let q = p(dx, dy);
                draw_circle(q.x, q.y, 2.6 * u, Color::from_rgba(196, 120, 70, 255));
            }
            let q = p(-2.0, 4.0);
            draw_circle(q.x, q.y, 1.6 * u, Color::from_rgba(230, 230, 240, 255));
        }
        ItemId::MetalIngot => {
            let metal = Color::from_rgba(170, 176, 186, 255);
            draw_triangle(p(-16.0, 8.0), p(16.0, 8.0), p(11.0, -4.0), metal);
            draw_triangle(p(-16.0, 8.0), p(11.0, -4.0), p(-11.0, -4.0), metal);
            rect_at(-11.0, -6.0, 22.0, 3.0, Color::from_rgba(215, 220, 228, 255));
        }
        ItemId::MetalPickaxe | ItemId::MetalHatchet | ItemId::MetalSword => {
            let metal = Color::from_rgba(196, 202, 212, 255);
            let shine = Color::from_rgba(235, 240, 248, 255);
            match item {
                ItemId::MetalPickaxe => {
                    line(p(-12.0, 14.0), p(8.0, -10.0), 4.0, wood_dark);
                    line(p(-6.0, -17.0), p(18.0, 1.0), 5.0, metal);
                    line(p(-2.0, -15.0), p(8.0, -8.0), 1.5, shine);
                }
                ItemId::MetalHatchet => {
                    line(p(-12.0, 14.0), p(8.0, -10.0), 4.0, wood_dark);
                    draw_triangle(p(4.0, -16.0), p(16.0, -4.0), p(2.0, -4.0), metal);
                    line(p(6.0, -13.0), p(13.0, -6.0), 1.5, shine);
                }
                _ => {
                    line(p(-14.0, 14.0), p(-8.0, 8.0), 4.0, wood_dark);
                    line(p(-14.0, 6.0), p(-6.0, 14.0), 3.0, Color::from_rgba(150, 120, 60, 255));
                    line(p(-8.0, 8.0), p(14.0, -14.0), 5.0, metal);
                    line(p(-4.0, 3.0), p(12.0, -13.0), 1.5, shine);
                }
            }
        }
        ItemId::Forge => {
            let brick = Color::from_rgba(120, 112, 108, 255);
            rect_at(-15.0, -12.0, 30.0, 26.0, brick);
            rect_at(-15.0, -16.0, 30.0, 5.0, Color::from_rgba(150, 142, 136, 255));
            rect_at(-7.0, 0.0, 14.0, 10.0, Color::from_rgba(40, 30, 28, 255));
            draw_triangle(
                p(-5.0, 10.0),
                p(5.0, 10.0),
                p(0.0, 2.0),
                Color::from_rgba(250, 140, 40, 255),
            );
        }
        ItemId::StonePickaxe => {
            line(p(-12.0, 14.0), p(8.0, -10.0), 4.0, wood);
            line(p(-4.0, -16.0), p(16.0, 0.0), 6.0, stone);
        }
        ItemId::StoneHatchet => {
            line(p(-12.0, 14.0), p(8.0, -10.0), 4.0, wood);
            let h = p(8.0, -10.0);
            draw_circle(h.x, h.y, 8.0 * u, stone);
            draw_circle(
                h.x - 2.0 * u,
                h.y - 2.0 * u,
                3.0 * u,
                Color::from_rgba(186, 186, 194, 255),
            );
        }
        ItemId::WoodenSpear => {
            line(p(-16.0, 16.0), p(10.0, -10.0), 3.5, wood);
            draw_triangle(
                p(18.0, -18.0),
                p(6.0, -10.0),
                p(10.0, -6.0),
                Color::from_rgba(60, 62, 72, 255),
            );
        }
        ItemId::WoodenClub => {
            line(p(-14.0, 16.0), p(6.0, -8.0), 4.0, wood);
            let head = p(8.0, -11.0);
            draw_circle(head.x, head.y, 8.0 * u, wood);
            draw_circle(head.x - 2.0 * u, head.y - 2.0 * u, 3.0 * u, wood_light);
        }
        ItemId::Slingshot => {
            line(p(0.0, 16.0), p(0.0, 0.0), 4.0, wood);
            line(p(0.0, 0.0), p(-9.0, -14.0), 4.0, wood);
            line(p(0.0, 0.0), p(9.0, -14.0), 4.0, wood);
            line(p(-9.0, -14.0), p(0.0, -6.0), 1.5, Color::from_rgba(120, 84, 56, 255));
            line(p(9.0, -14.0), p(0.0, -6.0), 1.5, Color::from_rgba(120, 84, 56, 255));
            let s = p(0.0, -6.0);
            draw_circle(s.x, s.y, 3.0 * u, stone);
        }
        ItemId::Narcotic => {
            let glass = Color::from_rgba(200, 210, 220, 200);
            rect_at(-8.0, -6.0, 16.0, 20.0, glass);
            rect_at(-6.0, 0.0, 12.0, 12.0, Color::from_rgba(120, 60, 150, 255));
            rect_at(-4.0, -12.0, 8.0, 6.0, wood_dark);
        }
        ItemId::RaptorSaddle => {
            let leather = Color::from_rgba(120, 76, 40, 255);
            draw_ellipse(c.x, c.y, 16.0 * u, 9.0 * u, 0.0, leather);
            rect_at(-12.0, -8.0, 6.0, 8.0, Color::from_rgba(150, 100, 56, 255));
            line(p(-4.0, 6.0), p(-6.0, 16.0), 2.0, wood_dark);
            line(p(6.0, 6.0), p(8.0, 16.0), 2.0, wood_dark);
        }
        ItemId::PteranodonSaddle => {
            // Sela leve, com tiras que abraçam as asas.
            let leather = Color::from_rgba(96, 78, 62, 255);
            line(p(-17.0, -2.0), p(17.0, -2.0), 2.0, Color::from_rgba(150, 102, 80, 255));
            draw_ellipse(c.x, c.y, 12.0 * u, 7.0 * u, 0.0, leather);
            rect_at(-9.0, -7.0, 5.0, 6.0, Color::from_rgba(128, 104, 82, 255));
            line(p(-3.0, 5.0), p(-5.0, 15.0), 2.0, wood_dark);
            line(p(4.0, 5.0), p(6.0, 15.0), 2.0, wood_dark);
        }
        ItemId::MammothSaddle => {
            // Plataforma larga de madeira sobre couro.
            draw_ellipse(
                c.x,
                c.y + 4.0 * u,
                18.0 * u,
                8.0 * u,
                0.0,
                Color::from_rgba(110, 72, 42, 255),
            );
            rect_at(-15.0, -10.0, 30.0, 8.0, wood);
            line(p(-15.0, -6.0), p(15.0, -6.0), 1.5, wood_dark);
            line(p(-8.0, 8.0), p(-10.0, 17.0), 2.5, wood_dark);
            line(p(8.0, 8.0), p(10.0, 17.0), 2.5, wood_dark);
        }
        ItemId::SmilodonSaddle => {
            let leather = Color::from_rgba(150, 104, 60, 255);
            draw_ellipse(c.x, c.y, 15.0 * u, 8.0 * u, 0.0, leather);
            rect_at(-11.0, -7.0, 6.0, 7.0, Color::from_rgba(196, 148, 90, 255));
            line(p(-10.0, -1.0), p(10.0, -1.0), 1.5, Color::from_rgba(226, 200, 160, 255));
            line(p(-4.0, 6.0), p(-6.0, 15.0), 2.0, wood_dark);
            line(p(5.0, 6.0), p(7.0, 15.0), 2.0, wood_dark);
        }
        ItemId::BeelzebufoSaddle => {
            // Sela baixa e larga, com tiras verdes.
            let leather = Color::from_rgba(116, 90, 52, 255);
            draw_ellipse(c.x, c.y + 2.0 * u, 16.0 * u, 7.0 * u, 0.0, leather);
            line(p(-16.0, 2.0), p(16.0, 2.0), 2.0, Color::from_rgba(96, 130, 60, 255));
            rect_at(-10.0, -8.0, 6.0, 7.0, Color::from_rgba(140, 108, 64, 255));
            line(p(-6.0, 8.0), p(-9.0, 16.0), 2.0, wood_dark);
            line(p(6.0, 8.0), p(9.0, 16.0), 2.0, wood_dark);
        }
        ItemId::SarcosuchusSaddle => {
            // Sela comprida, com placas de couro.
            let leather = Color::from_rgba(84, 64, 40, 255);
            draw_ellipse(c.x, c.y, 18.0 * u, 7.0 * u, 0.0, leather);
            for i in -2..=2 {
                rect_at(i as f32 * 6.0 - 2.0, -7.0, 4.0, 4.0, Color::from_rgba(96, 110, 70, 255));
            }
            line(p(-8.0, 6.0), p(-10.0, 15.0), 2.0, wood_dark);
            line(p(8.0, 6.0), p(10.0, 15.0), 2.0, wood_dark);
        }
        ItemId::Raft => {
            for i in 0..4 {
                let ly = -8.0 + i as f32 * 5.0;
                rect_at(-15.0, ly, 30.0, 4.5, if i % 2 == 0 { wood } else { wood_dark });
            }
            line(p(-5.0, -2.0), p(-5.0, -18.0), 2.0, wood_dark);
            draw_triangle(
                p(-4.0, -17.0),
                p(-4.0, -5.0),
                p(8.0, -7.0),
                Color::from_rgba(236, 226, 196, 255),
            );
        }
        ItemId::KangarooSaddle => {
            let leather = Color::from_rgba(146, 96, 56, 255);
            draw_ellipse(c.x, c.y, 14.0 * u, 8.0 * u, 0.0, leather);
            rect_at(-10.0, -9.0, 6.0, 8.0, Color::from_rgba(178, 112, 66, 255));
            line(p(-14.0, 0.0), p(14.0, 0.0), 2.0, Color::from_rgba(224, 186, 142, 255));
            line(p(-4.0, 6.0), p(-6.0, 15.0), 2.0, wood_dark);
            line(p(5.0, 6.0), p(7.0, 15.0), 2.0, wood_dark);
        }
        ItemId::MegalodonSaddle | ItemId::IchthyosaurSaddle | ItemId::MosasaurSaddle | ItemId::KrakenSaddle => {
            // Selas do mar: couro com fivelas e uma tira colorida por espécie.
            let (stripe, w) = match item {
                ItemId::MegalodonSaddle => (Color::from_rgba(112, 122, 136, 255), 16.0),
                ItemId::IchthyosaurSaddle => (Color::from_rgba(92, 130, 170, 255), 14.0),
                ItemId::MosasaurSaddle => (Color::from_rgba(62, 98, 108, 255), 19.0),
                _ => (Color::from_rgba(178, 62, 84, 255), 17.0),
            };
            draw_ellipse(c.x, c.y, w * u, 8.0 * u, 0.0, Color::from_rgba(92, 64, 40, 255));
            line(p(-w, 0.0), p(w, 0.0), 3.0, stripe);
            rect_at(-w * 0.6, -9.0, 6.0, 7.0, Color::from_rgba(130, 94, 58, 255));
            rect_at(w * 0.4, -3.0, 4.0, 4.0, Color::from_rgba(150, 150, 160, 255));
            line(p(-6.0, 7.0), p(-8.0, 16.0), 2.0, wood_dark);
            line(p(6.0, 7.0), p(8.0, 16.0), 2.0, wood_dark);
        }
        ItemId::ArgentavisSaddle => {
            // Sela grande com fivela de metal.
            let leather = Color::from_rgba(90, 58, 34, 255);
            draw_ellipse(c.x, c.y, 18.0 * u, 10.0 * u, 0.0, leather);
            rect_at(-14.0, -10.0, 7.0, 9.0, Color::from_rgba(132, 88, 50, 255));
            rect_at(6.0, -9.0, 8.0, 8.0, Color::from_rgba(130, 130, 140, 255));
            line(p(-6.0, 7.0), p(-8.0, 17.0), 2.5, wood_dark);
            line(p(6.0, 7.0), p(8.0, 17.0), 2.5, wood_dark);
        }
        ItemId::Campfire => {
            line(p(-14.0, 12.0), p(14.0, 6.0), 6.0, wood);
            line(p(-14.0, 6.0), p(14.0, 12.0), 6.0, wood);
            draw_triangle(
                p(-9.0, 6.0),
                p(9.0, 6.0),
                p(0.0, -16.0),
                Color::from_rgba(240, 120, 40, 255),
            );
            draw_triangle(
                p(-4.0, 6.0),
                p(4.0, 6.0),
                p(0.0, -6.0),
                Color::from_rgba(255, 210, 90, 255),
            );
        }
        ItemId::Workbench => {
            rect_at(-16.0, -6.0, 32.0, 6.0, wood_light);
            rect_at(-16.0, 0.0, 32.0, 3.0, wood);
            rect_at(-13.0, 3.0, 4.0, 12.0, wood_dark);
            rect_at(9.0, 3.0, 4.0, 12.0, wood_dark);
            rect_at(-6.0, -11.0, 7.0, 5.0, stone);
        }
        ItemId::StorageBox => {
            rect_at(-15.0, -6.0, 30.0, 18.0, wood);
            rect_at(-15.0, -13.0, 30.0, 7.0, wood_light);
            rect_at(-11.0, -13.0, 3.0, 25.0, wood_dark);
            rect_at(8.0, -13.0, 3.0, 25.0, wood_dark);
            rect_at(-2.0, -6.0, 4.0, 5.0, Color::from_rgba(200, 190, 150, 255));
        }
        ItemId::WoodFoundation => {
            rect_at(-15.0, -15.0, 30.0, 30.0, wood);
            for i in 1..4 {
                let y = -15.0 + i as f32 * 7.5;
                line(p(-15.0, y), p(15.0, y), 1.5, wood_dark);
            }
        }
        ItemId::WoodWall => {
            rect_at(-15.0, -16.0, 30.0, 10.0, wood_light);
            rect_at(-15.0, -6.0, 30.0, 20.0, wood);
            for i in 1..4 {
                let x = -15.0 + i as f32 * 7.5;
                line(p(x, -6.0), p(x, 14.0), 1.5, wood_dark);
            }
        }
        ItemId::WoodDoor => {
            rect_at(-11.0, -16.0, 22.0, 32.0, wood);
            draw_rectangle_lines(c.x - 8.0 * u, c.y - 13.0 * u, 16.0 * u, 26.0 * u, 1.5 * u, wood_dark);
            let k = p(6.0, 1.0);
            draw_circle(k.x, k.y, 2.0 * u, Color::from_rgba(220, 190, 100, 255));
        }
        ItemId::StoneFoundation
        | ItemId::StoneWall
        | ItemId::StoneDoor
        | ItemId::MetalFoundation
        | ItemId::MetalWall
        | ItemId::MetalDoor
        | ItemId::WoodWindow
        | ItemId::StoneWindow
        | ItemId::MetalWindow
        | ItemId::WoodGate
        | ItemId::StoneGate
        | ItemId::MetalGate
        | ItemId::WoodRoof
        | ItemId::StoneRoof
        | ItemId::MetalRoof
        | ItemId::WoodFence => {
            if let Some(kind) = item.structure() {
                draw_piece_icon(kind, rect);
            }
        }
        ItemId::Bed => {
            rect_at(-12.0, -16.0, 24.0, 32.0, wood);
            rect_at(-10.0, -13.0, 20.0, 26.0, Color::from_rgba(236, 232, 222, 255));
            rect_at(-7.0, -12.0, 14.0, 6.0, WHITE);
            rect_at(-10.0, -3.0, 20.0, 16.0, Color::from_rgba(170, 52, 52, 255));
        }
        ItemId::Torch => {
            line(p(0.0, 16.0), p(0.0, -4.0), 4.0, wood_dark);
            draw_triangle(
                p(-6.0, -4.0),
                p(6.0, -4.0),
                p(0.0, -18.0),
                Color::from_rgba(240, 120, 30, 255),
            );
            draw_triangle(
                p(-3.0, -4.0),
                p(3.0, -4.0),
                p(0.0, -11.0),
                Color::from_rgba(255, 220, 100, 255),
            );
        }
        ItemId::Table => {
            rect_at(-16.0, -6.0, 32.0, 6.0, wood_light);
            rect_at(-16.0, 0.0, 32.0, 3.0, wood);
            rect_at(-13.0, 3.0, 4.0, 12.0, wood_dark);
            rect_at(9.0, 3.0, 4.0, 12.0, wood_dark);
        }
        ItemId::Chair => {
            rect_at(-8.0, -16.0, 16.0, 14.0, wood);
            rect_at(-8.0, -2.0, 16.0, 5.0, wood_light);
            rect_at(-8.0, 3.0, 3.0, 12.0, wood_dark);
            rect_at(5.0, 3.0, 3.0, 12.0, wood_dark);
        }
        ItemId::Rug => {
            let gold = Color::from_rgba(220, 180, 90, 255);
            rect_at(-16.0, -11.0, 32.0, 22.0, Color::from_rgba(150, 48, 52, 255));
            draw_rectangle_lines(c.x - 13.0 * u, c.y - 8.0 * u, 26.0 * u, 16.0 * u, 1.5 * u, gold);
            draw_triangle(p(-6.0, 0.0), p(0.0, -5.0), p(6.0, 0.0), gold);
            draw_triangle(p(-6.0, 0.0), p(0.0, 5.0), p(6.0, 0.0), gold);
        }
        ItemId::FlowerPot => {
            let clay = Color::from_rgba(184, 98, 60, 255);
            draw_triangle(p(-10.0, 0.0), p(10.0, 0.0), p(7.0, 16.0), clay);
            draw_triangle(p(-10.0, 0.0), p(7.0, 16.0), p(-7.0, 16.0), clay);
            let leaf = Color::from_rgba(70, 140, 60, 255);
            for (dx, dy) in [(-6.0, -4.0), (5.0, -5.0), (0.0, -10.0)] {
                let q = p(dx, dy);
                draw_circle(q.x, q.y, 6.0 * u, leaf);
            }
            for (dx, dy, col) in [
                (-5.0, -9.0, Color::from_rgba(240, 90, 120, 255)),
                (5.0, -11.0, Color::from_rgba(250, 210, 80, 255)),
                (0.0, -15.0, Color::from_rgba(200, 120, 230, 255)),
            ] {
                let q = p(dx, dy);
                draw_circle(q.x, q.y, 3.0 * u, col);
            }
        }
        ItemId::Hammer => {
            line(p(-10.0, 14.0), p(6.0, -6.0), 4.0, wood);
            let (a, b) = (p(0.0, -14.0), p(14.0, 0.0));
            line(a, b, 8.0, stone_dark);
            line(p(2.0, -12.0), p(12.0, -2.0), 4.0, stone);
        }
    }
}

/// Ícone de peça de construção (parede, porta, janela, portão, teto, cerca, fundação) na cor do material.
fn draw_piece_icon(kind: StructureKind, rect: Rect) {
    let c = rect.center();
    let u = rect.w / 52.0;
    let p = |dx: f32, dy: f32| vec2(c.x + dx * u, c.y + dy * u);
    let rect_at = |dx: f32, dy: f32, w: f32, h: f32, col| draw_rectangle(c.x + dx * u, c.y + dy * u, w * u, h * u, col);
    let line = |a: Vec2, b: Vec2, t: f32, col| draw_line(a.x, a.y, b.x, b.y, t * u, col);
    let (top, front, dark) = match kind.material().unwrap_or(Material::Wood) {
        Material::Wood => (
            Color::from_rgba(214, 170, 112, 255),
            Color::from_rgba(150, 104, 60, 255),
            Color::from_rgba(97, 64, 33, 255),
        ),
        Material::Stone => (
            Color::from_rgba(178, 174, 168, 255),
            Color::from_rgba(130, 126, 120, 255),
            Color::from_rgba(84, 80, 76, 255),
        ),
        Material::Metal => (
            Color::from_rgba(170, 180, 192, 255),
            Color::from_rgba(106, 116, 130, 255),
            Color::from_rgba(60, 66, 78, 255),
        ),
    };
    match kind.piece() {
        Piece::Foundation => {
            rect_at(-15.0, -15.0, 30.0, 30.0, front);
            line(p(0.0, -15.0), p(0.0, 15.0), 1.5, dark);
            line(p(-15.0, 0.0), p(15.0, 0.0), 1.5, dark);
        }
        Piece::Wall | Piece::Window => {
            rect_at(-15.0, -16.0, 30.0, 10.0, top);
            rect_at(-15.0, -6.0, 30.0, 20.0, front);
            if kind.piece() == Piece::Window {
                rect_at(-8.0, -3.0, 16.0, 13.0, Color::from_rgba(140, 190, 230, 255));
                line(p(0.0, -3.0), p(0.0, 10.0), 1.5, dark);
                line(p(-8.0, 3.5), p(8.0, 3.5), 1.5, dark);
            } else {
                line(p(-15.0, 4.0), p(15.0, 4.0), 1.5, dark);
                line(p(-5.0, -6.0), p(-5.0, 4.0), 1.5, dark);
                line(p(6.0, 4.0), p(6.0, 14.0), 1.5, dark);
            }
        }
        Piece::Door => {
            rect_at(-11.0, -16.0, 22.0, 32.0, front);
            draw_rectangle_lines(c.x - 8.0 * u, c.y - 13.0 * u, 16.0 * u, 26.0 * u, 1.5 * u, dark);
            let k = p(6.0, 1.0);
            draw_circle(k.x, k.y, 2.0 * u, top);
        }
        Piece::Gate => {
            rect_at(-18.0, -12.0, 36.0, 6.0, top);
            rect_at(-18.0, -6.0, 36.0, 18.0, front);
            line(p(-15.0, -4.0), p(-1.0, 10.0), 2.0, dark);
            line(p(-15.0, 10.0), p(-1.0, -4.0), 2.0, dark);
            line(p(1.0, -4.0), p(15.0, 10.0), 2.0, dark);
            line(p(1.0, 10.0), p(15.0, -4.0), 2.0, dark);
            line(p(0.0, -6.0), p(0.0, 12.0), 2.0, dark);
        }
        Piece::Roof => {
            draw_triangle(p(-18.0, 2.0), p(18.0, 2.0), p(0.0, -16.0), top);
            rect_at(-14.0, 2.0, 28.0, 12.0, front);
            line(p(-18.0, 2.0), p(0.0, -16.0), 2.0, dark);
            line(p(18.0, 2.0), p(0.0, -16.0), 2.0, dark);
        }
        Piece::Fence => {
            for dx in [-12.0, 0.0, 12.0] {
                rect_at(dx - 2.0, -12.0, 4.0, 26.0, top);
            }
            line(p(-16.0, -4.0), p(16.0, -4.0), 3.0, front);
            line(p(-16.0, 6.0), p(16.0, 6.0), 3.0, front);
        }
        Piece::Station | Piece::Furniture => {}
    }
}
