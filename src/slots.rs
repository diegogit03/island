//! Operações de arrastar itens entre slots (pegar, soltar, dividir, transferir).
//! Rodam no servidor: o inventário e a pilha "no cursor" (`held`) são autoritativos lá.

use crate::inventory::{Inventory, Stack};

/// Clique esquerdo: pegar, soltar, juntar ou trocar pilhas.
pub fn click(held: &mut Option<Stack>, slot: &mut Option<Stack>) {
    match (held.take(), slot.take()) {
        (None, s) => *held = s,
        (Some(h), None) => *slot = Some(h),
        (Some(mut h), Some(mut s)) if h.item == s.item && s.count < s.item.max_stack() => {
            let moved = (s.item.max_stack() - s.count).min(h.count);
            s.count += moved;
            h.count -= moved;
            *slot = Some(s);
            *held = (h.count > 0).then_some(h);
        }
        (Some(h), Some(s)) => {
            *slot = Some(h);
            *held = Some(s);
        }
    }
}

/// Clique direito: pega metade da pilha (sem nada no cursor) ou solta 1 unidade.
pub fn split(held: &mut Option<Stack>, slot: &mut Option<Stack>) {
    match (held.as_mut(), slot.as_mut()) {
        (None, Some(s)) if s.count > 1 => {
            let half = s.count / 2;
            s.count -= half;
            *held = Some(Stack { count: half, ..*s });
        }
        (Some(h), None) => {
            *slot = Some(Stack { count: 1, ..*h });
            h.count -= 1;
        }
        (Some(h), Some(s)) if h.item == s.item && s.count < s.item.max_stack() => {
            s.count += 1;
            h.count -= 1;
        }
        _ => {}
    }
    if held.is_some_and(|h| h.count == 0) {
        *held = None;
    }
}

/// Shift+clique: move a pilha inteira para o outro inventário (o que não couber fica).
pub fn transfer(slot: &mut Option<Stack>, to: &mut Inventory) {
    if let Some(stack) = slot.take() {
        *slot = to.insert_stack(stack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemId;

    #[test]
    fn pick_place_merge_swap() {
        let mut held = None;
        let mut a = Some(Stack::new(ItemId::Wood, 10));
        let mut b = Some(Stack::new(ItemId::Wood, 95));
        click(&mut held, &mut a);
        assert_eq!(held.unwrap().count, 10);
        assert!(a.is_none());
        click(&mut held, &mut b);
        assert_eq!(b.unwrap().count, 100);
        assert_eq!(held.unwrap().count, 5, "sobra fica no cursor");

        let mut stone = Some(Stack::new(ItemId::Stone, 3));
        click(&mut held, &mut stone);
        assert_eq!(stone.unwrap().item, ItemId::Wood);
        assert_eq!(held.unwrap().item, ItemId::Stone, "itens diferentes trocam");
    }

    #[test]
    fn split_takes_half_and_drops_one() {
        let mut held = None;
        let mut slot = Some(Stack::new(ItemId::Berry, 9));
        split(&mut held, &mut slot);
        assert_eq!((held.unwrap().count, slot.unwrap().count), (4, 5));
        let mut empty = None;
        split(&mut held, &mut empty);
        assert_eq!((held.unwrap().count, empty.unwrap().count), (3, 1));
    }

    #[test]
    fn transfer_keeps_leftovers() {
        let mut to = Inventory::new(1);
        to.add(ItemId::Stone, 90);
        let mut slot = Some(Stack::new(ItemId::Stone, 30));
        transfer(&mut slot, &mut to);
        assert_eq!(to.count(ItemId::Stone), 100);
        assert_eq!(slot.unwrap().count, 20);
    }
}
