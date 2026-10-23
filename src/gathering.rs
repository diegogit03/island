//! O que cada golpe rende, por tipo de nó e ferramenta.
//! Como no ARK: machado favorece madeira/pedra, picareta favorece palha/sílex.

use crate::creatures::Species;
use crate::items::{ItemId, ToolKind};
use crate::rng::Rng;
use crate::world::NodeKind;

/// Ferramentas de tier 2 (metal) rendem o dobro.
fn apply_tier(drops: Vec<(ItemId, u32)>, tier: u32) -> Vec<(ItemId, u32)> {
    drops.into_iter().map(|(item, n)| (item, n * tier.max(1))).collect()
}

pub fn roll_yield(kind: NodeKind, tool: Option<ToolKind>, tier: u32, rng: &mut Rng) -> Vec<(ItemId, u32)> {
    apply_tier(roll_base_yield(kind, tool, rng), tier)
}

fn roll_base_yield(kind: NodeKind, tool: Option<ToolKind>, rng: &mut Rng) -> Vec<(ItemId, u32)> {
    let mut drops = Vec::new();
    let mut push = |item, n| {
        if n > 0 {
            drops.push((item, n));
        }
    };
    // Lança não é ferramenta de coleta: conta como mão.
    let tool = tool.filter(|t| *t != ToolKind::Spear);
    match (kind, tool) {
        (NodeKind::Pebble, _) => push(ItemId::Stone, 1),
        // Colher frutinhas é trabalho para as mãos.
        (NodeKind::Bush, None) => push(ItemId::Berry, rng.range_u32(2, 3)),
        (NodeKind::Bush, Some(_)) => push(ItemId::Berry, 1),
        (NodeKind::Tree, None) => {
            push(ItemId::Wood, 1);
            push(ItemId::Thatch, rng.range_u32(1, 2));
        }
        (NodeKind::Tree, Some(ToolKind::Hatchet)) => {
            push(ItemId::Wood, rng.range_u32(3, 4));
            push(ItemId::Thatch, u32::from(rng.chance(0.3)));
        }
        (NodeKind::Tree, Some(_)) => {
            push(ItemId::Wood, 1);
            push(ItemId::Thatch, rng.range_u32(3, 4));
        }
        (NodeKind::Rock, None) => {
            push(ItemId::Stone, 1);
            push(ItemId::Flint, u32::from(rng.chance(0.2)));
        }
        (NodeKind::Rock, Some(ToolKind::Hatchet)) => {
            push(ItemId::Stone, rng.range_u32(3, 4));
            push(ItemId::Flint, u32::from(rng.chance(0.3)));
        }
        (NodeKind::Rock, Some(_)) => {
            push(ItemId::Stone, 1);
            push(ItemId::Flint, rng.range_u32(2, 3));
        }
        // Metal só sai bem com picareta.
        (NodeKind::MetalRock, Some(ToolKind::Pickaxe)) => {
            push(ItemId::MetalOre, rng.range_u32(1, 2));
            push(ItemId::Stone, 1);
        }
        (NodeKind::MetalRock, Some(ToolKind::Hatchet)) => {
            push(ItemId::Stone, rng.range_u32(2, 3));
            push(ItemId::MetalOre, u32::from(rng.chance(0.3)));
        }
        (NodeKind::MetalRock, _) => {
            push(ItemId::Stone, 1);
            push(ItemId::MetalOre, u32::from(rng.chance(0.1)));
        }
    }
    drops
}

/// Golpe de mamute montado numa árvore: muito mais madeira que qualquer machado.
pub fn roll_mammoth_tree(rng: &mut Rng) -> Vec<(ItemId, u32)> {
    vec![
        (ItemId::Wood, rng.range_u32(8, 12)),
        (ItemId::Thatch, rng.range_u32(1, 3)),
    ]
}

/// Golpe de smilodon montado numa carcaça: couro de sobra e um pouco de carne.
pub fn roll_smilodon_corpse(corpse: Species, rng: &mut Rng) -> Vec<(ItemId, u32)> {
    let big = u32::from(corpse.is_big());
    vec![
        (ItemId::Hide, rng.range_u32(4, 6) + big * 2),
        (ItemId::RawMeat, rng.range_u32(1, 2)),
    ]
}

/// O que cada golpe numa carcaça rende. Machado favorece carne; picareta, couro.
pub fn roll_corpse_yield(species: Species, tool: Option<ToolKind>, tier: u32, rng: &mut Rng) -> Vec<(ItemId, u32)> {
    let big = u32::from(species.is_big());
    let tool = tool.filter(|t| *t != ToolKind::Spear);
    let (meat, hide) = match tool {
        Some(ToolKind::Hatchet) => (rng.range_u32(2, 3) + big, u32::from(rng.chance(0.5))),
        Some(_) => (u32::from(rng.chance(0.5)), rng.range_u32(2, 3) + big),
        None => (1, u32::from(rng.chance(0.3))),
    };
    let drops = [(ItemId::RawMeat, meat), (ItemId::Hide, hide)]
        .into_iter()
        .filter(|&(_, n)| n > 0)
        .collect();
    apply_tier(drops, tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metal_rock_needs_a_pickaxe_and_metal_tools_double() {
        let mut rng = Rng::new(3);
        for _ in 0..50 {
            let pick = roll_yield(NodeKind::MetalRock, Some(ToolKind::Pickaxe), 1, &mut rng);
            assert!(total(&pick, ItemId::MetalOre) >= 1);
            let metal = roll_yield(NodeKind::MetalRock, Some(ToolKind::Pickaxe), 2, &mut rng);
            assert!(total(&metal, ItemId::MetalOre) >= 2);
            let hand = roll_yield(NodeKind::MetalRock, None, 1, &mut rng);
            assert!(total(&hand, ItemId::MetalOre) <= 1);
        }
    }

    fn total(drops: &[(ItemId, u32)], item: ItemId) -> u32 {
        drops.iter().filter(|d| d.0 == item).map(|d| d.1).sum()
    }

    #[test]
    fn tools_beat_bare_hands() {
        let mut rng = Rng::new(1);
        for _ in 0..50 {
            let hand = roll_yield(NodeKind::Tree, None, 1, &mut rng);
            let hatchet = roll_yield(NodeKind::Tree, Some(ToolKind::Hatchet), 1, &mut rng);
            assert!(total(&hatchet, ItemId::Wood) > total(&hand, ItemId::Wood));
            let pick = roll_yield(NodeKind::Rock, Some(ToolKind::Pickaxe), 1, &mut rng);
            assert!(total(&pick, ItemId::Flint) >= 2);
        }
    }

    #[test]
    fn hatchet_gives_meat_and_pickaxe_gives_hide() {
        let mut rng = Rng::new(2);
        for _ in 0..50 {
            let hatchet = roll_corpse_yield(Species::Raptor, Some(ToolKind::Hatchet), 1, &mut rng);
            let pick = roll_corpse_yield(Species::Raptor, Some(ToolKind::Pickaxe), 1, &mut rng);
            let hand = roll_corpse_yield(Species::Raptor, None, 1, &mut rng);
            assert!(total(&hatchet, ItemId::RawMeat) > total(&hand, ItemId::RawMeat));
            assert!(total(&pick, ItemId::Hide) > total(&hand, ItemId::Hide));
        }
    }
}
