//! Deterministic motorcycle paint palettes and cosmetic condition selection.
//!
//! This module is the canonical runtime palette registry. The mesh header's
//! generated comment still refers to the original asset generator; do not
//! regenerate the triangle tables just to change paint. Add or edit palette
//! rows here instead. Paint and cosmetic age are renderer-only presentation
//! state derived from the stable [`VehicleId`]; they never affect simulation.

use zombiebox_core::{VehicleId, mix64};

/// Number of deterministic motorcycle slots in a generated county run.
///
/// Mirrors the core roster size. Slot `0` is always the starter bike.
pub const MOTORCYCLE_SLOT_COUNT: usize = 4;

/// Period-inspired paint schemes applied to generated motorcycles.
///
/// Names such as `MilitarySurplus` and `Copperhead` describe paint colors only;
/// chrome, rubber and seat materials remain their fixed shared colors.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum MotorcyclePaint {
    #[default]
    GreenBlack,
    BurgundyBlack,
    CreamBlack,
    MidnightBlue,
    MatteBlack,
    SomersetOrangeWhite,
    SonomaGreenWhite,
    SedonaRedSilver,
    BeringBlueBlack,
    BlazeYellowBlack,
    ChaparralBeigeBlack,
    AmericanRedBlack,
    OrionSilverWhite,
    RoyalPurpleSilver,
    SunburstOrangeBlack,
    MilitarySurplus,
    RustDevil,
    Route66,
    Copperhead,
    GhostRider,
    BlackCherry,
    Wasteland,
}

const fn rgb(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

impl MotorcyclePaint {
    /// All paint variants in stable selection order.
    pub const ALL: [Self; 22] = [
        Self::GreenBlack,
        Self::BurgundyBlack,
        Self::CreamBlack,
        Self::MidnightBlue,
        Self::MatteBlack,
        Self::SomersetOrangeWhite,
        Self::SonomaGreenWhite,
        Self::SedonaRedSilver,
        Self::BeringBlueBlack,
        Self::BlazeYellowBlack,
        Self::ChaparralBeigeBlack,
        Self::AmericanRedBlack,
        Self::OrionSilverWhite,
        Self::RoyalPurpleSilver,
        Self::SunburstOrangeBlack,
        Self::MilitarySurplus,
        Self::RustDevil,
        Self::Route66,
        Self::Copperhead,
        Self::GhostRider,
        Self::BlackCherry,
        Self::Wasteland,
    ];

    /// Returns `[primary, secondary, pinstripe]` RGB24 colors.
    ///
    /// The first five rows preserve the original asset palette exactly.
    #[must_use]
    pub const fn palette(self) -> [[u8; 3]; 3] {
        match self {
            Self::GreenBlack => [rgb(0x265B43), rgb(0x181D1E), rgb(0xAC9052)],
            Self::BurgundyBlack => [rgb(0x71232B), rgb(0x191B1D), rgb(0xB18F4F)],
            Self::CreamBlack => [rgb(0xCCBE9A), rgb(0x1B1F1F), rgb(0x97753C)],
            Self::MidnightBlue => [rgb(0x294569), rgb(0x161B21), rgb(0x97A6B1)],
            Self::MatteBlack => [rgb(0x313333), rgb(0x141617), rgb(0x696967)],
            Self::SomersetOrangeWhite => [rgb(0xC36331), rgb(0xEEE4CF), rgb(0xA47D40)],
            Self::SonomaGreenWhite => [rgb(0x285D47), rgb(0xEAE1CC), rgb(0xB59658)],
            Self::SedonaRedSilver => [rgb(0x8E2936), rgb(0x92969B), rgb(0xD0D2CF)],
            Self::BeringBlueBlack => [rgb(0x284B7B), rgb(0x191B1D), rgb(0xB5BEC7)],
            Self::BlazeYellowBlack => [rgb(0xD9AE24), rgb(0x191B1D), rgb(0xA47C36)],
            Self::ChaparralBeigeBlack => [rgb(0xC0AD8B), rgb(0x191B1D), rgb(0x8B7046)],
            Self::AmericanRedBlack => [rgb(0xB62927), rgb(0x191B1D), rgb(0xC4B48B)],
            Self::OrionSilverWhite => [rgb(0xAEB5BF), rgb(0xEEECE4), rgb(0x777E89)],
            Self::RoyalPurpleSilver => [rgb(0x573477), rgb(0xA6A8AF), rgb(0xD6D5C9)],
            Self::SunburstOrangeBlack => [rgb(0xD37126), rgb(0x191B1D), rgb(0xC5A26A)],
            Self::MilitarySurplus => [rgb(0x62694A), rgb(0x262925), rgb(0x777C63)],
            Self::RustDevil => [rgb(0x75332D), rgb(0x242220), rgb(0x98704F)],
            Self::Route66 => [rgb(0x418783), rgb(0xE5D4B2), rgb(0xBBA16D)],
            Self::Copperhead => [rgb(0xA26D45), rgb(0x272323), rgb(0xD4AA71)],
            Self::GhostRider => [rgb(0xD9D7D0), rgb(0x26282B), rgb(0x82868A)],
            Self::BlackCherry => [rgb(0x72324B), rgb(0x201E24), rgb(0xBCA071)],
            Self::Wasteland => [rgb(0xB3A17B), rgb(0x665A44), rgb(0x8C7954)],
        }
    }
}

/// Convenience alias for [`MotorcyclePaint::ALL`].
pub const ALL: [MotorcyclePaint; 22] = MotorcyclePaint::ALL;

/// Cosmetic aging independent of the mechanical `condition` field.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum CosmeticAge {
    #[default]
    Pristine,
    Dusty,
    Weathered,
    Rusted,
    BattleDamaged,
}

/// Stable visual appearance for one motorcycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MotorcycleAppearance {
    pub paint: MotorcyclePaint,
    pub age: CosmeticAge,
    pub pattern_seed: u64,
}

/// Complete cache key for uploaded motorcycle prototype meshes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MotorcycleVisualKey {
    pub appearance: MotorcycleAppearance,
    /// `0..=5`; `0` means no mechanical damage. Drives extra chips/dust/tarnish.
    pub damage_bucket: u8,
}

/// Returns the roster slot encoded in the low byte of a vehicle ID.
#[must_use]
pub fn motorcycle_slot(id: VehicleId) -> Option<usize> {
    let slot = (id.0 & 0xff) as usize;
    (slot < MOTORCYCLE_SLOT_COUNT).then_some(slot)
}

/// Deterministically chooses paint and cosmetic age for a generated motorcycle.
///
/// Slot `0` (the starter) always stays pristine `GreenBlack`. Other slots use
/// independent domain hashes for paint, age and noise pattern, so appearance is
/// stable across frames, streaming and mounting.
#[must_use]
pub fn appearance_for(id: VehicleId) -> MotorcycleAppearance {
    let pattern_seed = mix64(id.0 ^ 0x5041_5454_4552_4e31);
    if motorcycle_slot(id) == Some(0) {
        return MotorcycleAppearance {
            paint: MotorcyclePaint::GreenBlack,
            age: CosmeticAge::Pristine,
            pattern_seed,
        };
    }
    let p = mix64(id.0 ^ 0x5041_494e_545f_3031);
    let a = mix64(id.0 ^ 0x4147_455f_3030_3031) % 100;
    MotorcycleAppearance {
        paint: MotorcyclePaint::ALL[(p % MotorcyclePaint::ALL.len() as u64) as usize],
        age: match a {
            0..=9 => CosmeticAge::Pristine,
            10..=39 => CosmeticAge::Dusty,
            40..=69 => CosmeticAge::Weathered,
            70..=89 => CosmeticAge::Rusted,
            _ => CosmeticAge::BattleDamaged,
        },
        pattern_seed,
    }
}

/// Builds the mesh cache key for a vehicle at its current mechanical condition.
#[must_use]
pub fn visual_key(id: VehicleId, condition: u8) -> MotorcycleVisualKey {
    MotorcycleVisualKey {
        appearance: appearance_for(id),
        damage_bucket: (100 - condition.min(100)).div_ceil(20),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rogue(slot: u8, seed: u64) -> VehicleId {
        // Keep the low byte as the slot and vary the rest like `vehicle_id`.
        VehicleId((mix64(seed ^ 0x7665_6869_636c_6569) & !0xff) | u64::from(slot))
    }

    #[test]
    fn original_palettes_are_preserved_byte_for_byte() {
        assert_eq!(
            MotorcyclePaint::GreenBlack.palette(),
            [[38, 91, 67], [24, 29, 30], [172, 144, 82]]
        );
        assert_eq!(
            MotorcyclePaint::BurgundyBlack.palette(),
            [[113, 35, 43], [25, 27, 29], [177, 143, 79]]
        );
        assert_eq!(
            MotorcyclePaint::CreamBlack.palette(),
            [[204, 190, 154], [27, 31, 31], [151, 117, 60]]
        );
        assert_eq!(
            MotorcyclePaint::MidnightBlue.palette(),
            [[41, 69, 105], [22, 27, 33], [151, 166, 177]]
        );
        assert_eq!(
            MotorcyclePaint::MatteBlack.palette(),
            [[49, 51, 51], [20, 22, 23], [105, 105, 103]]
        );
    }

    #[test]
    fn all_palettes_are_unique_and_default_is_green_black() {
        assert_eq!(MotorcyclePaint::default(), MotorcyclePaint::GreenBlack);
        assert_eq!(MotorcyclePaint::ALL.len(), 22);
        for (i, a) in MotorcyclePaint::ALL.iter().enumerate() {
            for b in &MotorcyclePaint::ALL[i + 1..] {
                assert_ne!(a.palette(), b.palette(), "{a:?} and {b:?} share a palette");
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn starter_is_always_pristine_green_black() {
        for seed in 0..5_000u64 {
            let id = rogue(0, seed);
            let appearance = appearance_for(id);
            assert_eq!(appearance.paint, MotorcyclePaint::GreenBlack);
            assert_eq!(appearance.age, CosmeticAge::Pristine);
            // Pattern seed still varies, but pristine output stays exact.
            assert_eq!(appearance.pattern_seed, mix64(id.0 ^ 0x5041_5454_4552_4e31));
        }
    }

    #[test]
    fn appearance_is_deterministic_and_stable_across_conditions() {
        for slot in 1..MOTORCYCLE_SLOT_COUNT as u8 {
            let id = rogue(slot, 42);
            let first = appearance_for(id);
            assert_eq!(first, appearance_for(id));
            for condition in [100, 99, 79, 59, 39, 19, 0, 255] {
                assert_eq!(visual_key(id, condition).appearance, first);
            }
        }
    }

    #[test]
    fn non_starter_sampling_is_diverse_and_covers_every_age() {
        let mut paints = std::collections::HashSet::new();
        let mut ages = std::collections::HashSet::new();
        for seed in 0..4_096u64 {
            for slot in 1..MOTORCYCLE_SLOT_COUNT as u8 {
                let appearance = appearance_for(rogue(slot, seed));
                paints.insert(appearance.paint);
                ages.insert(appearance.age);
                assert_eq!(appearance, appearance_for(rogue(slot, seed)));
            }
        }
        assert_eq!(paints.len(), MotorcyclePaint::ALL.len());
        for age in [
            CosmeticAge::Pristine,
            CosmeticAge::Dusty,
            CosmeticAge::Weathered,
            CosmeticAge::Rusted,
            CosmeticAge::BattleDamaged,
        ] {
            assert!(ages.contains(&age), "missing age {age:?}");
        }
    }

    #[test]
    fn damage_buckets_follow_condition_thresholds() {
        let id = rogue(1, 42);
        for (condition, expected) in [
            (100, 0),
            (99, 1),
            (80, 1),
            (79, 2),
            (60, 2),
            (59, 3),
            (40, 3),
            (39, 4),
            (20, 4),
            (19, 5),
            (0, 5),
            (200, 0),
            (255, 0),
        ] {
            assert_eq!(
                visual_key(id, condition).damage_bucket,
                expected,
                "condition {condition}"
            );
        }
    }

    #[test]
    fn stale_or_unexpected_slots_are_rejected() {
        assert_eq!(motorcycle_slot(VehicleId(0)), Some(0));
        assert_eq!(motorcycle_slot(VehicleId(3)), Some(3));
        assert_eq!(motorcycle_slot(VehicleId(4)), None);
        assert_eq!(motorcycle_slot(VehicleId(0x1_0000_00FF)), None);
    }
}
