//! Inventário em slots com empilhamento. Os primeiros `HOTBAR_SIZE` slots formam a hotbar.

use serde::{Deserialize, Serialize};

use crate::items::ItemId;

pub const INVENTORY_SIZE: usize = 30;
pub const HOTBAR_SIZE: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    pub item: ItemId,
    pub count: u32,
    /// Usos restantes (0 para itens sem durabilidade). Só faz sentido em pilhas de 1.
    #[serde(default)]
    pub durability: u32,
}

impl Stack {
    pub fn new(item: ItemId, count: u32) -> Self {
        Self {
            item,
            count,
            durability: item.max_durability().unwrap_or(0),
        }
    }

    /// Fração de durabilidade restante, se o item tiver durabilidade.
    pub fn durability_frac(&self) -> Option<f32> {
        self.item
            .max_durability()
            .map(|max| self.durability as f32 / max as f32)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
}

impl Inventory {
    pub fn new(size: usize) -> Self {
        Self {
            slots: vec![None; size],
        }
    }

    /// Adiciona itens completando pilhas existentes antes de ocupar slots vazios.
    /// Retorna a quantidade que não coube.
    pub fn add(&mut self, item: ItemId, mut count: u32) -> u32 {
        let max = item.max_stack();
        for stack in self.slots.iter_mut().flatten() {
            if count == 0 {
                break;
            }
            if stack.item == item && stack.count < max {
                let n = (max - stack.count).min(count);
                stack.count += n;
                count -= n;
            }
        }
        for slot in self.slots.iter_mut().filter(|s| s.is_none()) {
            if count == 0 {
                break;
            }
            let n = max.min(count);
            *slot = Some(Stack::new(item, n));
            count -= n;
        }
        count
    }

    /// Insere uma pilha preservando seus dados (ex.: durabilidade). Retorna o que não coube.
    pub fn insert_stack(&mut self, stack: Stack) -> Option<Stack> {
        if stack.item.max_stack() == 1 {
            let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
                return Some(stack);
            };
            *slot = Some(stack);
            return None;
        }
        let leftover = self.add(stack.item, stack.count);
        (leftover > 0).then_some(Stack {
            count: leftover,
            ..stack
        })
    }

    /// Move para o inventário tudo o que couber; `stacks` fica só com o que sobrou.
    pub fn take_all(&mut self, stacks: &mut Vec<Stack>) {
        *stacks = stacks.drain(..).filter_map(|s| self.insert_stack(s)).collect();
    }

    /// Esvazia o inventário devolvendo todas as pilhas.
    pub fn drain_all(&mut self) -> Vec<Stack> {
        self.slots.iter_mut().filter_map(Option::take).collect()
    }

    /// Gasta 1 de durabilidade do item no slot. Retorna o item se ele quebrou.
    pub fn wear(&mut self, index: usize) -> Option<ItemId> {
        let stack = self.slots[index].as_mut()?;
        stack.item.max_durability()?;
        stack.durability = stack.durability.saturating_sub(1);
        if stack.durability > 0 {
            return None;
        }
        self.slots[index].take().map(|s| s.item)
    }

    /// Retira `count` unidades de um slot específico (ex.: comer, largar).
    pub fn take_from_slot(&mut self, index: usize, count: u32) -> Option<Stack> {
        let slot = &mut self.slots[index];
        let stack = slot.as_mut()?;
        let n = stack.count.min(count);
        let taken = Stack { count: n, ..*stack };
        stack.count -= n;
        if stack.count == 0 {
            *slot = None;
        }
        Some(taken)
    }

    /// Peso total das pilhas (ver `ItemId::weight`).
    pub fn weight(&self) -> f32 {
        self.slots
            .iter()
            .flatten()
            .map(|s| s.item.weight() * s.count as f32)
            .sum()
    }

    pub fn count(&self, item: ItemId) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.count)
            .sum()
    }

    /// Remove `count` unidades (tudo ou nada), consumindo do fim para preservar a hotbar.
    pub fn remove(&mut self, item: ItemId, mut count: u32) -> bool {
        if self.count(item) < count {
            return false;
        }
        for slot in self.slots.iter_mut().rev() {
            if count == 0 {
                break;
            }
            let Some(stack) = slot.as_mut() else { continue };
            if stack.item != item {
                continue;
            }
            let n = stack.count.min(count);
            stack.count -= n;
            count -= n;
            if stack.count == 0 {
                *slot = None;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_stacks_then_overflows() {
        let mut inv = Inventory::new(2);
        assert_eq!(inv.add(ItemId::Wood, 150), 0);
        assert_eq!(inv.slots[0].unwrap().count, 100);
        assert_eq!(inv.slots[1].unwrap().count, 50);
        assert_eq!(inv.add(ItemId::Wood, 60), 10);
        assert_eq!(inv.count(ItemId::Wood), 200);
    }

    #[test]
    fn tools_do_not_stack() {
        let mut inv = Inventory::new(3);
        assert_eq!(inv.add(ItemId::StonePickaxe, 2), 0);
        assert!(inv.slots[0].is_some() && inv.slots[1].is_some() && inv.slots[2].is_none());
    }

    #[test]
    fn tools_wear_out_and_break() {
        let mut inv = Inventory::new(2);
        inv.add(ItemId::StonePickaxe, 1);
        inv.slots[0].as_mut().unwrap().durability = 2;
        assert_eq!(inv.wear(0), None);
        assert_eq!(inv.slots[0].unwrap().durability, 1);
        assert_eq!(inv.wear(0), Some(ItemId::StonePickaxe));
        assert!(inv.slots[0].is_none());
        inv.add(ItemId::Wood, 5);
        assert_eq!(inv.wear(0), None, "itens sem durabilidade não desgastam");
        assert_eq!(inv.count(ItemId::Wood), 5);
    }

    #[test]
    fn insert_stack_keeps_durability_and_take_all_leaves_leftovers() {
        let mut inv = Inventory::new(2);
        let worn = Stack {
            durability: 7,
            ..Stack::new(ItemId::StoneHatchet, 1)
        };
        assert_eq!(inv.insert_stack(worn), None);
        assert_eq!(inv.slots[0].unwrap().durability, 7);

        let mut bag = vec![Stack::new(ItemId::Stone, 150), Stack::new(ItemId::StonePickaxe, 1)];
        inv.take_all(&mut bag);
        assert_eq!(inv.count(ItemId::Stone), 100);
        assert_eq!(bag.len(), 2);
        assert_eq!(bag[0].count, 50);
    }

    #[test]
    fn take_from_slot_partial() {
        let mut inv = Inventory::new(1);
        inv.add(ItemId::Berry, 3);
        assert_eq!(inv.take_from_slot(0, 1).unwrap().count, 1);
        assert_eq!(inv.count(ItemId::Berry), 2);
        assert_eq!(inv.take_from_slot(0, 5).unwrap().count, 2);
        assert!(inv.slots[0].is_none());
    }

    #[test]
    fn remove_is_all_or_nothing() {
        let mut inv = Inventory::new(4);
        inv.add(ItemId::Stone, 5);
        assert!(!inv.remove(ItemId::Stone, 6));
        assert_eq!(inv.count(ItemId::Stone), 5);
        assert!(inv.remove(ItemId::Stone, 5));
        assert!(inv.slots.iter().all(Option::is_none));
    }
}
