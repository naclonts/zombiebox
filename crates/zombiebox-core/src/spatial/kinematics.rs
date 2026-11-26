//! Continuous swept kinematic body solver (`step_kinematic_body`, `+z` up).

use super::collision::sweep_circle_rect_2d;
use super::query::{Collider, ColliderKind, SpatialWorld, SupportSurface};
use super::support::{select_grounded_support, sweep_airborne_support};
use super::{BodyShape, BodyState, MotionState, SupportId, Vec2, Vec3, WINDOW_LINTEL_Z};
use crate::Surface;

/// Tuning parameters for [`step_kinematic_body`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveParams {
    pub max_step_up: f32,
    pub max_step_down: f32,
    pub max_walkable_slope: f32,
    pub gravity: f32,
    pub terminal_velocity: f32,
    pub can_vault_broken_windows: bool,
    pub avoid_water: bool,
    pub doorway_assist: bool,
}

impl Default for MoveParams {
    fn default() -> Self {
        Self {
            max_step_up: 0.22,
            max_step_down: 0.30,
            max_walkable_slope: 0.85,
            gravity: 24.0,
            terminal_velocity: 28.0,
            can_vault_broken_windows: false,
            avoid_water: true,
            doorway_assist: false,
        }
    }
}

/// Output state and collision telemetry from [`step_kinematic_body`].
#[derive(Clone, Debug, PartialEq)]
pub struct MoveResult {
    pub state: BodyState,
    pub blocked_xy: bool,
    pub blocked_axes: (bool, bool),
    pub hit_ceiling: bool,
    pub stepped: bool,
    pub landed_impact_speed: f32,
    pub hit_colliders: Vec<ColliderKind>,
}

#[inline]
fn record_hit(hits: &mut Vec<ColliderKind>, kind: ColliderKind) {
    if !hits.contains(&kind) {
        hits.push(kind);
    }
}

#[inline]
fn circle_overlaps_rect_2d(center: Vec2, radius: f32, bmin: Vec2, bmax: Vec2) -> bool {
    let cx = center.x.clamp(bmin.x, bmax.x);
    let cy = center.y.clamp(bmin.y, bmax.y);
    let dx = center.x - cx;
    let dy = center.y - cy;
    dx * dx + dy * dy < radius * radius
}

#[inline]
fn collider_blocks_body(c: &Collider, all_colliders: &[Collider], params: &MoveParams) -> bool {
    if !params.avoid_water && matches!(c.kind, ColliderKind::WaterHazard { .. }) {
        return false;
    }
    if params.can_vault_broken_windows {
        if matches!(c.kind, ColliderKind::WindowSill { tile, broken: true } if tile.floor == 0) {
            return false;
        }
        // Allow crouching beneath a broken F0 window's upper lintel while keeping jamb piers solid.
        if let ColliderKind::Wall { tile } = c.kind
            && tile.floor == 0
            && all_colliders.iter().any(|other| {
                matches!(
                    other.kind,
                    ColliderKind::WindowSill {
                        tile: sill_tile,
                        broken: true
                    } if sill_tile == tile
                        && c.bounds.min.z >= other.bounds.min.z + WINDOW_LINTEL_Z - 0.02
                )
            })
        {
            return false;
        }
    }
    true
}

fn has_headroom_at(
    pos: Vec3,
    shape: BodyShape,
    colliders: &[Collider],
    params: &MoveParams,
) -> bool {
    let z0 = pos.z + 1e-3;
    let z1 = pos.z + shape.height - 1e-3;
    if z1 <= z0 {
        return true;
    }
    let r = (shape.radius - 1e-4).max(0.0);
    for c in colliders {
        if !collider_blocks_body(c, colliders, params) {
            continue;
        }
        if c.bounds.max.z <= z0 || c.bounds.min.z >= z1 {
            continue;
        }
        if circle_overlaps_rect_2d(pos.xy(), r, c.bounds.min.xy(), c.bounds.max.xy()) {
            return false;
        }
    }
    true
}

fn depenetrate_xy(
    pos: Vec3,
    shape: BodyShape,
    colliders: &[Collider],
    params: &MoveParams,
) -> Vec2 {
    let mut xy = pos.xy();
    let z0 = pos.z + params.max_step_up;
    let z1 = pos.z + shape.height - 1e-3;
    if z1 <= z0 {
        return xy;
    }
    let r = (shape.radius - 5e-4).max(0.0);

    for _ in 0..2 {
        let mut moved = false;
        for c in colliders {
            if !collider_blocks_body(c, colliders, params) {
                continue;
            }
            if c.bounds.max.z <= z0 || c.bounds.min.z >= z1 {
                continue;
            }
            let bmin = c.bounds.min.xy();
            let bmax = c.bounds.max.xy();
            let closest = Vec2::new(xy.x.clamp(bmin.x, bmax.x), xy.y.clamp(bmin.y, bmax.y));
            let diff = xy - closest;
            let dist_sq = diff.length_squared();
            if dist_sq < r * r {
                let push = if dist_sq > 1e-10 {
                    let dist = dist_sq.sqrt();
                    diff * ((shape.radius - dist + 1e-4) / dist)
                } else {
                    let dl = (xy.x - bmin.x).abs();
                    let dr = (bmax.x - xy.x).abs();
                    let db = (xy.y - bmin.y).abs();
                    let dt = (bmax.y - xy.y).abs();
                    let m = dl.min(dr).min(db).min(dt);
                    if m == dl {
                        Vec2::new(-(dl + shape.radius + 1e-4), 0.0)
                    } else if m == dr {
                        Vec2::new(dr + shape.radius + 1e-4, 0.0)
                    } else if m == db {
                        Vec2::new(0.0, -(db + shape.radius + 1e-4))
                    } else {
                        Vec2::new(0.0, dt + shape.radius + 1e-4)
                    }
                };
                let cand = xy + push;
                if has_headroom_at(cand.extend(pos.z), shape, colliders, params) {
                    xy = cand;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    xy
}

fn try_doorway_assist(
    cur_xy: Vec2,
    rem_xy: Vec2,
    n_xy: Vec2,
    z: f32,
    shape: BodyShape,
    colliders: &[Collider],
    params: &MoveParams,
) -> Option<Vec2> {
    let rem_len = rem_xy.length();
    if rem_len <= 1e-6 || rem_xy.normalized().dot(-n_xy) <= 0.7 {
        return None;
    }
    let perp = Vec2::new(-n_xy.y, n_xy.x);
    let max_nudge = (rem_len * 0.85).min(0.42);
    let z0 = z + 1e-3;
    let z1 = z + shape.height - 1e-3;

    let mut best: Option<(f32, Vec2)> = None;
    for sign in [-1.0_f32, 1.0_f32] {
        for frac in [0.15_f32, 0.30, 0.42] {
            let candidate_xy = cur_xy + perp * (sign * frac);
            let step_vec = candidate_xy - cur_xy;
            // Ensure lateral nudge doesn't enter a solid wall and has headroom.
            if !has_headroom_at(candidate_xy.extend(z), shape, colliders, params) {
                continue;
            }
            let nudge_blocked = colliders.iter().any(|c| {
                collider_blocks_body(c, colliders, params)
                    && c.bounds.max.z > z0
                    && c.bounds.min.z < z1
                    && sweep_circle_rect_2d(
                        cur_xy,
                        step_vec,
                        shape.radius,
                        c.bounds.min.xy(),
                        c.bounds.max.xy(),
                    )
                    .is_some()
            });
            if nudge_blocked {
                continue;
            }
            let forward_clear = colliders.iter().all(|c| {
                if !collider_blocks_body(c, colliders, params)
                    || c.bounds.max.z <= z0
                    || c.bounds.min.z >= z1
                {
                    return true;
                }
                sweep_circle_rect_2d(
                    candidate_xy,
                    rem_xy,
                    shape.radius,
                    c.bounds.min.xy(),
                    c.bounds.max.xy(),
                )
                .is_none()
            });
            if forward_clear && best.is_none_or(|(b, _)| frac < b) {
                best = Some((frac, perp * (sign * frac.min(max_nudge))));
                break;
            }
        }
    }
    best.map(|(_, offset)| cur_xy + offset)
}

fn find_blocking_obstacle(
    supports: &[SupportSurface],
    pos_z: f32,
    shape_height: f32,
    params: &MoveParams,
) -> Option<SupportSurface> {
    for &s in supports {
        let too_steep_or_unwalkable = !s.walkable
            || (!s.is_stair && s.slope() > params.max_walkable_slope + 1e-4)
            || s.surface == Surface::Mountain;
        if (s.z > pos_z + params.max_step_up + 1e-4 && s.z <= pos_z + shape_height)
            || (too_steep_or_unwalkable
                && s.z >= pos_z - params.max_step_down - 1e-4
                && s.z <= pos_z + shape_height)
        {
            return Some(s);
        }
    }
    None
}

/// Steps a cylindrical body forward over `dt` seconds with continuous collision detection,
/// anti-snap support selection, stair ramps, gravity, and slide response.
pub fn step_kinematic_body(
    world: &dyn SpatialWorld,
    shape: BodyShape,
    mut state: BodyState,
    dt: f32,
    params: &MoveParams,
) -> MoveResult {
    if dt <= 0.0 || !dt.is_finite() {
        return MoveResult {
            state,
            blocked_xy: false,
            blocked_axes: (false, false),
            hit_ceiling: false,
            stepped: false,
            landed_impact_speed: 0.0,
            hit_colliders: Vec::new(),
        };
    }

    let max_h = shape.radius.clamp(0.02, 0.06);
    let max_v = (params.max_step_up * 0.45).clamp(0.02, 0.09);
    let horiz_len = (state.velocity.xy() * dt).length();
    let vert_len = (state.velocity.z.abs() + params.gravity * dt) * dt;
    let substeps = ((horiz_len / max_h).ceil() as usize)
        .max((vert_len / max_v).ceil() as usize)
        .clamp(1, 2048);
    let dt_sub = dt / substeps as f32;

    let mut blocked_xy = false;
    let mut blocked_axes = (false, false);
    let mut hit_ceiling = false;
    let mut stepped = false;
    let mut landed_impact_speed = 0.0_f32;
    let mut hit_colliders = Vec::new();

    let mut supports = Vec::with_capacity(8);
    let mut colliders = Vec::with_capacity(32);

    for _ in 0..substeps {
        // Pre-check grounded support validity (detects upward jump impulse or collapsed floor).
        if let MotionState::Grounded(cur_sup) = state.motion {
            if state.velocity.z > 1e-4 {
                state.motion = MotionState::Airborne;
            } else {
                world.supports_at(state.position.xy(), &mut supports);
                if let Some(s) = select_grounded_support(
                    &supports,
                    state.position.z,
                    Some(cur_sup),
                    params.max_step_up,
                    params.max_step_down,
                    params.max_walkable_slope,
                    params.avoid_water,
                ) {
                    state.position.z = s.z;
                    state.motion = MotionState::Grounded(s.id);
                } else {
                    state.motion = MotionState::Airborne;
                }
            }
        }

        match state.motion {
            MotionState::Grounded(cur_sup) => {
                let step_delta = state.velocity.xy() * dt_sub;
                let query_aabb = shape
                    .swept_aabb(
                        state.position,
                        state.position + step_delta.extend(params.max_step_up),
                    )
                    .expand(0.25);
                world.colliders_in(query_aabb, &mut colliders);

                let mut cur_xy = depenetrate_xy(state.position, shape, &colliders, params);
                let mut rem_xy = step_delta;
                let mut vel_xy = state.velocity.xy();
                let mut min_collider_step_z = f32::NEG_INFINITY;

                for iter in 0..3 {
                    if rem_xy.length_squared() <= 1e-12 {
                        break;
                    }
                    let z0 = state.position.z + 1e-3;
                    let z1 = state.position.z + shape.height - 1e-3;
                    let mut earliest: Option<(f32, Vec2, ColliderKind)> = None;

                    for c in &colliders {
                        if !collider_blocks_body(c, &colliders, params) {
                            continue;
                        }
                        if c.bounds.max.z <= z0 || c.bounds.min.z >= z1 {
                            continue;
                        }
                        let c_top = c.bounds.max.z;
                        if c_top <= state.position.z + params.max_step_up + 1e-4
                            && !matches!(
                                c.kind,
                                ColliderKind::WaterHazard { .. }
                                    | ColliderKind::WindowGlass { .. }
                                    | ColliderKind::ClosedDoor { .. }
                            )
                        {
                            let cand_step_pos = (cur_xy + rem_xy).extend(c_top);
                            if has_headroom_at(cand_step_pos, shape, &colliders, params) {
                                if circle_overlaps_rect_2d(
                                    cur_xy + rem_xy,
                                    shape.radius,
                                    c.bounds.min.xy(),
                                    c.bounds.max.xy(),
                                ) {
                                    min_collider_step_z = min_collider_step_z.max(c_top);
                                }
                                continue;
                            }
                        }

                        if let Some((t_hit, n_xy)) = sweep_circle_rect_2d(
                            cur_xy,
                            rem_xy,
                            shape.radius,
                            c.bounds.min.xy(),
                            c.bounds.max.xy(),
                        ) && earliest.is_none_or(|(best_t, _, _)| t_hit < best_t)
                        {
                            earliest = Some((t_hit, n_xy, c.kind));
                        }
                    }

                    let Some((t_hit, n_xy, kind)) = earliest else {
                        cur_xy += rem_xy;
                        break;
                    };

                    record_hit(&mut hit_colliders, kind);
                    cur_xy += rem_xy * (t_hit - 1e-4).max(0.0);

                    if iter == 0
                        && params.doorway_assist
                        && let Some(nudged_xy) = try_doorway_assist(
                            cur_xy,
                            rem_xy * (1.0 - t_hit),
                            n_xy,
                            state.position.z,
                            shape,
                            &colliders,
                            params,
                        )
                    {
                        cur_xy = nudged_xy;
                        rem_xy *= 1.0 - t_hit;
                        continue;
                    }

                    blocked_xy = true;
                    if n_xy.x.abs() > 0.25 {
                        blocked_axes.0 = true;
                    }
                    if n_xy.y.abs() > 0.25 {
                        blocked_axes.1 = true;
                    }

                    let rem_after = rem_xy * (1.0 - t_hit);
                    rem_xy = rem_after - n_xy * rem_after.dot(n_xy).min(0.0);
                    let vn = vel_xy.dot(n_xy);
                    if vn < 0.0 {
                        vel_xy -= n_xy * vn;
                    }
                }

                state.velocity.x = vel_xy.x;
                state.velocity.y = vel_xy.y;

                // Evaluate support at `cand_xy = cur_xy`.
                let mut cand_xy = cur_xy;
                world.supports_at(cand_xy, &mut supports);
                let raw_match = select_grounded_support(
                    &supports,
                    state.position.z,
                    Some(cur_sup),
                    params.max_step_up,
                    params.max_step_down,
                    params.max_walkable_slope,
                    params.avoid_water,
                );
                let mut matched = raw_match.filter(|s| {
                    let target_z = s.z.max(min_collider_step_z);
                    has_headroom_at(cand_xy.extend(target_z), shape, &colliders, params)
                });

                if let Some(s) = matched {
                    let new_z = s.z.max(min_collider_step_z);
                    if new_z - state.position.z > 0.035
                        && ((!s.is_stair && (s.id != cur_sup || s.normal.z > 0.99))
                            || min_collider_step_z > s.z)
                    {
                        stepped = true;
                    }
                    if s.is_stair != matches!(cur_sup, SupportId::Stair(_)) {
                        stepped = true;
                    }
                    state.position = cand_xy.extend(new_z);
                    state.motion = MotionState::Grounded(s.id);
                    state.velocity.z = 0.0;
                } else if let Some(obs) = raw_match.or_else(|| {
                    find_blocking_obstacle(&supports, state.position.z, shape.height, params)
                }) {
                    let n_slope = if !obs.is_stair && obs.slope() > params.max_walkable_slope {
                        (-obs.normal.xy()).normalized()
                    } else {
                        (state.position.xy() - cand_xy).normalized()
                    };
                    let mut slid_ok = false;
                    if n_slope.length_squared() > 1e-6 {
                        blocked_xy = true;
                        if n_slope.x.abs() > 0.25 {
                            blocked_axes.0 = true;
                        }
                        if n_slope.y.abs() > 0.25 {
                            blocked_axes.1 = true;
                        }
                        let delta_xy = cand_xy - state.position.xy();
                        let slide_xy = delta_xy - n_slope * delta_xy.dot(n_slope).min(0.0);
                        let slide_cand = state.position.xy() + slide_xy;
                        world.supports_at(slide_cand, &mut supports);
                        matched = select_grounded_support(
                            &supports,
                            state.position.z,
                            Some(cur_sup),
                            params.max_step_up,
                            params.max_step_down,
                            params.max_walkable_slope,
                            params.avoid_water,
                        )
                        .filter(|s| {
                            has_headroom_at(slide_cand.extend(s.z), shape, &colliders, params)
                        });
                        if let Some(s2) = matched {
                            cand_xy = slide_cand;
                            state.position = cand_xy.extend(s2.z);
                            state.motion = MotionState::Grounded(s2.id);
                            state.velocity.z = 0.0;
                            slid_ok = true;
                        }
                    }
                    if !slid_ok {
                        blocked_xy = true;
                        if n_slope.x.abs() > 0.25 || step_delta.x.abs() > 1e-5 {
                            blocked_axes.0 = true;
                        }
                        if n_slope.y.abs() > 0.25 || step_delta.y.abs() > 1e-5 {
                            blocked_axes.1 = true;
                        }
                    }
                } else {
                    // Ledge drop-off: commit horizontal move and become airborne.
                    state.position.x = cand_xy.x;
                    state.position.y = cand_xy.y;
                    state.motion = MotionState::Airborne;
                    state.velocity.z =
                        (state.velocity.z - params.gravity * dt_sub).max(-params.terminal_velocity);
                }
            }
            MotionState::Airborne => {
                state.velocity.z =
                    (state.velocity.z - params.gravity * dt_sub).max(-params.terminal_velocity);
                let delta_sub = Vec3::new(
                    state.velocity.x * dt_sub,
                    state.velocity.y * dt_sub,
                    state.velocity.z * dt_sub,
                );
                let query_aabb = shape
                    .swept_aabb(state.position, state.position + delta_sub)
                    .expand(0.25);
                world.colliders_in(query_aabb, &mut colliders);

                // Horizontal XY sweep (up to 2 slide iterations).
                let mut cur_xy = state.position.xy();
                let mut rem_xy = delta_sub.xy();
                let mut vel_xy = state.velocity.xy();
                let z0 = state.position.z + 1e-3;
                let z1 = state.position.z + shape.height - 1e-3;

                for _ in 0..2 {
                    if rem_xy.length_squared() <= 1e-12 {
                        break;
                    }
                    let mut earliest: Option<(f32, Vec2, ColliderKind)> = None;
                    for c in &colliders {
                        if !collider_blocks_body(c, &colliders, params) {
                            continue;
                        }
                        if c.bounds.max.z <= z0 || c.bounds.min.z >= z1 {
                            continue;
                        }
                        if let Some((t_hit, n_xy)) = sweep_circle_rect_2d(
                            cur_xy,
                            rem_xy,
                            shape.radius,
                            c.bounds.min.xy(),
                            c.bounds.max.xy(),
                        ) && earliest.is_none_or(|(best_t, _, _)| t_hit < best_t)
                        {
                            earliest = Some((t_hit, n_xy, c.kind));
                        }
                    }
                    let Some((t_hit, n_xy, kind)) = earliest else {
                        cur_xy += rem_xy;
                        break;
                    };
                    record_hit(&mut hit_colliders, kind);
                    cur_xy += rem_xy * (t_hit - 1e-4).max(0.0);
                    blocked_xy = true;
                    if n_xy.x.abs() > 0.25 {
                        blocked_axes.0 = true;
                    }
                    if n_xy.y.abs() > 0.25 {
                        blocked_axes.1 = true;
                    }
                    let rem_after = rem_xy * (1.0 - t_hit);
                    rem_xy = rem_after - n_xy * rem_after.dot(n_xy).min(0.0);
                    let vn = vel_xy.dot(n_xy);
                    if vn < 0.0 {
                        vel_xy -= n_xy * vn;
                    }
                }
                state.position.x = cur_xy.x;
                state.position.y = cur_xy.y;
                state.velocity.x = vel_xy.x;
                state.velocity.y = vel_xy.y;

                // Vertical Z sweep.
                let dz = delta_sub.z;
                let r_inner = (shape.radius - 1e-4).max(0.0);
                if dz > 0.0 {
                    let mut min_ceil: Option<(f32, ColliderKind)> = None;
                    for c in &colliders {
                        if !collider_blocks_body(c, &colliders, params) {
                            continue;
                        }
                        let min_z = c.bounds.min.z;
                        if min_z >= state.position.z + shape.height - 0.04
                            && min_z <= state.position.z + shape.height + dz
                            && circle_overlaps_rect_2d(
                                cur_xy,
                                r_inner,
                                c.bounds.min.xy(),
                                c.bounds.max.xy(),
                            )
                            && min_ceil.is_none_or(|(bz, _)| min_z < bz)
                        {
                            min_ceil = Some((min_z, c.kind));
                        }
                    }
                    if let Some((min_ceil_z, kind)) = min_ceil {
                        state.position.z = (min_ceil_z - shape.height - 1e-4).max(state.position.z);
                        state.velocity.z = 0.0;
                        hit_ceiling = true;
                        record_hit(&mut hit_colliders, kind);
                    } else {
                        state.position.z += dz;
                    }
                } else {
                    let z_next = state.position.z + dz;
                    world.supports_at(cur_xy, &mut supports);
                    let support_hit = sweep_airborne_support(
                        &supports,
                        state.position.z,
                        z_next,
                        params.max_walkable_slope,
                        params.avoid_water,
                    );

                    let mut best_collider_top: Option<f32> = None;
                    for c in &colliders {
                        if !collider_blocks_body(c, &colliders, params)
                            || matches!(
                                c.kind,
                                ColliderKind::WaterHazard { .. } | ColliderKind::WindowGlass { .. }
                            )
                        {
                            continue;
                        }
                        let top_z = c.bounds.max.z;
                        if top_z >= z_next - 1e-4
                            && top_z <= state.position.z + 0.04
                            && circle_overlaps_rect_2d(
                                cur_xy,
                                r_inner,
                                c.bounds.min.xy(),
                                c.bounds.max.xy(),
                            )
                            && has_headroom_at(cur_xy.extend(top_z), shape, &colliders, params)
                            && best_collider_top.is_none_or(|bz| top_z > bz)
                        {
                            best_collider_top = Some(top_z);
                        }
                    }

                    if let Some(sh) = support_hit
                        && best_collider_top.is_none_or(|ct| sh.surface.z >= ct - 1e-4)
                    {
                        if sh.can_land {
                            landed_impact_speed =
                                landed_impact_speed.max((-state.velocity.z).max(0.0));
                            state.position.z = sh.surface.z;
                            state.velocity.z = 0.0;
                            state.motion = MotionState::Grounded(sh.surface.id);
                        } else {
                            state.position.z = sh.surface.z;
                            if state.velocity.dot(sh.surface.normal) < 0.0 {
                                state.velocity =
                                    state.velocity.project_onto_plane(sh.surface.normal);
                            }
                        }
                    } else if let Some(top_z) = best_collider_top {
                        landed_impact_speed = landed_impact_speed.max((-state.velocity.z).max(0.0));
                        state.position.z = top_z;
                        state.velocity.z = 0.0;
                        state.motion = MotionState::Grounded(SupportId::Terrain);
                    } else {
                        state.position.z = z_next;
                    }
                }
            }
        }
    }

    MoveResult {
        state,
        blocked_xy,
        blocked_axes,
        hit_ceiling,
        stepped,
        landed_impact_speed,
        hit_colliders,
    }
}

/// Convenience wrapper that sets horizontal velocity `[horizontal_vel.x, horizontal_vel.y]`
/// while preserving `state.velocity.z` and calling [`step_kinematic_body`].
pub fn move_kinematic_with_horizontal_vel(
    world: &dyn SpatialWorld,
    shape: BodyShape,
    mut state: BodyState,
    horizontal_vel: Vec2,
    dt: f32,
    params: &MoveParams,
) -> MoveResult {
    state.velocity.x = horizontal_vel.x;
    state.velocity.y = horizontal_vel.y;
    step_kinematic_body(world, shape, state, dt, params)
}
