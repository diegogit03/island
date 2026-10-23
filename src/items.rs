//! Definições de itens. Todo item do jogo nasce aqui.

use serde::{Deserialize, Serialize};

use crate::building::StructureKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemId {
    Wood,
    Thatch,
    Stone,
    Flint,
    Berry,
    RawMeat,
    CookedMeat,
    Hide,
    MetalOre,
    MetalIngot,
    StonePickaxe,
    StoneHatchet,
    WoodenSpear,
    MetalPickaxe,
    MetalHatchet,
    MetalSword,
    WoodenClub,
    Slingshot,
    Narcotic,
    RaptorSaddle,
    PteranodonSaddle,
    ArgentavisSaddle,
    MammothSaddle,
    SmilodonSaddle,
    BeelzebufoSaddle,
    SarcosuchusSaddle,
    MegalodonSaddle,
    IchthyosaurSaddle,
    MosasaurSaddle,
    KrakenSaddle,
    KangarooSaddle,
    Raft,
    Campfire,
    Forge,
    Workbench,
    StorageBox,
    WoodFoundation,
    WoodWall,
    WoodDoor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Pickaxe,
    Hatchet,
    Spear,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoodEffect {
    pub food: f32,
    pub water: f32,
    pub health: f32,
}

impl ItemId {
    #[cfg(test)]
    pub const ALL: [ItemId; 39] = [
        ItemId::Wood,
        ItemId::Thatch,
        ItemId::Stone,
        ItemId::Flint,
        ItemId::Berry,
        ItemId::RawMeat,
        ItemId::CookedMeat,
        ItemId::Hide,
        ItemId::MetalOre,
        ItemId::MetalIngot,
        ItemId::StonePickaxe,
        ItemId::StoneHatchet,
        ItemId::WoodenSpear,
        ItemId::MetalPickaxe,
        ItemId::MetalHatchet,
        ItemId::MetalSword,
        ItemId::WoodenClub,
        ItemId::Slingshot,
        ItemId::Narcotic,
        ItemId::RaptorSaddle,
        ItemId::PteranodonSaddle,
        ItemId::ArgentavisSaddle,
        ItemId::MammothSaddle,
        ItemId::SmilodonSaddle,
        ItemId::BeelzebufoSaddle,
        ItemId::SarcosuchusSaddle,
        ItemId::MegalodonSaddle,
        ItemId::IchthyosaurSaddle,
        ItemId::MosasaurSaddle,
        ItemId::KrakenSaddle,
        ItemId::KangarooSaddle,
        ItemId::Raft,
        ItemId::Campfire,
        ItemId::Forge,
        ItemId::Workbench,
        ItemId::StorageBox,
        ItemId::WoodFoundation,
        ItemId::WoodWall,
        ItemId::WoodDoor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ItemId::Wood => "Madeira",
            ItemId::Thatch => "Palha",
            ItemId::Stone => "Pedra",
            ItemId::Flint => "Sílex",
            ItemId::Berry => "Frutinha",
            ItemId::RawMeat => "Carne Crua",
            ItemId::CookedMeat => "Carne Cozida",
            ItemId::Hide => "Couro",
            ItemId::MetalOre => "Minério de Metal",
            ItemId::MetalIngot => "Lingote de Metal",
            ItemId::MetalPickaxe => "Picareta de Metal",
            ItemId::MetalHatchet => "Machado de Metal",
            ItemId::MetalSword => "Espada de Metal",
            ItemId::Forge => "Forja",
            ItemId::StonePickaxe => "Picareta de Pedra",
            ItemId::StoneHatchet => "Machado de Pedra",
            ItemId::WoodenSpear => "Lança de Madeira",
            ItemId::WoodenClub => "Clava de Madeira",
            ItemId::Slingshot => "Estilingue",
            ItemId::Narcotic => "Narcótico",
            ItemId::RaptorSaddle => "Sela de Raptor",
            ItemId::PteranodonSaddle => "Sela de Pteranodonte",
            ItemId::ArgentavisSaddle => "Sela de Argentavis",
            ItemId::MammothSaddle => "Sela de Mamute",
            ItemId::SmilodonSaddle => "Sela de Smilodon",
            ItemId::BeelzebufoSaddle => "Sela de Beelzebufo",
            ItemId::SarcosuchusSaddle => "Sela de Sarcosuchus",
            ItemId::MegalodonSaddle => "Sela de Megalodonte",
            ItemId::IchthyosaurSaddle => "Sela de Ictiossauro",
            ItemId::MosasaurSaddle => "Sela de Mosassauro",
            ItemId::KrakenSaddle => "Sela de Kraken",
            ItemId::KangarooSaddle => "Sela de Canguru",
            ItemId::Raft => "Jangada",
            ItemId::Campfire => "Fogueira",
            ItemId::Workbench => "Bancada",
            ItemId::StorageBox => "Caixa de Armazenamento",
            ItemId::WoodFoundation => "Fundação de Madeira",
            ItemId::WoodWall => "Parede de Madeira",
            ItemId::WoodDoor => "Porta de Madeira",
        }
    }

    pub fn max_stack(self) -> u32 {
        match self {
            ItemId::Wood
            | ItemId::Thatch
            | ItemId::Stone
            | ItemId::Flint
            | ItemId::Berry
            | ItemId::Hide
            | ItemId::MetalOre
            | ItemId::MetalIngot => 100,
            ItemId::RawMeat | ItemId::CookedMeat => 40,
            ItemId::Narcotic => 50,
            ItemId::RaptorSaddle
            | ItemId::PteranodonSaddle
            | ItemId::ArgentavisSaddle
            | ItemId::MammothSaddle
            | ItemId::SmilodonSaddle
            | ItemId::BeelzebufoSaddle
            | ItemId::SarcosuchusSaddle
            | ItemId::MegalodonSaddle
            | ItemId::IchthyosaurSaddle
            | ItemId::MosasaurSaddle
            | ItemId::KrakenSaddle
            | ItemId::KangarooSaddle
            | ItemId::Raft => 1,
            ItemId::Campfire | ItemId::Forge | ItemId::Workbench | ItemId::StorageBox => 5,
            ItemId::WoodFoundation | ItemId::WoodWall | ItemId::WoodDoor => 20,
            ItemId::StonePickaxe
            | ItemId::StoneHatchet
            | ItemId::WoodenSpear
            | ItemId::MetalPickaxe
            | ItemId::MetalHatchet
            | ItemId::MetalSword
            | ItemId::WoodenClub
            | ItemId::Slingshot => 1,
        }
    }

    /// Peso de uma unidade (conta para a carga dos dinos).
    pub fn weight(self) -> f32 {
        match self {
            ItemId::Thatch | ItemId::Flint | ItemId::Berry | ItemId::Narcotic => 0.1,
            ItemId::RawMeat | ItemId::CookedMeat | ItemId::Hide => 0.2,
            ItemId::Wood => 0.5,
            ItemId::Stone | ItemId::MetalOre | ItemId::MetalIngot => 1.0,
            ItemId::WoodenSpear | ItemId::WoodenClub | ItemId::Slingshot => 1.0,
            ItemId::StonePickaxe | ItemId::StoneHatchet => 1.5,
            ItemId::MetalPickaxe | ItemId::MetalHatchet | ItemId::MetalSword => 2.5,
            ItemId::RaptorSaddle => 6.0,
            ItemId::PteranodonSaddle => 5.0,
            ItemId::ArgentavisSaddle => 10.0,
            ItemId::MammothSaddle => 14.0,
            ItemId::SmilodonSaddle => 6.0,
            ItemId::BeelzebufoSaddle => 5.0,
            ItemId::SarcosuchusSaddle => 9.0,
            ItemId::MegalodonSaddle => 10.0,
            ItemId::IchthyosaurSaddle => 8.0,
            ItemId::MosasaurSaddle => 18.0,
            ItemId::KrakenSaddle => 14.0,
            ItemId::KangarooSaddle => 6.0,
            ItemId::Raft => 20.0,
            ItemId::WoodWall | ItemId::WoodDoor => 3.0,
            ItemId::WoodFoundation => 4.0,
            ItemId::Campfire => 5.0,
            ItemId::StorageBox => 8.0,
            ItemId::Workbench => 12.0,
            ItemId::Forge => 25.0,
        }
    }

    pub fn tool(self) -> Option<ToolKind> {
        match self {
            ItemId::StonePickaxe | ItemId::MetalPickaxe => Some(ToolKind::Pickaxe),
            ItemId::StoneHatchet | ItemId::MetalHatchet => Some(ToolKind::Hatchet),
            ItemId::WoodenSpear => Some(ToolKind::Spear),
            _ => None,
        }
    }

    /// Tier da ferramenta: metal rende o dobro ao coletar.
    pub fn tool_tier(item: Option<ItemId>) -> u32 {
        match item {
            Some(ItemId::MetalPickaxe | ItemId::MetalHatchet) => 2,
            _ => 1,
        }
    }

    /// Estrutura que o item vira ao ser posicionado no mundo.
    pub fn structure(self) -> Option<StructureKind> {
        match self {
            ItemId::Campfire => Some(StructureKind::Campfire),
            ItemId::Forge => Some(StructureKind::Forge),
            ItemId::Workbench => Some(StructureKind::Workbench),
            ItemId::StorageBox => Some(StructureKind::StorageBox),
            ItemId::WoodFoundation => Some(StructureKind::WoodFoundation),
            ItemId::WoodWall => Some(StructureKind::WoodWall),
            ItemId::WoodDoor => Some(StructureKind::WoodDoor),
            _ => None,
        }
    }

    /// Usos até quebrar (só itens com `max_stack == 1`).
    pub fn max_durability(self) -> Option<u32> {
        match self {
            ItemId::StonePickaxe | ItemId::StoneHatchet => Some(100),
            ItemId::WoodenSpear => Some(50),
            ItemId::MetalPickaxe | ItemId::MetalHatchet => Some(300),
            ItemId::MetalSword => Some(200),
            ItemId::WoodenClub => Some(60),
            ItemId::Slingshot => Some(80),
            _ => None,
        }
    }

    pub fn food_effect(self) -> Option<FoodEffect> {
        let effect = |food, water, health| Some(FoodEffect { food, water, health });
        match self {
            ItemId::Berry => effect(8.0, 2.0, 0.0),
            // Carne crua mata a fome mas faz mal; cozida também cura.
            ItemId::RawMeat => effect(10.0, 0.0, -5.0),
            ItemId::CookedMeat => effect(25.0, 0.0, 10.0),
            _ => None,
        }
    }

    /// Dano corpo a corpo ao golpear uma criatura com este item na mão.
    pub fn melee_damage(item: Option<ItemId>) -> f32 {
        match item {
            Some(ItemId::MetalSword) => 32.0,
            Some(ItemId::WoodenSpear) => 22.0,
            Some(ItemId::MetalHatchet) => 18.0,
            Some(ItemId::MetalPickaxe) => 15.0,
            Some(ItemId::StoneHatchet) => 12.0,
            Some(ItemId::StonePickaxe) => 10.0,
            Some(ItemId::WoodenClub) => 3.0,
            _ => 6.0,
        }
    }

    /// Torpor (atordoamento) causado por golpe corpo a corpo. Clava é feita para derrubar.
    pub fn melee_torpor(item: Option<ItemId>) -> f32 {
        match item {
            Some(ItemId::WoodenClub) => 15.0,
            None => 2.0,
            _ => 0.0,
        }
    }

    /// Alcance extra de ataque (a lança é comprida).
    pub fn reach_bonus(item: Option<ItemId>) -> f32 {
        match item {
            Some(ItemId::WoodenSpear) => 14.0,
            Some(ItemId::MetalSword) => 6.0,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_items_never_stack() {
        for item in ItemId::ALL {
            if item.max_durability().is_some() {
                assert_eq!(item.max_stack(), 1, "{item:?}");
            }
        }
    }

    #[test]
    fn sword_is_the_best_weapon_and_spear_the_best_primitive() {
        let sword = ItemId::melee_damage(Some(ItemId::MetalSword));
        let spear = ItemId::melee_damage(Some(ItemId::WoodenSpear));
        for item in ItemId::ALL {
            assert!(ItemId::melee_damage(Some(item)) <= sword);
        }
        for item in [ItemId::StonePickaxe, ItemId::StoneHatchet, ItemId::WoodenClub] {
            assert!(ItemId::melee_damage(Some(item)) < spear);
        }
    }

    #[test]
    fn structures_round_trip_to_items() {
        for item in ItemId::ALL {
            if let Some(kind) = item.structure() {
                assert_eq!(kind.item(), item);
            }
        }
    }
}
