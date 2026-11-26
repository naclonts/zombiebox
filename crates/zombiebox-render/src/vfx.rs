//! Combat, destruction, and readability VFX with fixed-capacity zero-alloc pools.

use zombiebox_core::{
    ActionEvent, Floor, Material, MaterialCategory, MonsterKind, MonsterView, PlayerRenderState,
    ProjectileKind, ProjectileView, TileVisibility, Vec2, Weapon,
};

use crate::lighting::{DynamicLight, LightGrid};
use crate::{Canvas, Projector, life, screen_direction};

const PARTICLE_CAP: usize = 128;
const TRACER_CAP: usize = 16;
const MUZZLE_CAP: usize = 4;
const SWING_CAP: usize = 4;
const BLAST_CAP: usize = 8;
const DECAL_CAP: usize = 24;
const LOOT_CAP: usize = 4;

const PISTOL_TRACER_MS: u64 = 95;
const SHOTGUN_TRACER_MS: u64 = 115;
const MUZZLE_MS: u64 = 85;
const AXE_SWING_MS: u64 = 170;
const SHOVE_MS: u64 = 130;
const ROCKET_BLAST_MS: u64 = 380;
const BLOATER_BURST_MS: u64 = 340;
const LOOT_SPARKLE_MS: u64 = 420;

#[derive(Clone, Copy, Eq, PartialEq)]
enum ParticleKind {
    Pixel,
    Shard,
    Smoke,
    HealCross,
}

#[derive(Clone, Copy)]
struct Particle {
    active: bool,
    pos: Vec2,
    floor_z: f32,
    z_px: f32,
    vel: Vec2,
    vz_px: f32,
    gravity: f32,
    age_ms: f32,
    life_ms: f32,
    size: u8,
    color: [u8; 4],
    kind: ParticleKind,
}

impl Particle {
    const INACTIVE: Self = Self {
        active: false,
        pos: Vec2::ZERO,
        floor_z: 0.0,
        z_px: 0.0,
        vel: Vec2::ZERO,
        vz_px: 0.0,
        gravity: 0.0,
        age_ms: 0.0,
        life_ms: 1.0,
        size: 1,
        color: [0; 4],
        kind: ParticleKind::Pixel,
    };
}

#[derive(Clone, Copy)]
struct BulletTracer {
    active: bool,
    weapon: Weapon,
    from: Vec2,
    from_z: f32,
    to: Vec2,
    to_z: f32,
    hit_material: MaterialCategory,
    started_ms: u64,
}

impl BulletTracer {
    const INACTIVE: Self = Self {
        active: false,
        weapon: Weapon::Pistol,
        from: Vec2::ZERO,
        from_z: 0.0,
        to: Vec2::ZERO,
        to_z: 0.0,
        hit_material: MaterialCategory::None,
        started_ms: 0,
    };
}

#[derive(Clone, Copy)]
struct MuzzleFlash {
    active: bool,
    weapon: Weapon,
    from: Vec2,
    floor: Floor,
    facing: Vec2,
    started_ms: u64,
}

impl MuzzleFlash {
    const INACTIVE: Self = Self {
        active: false,
        weapon: Weapon::Pistol,
        from: Vec2::ZERO,
        floor: 0,
        facing: Vec2::ZERO,
        started_ms: 0,
    };
}

#[derive(Clone, Copy)]
struct MeleeSwing {
    active: bool,
    is_axe: bool,
    from: Vec2,
    floor: Floor,
    facing: Vec2,
    hit_flesh: bool,
    hit_solid: bool,
    started_ms: u64,
}

impl MeleeSwing {
    const INACTIVE: Self = Self {
        active: false,
        is_axe: false,
        from: Vec2::ZERO,
        floor: 0,
        facing: Vec2::ZERO,
        hit_flesh: false,
        hit_solid: false,
        started_ms: 0,
    };
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BlastKind {
    Rocket,
    BloaterCaustic,
}

#[derive(Clone, Copy)]
struct BlastRing {
    active: bool,
    kind: BlastKind,
    at: Vec2,
    floor: Floor,
    radius: f32,
    started_ms: u64,
}

impl BlastRing {
    const INACTIVE: Self = Self {
        active: false,
        kind: BlastKind::Rocket,
        at: Vec2::ZERO,
        floor: 0,
        radius: 0.0,
        started_ms: 0,
    };
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DecalKind {
    Scorch,
    Caustic,
}

#[derive(Clone, Copy)]
struct GroundDecal {
    active: bool,
    kind: DecalKind,
    at: Vec2,
    floor: Floor,
    radius: f32,
    seed: u32,
}

impl GroundDecal {
    const INACTIVE: Self = Self {
        active: false,
        kind: DecalKind::Scorch,
        at: Vec2::ZERO,
        floor: 0,
        radius: 0.0,
        seed: 0,
    };
}

#[derive(Clone, Copy)]
struct LootSparkle {
    active: bool,
    at: Vec2,
    floor: Floor,
    started_ms: u64,
}

impl LootSparkle {
    const INACTIVE: Self = Self {
        active: false,
        at: Vec2::ZERO,
        floor: 0,
        started_ms: 0,
    };
}

pub(crate) struct VfxState {
    particles: [Particle; PARTICLE_CAP],
    particle_cursor: usize,
    tracers: [BulletTracer; TRACER_CAP],
    tracer_cursor: usize,
    muzzles: [MuzzleFlash; MUZZLE_CAP],
    muzzle_cursor: usize,
    swings: [MeleeSwing; SWING_CAP],
    swing_cursor: usize,
    blasts: [BlastRing; BLAST_CAP],
    blast_cursor: usize,
    decals: [GroundDecal; DECAL_CAP],
    decal_cursor: usize,
    loots: [LootSparkle; LOOT_CAP],
    loot_cursor: usize,
    rng: u32,
    rocket_smoke_accum_ms: f32,
}

impl VfxState {
    pub(crate) fn new() -> Self {
        Self {
            particles: [Particle::INACTIVE; PARTICLE_CAP],
            particle_cursor: 0,
            tracers: [BulletTracer::INACTIVE; TRACER_CAP],
            tracer_cursor: 0,
            muzzles: [MuzzleFlash::INACTIVE; MUZZLE_CAP],
            muzzle_cursor: 0,
            swings: [MeleeSwing::INACTIVE; SWING_CAP],
            swing_cursor: 0,
            blasts: [BlastRing::INACTIVE; BLAST_CAP],
            blast_cursor: 0,
            decals: [GroundDecal::INACTIVE; DECAL_CAP],
            decal_cursor: 0,
            loots: [LootSparkle::INACTIVE; LOOT_CAP],
            loot_cursor: 0,
            rng: 0x6d2b_79f5,
            rocket_smoke_accum_ms: 0.0,
        }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    #[inline]
    fn rand_f32(&mut self) -> f32 {
        (self.next_u32() & 0xffff) as f32 * (1.0 / 65535.0)
    }

    #[inline]
    fn rand_signed(&mut self) -> f32 {
        self.rand_f32() * 2.0 - 1.0
    }

    fn spawn_particle(&mut self, p: Particle) {
        let idx = self.particle_cursor;
        self.particles[idx] = p;
        self.particle_cursor = (idx + 1) % PARTICLE_CAP;
    }

    /// Observe one simulation event and emit particles, decals, tracers, or shockwaves.
    pub(crate) fn observe_event(&mut self, event: &ActionEvent, now_ms: u64) {
        match *event {
            ActionEvent::UnarmedShove {
                from,
                floor,
                facing,
            } => {
                let idx = self.swing_cursor;
                self.swings[idx] = MeleeSwing {
                    active: true,
                    is_axe: false,
                    from,
                    floor,
                    facing,
                    hit_flesh: false,
                    hit_solid: false,
                    started_ms: now_ms,
                };
                self.swing_cursor = (idx + 1) % SWING_CAP;
            }
            ActionEvent::AxeSwing {
                from,
                floor,
                facing,
                hit_flesh,
                hit_solid,
            } => {
                let idx = self.swing_cursor;
                self.swings[idx] = MeleeSwing {
                    active: true,
                    is_axe: true,
                    from,
                    floor,
                    facing,
                    hit_flesh,
                    hit_solid,
                    started_ms: now_ms,
                };
                self.swing_cursor = (idx + 1) % SWING_CAP;
            }
            ActionEvent::ShotFired {
                weapon,
                from,
                floor,
                facing,
            } => {
                let idx = self.muzzle_cursor;
                self.muzzles[idx] = MuzzleFlash {
                    active: true,
                    weapon,
                    from,
                    floor,
                    facing,
                    started_ms: now_ms,
                };
                self.muzzle_cursor = (idx + 1) % MUZZLE_CAP;
                if weapon == Weapon::Shotgun || weapon == Weapon::RocketLauncher {
                    let count = if weapon == Weapon::RocketLauncher {
                        6
                    } else {
                        4
                    };
                    for _ in 0..count {
                        let spread = self.rand_signed() * 0.7;
                        let dir = facing.rotate(spread);
                        let speed = 1.2 + self.rand_f32() * 2.2;
                        let z_px = 13.0 + self.rand_signed() * 2.0;
                        let vz_px = 12.0 + self.rand_f32() * 14.0;
                        let life_ms = 260.0 + self.rand_f32() * 140.0;
                        self.spawn_particle(Particle {
                            active: true,
                            pos: from + facing * 0.35,
                            floor_z: floor as f32,
                            z_px,
                            vel: dir * speed,
                            vz_px,
                            gravity: -18.0,
                            age_ms: 0.0,
                            life_ms,
                            size: 2,
                            color: [176, 174, 166, 185],
                            kind: ParticleKind::Smoke,
                        });
                    }
                }
            }
            ActionEvent::BulletTrace {
                weapon,
                from,
                from_z,
                to,
                to_z,
                hit_material,
            } => {
                let idx = self.tracer_cursor;
                self.tracers[idx] = BulletTracer {
                    active: true,
                    weapon,
                    from,
                    from_z,
                    to,
                    to_z,
                    hit_material,
                    started_ms: now_ms,
                };
                self.tracer_cursor = (idx + 1) % TRACER_CAP;
            }
            ActionEvent::RocketBlast { at, floor, radius } => {
                let idx = self.blast_cursor;
                self.blasts[idx] = BlastRing {
                    active: true,
                    kind: BlastKind::Rocket,
                    at,
                    floor,
                    radius,
                    started_ms: now_ms,
                };
                self.blast_cursor = (idx + 1) % BLAST_CAP;

                let didx = self.decal_cursor;
                self.decals[didx] = GroundDecal {
                    active: true,
                    kind: DecalKind::Scorch,
                    at,
                    floor,
                    radius: radius * 0.62,
                    seed: self.next_u32(),
                };
                self.decal_cursor = (didx + 1) % DECAL_CAP;

                for i in 0..22_u32 {
                    let ang = self.rand_f32() * std::f32::consts::TAU;
                    let spd = 1.4 + self.rand_f32() * 4.8;
                    let (s, c_ang) = ang.sin_cos();
                    let is_smoke = i.is_multiple_of(3);
                    let color = if is_smoke {
                        [98, 92, 86, 200]
                    } else if i.is_multiple_of(2) {
                        [255, 198, 76, 255]
                    } else {
                        [244, 102, 46, 255]
                    };
                    let offset = self.rand_f32() * 0.4;
                    let z_px = 6.0 + self.rand_f32() * 10.0;
                    let vz_px = if is_smoke {
                        36.0 + self.rand_f32() * 26.0
                    } else {
                        52.0 + self.rand_f32() * 58.0
                    };
                    let life_ms = if is_smoke {
                        420.0 + self.rand_f32() * 240.0
                    } else {
                        280.0 + self.rand_f32() * 220.0
                    };
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at + Vec2::new(c_ang, s) * offset,
                        floor_z: floor as f32,
                        z_px,
                        vel: Vec2::new(c_ang * spd, s * spd),
                        vz_px,
                        gravity: if is_smoke { 18.0 } else { 155.0 },
                        age_ms: 0.0,
                        life_ms,
                        size: if is_smoke { 3 } else { 2 },
                        color,
                        kind: if is_smoke {
                            ParticleKind::Smoke
                        } else {
                            ParticleKind::Pixel
                        },
                    });
                }
            }
            ActionEvent::BloaterBurst {
                at, floor, radius, ..
            } => {
                let idx = self.blast_cursor;
                self.blasts[idx] = BlastRing {
                    active: true,
                    kind: BlastKind::BloaterCaustic,
                    at,
                    floor,
                    radius,
                    started_ms: now_ms,
                };
                self.blast_cursor = (idx + 1) % BLAST_CAP;

                let didx = self.decal_cursor;
                self.decals[didx] = GroundDecal {
                    active: true,
                    kind: DecalKind::Caustic,
                    at,
                    floor,
                    radius: radius * 0.58,
                    seed: self.next_u32(),
                };
                self.decal_cursor = (didx + 1) % DECAL_CAP;

                for i in 0..18_u32 {
                    let ang = self.rand_f32() * std::f32::consts::TAU;
                    let spd = 1.2 + self.rand_f32() * 3.8;
                    let (s, c_ang) = ang.sin_cos();
                    let color = if i.is_multiple_of(3) {
                        [236, 255, 134, 255]
                    } else if i.is_multiple_of(2) {
                        [184, 228, 72, 255]
                    } else {
                        [112, 168, 52, 240]
                    };
                    let z_px = 10.0 + self.rand_f32() * 8.0;
                    let vz_px = 38.0 + self.rand_f32() * 48.0;
                    let life_ms = 300.0 + self.rand_f32() * 200.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at,
                        floor_z: floor as f32,
                        z_px,
                        vel: Vec2::new(c_ang * spd, s * spd),
                        vz_px,
                        gravity: 165.0,
                        age_ms: 0.0,
                        life_ms,
                        size: if i.is_multiple_of(4) { 2 } else { 1 },
                        color,
                        kind: ParticleKind::Pixel,
                    });
                }
            }
            ActionEvent::MaterialHit {
                at,
                hit_pos,
                material,
                destroyed,
                ..
            } => {
                self.spawn_material_burst(hit_pos, at.floor, material, destroyed);
            }
            ActionEvent::StructuralCollapse { at } => {
                for i in 0..14_u32 {
                    let ang = self.rand_f32() * std::f32::consts::TAU;
                    let spd = 0.8 + self.rand_f32() * 2.6;
                    let (s, c_ang) = ang.sin_cos();
                    let smoke = i.is_multiple_of(2);
                    let ox = self.rand_signed() * 0.4;
                    let oy = self.rand_signed() * 0.4;
                    let z_px = 18.0 + self.rand_f32() * 18.0;
                    let vz_px = 20.0 + self.rand_f32() * 34.0;
                    let life_ms = 380.0 + self.rand_f32() * 240.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at.center() + Vec2::new(ox, oy),
                        floor_z: at.floor as f32,
                        z_px,
                        vel: Vec2::new(c_ang * spd, s * spd),
                        vz_px,
                        gravity: if smoke { 22.0 } else { 170.0 },
                        age_ms: 0.0,
                        life_ms,
                        size: if smoke { 3 } else { 2 },
                        color: if smoke {
                            [152, 146, 134, 195]
                        } else {
                            [116, 102, 86, 255]
                        },
                        kind: if smoke {
                            ParticleKind::Smoke
                        } else {
                            ParticleKind::Pixel
                        },
                    });
                }
            }
            ActionEvent::MonsterHit {
                kind,
                at,
                floor,
                knockback_dir,
                ..
            } => {
                let is_bloater = kind == MonsterKind::Bloater;
                for i in 0..6_u32 {
                    let base_dir = if knockback_dir.length_squared() > 0.01 {
                        knockback_dir
                    } else {
                        Vec2::new(1.0, 0.0)
                    };
                    let dir = base_dir.rotate(self.rand_signed() * 0.85);
                    let spd = 1.0 + self.rand_f32() * 2.4;
                    let color = if is_bloater {
                        if i.is_multiple_of(2) {
                            [212, 240, 88, 255]
                        } else {
                            [134, 176, 58, 255]
                        }
                    } else if i.is_multiple_of(2) {
                        [188, 48, 56, 255]
                    } else {
                        [126, 30, 38, 255]
                    };
                    let z_px = 12.0 + self.rand_f32() * 6.0;
                    let vz_px = 28.0 + self.rand_f32() * 32.0;
                    let life_ms = 240.0 + self.rand_f32() * 140.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at,
                        floor_z: floor as f32,
                        z_px,
                        vel: dir * spd,
                        vz_px,
                        gravity: 165.0,
                        age_ms: 0.0,
                        life_ms,
                        size: if i.is_multiple_of(3) { 2 } else { 1 },
                        color,
                        kind: ParticleKind::Pixel,
                    });
                }
            }
            ActionEvent::MonsterDown {
                kind,
                at,
                floor,
                overkill,
                ..
            } => {
                let count = if overkill { 12_u32 } else { 6_u32 };
                let is_bloater = kind == MonsterKind::Bloater;
                for i in 0..count {
                    let ang = self.rand_f32() * std::f32::consts::TAU;
                    let (s, c_ang) = ang.sin_cos();
                    let spd = 0.8 + self.rand_f32() * 2.8;
                    let color = if is_bloater {
                        [196, 232, 82, 255]
                    } else if i.is_multiple_of(2) {
                        [168, 40, 48, 255]
                    } else {
                        [112, 26, 34, 255]
                    };
                    let z_px = 9.0 + self.rand_f32() * 7.0;
                    let vz_px = 24.0 + self.rand_f32() * 36.0;
                    let life_ms = 260.0 + self.rand_f32() * 160.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at,
                        floor_z: floor as f32,
                        z_px,
                        vel: Vec2::new(c_ang * spd, s * spd),
                        vz_px,
                        gravity: 170.0,
                        age_ms: 0.0,
                        life_ms,
                        size: if i.is_multiple_of(3) { 2 } else { 1 },
                        color,
                        kind: ParticleKind::Pixel,
                    });
                }
            }
            ActionEvent::Healed { at, floor, .. } => {
                for i in 0..6 {
                    let ox = self.rand_signed() * 0.45;
                    let oy = self.rand_signed() * 0.45;
                    let vx = self.rand_signed() * 0.25;
                    let vy = self.rand_signed() * 0.25;
                    let vz_px = 26.0 + self.rand_f32() * 14.0;
                    let life_ms = 520.0 + self.rand_f32() * 220.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: at + Vec2::new(ox, oy),
                        floor_z: floor as f32,
                        z_px: 6.0 + (i as f32) * 3.0,
                        vel: Vec2::new(vx, vy),
                        vz_px,
                        gravity: -8.0,
                        age_ms: 0.0,
                        life_ms,
                        size: 2,
                        color: [92, 240, 146, 255],
                        kind: ParticleKind::HealCross,
                    });
                }
            }
            ActionEvent::Looted { at, .. } => {
                let idx = self.loot_cursor;
                self.loots[idx] = LootSparkle {
                    active: true,
                    at: at.center(),
                    floor: at.floor,
                    started_ms: now_ms,
                };
                self.loot_cursor = (idx + 1) % LOOT_CAP;
            }
            _ => {}
        }
    }

    fn spawn_material_burst(&mut self, pos: Vec2, floor: Floor, mat: Material, destroyed: bool) {
        let count: usize = match mat {
            Material::Glass => {
                if destroyed {
                    14
                } else {
                    6
                }
            }
            Material::MetalPole => {
                if destroyed {
                    12
                } else {
                    7
                }
            }
            Material::WoodLight | Material::WoodDoor | Material::TreeWood => {
                if destroyed {
                    10
                } else {
                    5
                }
            }
            Material::Drywall | Material::WoodSiding | Material::BrickMasonry => {
                if destroyed {
                    12
                } else {
                    5
                }
            }
            _ => 0,
        };
        for i in 0..count {
            let ang = self.rand_f32() * std::f32::consts::TAU;
            let (s, c_ang) = ang.sin_cos();
            let spd = 1.0 + self.rand_f32() * if destroyed { 3.4 } else { 2.0 };
            let (color, kind, size, grav) = match mat {
                Material::Glass => (
                    if i.is_multiple_of(2) {
                        [226, 248, 255, 255]
                    } else {
                        [142, 212, 232, 245]
                    },
                    ParticleKind::Shard,
                    if i.is_multiple_of(3) { 2 } else { 1 },
                    155.0,
                ),
                Material::MetalPole => (
                    if i.is_multiple_of(2) {
                        [255, 236, 128, 255]
                    } else {
                        [255, 168, 62, 255]
                    },
                    ParticleKind::Pixel,
                    if i.is_multiple_of(3) { 2 } else { 1 },
                    140.0,
                ),
                Material::WoodLight | Material::WoodDoor | Material::TreeWood => (
                    if i.is_multiple_of(2) {
                        [178, 130, 82, 255]
                    } else {
                        [116, 80, 48, 255]
                    },
                    ParticleKind::Pixel,
                    2,
                    160.0,
                ),
                Material::BrickMasonry => (
                    if i.is_multiple_of(3) {
                        [176, 166, 152, 215]
                    } else {
                        [158, 92, 76, 255]
                    },
                    if i.is_multiple_of(3) {
                        ParticleKind::Smoke
                    } else {
                        ParticleKind::Pixel
                    },
                    2,
                    if i.is_multiple_of(3) { 35.0 } else { 175.0 },
                ),
                _ => (
                    if i.is_multiple_of(2) {
                        [178, 172, 156, 215]
                    } else {
                        [124, 120, 112, 255]
                    },
                    if i.is_multiple_of(2) {
                        ParticleKind::Smoke
                    } else {
                        ParticleKind::Pixel
                    },
                    2,
                    if i.is_multiple_of(2) { 30.0 } else { 170.0 },
                ),
            };
            let z_px = 10.0 + self.rand_f32() * 12.0;
            let vz_px = 34.0 + self.rand_f32() * 44.0;
            let life_ms = 280.0 + self.rand_f32() * 220.0;
            self.spawn_particle(Particle {
                active: true,
                pos,
                floor_z: floor as f32,
                z_px,
                vel: Vec2::new(c_ang * spd, s * spd),
                vz_px,
                gravity: grav,
                age_ms: 0.0,
                life_ms,
                size,
                color,
                kind,
            });
        }
    }

    /// Advance active parabolic particles and spawn trailing smoke behind rockets.
    pub(crate) fn step(&mut self, dt_sec: f32, projectiles: &[ProjectileView]) {
        if dt_sec <= 0.0 {
            return;
        }
        let dt_ms = dt_sec * 1000.0;
        for p in &mut self.particles {
            if !p.active {
                continue;
            }
            p.age_ms += dt_ms;
            if p.age_ms >= p.life_ms {
                p.active = false;
                continue;
            }
            p.pos.x += p.vel.x * dt_sec;
            p.pos.y += p.vel.y * dt_sec;
            p.z_px += p.vz_px * dt_sec;
            p.vz_px -= p.gravity * dt_sec;
            if p.z_px < 0.0 && p.gravity > 0.0 {
                p.z_px = 0.0;
                p.vz_px = -p.vz_px * 0.28;
                p.vel.x *= 0.55;
                p.vel.y *= 0.55;
            }
        }

        // Rocket exhaust smoke trail puffs.
        self.rocket_smoke_accum_ms += dt_ms;
        if self.rocket_smoke_accum_ms >= 24.0 {
            self.rocket_smoke_accum_ms = 0.0;
            for proj in projectiles {
                if proj.kind == ProjectileKind::Rocket {
                    let jx = self.rand_signed() * 0.08;
                    let jy = self.rand_signed() * 0.08;
                    let vx = self.rand_signed() * 0.35;
                    let vy = self.rand_signed() * 0.35;
                    let vz_px = 10.0 + self.rand_f32() * 8.0;
                    let life_ms = 280.0 + self.rand_f32() * 120.0;
                    self.spawn_particle(Particle {
                        active: true,
                        pos: proj.pos - proj.dir * 0.22 + Vec2::new(jx, jy),
                        floor_z: proj.z,
                        z_px: 13.0,
                        vel: Vec2::new(vx, vy),
                        vz_px,
                        gravity: -4.0,
                        age_ms: 0.0,
                        life_ms,
                        size: 2,
                        color: [188, 182, 170, 195],
                        kind: ParticleKind::Smoke,
                    });
                }
            }
        }
    }

    /// Return current melee swing progress `0.0..=1.0` if the survivor is mid-swing.
    pub(crate) fn active_swing_progress(&self, now_ms: u64) -> Option<f32> {
        let mut latest: Option<(u64, f32)> = None;
        for s in &self.swings {
            if !s.active {
                continue;
            }
            let dur = if s.is_axe { AXE_SWING_MS } else { SHOVE_MS };
            let rem = life(now_ms, s.started_ms, dur);
            if rem > 0.0 && latest.is_none_or(|(t, _)| s.started_ms >= t) {
                latest = Some((s.started_ms, 1.0 - rem));
            }
        }
        latest.map(|(_, p)| p)
    }

    /// Return current weapon recoil `0.0..=1.0` from recent firearm shots.
    pub(crate) fn active_recoil(&self, now_ms: u64) -> f32 {
        let mut best = 0.0_f32;
        for m in &self.muzzles {
            if !m.active {
                continue;
            }
            let dur = match m.weapon {
                Weapon::Shotgun => 160,
                Weapon::RocketLauncher => 200,
                _ => 115,
            };
            best = best.max(life(now_ms, m.started_ms, dur));
        }
        best
    }

    /// Fill fixed stack slice with transient combat lights (muzzle, rocket, blast, caustic).
    pub(crate) fn collect_flashes(
        &self,
        projectiles: &[ProjectileView],
        now_ms: u64,
        out: &mut [DynamicLight; 16],
    ) -> usize {
        let mut n = 0usize;
        for m in &self.muzzles {
            if !m.active || n >= out.len() {
                continue;
            }
            let rem = life(now_ms, m.started_ms, MUZZLE_MS);
            if rem <= 0.0 {
                continue;
            }
            let (rad, scale, px_rad) = match m.weapon {
                Weapon::Shotgun => (4.2, 1.25, 38.0),
                Weapon::RocketLauncher => (4.0, 1.15, 34.0),
                _ => (3.0, 0.9, 26.0),
            };
            out[n] = DynamicLight {
                pos: m.from + m.facing * 0.35,
                floor: m.floor,
                radius: rad,
                r: (78.0 * scale * rem) as u16,
                g: (56.0 * scale * rem) as u16,
                b: (22.0 * scale * rem) as u16,
                glow_rgb: [255, 194, 92],
                glow_strength: 76.0 * scale * rem,
                glow_radius_px: px_rad,
            };
            n += 1;
        }

        for b in &self.blasts {
            if !b.active || n >= out.len() {
                continue;
            }
            let dur = if b.kind == BlastKind::Rocket {
                ROCKET_BLAST_MS
            } else {
                BLOATER_BURST_MS
            };
            let rem = life(now_ms, b.started_ms, dur);
            if rem <= 0.0 {
                continue;
            }
            out[n] = match b.kind {
                BlastKind::Rocket => DynamicLight {
                    pos: b.at,
                    floor: b.floor,
                    radius: 6.0,
                    r: (118.0 * rem) as u16,
                    g: (78.0 * rem) as u16,
                    b: (26.0 * rem) as u16,
                    glow_rgb: [255, 148, 54],
                    glow_strength: 120.0 * rem,
                    glow_radius_px: 78.0 * (1.2 - 0.35 * rem),
                },
                BlastKind::BloaterCaustic => DynamicLight {
                    pos: b.at,
                    floor: b.floor,
                    radius: 4.0,
                    r: (56.0 * rem) as u16,
                    g: (96.0 * rem) as u16,
                    b: (24.0 * rem) as u16,
                    glow_rgb: [192, 242, 74],
                    glow_strength: 82.0 * rem,
                    glow_radius_px: 58.0 * (1.15 - 0.3 * rem),
                },
            };
            n += 1;
        }

        for proj in projectiles {
            if n >= out.len() {
                break;
            }
            out[n] = match proj.kind {
                ProjectileKind::Rocket => DynamicLight {
                    pos: proj.pos,
                    floor: proj.floor,
                    radius: 2.8,
                    r: 58,
                    g: 38,
                    b: 14,
                    glow_rgb: [255, 166, 68],
                    glow_strength: 62.0,
                    glow_radius_px: 26.0,
                },
                ProjectileKind::ShooterBolt => DynamicLight {
                    pos: proj.pos,
                    floor: proj.floor,
                    radius: 1.8,
                    r: 42,
                    g: 22,
                    b: 12,
                    glow_rgb: [255, 108, 56],
                    glow_strength: 46.0,
                    glow_radius_px: 16.0,
                },
            };
            n += 1;
        }

        n
    }

    /// Persistent scorch craters and caustic splatters drawn directly on the ground plane.
    pub(crate) fn draw_decals(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        floor: Floor,
        dim_scale_256: u16,
    ) {
        let z = floor as f32;
        for d in &self.decals {
            if !d.active || d.floor != floor {
                continue;
            }
            let (sx, sy) = proj.project_z(d.at.x, d.at.y, z);
            let rx = (d.radius * 18.0).clamp(12.0, 38.0);
            let ry = rx * 0.5;
            if sx + rx < 0.0
                || sx - rx > c.width() as f32
                || sy + ry < 0.0
                || sy - ry > c.height() as f32
            {
                continue;
            }
            let alpha_scale = |a: u8| -> u8 { ((a as u16 * dim_scale_256) >> 8) as u8 };
            match d.kind {
                DecalKind::Scorch => {
                    c.ellipse(sx, sy, rx, ry, [28, 25, 29, alpha_scale(138)]);
                    c.ellipse(sx, sy, rx * 0.62, ry * 0.62, [18, 16, 20, alpha_scale(178)]);
                    // Deterministic ember/char flecks around rim.
                    for i in 0..6_u32 {
                        let h = d
                            .seed
                            .wrapping_mul(i.wrapping_mul(1103515245).wrapping_add(12345));
                        let ox = ((h & 0xff) as f32 / 255.0 - 0.5) * rx * 1.4;
                        let oy = (((h >> 8) & 0xff) as f32 / 255.0 - 0.5) * ry * 1.4;
                        c.rect(sx + ox, sy + oy, 3.0, 2.0, [44, 38, 36, alpha_scale(165)]);
                    }
                }
                DecalKind::Caustic => {
                    c.ellipse(sx, sy, rx, ry, [98, 146, 46, alpha_scale(132)]);
                    c.ellipse(
                        sx,
                        sy,
                        rx * 0.58,
                        ry * 0.58,
                        [162, 212, 66, alpha_scale(152)],
                    );
                    for i in 0..7_u32 {
                        let h = d
                            .seed
                            .wrapping_mul(i.wrapping_mul(2246822519).wrapping_add(3266489917));
                        let ox = ((h & 0xff) as f32 / 255.0 - 0.5) * rx * 1.7;
                        let oy = (((h >> 8) & 0xff) as f32 / 255.0 - 0.5) * ry * 1.7;
                        c.ellipse(sx + ox, sy + oy, 3.5, 1.8, [186, 228, 76, alpha_scale(155)]);
                    }
                }
            }
        }
    }

    /// Ground-layer danger telegraphs: Bloater overhead slam lime circle & Shooter aim laser!
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_ground_telegraphs(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        monsters: &[MonsterView],
        grid: &LightGrid,
        floor: Floor,
        now_ms: u64,
    ) {
        let z = floor as f32;
        for m in monsters {
            if m.floor != floor {
                continue;
            }
            let vis = grid.sample_light(m.pos).vis;
            if vis != TileVisibility::Visible {
                continue;
            }
            if m.kind == MonsterKind::Bloater {
                if let Some(prog) = m.telegraph_progress {
                    let (sx, sy) = proj.project_z(m.pos.x, m.pos.y, z);
                    let max_rx = 27.0;
                    let max_ry = 13.5;
                    let pulse_rx = max_rx * (0.45 + 0.55 * prog);
                    let pulse_ry = max_ry * (0.45 + 0.55 * prog);
                    c.ellipse(
                        sx,
                        sy,
                        pulse_rx,
                        pulse_ry,
                        [166, 224, 62, (48.0 + 62.0 * prog) as u8],
                    );
                    c.ellipse_ring(sx, sy, max_rx, max_ry, 1.6, [216, 246, 94, 210]);
                    c.ellipse_ring(sx, sy, pulse_rx, pulse_ry, 1.4, [242, 255, 152, 235]);
                } else if m.swelling {
                    let (sx, sy) = proj.project_z(m.pos.x, m.pos.y, z);
                    let flash = ((now_ms / 60) & 1) == 0;
                    let col = if flash {
                        [244, 255, 134, 235]
                    } else {
                        [186, 234, 66, 190]
                    };
                    c.ellipse(sx, sy, 42.0, 21.0, [164, 222, 58, 56]);
                    c.ellipse_ring(sx, sy, 44.0, 22.0, 1.8, col);
                }
            }
        }
    }

    /// Draw Shooter wall-clipped dashed aim telegraph lines (crimson -> hot amber-white in final 35%).
    pub(crate) fn draw_shooter_telegraphs(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        monsters: &[MonsterView],
        light_floors: &[LightGrid; 3],
        active_floor: Floor,
        now_ms: u64,
    ) {
        for m in monsters {
            if m.kind != MonsterKind::Shooter {
                continue;
            }
            let Some(progress) = m.telegraph_progress else {
                continue;
            };
            let Some((end_pos, end_floor)) = m.telegraph_target else {
                continue;
            };
            let vis = if m.floor <= active_floor {
                light_floors[m.floor as usize].sample_light(m.pos).vis
            } else {
                TileVisibility::Hidden
            };
            if vis != TileVisibility::Visible {
                continue;
            }

            let (mut x0, mut y0) = proj.project_z(m.pos.x, m.pos.y, m.floor as f32);
            let dir_s = screen_direction(m.telegraph_dir);
            x0 += dir_s.x * 7.0;
            y0 += dir_s.y * 5.0 - 16.0;
            let (x1, mut y1) = proj.project_z(end_pos.x, end_pos.y, end_floor as f32);
            y1 -= 13.0;

            let hot = progress >= 0.65;
            let flicker = ((now_ms / 42) & 1) == 0;
            let (col, dash, gap) = if hot {
                if flicker {
                    ([255, 234, 148, 248], 5, 2)
                } else {
                    ([255, 92, 58, 245], 5, 2)
                }
            } else {
                ([232, 62, 52, 200], 4, 4)
            };
            let phase = ((now_ms / 30) % (dash + gap) as u64) as i32;
            c.dashed_line(
                x0,
                y0 + 1.0,
                x1,
                y1 + 1.0,
                dash,
                gap,
                phase,
                [24, 16, 20, 145],
            );
            c.dashed_line(x0, y0, x1, y1, dash, gap, phase, col);
            // Muzzle glint & endpoint lock diamond.
            c.diamond(
                x0,
                y0,
                if hot { 3.5 } else { 2.5 },
                if hot { 3.5 } else { 2.5 },
                col,
            );
            c.diamond(
                x1,
                y1,
                if hot { 3.5 } else { 2.0 },
                if hot { 2.5 } else { 1.5 },
                col,
            );
        }
    }

    /// Foreground combat effects: melee smear arcs, tracers, muzzle flares, projectiles,
    /// explosion/caustic shockwaves, parabolic particles, and loot sparkles.
    pub(crate) fn draw_effects(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        projectiles: &[ProjectileView],
        now_ms: u64,
    ) {
        // 1. Axe 110° steel smear arcs & unarmed shove puffs.
        for s in &self.swings {
            if !s.active {
                continue;
            }
            let dur = if s.is_axe { AXE_SWING_MS } else { SHOVE_MS };
            let rem = life(now_ms, s.started_ms, dur);
            if rem <= 0.0 {
                continue;
            }
            let prog = 1.0 - rem;
            let z = s.floor as f32;
            if s.is_axe {
                // Sweep across 110° (-55° to +55°) centered on facing.
                let half_arc = 55.0_f32.to_radians();
                let segs = 10usize;
                let max_t = (prog * 1.45).min(1.0);
                let min_t = (prog * 1.25 - 0.55).max(0.0);
                let mut prev_outer: Option<(f32, f32)> = None;
                let mut prev_inner: Option<(f32, f32)> = None;
                for i in 0..=segs {
                    let u = i as f32 / segs as f32;
                    if u < min_t || u > max_t {
                        prev_outer = None;
                        prev_inner = None;
                        continue;
                    }
                    let ang = -half_arc + u * (half_arc * 2.0);
                    let dir = s.facing.rotate(ang);
                    let o_w = s.from + dir * 1.28;
                    let i_w = s.from + dir * 0.72;
                    let (ox, oy) = proj.project_z(o_w.x, o_w.y, z);
                    let (ix, iy) = proj.project_z(i_w.x, i_w.y, z);
                    let cur_outer = (ox, oy - 13.0);
                    let cur_inner = (ix, iy - 13.0);
                    if let (Some(po), Some(pi)) = (prev_outer, prev_inner) {
                        let lead = ((u - min_t) / (max_t - min_t).max(0.05)).clamp(0.0, 1.0);
                        let smear_a = (155.0 * rem * lead) as u8;
                        let edge_a = (245.0 * rem * (0.4 + 0.6 * lead)) as u8;
                        c.polygon(&[pi, po, cur_outer, cur_inner], [152, 186, 204, smear_a]);
                        c.line(
                            po.0,
                            po.1,
                            cur_outer.0,
                            cur_outer.1,
                            [238, 248, 255, edge_a],
                        );
                    }
                    prev_outer = Some(cur_outer);
                    prev_inner = Some(cur_inner);
                }
                if (s.hit_flesh || s.hit_solid) && rem > 0.45 {
                    let hit_w = s.from + s.facing * 1.05;
                    let (hx, hy) = proj.project_z(hit_w.x, hit_w.y, z);
                    let col = if s.hit_flesh {
                        [255, 198, 134, (235.0 * rem) as u8]
                    } else {
                        [246, 240, 184, (235.0 * rem) as u8]
                    };
                    draw_spark(c, hx, hy - 13.0, 5.0 * rem + 2.0, col);
                }
            } else {
                // Short unarmed shove crescent.
                let push = 0.55 + prog * 0.42;
                let a = (150.0 * rem) as u8;
                let l_dir = s.facing.rotate(-0.48);
                let r_dir = s.facing.rotate(0.48);
                let (lx, ly) =
                    proj.project_z(s.from.x + l_dir.x * push, s.from.y + l_dir.y * push, z);
                let (mx, my) = proj.project_z(
                    s.from.x + s.facing.x * (push + 0.14),
                    s.from.y + s.facing.y * (push + 0.14),
                    z,
                );
                let (rx, ry) =
                    proj.project_z(s.from.x + r_dir.x * push, s.from.y + r_dir.y * push, z);
                c.line(lx, ly - 12.0, mx, my - 12.0, [220, 228, 232, a]);
                c.line(mx, my - 12.0, rx, ry - 12.0, [220, 228, 232, a]);
            }
        }

        // 2. Bullet tracers (pistol single crisp line & shotgun 5-pellet diverging cone).
        for tr in &self.tracers {
            if !tr.active {
                continue;
            }
            let dur = if tr.weapon == Weapon::Shotgun {
                SHOTGUN_TRACER_MS
            } else {
                PISTOL_TRACER_MS
            };
            let rem = life(now_ms, tr.started_ms, dur);
            if rem <= 0.0 {
                continue;
            }
            let (mut x0, mut y0) = proj.project_z(tr.from.x, tr.from.y, tr.from_z);
            let (x1, mut y1) = proj.project_z(tr.to.x, tr.to.y, tr.to_z);
            y0 -= 15.0;
            y1 -= 14.0;
            let dx = x1 - x0;
            let dy = y1 - y0;
            let dist = dx.hypot(dy).max(1.0);
            let start_off = dist.min(10.0);
            x0 += dx / dist * start_off;
            y0 += dy / dist * start_off;

            let line_col = if tr.weapon == Weapon::Shotgun {
                [255, 218, 134, (195.0 * rem) as u8]
            } else {
                [255, 232, 162, (225.0 * rem) as u8]
            };
            c.line(x0, y0, x1, y1, line_col);

            if tr.hit_material != MaterialCategory::None {
                let impact_rgb = match tr.hit_material {
                    MaterialCategory::Flesh => [255, 146, 98],
                    MaterialCategory::Glass => [198, 244, 255],
                    MaterialCategory::Metal => [255, 232, 118],
                    MaterialCategory::Caustic => [206, 246, 92],
                    _ => [226, 212, 174],
                };
                draw_spark(
                    c,
                    x1,
                    y1,
                    2.5 + (1.0 - rem) * 4.5,
                    [
                        impact_rgb[0],
                        impact_rgb[1],
                        impact_rgb[2],
                        (235.0 * rem) as u8,
                    ],
                );
            }
        }

        // 3. Muzzle flares.
        for m in &self.muzzles {
            if !m.active {
                continue;
            }
            let rem = life(now_ms, m.started_ms, MUZZLE_MS);
            if rem <= 0.0 {
                continue;
            }
            let (mut x, mut y) = proj.project_z(m.from.x, m.from.y, m.floor as f32);
            let dir_s = screen_direction(m.facing);
            let offset = match m.weapon {
                Weapon::Shotgun | Weapon::RocketLauncher => 13.0,
                _ => 10.0,
            };
            x += dir_s.x * offset;
            y += dir_s.y * offset - 15.0;
            let sz = match m.weapon {
                Weapon::Shotgun => 8.5 * rem + 2.5,
                Weapon::RocketLauncher => 7.5 * rem + 2.0,
                _ => 5.0 * rem + 1.5,
            };
            draw_spark(c, x, y, sz, [255, 186, 76, (230.0 * rem) as u8]);
            c.diamond(
                x,
                y,
                sz * 0.52,
                sz * 0.52,
                [255, 246, 206, (255.0 * rem) as u8],
            );
        }

        // 4. Active projectiles (Rocket darts & Shooter bolts).
        for p in projectiles {
            let (sx, mut sy) = proj.project_z(p.pos.x, p.pos.y, p.z);
            sy -= 14.0;
            let dir_s = screen_direction(p.dir);
            match p.kind {
                ProjectileKind::Rocket => {
                    let tail_x = sx - dir_s.x * 7.0;
                    let tail_y = sy - dir_s.y * 7.0;
                    let nose_x = sx + dir_s.x * 4.0;
                    let nose_y = sy + dir_s.y * 4.0;
                    // Thruster flame behind rocket.
                    let flame = 2.5 + ((now_ms / 35) & 1) as f32 * 1.5;
                    c.diamond(
                        tail_x - dir_s.x * 2.0,
                        tail_y - dir_s.y * 2.0,
                        flame + 1.0,
                        flame,
                        [255, 162, 58, 240],
                    );
                    c.diamond(
                        tail_x - dir_s.x * 1.0,
                        tail_y - dir_s.y * 1.0,
                        flame * 0.55,
                        flame * 0.55,
                        [255, 244, 184, 255],
                    );
                    // Olive warhead body + crimson tip.
                    c.line(
                        tail_x,
                        tail_y + 1.0,
                        nose_x,
                        nose_y + 1.0,
                        [34, 42, 32, 255],
                    );
                    c.line(tail_x, tail_y, nose_x, nose_y, [122, 144, 96, 255]);
                    c.rect(nose_x, nose_y, 2.5, 2.5, [224, 78, 56, 255]);
                }
                ProjectileKind::ShooterBolt => {
                    let tail_x = sx - dir_s.x * 6.5;
                    let tail_y = sy - dir_s.y * 6.5;
                    c.line(tail_x, tail_y, sx, sy, [255, 92, 48, 225]);
                    c.line(
                        (tail_x + sx) * 0.5,
                        (tail_y + sy) * 0.5,
                        sx,
                        sy,
                        [255, 212, 114, 250],
                    );
                    c.diamond(sx, sy, 2.2, 2.2, [255, 248, 206, 255]);
                }
            }
        }

        // 5. Expanding explosion & caustic shockwave rings.
        for b in &self.blasts {
            if !b.active {
                continue;
            }
            let dur = if b.kind == BlastKind::Rocket {
                ROCKET_BLAST_MS
            } else {
                BLOATER_BURST_MS
            };
            let rem = life(now_ms, b.started_ms, dur);
            if rem <= 0.0 {
                continue;
            }
            let prog = 1.0 - rem;
            let (sx, sy) = proj.project_z(b.at.x, b.at.y, b.floor as f32);
            let max_rx = b.radius * 21.0;
            let max_ry = max_rx * 0.5;
            let cur_rx = max_rx * (0.2 + 0.85 * prog.sqrt());
            let cur_ry = max_ry * (0.2 + 0.85 * prog.sqrt());

            match b.kind {
                BlastKind::Rocket => {
                    let outer_a = (245.0 * rem) as u8;
                    let mid_a = (235.0 * rem) as u8;
                    c.ellipse_ring(sx, sy - 6.0, cur_rx, cur_ry, 3.0, [238, 78, 42, outer_a]);
                    c.ellipse_ring(
                        sx,
                        sy - 8.0,
                        cur_rx * 0.72,
                        cur_ry * 0.72,
                        2.5,
                        [255, 198, 72, mid_a],
                    );
                    if rem > 0.35 {
                        let core_t = (rem - 0.35) / 0.65;
                        c.ellipse(
                            sx,
                            sy - 9.0,
                            cur_rx * 0.44 * core_t,
                            cur_ry * 0.55 * core_t,
                            [255, 246, 202, (240.0 * core_t) as u8],
                        );
                    }
                }
                BlastKind::BloaterCaustic => {
                    let a = (235.0 * rem) as u8;
                    c.ellipse_ring(sx, sy - 4.0, cur_rx, cur_ry, 2.8, [132, 208, 54, a]);
                    c.ellipse_ring(
                        sx,
                        sy - 5.0,
                        cur_rx * 0.68,
                        cur_ry * 0.68,
                        2.0,
                        [218, 248, 96, a],
                    );
                    if rem > 0.45 {
                        let core_t = (rem - 0.45) / 0.55;
                        c.ellipse(
                            sx,
                            sy - 6.0,
                            cur_rx * 0.38 * core_t,
                            cur_ry * 0.45 * core_t,
                            [234, 255, 146, (215.0 * core_t) as u8],
                        );
                    }
                }
            }
        }

        // 6. Parabolic particles (shards, splinters, sparks, smoke, droplets, emerald '+' crosses).
        let w = c.width() as f32;
        let h = c.height() as f32;
        for p in &self.particles {
            if !p.active {
                continue;
            }
            let rem = (1.0 - p.age_ms / p.life_ms).clamp(0.0, 1.0);
            let (sx, sy_base) = proj.project_z(p.pos.x, p.pos.y, p.floor_z);
            let sy = sy_base - p.z_px;
            if sx < -8.0 || sx > w + 8.0 || sy < -8.0 || sy > h + 8.0 {
                continue;
            }
            let a = ((p.color[3] as f32) * rem.min(0.85) / 0.85).clamp(0.0, 255.0) as u8;
            let col = [p.color[0], p.color[1], p.color[2], a];
            let ix = sx.round() as i32;
            let iy = sy.round() as i32;
            match p.kind {
                ParticleKind::Pixel => {
                    if p.size <= 1 {
                        c.put(ix, iy, col);
                    } else {
                        c.rect_xywh(ix, iy, p.size as i32, p.size as i32, col);
                    }
                }
                ParticleKind::Shard => {
                    let twinkle = (((p.age_ms as u32) / 45) & 1) == 0;
                    let c2 = if twinkle { [246, 254, 255, a] } else { col };
                    c.put(ix, iy, c2);
                    if p.size >= 2 {
                        c.put(ix + 1, iy, col);
                        c.put(ix, iy - 1, c2);
                    }
                }
                ParticleKind::Smoke => {
                    let r = p.size as f32 + (1.0 - rem) * 2.5;
                    c.ellipse(sx, sy, r, r * 0.72, [col[0], col[1], col[2], a / 2]);
                }
                ParticleKind::HealCross => {
                    let out_col = [26, 76, 48, a];
                    c.put(ix - 2, iy, out_col);
                    c.put(ix + 2, iy, out_col);
                    c.put(ix, iy - 2, out_col);
                    c.put(ix, iy + 2, out_col);
                    c.line(sx - 1.0, sy, sx + 1.0, sy, col);
                    c.line(sx, sy - 1.0, sx, sy + 1.0, col);
                    c.put(ix, iy, [218, 255, 224, a]);
                }
            }
        }

        // 7. Rising golden loot sparkles.
        for l in &self.loots {
            if !l.active {
                continue;
            }
            let rem = life(now_ms, l.started_ms, LOOT_SPARKLE_MS);
            if rem <= 0.0 {
                continue;
            }
            let (sx, sy) = proj.project_z(l.at.x, l.at.y, l.floor as f32);
            let rise = (1.0 - rem) * 20.0;
            draw_spark(
                c,
                sx,
                sy - 11.0 - rise,
                3.0 + rise * 0.18,
                [255, 222, 134, (235.0 * rem) as u8],
            );
        }
    }

    /// Tool-adaptive aim reticle with pulsing warning crimson when `rocket_danger_close` is true.
    pub(crate) fn draw_reticle(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        state: PlayerRenderState,
        loaded: bool,
        now_ms: u64,
    ) {
        let dist = match state.equipped {
            Some(Weapon::RocketLauncher) => 2.7,
            Some(Weapon::Pistol) => 2.35,
            Some(Weapon::Shotgun) => 2.05,
            Some(Weapon::Axe) => 1.35,
            None => 1.05,
        };
        let aim = state.pose.position + state.pose.facing * dist;
        let (x, y) = proj.project_z(aim.x, aim.y, state.elevation);
        let x = x.round();
        let y = (y - 13.0).round();

        match state.equipped {
            None => {
                let ink = [208, 204, 192, 170];
                c.diamond(x, y, 3.0, 2.0, [22, 26, 34, 140]);
                c.put(x as i32, y as i32, ink);
                c.put(x as i32 - 3, y as i32, ink);
                c.put(x as i32 + 3, y as i32, ink);
            }
            Some(Weapon::Axe) => {
                // Crisp melee cleave bracket.
                let ink = [226, 238, 246, 225];
                c.diamond(x, y, 5.0, 3.0, [24, 28, 36, 155]);
                c.line(x - 5.0, y - 2.0, x - 3.0, y - 3.0, ink);
                c.line(x - 5.0, y + 2.0, x - 3.0, y + 3.0, ink);
                c.line(x + 5.0, y - 2.0, x + 3.0, y - 3.0, ink);
                c.line(x + 5.0, y + 2.0, x + 3.0, y + 3.0, ink);
                c.put(x as i32, y as i32, ink);
            }
            Some(Weapon::Pistol) => {
                let ink = if loaded {
                    [255, 222, 138, 255]
                } else {
                    [194, 178, 162, 210]
                };
                c.diamond(x, y, 6.0, 4.0, [24, 27, 36, 185]);
                c.line(x - 6.0, y, x - 3.0, y, ink);
                c.line(x + 3.0, y, x + 6.0, y, ink);
                c.line(x, y - 4.0, x, y - 3.0, ink);
                c.line(x, y + 3.0, x, y + 4.0, ink);
                c.put(x as i32, y as i32, ink);
            }
            Some(Weapon::Shotgun) => {
                // Wider buckshot spread bracket.
                let ink = if loaded {
                    [255, 198, 108, 250]
                } else {
                    [194, 178, 162, 210]
                };
                c.ellipse_ring(x, y, 8.5, 5.0, 1.2, [24, 27, 36, 175]);
                c.line(x - 8.0, y - 2.0, x - 8.0, y + 2.0, ink);
                c.line(x + 8.0, y - 2.0, x + 8.0, y + 2.0, ink);
                c.line(x - 8.0, y - 2.0, x - 5.0, y - 4.0, ink);
                c.line(x + 8.0, y - 2.0, x + 5.0, y - 4.0, ink);
                c.line(x - 8.0, y + 2.0, x - 5.0, y + 4.0, ink);
                c.line(x + 8.0, y + 2.0, x + 5.0, y + 4.0, ink);
                c.put(x as i32, y as i32, ink);
            }
            Some(Weapon::RocketLauncher) => {
                let danger = state.rocket_danger_close;
                let pulse = ((now_ms / 110) & 1) == 0;
                let ink = if danger {
                    if pulse {
                        [255, 68, 56, 255]
                    } else {
                        [255, 198, 92, 255]
                    }
                } else if loaded {
                    [255, 176, 86, 255]
                } else {
                    [194, 178, 162, 210]
                };
                c.ellipse_ring(
                    x,
                    y,
                    if danger && pulse { 10.5 } else { 9.0 },
                    if danger && pulse { 6.5 } else { 5.5 },
                    1.5,
                    if danger {
                        [255, 64, 54, 225]
                    } else {
                        [24, 27, 36, 190]
                    },
                );
                c.line(x - 9.0, y, x - 4.0, y, ink);
                c.line(x + 4.0, y, x + 9.0, y, ink);
                c.line(x, y - 6.0, x, y - 3.0, ink);
                c.line(x, y + 3.0, x, y + 6.0, ink);
                if danger {
                    // Warning X inside sight when blast would hit the survivor!
                    c.line(x - 3.0, y - 3.0, x + 3.0, y + 3.0, [255, 68, 56, 255]);
                    c.line(x - 3.0, y + 3.0, x + 3.0, y - 3.0, [255, 68, 56, 255]);
                } else {
                    c.put(x as i32, y as i32, ink);
                }
            }
        }
    }
}

fn draw_spark(c: &mut Canvas<'_>, x: f32, y: f32, size: f32, ink: [u8; 4]) {
    c.diamond(x, y, size * 0.66, size, ink);
    c.line(x - size, y, x + size, y, ink);
}
