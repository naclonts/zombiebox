// 3D world vertex shader with instancing and lighting (`T03`).

struct CameraUniforms {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    eye_pos: vec4<f32>,
    forward_dir: vec4<f32>,
    right_dir: vec4<f32>,
    up_dir: vec4<f32>,
    viewport_and_internal: vec4<f32>,
    clip_and_scale: vec4<f32>,
};

struct PointLight {
    pos_and_radius: vec4<f32>,
    color_and_floor: vec4<f32>,
};

struct SceneLighting {
    sky_ambient: vec4<f32>,
    ground_ambient: vec4<f32>,
    sun_dir_and_intensity: vec4<f32>,
    sun_color: vec4<f32>,
    shroud_and_dim: vec4<f32>,
    fog_color_and_density: vec4<f32>,
    counts_and_flags: vec4<u32>,
    point_lights: array<PointLight, 32>,
    visibility_grid: vec4<i32>,
    player_position: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(0) @binding(1) var<uniform> lighting: SceneLighting;
@group(0) @binding(2) var visibility: texture_2d<u32>;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) v_meta: vec2<u32>,
    @location(4) model_col0: vec4<f32>,
    @location(5) model_col1: vec4<f32>,
    @location(6) model_col2: vec4<f32>,
    @location(7) model_col3: vec4<f32>,
    @location(8) color_tint: vec4<f32>,
    @location(9) instance_meta: vec4<u32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
    @location(3) @interpolate(flat) v_meta: vec2<u32>,
    @location(4) @interpolate(flat) inst_fx: vec2<u32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(
        in.model_col0,
        in.model_col1,
        in.model_col2,
        in.model_col3,
    );
    let world_p = model * vec4<f32>(in.position, 1.0);
    let normal_matrix = mat3x3<f32>(
        in.model_col0.xyz,
        in.model_col1.xyz,
        in.model_col2.xyz,
    );
    let world_n = normalize(normal_matrix * in.normal);

    var v_meta = in.v_meta;
    if in.instance_meta.x != 0u || in.instance_meta.y != 0u {
        v_meta = vec2<u32>(in.instance_meta.x, in.instance_meta.y);
    }

    var tint_color = in.color * in.color_tint;
    let hit_flash = f32(in.instance_meta.z & 0xFFu) / 255.0;
    if hit_flash > 0.0 {
        tint_color = mix(tint_color, vec4<f32>(1.0, 1.0, 1.0, tint_color.a), hit_flash);
    }

    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_p;
    out.color = tint_color;
    out.normal = world_n;
    out.world_pos = world_p.xyz;
    out.v_meta = v_meta;
    out.inst_fx = vec2<u32>(in.instance_meta.z, in.instance_meta.w);
    return out;
}

fn bayer4x4(x: u32, y: u32) -> u32 {
    let b0 = ((x & 1u) << 1u) | (y & 1u);
    let d0 = select(select(select(0u, 3u, b0 == 1u), 2u, b0 == 2u), 1u, b0 == 3u);
    let b1 = ((x >> 1u) << 1u) | (y >> 1u);
    let d1 = select(select(select(0u, 3u, b1 == 1u), 2u, b1 == 2u), 1u, b1 == 3u);
    return (d0 << 2u) | d1;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let flags = (in.v_meta.x >> 16u) & 0xFFu;
    let tag = in.v_meta.x & 0xFFu;
    let floor_index = (in.v_meta.x >> 8u) & 0xFFu;
    let building = in.v_meta.y & 0xFFFFu;
    let first_person = (lighting.counts_and_flags.y & 8u) != 0u;
    // A slab's underside belongs to the room below, not the floor above it.
    let ceiling = first_person && building != 0u &&
        (tag == 4u || tag == 8u) && floor_index > 0u && in.normal.y < -0.01;
    let shading_floor = floor_index - select(0u, 1u, ceiling);
    let active_floor = lighting.counts_and_flags.z;
    var opacity_u8 = in.inst_fx.y & 0xFFu;
    var explored = false;
    var shrouded = false;
    if (lighting.counts_and_flags.y & 4u) != 0u {
        if building != 0u && (flags & 8u) == 0u && !first_person {
            let view_side = camera.eye_pos.xz - lighting.player_position.xz;
            let offset = in.world_pos.xz - lighting.player_position.xz;
            let front_wall = tag == 6u && length(offset) < 5.0 && dot(view_side, offset) > 0.0;
            if tag == 8u || floor_index > active_floor || front_wall {
                let table_pos = vec2<i32>(i32(building & 255u), i32(building >> 8u));
                opacity_u8 = min(opacity_u8, textureLoad(visibility, table_pos, 0).r);
            }
        }

        if tag == 4u || tag == 12u || (tag == 9u && (flags & 4u) != 0u) {
            let grid = lighting.visibility_grid;
            let tile = vec2<i32>(floor(in.world_pos.xz + vec2(0.5))) - grid.xy;
            var state = 0u;
            if i32(shading_floor) < grid.w && all(tile >= vec2(0)) && all(tile < vec2(grid.z)) {
                state = textureLoad(visibility, vec2(tile.x, tile.y + 256 + i32(shading_floor) * grid.z), 0).r;
            }
            shrouded = state == 0u;
            explored = state == 1u;
        }
    }

    if (lighting.counts_and_flags.y & 1u) != 0u && opacity_u8 < 255u {
        let px = u32(in.clip_position.x) & 3u;
        let py = u32(in.clip_position.y) & 3u;
        let bayer = bayer4x4(px, py);
        if (opacity_u8 * 16u) <= (bayer * 255u + 127u) {
            discard;
        }
    }

    if shrouded {
        if tag == 9u {
            discard;
        }
        return vec4<f32>(lighting.shroud_and_dim.rgb * 0.33, 1.0);
    }

    let alpha = in.color.a;
    if alpha < 0.02 {
        discard;
    }

    var lit_rgb = in.color.rgb;
    if (flags & 1u) == 0u {
        let N = normalize(in.normal);
        let hemi_t = clamp(N.y * 0.5 + 0.5, 0.0, 1.0);
        var ambient = mix(lighting.ground_ambient.rgb, lighting.sky_ambient.rgb, hemi_t);
        if (flags & 4u) != 0u {
            ambient = ambient * lighting.sky_ambient.w;
        }
        let ao = f32((in.v_meta.x >> 24u) & 0xFFu) / 255.0;

        let wrap = lighting.sun_color.w;
        let ndotl = clamp((dot(N, lighting.sun_dir_and_intensity.xyz) + wrap) / (1.0 + wrap), 0.0, 1.0);
        let direct = lighting.sun_color.rgb * (lighting.sun_dir_and_intensity.w * ndotl);

        var point_sum = vec3<f32>(0.0);
        let point_count = min(lighting.counts_and_flags.x, 32u);
        for (var i: u32 = 0u; i < point_count; i = i + 1u) {
            let pl = lighting.point_lights[i];
            let pl_pos = pl.pos_and_radius.xyz;
            let pl_radius = pl.pos_and_radius.w;
            let light_vec = pl_pos - in.world_pos;
            let dist = length(light_vec);
            let light_floor = pl.color_and_floor.w;
            if dist < pl_radius && (shading_floor == 255u || light_floor < 0.0 || f32(shading_floor) == light_floor) {
                let L = light_vec / max(dist, 1e-4);
                let att = pow(clamp(1.0 - dist / pl_radius, 0.0, 1.0), 2.0) * max(dot(N, L), 0.25);
                point_sum = point_sum + pl.color_and_floor.rgb * att;
            }
        }

        var total_light = (ambient * ao) + direct + point_sum;
        if (flags & 2u) != 0u {
            let actor_min = vec3<f32>(80.0 / 128.0, 84.0 / 128.0, 94.0 / 128.0) * lighting.ground_ambient.w;
            total_light = max(total_light, actor_min);
        }
        lit_rgb = in.color.rgb * total_light;
    }

    let emissive_u8 = (in.inst_fx.x >> 8u) & 0xFFu;
    if emissive_u8 > 0u {
        lit_rgb = lit_rgb + vec3<f32>(f32(emissive_u8) / 255.0);
    }

    if explored {
        let luminance = dot(lit_rgb, vec3(0.299, 0.587, 0.114));
        lit_rgb = mix(lit_rgb, vec3(luminance), 0.45) * 0.45;
    }
    if (lighting.counts_and_flags.y & 2u) != 0u && shading_floor < active_floor {
        lit_rgb *= lighting.shroud_and_dim.w;
    }

    let fog_density = lighting.fog_color_and_density.w;
    if fog_density > 0.0 {
        let dist = length(in.world_pos - camera.eye_pos.xyz);
        let fog_factor = clamp(exp(-fog_density * dist), 0.0, 1.0);
        lit_rgb = mix(lighting.fog_color_and_density.rgb, lit_rgb, fog_factor);
    }

    return vec4<f32>(clamp(lit_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}
