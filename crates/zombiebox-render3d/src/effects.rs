//! Transient, visibility-gated CPU effect geometry.
use crate::{
    FrameSnapshot,
    mesh::{
        FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_box_min_max,
    },
};
use glam::Vec3;
use zombiebox_core::{ActionEvent, Floor, ProjectileKind, TileVisibility, Vec3 as SimVec3};

#[derive(Clone, Copy)]
struct Spark {
    at: SimVec3,
    floor: Floor,
    color: [f32; 4],
    life: f32,
    duration: f32,
    size: f32,
}
#[derive(Clone, Copy)]
struct Trace {
    from: SimVec3,
    to: SimVec3,
    floor: Floor,
    life: f32,
}
/// Effects are retained for a short, bounded interval and never mutate the simulation.
#[derive(Default)]
pub struct Effects {
    sparks: Vec<Spark>,
    traces: Vec<Trace>,
}
impl Effects {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn prepare(&mut self, frame: &FrameSnapshot, dt: f32) -> MeshBuffer {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        for s in &mut self.sparks {
            s.life -= dt
        }
        self.sparks.retain(|s| s.life > 0.0);
        for t in &mut self.traces {
            t.life -= dt
        }
        self.traces.retain(|t| t.life > 0.0);
        for event in &frame.events {
            match *event {
                ActionEvent::ShotFired { muzzle, floor, .. } => {
                    self.add(muzzle, floor, [1.0, 0.73, 0.25, 1.0], 0.08, 0.13)
                }
                ActionEvent::BulletTrace { from, to, .. } => self.traces.push(Trace {
                    from,
                    to,
                    floor: frame.dynamic.active_floor,
                    life: 0.08,
                }),
                ActionEvent::MaterialHit {
                    at,
                    hit_pos,
                    destroyed,
                    ..
                } => self.add(
                    hit_pos,
                    at.floor,
                    [0.75, 0.60, 0.40, 1.0],
                    if destroyed { 0.32 } else { 0.18 },
                    if destroyed { 0.15 } else { 0.08 },
                ),
                ActionEvent::StructuralCollapse { at, pos } => {
                    self.add(pos, at.floor, [0.55, 0.48, 0.40, 1.0], 0.4, 0.22)
                }
                ActionEvent::MonsterHit { hit_pos, floor, .. } => {
                    self.add(hit_pos, floor, [0.65, 0.10, 0.08, 1.0], 0.25, 0.10)
                }
                ActionEvent::MonsterDown { at, floor, .. } => {
                    self.add(at, floor, [0.50, 0.07, 0.06, 1.0], 0.35, 0.17)
                }
                ActionEvent::RocketBlast { at, floor, radius } => self.add(
                    at,
                    floor,
                    [1.0, 0.40, 0.10, 1.0],
                    0.32,
                    (radius * 0.16).clamp(0.15, 0.6),
                ),
                ActionEvent::BloaterBurst {
                    at, floor, radius, ..
                } => self.add(
                    at,
                    floor,
                    [0.55, 0.73, 0.20, 1.0],
                    0.4,
                    (radius * 0.16).clamp(0.15, 0.6),
                ),
                _ => {}
            }
        }
        // Keep memory bounded even after a long catch-up frame with many events.
        if self.sparks.len() > 1024 {
            self.sparks.drain(..self.sparks.len() - 1024);
        }
        if self.traces.len() > 256 {
            self.traces.drain(..self.traces.len() - 256);
        }
        let mut mesh = MeshBuilder::new();
        for s in &self.sparks {
            // Impact sparks are local surface effects: keep them tied to tile discovery
            // so a hidden room does not leak its damage state through the shroud.
            if tile_visible(frame, s.at, s.floor) {
                let fraction = (s.life / s.duration).clamp(0.0, 1.0);
                cube(
                    &mut mesh,
                    s.at,
                    s.size * (0.5 + 0.5 * fraction),
                    s.color,
                    s.floor,
                );
            }
        }
        for trace in &self.traces {
            // Tracers always originate from an actor the player can see (shooters need
            // line of sight before firing; player shots start at the player), so the
            // depth buffer is the correct visibility policy. Floor-tile gating here
            // made tracers pop between visible/explored tiles mid-flight.
            let d = trace.to - trace.from;
            let len = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
            let steps = ((len / 0.2).ceil() as usize).clamp(1, 512);
            for i in 0..steps {
                let a = i as f32 / steps as f32;
                let b = (i + 1) as f32 / steps as f32;
                let p = trace.from + d * ((a + b) * 0.5);
                cube(&mut mesh, p, 0.035, [1.0, 0.82, 0.45, 1.0], trace.floor);
            }
        }
        for p in &frame.dynamic.projectiles {
            // Projectiles are 3D actors: rely on the shared depth pass for world
            // occlusion and on the authoritative gameplay LOS flag for secrecy. The
            // floor tile under the projectile is deliberately irrelevant.
            if p.visible_to_player {
                let col = match p.kind {
                    ProjectileKind::Rocket => [1.0, 0.45, 0.12, 1.0],
                    ProjectileKind::ShooterBolt => [0.35, 0.90, 0.28, 1.0],
                };
                cube(&mut mesh, p.position, p.radius.max(0.07), col, p.floor);
            }
        }
        for blast in &frame.dynamic.blasts {
            if tile_visible(frame, blast.position, blast.floor) {
                let col = match blast.kind {
                    crate::scene::BlastSnapshotKind::Rocket => [1.0, 0.43, 0.12, 1.0],
                    crate::scene::BlastSnapshotKind::BloaterCaustic => [0.56, 0.75, 0.20, 1.0],
                };
                cube(
                    &mut mesh,
                    blast.position,
                    (blast.radius * 0.18 * (1.0 - blast.progress).max(0.1)).clamp(0.08, 0.6),
                    col,
                    blast.floor,
                );
            }
        }
        mesh.build()
    }
    fn add(&mut self, at: SimVec3, floor: Floor, color: [f32; 4], duration: f32, size: f32) {
        self.sparks.push(Spark {
            at,
            floor,
            color,
            life: duration,
            duration,
            size,
        });
    }
}
/// Local fog-of-war gate for surface-bound effects (impact sparks, blasts).
///
/// Projectiles and tracers intentionally do not use this: they carry authoritative
/// 3D positions and are resolved by depth testing (and, for projectiles, a 3D LOS
/// flag computed by the gameplay layer).
fn tile_visible(frame: &FrameSnapshot, p: SimVec3, floor: Floor) -> bool {
    frame
        .dynamic
        .local_visibility
        .visibility_at(zombiebox_core::Tile3D::from_vec2(p.xy(), floor))
        == TileVisibility::Visible
}
fn cube(mesh: &mut MeshBuilder, p: SimVec3, r: f32, color: [f32; 4], floor: Floor) {
    let center = Vec3::new(p.x, p.z, p.y);
    let meta = VertexMeta::DEFAULT
        .with_tag(GeometryTag::Effect)
        .with_floor(floor)
        .with_flags(MaterialFlags::UNLIT);
    emit_box_min_max(
        mesh,
        center - Vec3::splat(r),
        center + Vec3::splat(r),
        FaceMask::ALL,
        color,
        meta,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SceneManager;
    use zombiebox_core::{ActionGame, ProjectileId, ProjectileView, Vec2, Weapon};

    #[test]
    fn effects_follow_visibility_and_expire() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new();
        let mut frame = scene.prepare_at_focus(&game, game.player_tile().pos(), 0.0);
        let p = frame.dynamic.player.foot_pos;
        for cell in &mut frame.dynamic.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        frame.events.push(ActionEvent::ShotFired {
            weapon: Weapon::Pistol,
            from: p,
            muzzle: p,
            floor: 0,
            facing: Vec2::new(1.0, 0.0),
            dir: SimVec3::new(1.0, 0.0, 0.0),
        });
        let mut effects = Effects::new();
        let mesh = effects.prepare(&frame, 0.0);
        assert!(!mesh.is_empty());
        mesh.validate().unwrap();
        frame.events.clear();
        for cell in &mut frame.dynamic.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        assert!(effects.prepare(&frame, 0.01).is_empty());
        for cell in &mut frame.dynamic.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        assert!(effects.prepare(&frame, 0.1).is_empty());
    }

    #[test]
    fn projectile_visibility_is_decoupled_from_floor_tiles() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new();
        let mut frame = scene.prepare_at_focus(&game, game.player_tile().pos(), 0.0);
        frame.events.clear();
        // Simulate the projectile crossing an explored/shrouded floor tile.
        for cell in &mut frame.dynamic.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        let p = frame.dynamic.player.foot_pos;
        let projectile_pos = SimVec3::new(p.x, p.y, p.z + 1.8);
        frame.dynamic.projectiles.push(ProjectileView {
            id: ProjectileId(7),
            kind: ProjectileKind::ShooterBolt,
            position: projectile_pos,
            velocity: SimVec3::new(1.0, 0.0, 0.0),
            radius: 0.12,
            pos: projectile_pos.xy(),
            dir: Vec2::new(1.0, 0.0),
            floor: 0,
            z: projectile_pos.z,
            visible_to_player: true,
        });

        let mut effects = Effects::new();
        let mesh = effects.prepare(&frame, 0.0);
        assert!(
            !mesh.is_empty(),
            "projectile with 3D LOS must render above a hidden floor tile"
        );
        mesh.validate().unwrap();

        frame.dynamic.projectiles[0].visible_to_player = false;
        assert!(
            effects.prepare(&frame, 0.0).is_empty(),
            "projectile without 3D LOS is culled even when floor tiles are visible"
        );
    }
}
