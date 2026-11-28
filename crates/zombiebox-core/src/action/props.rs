use super::{
    prop_catalog::PropId,
    types::{Floor, Tile3D, TileVisibility, Vec3},
};
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PropState {
    #[default]
    Closed,
    Opened,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum PropFacing {
    #[default]
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PropSpan {
    pub width: u8,
    pub height: u8,
}
impl PropSpan {
    pub const fn new(width: u8, height: u8) -> Self {
        Self { width, height }
    }
    pub fn covered_tiles(self, a: Tile3D) -> impl Iterator<Item = Tile3D> {
        (0..self.height as i32).flat_map(move |y| {
            (0..self.width as i32).map(move |x| Tile3D::new(a.x + x, a.y + y, a.floor))
        })
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CellAttachment {
    #[default]
    Floor,
    CounterTop,
    WallMount,
    Ceiling,
    Rooftop,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PropAnchor {
    pub anchor_uid: u64,
    pub tile: Tile3D,
    pub id: PropId,
    pub span: PropSpan,
    pub attachment: CellAttachment,
    pub facing: PropFacing,
    pub offset: Vec3,
}
impl PropAnchor {
    pub fn pivot(self, support_z: f32) -> Vec3 {
        let (w, h) = (self.span.width as f32, self.span.height as f32);
        Vec3::new(
            self.tile.x as f32 + (w - 1.0) * 0.5,
            self.tile.y as f32 + (h - 1.0) * 0.5,
            support_z,
        ) + self.offset
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropView {
    pub id: PropId,
    pub anchor: PropAnchor,
    pub world_pivot: Vec3,
    pub floor: Floor,
    pub facing: PropFacing,
    pub state: PropState,
    pub anchor_uid: u64,
    pub visibility: TileVisibility,
}
