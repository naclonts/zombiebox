//! Authoritative text for world-readable signs, notes, and journals.
//! Frontends receive only the stable ID in frame HUD data and resolve text
//! through this table, keeping browser and Android wording identical.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReadableId {
    BirchDirectory = 1,
    MaintenanceNote = 2,
    SurvivorJournal = 3,
    CountyDirectory = 4,
    CraftsmanNote = 5,
    GroceryNotice = 6,
}

impl ReadableId {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::BirchDirectory),
            2 => Some(Self::MaintenanceNote),
            3 => Some(Self::SurvivorJournal),
            4 => Some(Self::CountyDirectory),
            5 => Some(Self::CraftsmanNote),
            6 => Some(Self::GroceryNotice),
            _ => None,
        }
    }
    pub const fn title(self) -> &'static str {
        match self {
            Self::BirchDirectory => "Birch Street directory",
            Self::MaintenanceNote => "Maintenance note",
            Self::SurvivorJournal => "Survivor journal",
            Self::CountyDirectory => "County directory",
            Self::CraftsmanNote => "Craftsman house note",
            Self::GroceryNotice => "Market notice",
        }
    }
    pub const fn body(self) -> &'static str {
        match self {
            Self::BirchDirectory => {
                "Birch Street homes are numbered from the corner. Keep the porch light in sight."
            }
            Self::MaintenanceNote => {
                "The spare key is taped beneath the third shelf. Check the door before you leave."
            }
            Self::SurvivorJournal => {
                "Noise carries after dusk. Move quietly, and keep an exit in view."
            }
            Self::CountyDirectory => {
                "County roads radiate from the town center. Bridges and marked routes are safest after dusk."
            }
            Self::CraftsmanNote => {
                "Porch light stays on. The spare key is under the kitchen sink, and the pantry is stocked."
            }
            Self::GroceryNotice => {
                "Store closed at dusk. Take only what you need, and keep the loading door clear."
            }
        }
    }
    pub const fn title_c(self) -> &'static std::ffi::CStr {
        match self {
            Self::BirchDirectory => c"Birch Street directory",
            Self::MaintenanceNote => c"Maintenance note",
            Self::SurvivorJournal => c"Survivor journal",
            Self::CountyDirectory => c"County directory",
            Self::CraftsmanNote => c"Craftsman house note",
            Self::GroceryNotice => c"Market notice",
        }
    }
    pub const fn body_c(self) -> &'static std::ffi::CStr {
        match self {
            Self::BirchDirectory => c"Birch Street homes are numbered from the corner. Keep the porch light in sight.",
            Self::MaintenanceNote => c"The spare key is taped beneath the third shelf. Check the door before you leave.",
            Self::SurvivorJournal => c"Noise carries after dusk. Move quietly, and keep an exit in view.",
            Self::CountyDirectory => c"County roads radiate from the town center. Bridges and marked routes are safest after dusk.",
            Self::CraftsmanNote => c"Porch light stays on. The spare key is under the kitchen sink, and the pantry is stocked.",
            Self::GroceryNotice => c"Store closed at dusk. Take only what you need, and keep the loading door clear.",
        }
    }
}

pub fn readable_text(id: u8) -> Option<(u8, &'static str, &'static str)> {
    ReadableId::from_u8(id).map(|value| (value as u8, value.title(), value.body()))
}
