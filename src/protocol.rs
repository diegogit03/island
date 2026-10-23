//! Mensagens entre cliente e servidor. Serializadas com bincode sobre TCP, ou passadas direto
//! por canal quando cliente e servidor estão no mesmo processo (um jogador / host).

use serde::{Deserialize, Serialize};

use crate::building::Structure;
use crate::crafting::CraftQueue;
use crate::creatures::{Creature, CreatureSave, State};
use crate::inventory::{Inventory, Stack};
use crate::items::ItemId;
use crate::npc::{GatherKind, NpcView};
use crate::progression::{Attribute, Progression};
use crate::raft::RaftSave;
use crate::save::BagSave;
use crate::vitals::Vitals;

/// Cliente e servidor precisam da mesma versão para conversar.
pub const PROTOCOL_VERSION: u32 = 7;
pub const DEFAULT_PORT: u16 = 7777;
/// Porta UDP dos anúncios de servidor na LAN.
pub const DISCOVERY_PORT: u16 = 7778;
pub const MAX_NAME_LEN: usize = 16;
pub const MAX_CHAT_LEN: usize = 120;

pub type PlayerId = u32;

/// O que está aberto ao lado do inventário de um jogador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerRef {
    /// Índice em `world.structures`.
    Structure(usize),
    /// `id` da criatura.
    Creature(u32),
    /// `id` do NPC (morador de acampamento ou membro da tribo).
    Npc(u32),
}

/// Apito: ordem para toda a tribo (NPCs e domesticados) perto do jogador.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Whistle {
    Follow,
    Stay,
    Defend,
    /// Atacar a criatura mais perto deste ponto (o cursor).
    Attack {
        at: [f32; 2],
    },
}

/// Botões do painel de um NPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpcCmd {
    Follow,
    Stay,
    Defend,
    Gather(GatherKind),
    Recruit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupStyle {
    /// Itens ganhos ("+3 Madeira").
    Gain,
    /// Dano causado numa criatura.
    Damage,
    /// Dano recebido por um jogador.
    Hurt,
}

/// Estado contínuo dos controles, enviado todo frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub dir: [f32; 2],
    pub sprint: bool,
    /// E ou botão esquerdo segurado (coletar/atacar/morder).
    pub attack: bool,
    /// Ponto do mundo sob o cursor quando o ataque vem do mouse (mira do estilingue).
    pub aim: Option<[f32; 2]>,
}

/// Slot clicado num painel de inventário.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotTarget {
    Inventory(usize),
    Container(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugCmd {
    Items,
    Damage,
    SkipHours,
    Xp,
}

/// Ações pontuais do jogador.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    SelectHotbar(usize),
    /// Comer o item selecionado (R / botão direito).
    Use,
    Drop {
        all: bool,
    },
    /// F: pegar bolsa, abrir porta/baú/fogueira/dino, desmontar.
    Interact,
    Place {
        tile: (i32, i32),
    },
    Demolish {
        tile: (i32, i32),
    },
    Craft(usize),
    CancelCraft(usize),
    /// Clique num slot: esquerdo (pegar/soltar) ou direito (dividir); Shift transfere.
    Slot {
        target: SlotTarget,
        left: bool,
        shift: bool,
    },
    /// Soltar no chão a pilha presa no cursor.
    DropHeld,
    /// Fechar o inventário (devolve a pilha do cursor e fecha o contêiner).
    CloseContainer,
    /// Acender/apagar a fogueira ou forja aberta.
    ToggleFire,
    /// Alternar seguir/ficar do dino aberto.
    ToggleOrder,
    Mount,
    /// Espaço montado: voador decola/pousa, beelzebufo pula.
    MountAction,
    SpendAttribute(Attribute),
    Respawn,
    /// Ordem para toda a tribo por perto (Z, C, B, V).
    Whistle(Whistle),
    /// Ordem para o NPC com o painel aberto (ou recrutá-lo).
    NpcOrder(NpcCmd),
    Chat(String),
    Save,
    Debug(DebugCmd),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello { version: u32, name: String },
    Input(Input),
    Command(Command),
    Bye,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NetEvent {
    Popup {
        pos: [f32; 2],
        text: String,
        style: PopupStyle,
    },
    /// Mensagem no centro da tela (só para o jogador destinatário).
    Notify(String),
    /// O jogador levou dano (flash vermelho).
    Hurt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        you: PlayerId,
        seed: u64,
    },
    Rejected(String),
    Snapshot(Box<Snapshot>),
    Events(Vec<NetEvent>),
    Chat {
        from: String,
        text: String,
    },
    /// O servidor está fechando (host saiu, por exemplo).
    Shutdown(String),
}

/// O que todo mundo vê de um jogador.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerView {
    pub id: PlayerId,
    pub name: String,
    pub pos: [f32; 2],
    pub facing_left: bool,
    pub moving: bool,
    pub anim_time: f32,
    pub swing_timer: f32,
    pub held: Option<ItemId>,
    pub dead: bool,
    pub mount: Option<u32>,
}

/// O que só o próprio jogador vê de si.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrivateState {
    pub vitals: Vitals,
    pub inventory: Inventory,
    pub hotbar_selected: usize,
    pub progression: Progression,
    pub queue: CraftQueue,
    /// Pilha presa no cursor.
    pub held: Option<Stack>,
    pub container: Option<ContainerRef>,
    pub dead: bool,
    pub mount: Option<u32>,
    /// `id` da jangada em que está remando.
    pub raft: Option<u32>,
}

/// Criatura como o cliente precisa para desenhar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatureView {
    pub save: CreatureSave,
    pub facing_left: bool,
    pub moving: bool,
    pub anim_time: f32,
    pub hurt: f32,
    pub lunge: f32,
    pub mounted: bool,
    pub flying: bool,
    pub buried: bool,
    pub altitude: f32,
    pub chasing: bool,
    /// Tem sela (o inventário só vai junto quando o painel dela está aberto).
    pub saddled: bool,
}

impl CreatureView {
    /// `with_inventory`: mandar o inventário (só para quem está com o painel dela aberto).
    pub fn new(c: &Creature, with_inventory: bool) -> Self {
        let mut save = CreatureSave::from(c);
        if !with_inventory {
            save.inventory = None;
        }
        Self {
            save,
            facing_left: c.facing_left,
            moving: c.moving,
            anim_time: c.anim_time,
            hurt: c.hurt,
            lunge: c.lunge,
            mounted: c.mounted,
            flying: c.flying,
            buried: c.buried,
            altitude: c.altitude,
            chasing: c.state == State::Chase,
            saddled: c.can_ride(),
        }
    }

    pub fn to_creature(&self) -> Creature {
        let mut c = Creature::from(self.save.clone());
        c.facing_left = self.facing_left;
        c.moving = self.moving;
        c.anim_time = self.anim_time;
        c.hurt = self.hurt;
        c.lunge = self.lunge;
        c.mounted = self.mounted;
        c.flying = self.flying;
        c.altitude = self.altitude;
        c.buried = self.buried;
        if self.chasing {
            c.state = State::Chase;
        }
        // Sem o inventário real, uma sela simbólica faz `can_ride()` (e o desenho dela) funcionar.
        if self.saddled
            && c.inventory.is_none()
            && let Some(saddle) = c.species.saddle()
        {
            let mut inv = Inventory::new(1);
            inv.add(saddle, 1);
            c.inventory = Some(inv);
        }
        c
    }
}

/// Estado do mundo enviado a cada cliente várias vezes por segundo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub time: f32,
    /// Vida de cada nó de recurso, na ordem de `world.nodes` (o resto vem da seed).
    pub node_hp: Vec<u8>,
    pub bags: Vec<BagSave>,
    /// Inventários só do contêiner aberto por este cliente.
    pub structures: Vec<Structure>,
    pub creatures: Vec<CreatureView>,
    pub projectiles: Vec<[f32; 2]>,
    pub rafts: Vec<RaftSave>,
    pub npcs: Vec<NpcView>,
    pub players: Vec<PlayerView>,
    pub me: PrivateState,
}

/// Limpa um nome de jogador (tamanho e caracteres visíveis).
pub fn sanitize_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME_LEN).collect();
    let name = name.trim().to_owned();
    if name.is_empty() { "Jogador".to_owned() } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creatures::Species;

    #[test]
    fn messages_round_trip_in_bincode() {
        let msg = ClientMsg::Command(Command::Slot {
            target: SlotTarget::Container(3),
            left: true,
            shift: false,
        });
        let bytes = bincode::serialize(&msg).unwrap();
        assert_eq!(bincode::deserialize::<ClientMsg>(&bytes).unwrap(), msg);
    }

    #[test]
    fn creature_view_keeps_saddle_without_inventory() {
        let mut raptor = Creature::new(Species::Raptor, macroquad::math::vec2(1.0, 2.0));
        raptor.command = Some(crate::creatures::Command::Follow);
        let mut inv = Inventory::new(12);
        inv.add(ItemId::RaptorSaddle, 1);
        inv.add(ItemId::RawMeat, 5);
        raptor.inventory = Some(inv);
        let view = CreatureView::new(&raptor, false);
        assert!(view.save.inventory.is_none());
        let back = view.to_creature();
        assert!(back.can_ride());
        assert_eq!(back.inventory.unwrap().count(ItemId::RawMeat), 0);
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("  "), "Jogador");
        assert_eq!(sanitize_name("Ana\n"), "Ana");
        assert_eq!(sanitize_name(&"x".repeat(40)).len(), MAX_NAME_LEN);
    }
}
