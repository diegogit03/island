//! Domesticação estilo ARK: o dino desacordado come do próprio inventário até a barra encher.
//! Golpes nele enquanto desacordado reduzem a efetividade, que define os níveis bônus.

use serde::{Deserialize, Serialize};

use crate::items::ItemId;

/// Segundos entre uma mordida e outra do dino desacordado.
pub const EAT_INTERVAL: f32 = 5.0;
/// Efetividade perdida a cada golpe no dino desacordado.
pub const HIT_PENALTY: f32 = 0.1;
/// Torpor que um narcótico devolve, em fração do máximo.
pub const NARCOTIC_TORPOR: f32 = 0.4;
/// O dino come narcóticos sozinho quando o torpor cai abaixo desta fração.
pub const NARCOTIC_THRESHOLD: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diet {
    Herbivore,
    Carnivore,
    /// Come pedra (golem de pedra).
    Mineral,
}

/// Pontos de domesticação que cada comida vale para a dieta (`None` = não come).
pub fn food_value(diet: Diet, item: ItemId) -> Option<f32> {
    match (diet, item) {
        (Diet::Herbivore, ItemId::Berry) => Some(10.0),
        (Diet::Carnivore, ItemId::RawMeat) => Some(20.0),
        (Diet::Carnivore, ItemId::CookedMeat) => Some(10.0),
        (Diet::Mineral, ItemId::MetalOre) => Some(20.0),
        (Diet::Mineral, ItemId::Stone) => Some(8.0),
        _ => None,
    }
}

/// Comidas aceitas, da melhor para a pior (para dicas na UI).
pub fn preferred_foods(diet: Diet) -> &'static [ItemId] {
    match diet {
        Diet::Herbivore => &[ItemId::Berry],
        Diet::Carnivore => &[ItemId::RawMeat, ItemId::CookedMeat],
        Diet::Mineral => &[ItemId::MetalOre, ItemId::Stone],
    }
}

/// Pontos de comida necessários: criaturas de nível alto exigem mais.
pub fn required_food(base: f32, level: u32) -> f32 {
    base * (1.0 + 0.08 * (level.max(1) - 1) as f32)
}

/// Níveis extras ganhos ao domesticar (até +50% do nível com efetividade máxima).
pub fn bonus_levels(level: u32, effectiveness: f32) -> u32 {
    (level as f32 * 0.5 * effectiveness.clamp(0.0, 1.0)).floor() as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Taming {
    /// 0..1
    pub progress: f32,
    /// 0..1
    pub effectiveness: f32,
    pub eat_timer: f32,
}

impl Default for Taming {
    fn default() -> Self {
        Self {
            progress: 0.0,
            effectiveness: 1.0,
            // Primeira mordida logo após receber comida.
            eat_timer: 1.0,
        }
    }
}

impl Taming {
    pub fn feed(&mut self, value: f32, required: f32) {
        self.progress = (self.progress + value / required).min(1.0);
    }

    pub fn on_hit(&mut self) {
        self.effectiveness = (self.effectiveness - HIT_PENALTY).max(0.0);
    }

    pub fn is_done(&self) -> bool {
        self.progress >= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diets_only_eat_their_food() {
        assert!(food_value(Diet::Herbivore, ItemId::Berry).is_some());
        assert!(food_value(Diet::Herbivore, ItemId::RawMeat).is_none());
        let raw = food_value(Diet::Carnivore, ItemId::RawMeat).unwrap();
        let cooked = food_value(Diet::Carnivore, ItemId::CookedMeat).unwrap();
        assert!(raw > cooked, "carne crua é a preferida");
    }

    #[test]
    fn higher_levels_need_more_food_and_give_more_bonus() {
        assert!(required_food(100.0, 30) > required_food(100.0, 1));
        assert_eq!(bonus_levels(20, 1.0), 10);
        assert_eq!(bonus_levels(20, 0.5), 5);
        assert_eq!(bonus_levels(20, 0.0), 0);
    }

    #[test]
    fn hits_lower_effectiveness() {
        let mut t = Taming::default();
        t.on_hit();
        t.on_hit();
        assert!((t.effectiveness - 0.8).abs() < 1e-6);
        t.feed(50.0, 100.0);
        assert!(!t.is_done());
        t.feed(60.0, 100.0);
        assert!(t.is_done());
    }
}
