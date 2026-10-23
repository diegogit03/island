//! Save/load em JSON. O mundo é regenerado pela seed; só o estado que muda é salvo.
//! Versão 2: vários jogadores (identificados pelo nome). Saves v1 (um jogador) são convertidos.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::building::Structure;
use crate::crafting::CraftQueue;
use crate::creatures::CreatureSave;
use crate::inventory::Stack;
use crate::progression::Progression;
use crate::raft::RaftSave;
use crate::vitals::Vitals;
use crate::world::BagKind;

pub const SAVE_VERSION: u32 = 2;

/// Nome do jogador vindo de um save de um jogador só (v1): fica com o primeiro que entrar (o host).
pub const LEGACY_PLAYER: &str = "";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerSave {
    pub name: String,
    pub pos: [f32; 2],
    pub facing_left: bool,
    pub vitals: Vitals,
    pub inventory: Vec<Option<Stack>>,
    pub hotbar_selected: usize,
    #[serde(default)]
    pub craft_queue: CraftQueue,
    /// `None` em saves anteriores à v0.6 (receitas antigas liberadas, nível 1).
    #[serde(default)]
    pub progression: Option<Progression>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    pub version: u32,
    pub seed: u64,
    pub time: f32,
    /// Apenas nós alterados: (índice, hp, respawn_timer).
    pub nodes: Vec<(usize, u32, f32)>,
    pub bags: Vec<BagSave>,
    /// Campos novos usam `default` para saves antigos continuarem válidos.
    #[serde(default)]
    pub structures: Vec<Structure>,
    /// `None` em saves anteriores à v0.4 (a fauna é gerada do zero).
    #[serde(default)]
    pub creatures: Option<Vec<CreatureSave>>,
    #[serde(default)]
    pub players: Vec<PlayerSave>,
    #[serde(default)]
    pub rafts: Vec<RaftSave>,

    // ----- Formato v1 (um jogador): só lidos, nunca gravados. -----
    #[serde(default, skip_serializing)]
    player_pos: Option<[f32; 2]>,
    #[serde(default, skip_serializing)]
    facing_left: Option<bool>,
    #[serde(default, skip_serializing)]
    vitals: Option<Vitals>,
    #[serde(default, skip_serializing)]
    inventory: Option<Vec<Option<Stack>>>,
    #[serde(default, skip_serializing)]
    hotbar_selected: Option<usize>,
    #[serde(default, skip_serializing)]
    craft_queue: Option<CraftQueue>,
    #[serde(default, skip_serializing)]
    progression: Option<Progression>,
}

impl SaveData {
    pub fn new(seed: u64, time: f32) -> Self {
        Self {
            version: SAVE_VERSION,
            seed,
            time,
            nodes: Vec::new(),
            bags: Vec::new(),
            structures: Vec::new(),
            creatures: None,
            players: Vec::new(),
            rafts: Vec::new(),
            player_pos: None,
            facing_left: None,
            vitals: None,
            inventory: None,
            hotbar_selected: None,
            craft_queue: None,
            progression: None,
        }
    }

    /// Converte um save v1 (campos do jogador soltos) para a lista de jogadores.
    fn upgrade(&mut self) {
        if !self.players.is_empty() {
            return;
        }
        let (Some(pos), Some(vitals)) = (self.player_pos.take(), self.vitals.take()) else {
            return;
        };
        self.players.push(PlayerSave {
            name: LEGACY_PLAYER.to_owned(),
            pos,
            facing_left: self.facing_left.take().unwrap_or(false),
            vitals,
            inventory: self.inventory.take().unwrap_or_default(),
            hotbar_selected: self.hotbar_selected.take().unwrap_or(0),
            craft_queue: self.craft_queue.take().unwrap_or_default(),
            progression: self.progression.take(),
        });
        self.version = SAVE_VERSION;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BagSave {
    pub pos: [f32; 2],
    pub stacks: Vec<Stack>,
    pub kind: BagKind,
    pub ttl: f32,
}

/// Grava de forma atômica (arquivo temporário + rename) para não corromper o save.
pub fn write(path: &Path, data: &SaveData) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

/// `Ok(None)` se não existe save; `Err` se existe mas é inválido ou de versão futura.
pub fn read(path: &Path) -> io::Result<Option<SaveData>> {
    let json = match fs::read_to_string(path) {
        Ok(json) => json,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut data: SaveData = serde_json::from_str(&json).map_err(io::Error::other)?;
    if data.version > SAVE_VERSION {
        return Err(io::Error::other(format!(
            "versão de save {} não suportada (esta versão lê até {SAVE_VERSION})",
            data.version
        )));
    }
    data.upgrade();
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemId;

    fn player(name: &str) -> PlayerSave {
        PlayerSave {
            name: name.to_owned(),
            pos: [100.0, 200.0],
            facing_left: true,
            vitals: Vitals::full(),
            inventory: vec![Some(Stack::new(ItemId::StonePickaxe, 1)), None],
            hotbar_selected: 1,
            craft_queue: CraftQueue::default(),
            progression: Some(Progression::default()),
        }
    }

    #[test]
    fn round_trip() {
        let mut data = SaveData::new(42, 12.5);
        data.nodes = vec![(3, 0, 10.0)];
        data.bags = vec![BagSave {
            pos: [1.0, 2.0],
            stacks: vec![Stack::new(ItemId::Berry, 4)],
            kind: BagKind::Death,
            ttl: 99.0,
        }];
        data.structures = vec![Structure::new(crate::building::StructureKind::StorageBox, (1, 2))];
        data.players = vec![player("Ana"), player("Bia")];
        let path = std::env::temp_dir().join(format!("island-test-{}/save.json", std::process::id()));
        write(&path, &data).unwrap();
        assert_eq!(read(&path).unwrap(), Some(data));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_file_is_none() {
        assert_eq!(read(Path::new("nao/existe/save.json")).unwrap(), None);
    }

    #[test]
    fn v1_single_player_save_becomes_legacy_player() {
        let json = r#"{"version":1,"seed":1,"time":0.0,"player_pos":[5.0,6.0],"facing_left":false,
            "vitals":{"health":100.0,"food":100.0,"water":100.0,"stamina":100.0,"exhausted":false},
            "inventory":[null],"hotbar_selected":0,"nodes":[],"bags":[]}"#;
        let mut data: SaveData = serde_json::from_str(json).unwrap();
        data.upgrade();
        assert_eq!(data.players.len(), 1);
        assert_eq!(data.players[0].name, LEGACY_PLAYER);
        assert_eq!(data.players[0].pos, [5.0, 6.0]);
        assert!(
            data.players[0].progression.is_none(),
            "v0.5 e anteriores: receitas antigas"
        );
        assert!(data.structures.is_empty() && data.creatures.is_none());
        let rewritten = serde_json::to_string(&data).unwrap();
        assert!(!rewritten.contains("player_pos"), "campos v1 não são gravados de novo");
    }
}
