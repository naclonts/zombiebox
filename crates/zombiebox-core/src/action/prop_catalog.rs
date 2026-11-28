//! Stable Phase 2 prop definitions and canonical simulation-space dimensions.
//!
//! The imported bounds round out the closed meshes to the next centimeter.
//! Footprints also reserve room for opened container geometry.

use super::{
    props::{CellAttachment, PropSpan},
    types::{CacheLoot, Material, Vec3},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PropCover {
    #[default]
    None,
    Low,
    HighBlocker,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropDef {
    pub id: PropId,
    pub span: PropSpan,
    pub collision_min: Vec3,
    pub collision_max: Vec3,
    pub cover: PropCover,
    pub attachment: CellAttachment,
    pub durability: u16,
    pub material: Material,
    pub loot: Option<CacheLoot>,
}

const OFFICE_SUPPLIES: Option<CacheLoot> = Some(CacheLoot {
    weapon: None,
    pistol_ammo: 6,
    shotgun_ammo: 0,
    rockets: 0,
    medkits: 0,
});

const MEDICAL_SUPPLIES: Option<CacheLoot> = Some(CacheLoot {
    weapon: None,
    pistol_ammo: 0,
    shotgun_ammo: 0,
    rockets: 0,
    medkits: 1,
});

macro_rules! define_props {
    ($(
        $id:ident = $value:literal, $stem:literal, $span:expr, $half:expr,
        $cover:ident, $layer:ident, $material:ident, $hp:literal, $loot:expr;
    )+) => {
        /// Stable numeric IDs shared by gameplay and the prototype library.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        #[repr(u16)]
        pub enum PropId {
            $($id = $value,)+
        }

        impl PropId {
            pub const ALL: &'static [Self] = &[$(Self::$id,)+];

            pub const fn stem(self) -> &'static str {
                match self { $(Self::$id => $stem,)+ }
            }

            pub fn from_stem(stem: &str) -> Option<Self> {
                match stem {
                    $($stem => Some(Self::$id),)+
                    _ => None,
                }
            }

            pub const fn def(self) -> PropDef {
                match self {
                    $(Self::$id => {
                        let ([w, d], [x, y, z]) = ($span, $half);
                        let scale = self.presentation_scale();
                        PropDef {
                            id: self,
                            span: PropSpan::new(w, d),
                            collision_min: Vec3::new(-x * scale, -y * scale, 0.0),
                            collision_max: Vec3::new(x * scale, y * scale, z * scale),
                            cover: PropCover::$cover,
                            attachment: CellAttachment::$layer,
                            durability: $hp,
                            material: Material::$material,
                            loot: $loot,
                        }
                    },)+
                }
            }
        }
    }
}

impl PropId {
    /// Furniture was authored at human meter scale; the survivor is 1.05 units tall.
    /// Keep rendered meshes and their simulation bounds in the same scale.
    pub const fn presentation_scale(self) -> f32 {
        match self {
            Self::Table
            | Self::Sofa
            | Self::Bed
            | Self::Chair
            | Self::Desk
            | Self::Toilet
            | Self::Fridge
            | Self::FirstAidKit
            | Self::HospitalBed
            | Self::IvStand
            | Self::ShoppingCart
            | Self::BedDouble
            | Self::CardboardBox
            | Self::ChairDesk
            | Self::BathroomSink
            | Self::Bathtub
            | Self::TableCoffee
            | Self::KitchenSink
            | Self::KitchenStove
            | Self::KitchenCabinet
            | Self::Television
            | Self::BookcaseOpen
            | Self::BookcaseClosed
            | Self::MedicalCart
            | Self::DiagnosticMonitor
            | Self::Wheelchair
            | Self::CheckoutTill
            | Self::CheckoutLane
            | Self::GondolaShelf
            | Self::Note
            | Self::Journal
            | Self::TvStand
            | Self::Nightstand => 0.6,
            _ => 1.0,
        }
    }
}

impl Default for PropId {
    fn default() -> Self {
        Self::Table
    }
}

#[rustfmt::skip]
define_props! {
    Table = 1, "table", [2, 1], [0.85, 0.45, 0.66], Low, Floor, WoodLight, 55, None;
    Sofa = 2, "lounge-sofa", [2, 1], [0.98, 0.41, 0.92], Low, Floor, WoodLight, 75, None;
    Counter = 3, "counter", [1, 1], [0.42, 0.34, 1.00], Low, Floor, WoodLight, 75, None;
    Bed = 4, "bed-single", [2, 3], [0.58, 1.13, 0.75], Low, Floor, WoodLight, 65, None;
    Crate = 5, "crate", [1, 1], [0.25, 0.25, 0.49], Low, Floor, WoodLight, 45, None;
    Fence = 6, "fence", [1, 1], [0.50, 0.08, 1.00], Low, Floor, WoodLight, 40, None;
    Mailbox = 7, "mailbox", [1, 1], [0.20, 0.25, 1.00], Low, Floor, MetalPole, 30, None;
    Bench = 8, "bench", [1, 1], [0.49, 0.27, 1.00], Low, Floor, WoodLight, 50, None;
    Shed = 9, "shed", [1, 1], [0.50, 0.50, 1.90], HighBlocker, Floor, WoodSiding, 90, None;
    BirchTree = 10, "birch-tree", [2, 2], [0.62, 0.62, 2.20], HighBlocker, Floor, TreeWood, 120, None;
    Chair = 11, "chair", [1, 1], [0.20, 0.20, 0.94], Low, Floor, WoodLight, 30, None;
    Desk = 12, "desk", [2, 2], [0.74, 0.40, 0.77], Low, Floor, WoodLight, 65, OFFICE_SUPPLIES;
    Toilet = 13, "toilet", [1, 1], [0.32, 0.48, 0.91], Low, Floor, BrickMasonry, 55, None;
    Fridge = 14, "kitchen-fridge", [2, 2], [0.43, 0.30, 1.84], HighBlocker, Floor, MetalPole, 95, MEDICAL_SUPPLIES;
    FirstAidKit = 15, "first-aid-kit", [1, 1], [0.19, 0.07, 0.26], None, CounterTop, MetalPole, 20, None;
    HospitalBed = 16, "hospital-bed", [1, 2], [0.33, 0.99, 1.79], Low, Floor, MetalPole, 75, None;
    IvStand = 17, "iv-stand", [1, 1], [0.14, 0.14, 1.68], Low, Floor, MetalPole, 30, None;
    ShoppingCart = 18, "shopping-cart", [1, 1], [0.30, 0.49, 1.05], Low, Floor, MetalPole, 45, None;
    PoliceCar = 19, "police-car", [2, 4], [0.89, 1.87, 1.24], HighBlocker, Floor, MetalPole, 180, None;
    BedDouble = 20, "bed-double", [2, 3], [0.96, 1.13, 0.75], Low, Floor, WoodLight, 75, None;
    CardboardBox = 21, "cardboard-box", [1, 1], [0.22, 0.22, 0.57], Low, Floor, WoodLight, 25, OFFICE_SUPPLIES;
    ChairDesk = 22, "chair-desk", [1, 1], [0.34, 0.32, 1.22], Low, Floor, MetalPole, 30, None;
    BathroomSink = 23, "bathroom-sink", [1, 1], [0.34, 0.29, 1.12], Low, Floor, BrickMasonry, 60, None;
    Bathtub = 24, "bathtub", [3, 2], [1.19, 0.56, 0.84], Low, Floor, BrickMasonry, 85, None;
    TableCoffee = 25, "table-coffee", [2, 1], [0.67, 0.40, 0.46], Low, Floor, WoodLight, 45, None;
    KitchenSink = 26, "kitchen-sink", [1, 1], [0.43, 0.45, 0.98], Low, Floor, MetalPole, 65, None;
    KitchenStove = 27, "kitchen-stove-electric", [1, 1], [0.43, 0.45, 0.90], Low, Floor, MetalPole, 70, None;
    KitchenCabinet = 28, "kitchen-cabinet", [1, 1], [0.43, 0.45, 0.90], Low, Floor, WoodLight, 55, None;
    Television = 29, "television-vintage", [1, 1], [0.41, 0.27, 0.54], None, CounterTop, Glass, 30, None;
    BookcaseOpen = 30, "bookcase-open", [1, 1], [0.40, 0.25, 1.76], HighBlocker, Floor, WoodLight, 75, None;
    BookcaseClosed = 31, "bookcase-closed-doors", [1, 1], [0.40, 0.25, 1.70], HighBlocker, Floor, WoodLight, 75, None;
    MedicalCart = 32, "medical-cart", [1, 1], [0.28, 0.20, 0.95], Low, Floor, MetalPole, 50, None;
    DiagnosticMonitor = 33, "diagnostic-monitor", [1, 1], [0.11, 0.06, 0.28], None, CounterTop, Glass, 25, None;
    Wheelchair = 34, "wheelchair", [1, 1], [0.37, 0.22, 0.87], Low, Floor, MetalPole, 40, None;
    Car = 35, "car", [2, 5], [0.90, 2.11, 1.19], HighBlocker, Floor, MetalPole, 165, None;
    CheckoutTill = 36, "assisted-checkout-till", [1, 1], [0.38, 0.26, 0.51], None, CounterTop, MetalPole, 45, OFFICE_SUPPLIES;
    CheckoutLane = 37, "checkout-lane", [1, 2], [0.46, 0.99, 0.95], Low, Floor, MetalPole, 80, None;
    GondolaShelf = 38, "gondola-shelf", [2, 1], [0.99, 0.34, 1.45], HighBlocker, Floor, MetalPole, 85, None;
    Sign = 39, "sign", [1, 1], [0.50, 0.06, 0.40], None, WallMount, WoodLight, 25, None;
    Note = 40, "note", [1, 1], [0.32, 0.04, 0.24], None, WallMount, WoodLight, 10, None;
    Journal = 41, "journal", [1, 1], [0.38, 0.22, 0.08], None, Floor, WoodLight, 15, None;
    TvStand = 42, "tv-stand", [2, 1], [0.60, 0.22, 0.42], Low, Floor, WoodLight, 60, None;
    Nightstand = 43, "nightstand", [1, 1], [0.26, 0.24, 0.58], Low, Floor, WoodLight, 40, None;
    GrocerySign = 44, "grocery-sign", [1, 1], [0.90, 0.06, 0.40], None, WallMount, WoodLight, 25, None;
    RestroomSign = 45, "restroom-sign", [1, 1], [0.42, 0.06, 0.30], None, WallMount, WoodLight, 20, None;
    FuelPump = 47, "fuel-pump", [1, 1], [0.45, 0.35, 1.65], HighBlocker, Floor, MetalPole, 100, None;
    FuelPylon = 48, "fuel-pylon", [1, 1], [0.45, 0.25, 2.40], HighBlocker, Floor, MetalPole, 100, None;
    AisleSign = 46, "aisle-sign", [1, 1], [0.40, 0.06, 0.28], None, WallMount, WoodLight, 20, None;
}
