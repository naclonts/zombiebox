//! Clustered-pixel isometric sprites for the Survivor, 4 enemy archetypes,
//! equipped tools, hit flashes, HP bars, awareness badges, and fallen corpses.

use zombiebox_core::{AwarenessIcon, MonsterKind, Vec2, Weapon};

use crate::Canvas;
use crate::lighting::{CellLight, tint};

pub(crate) enum ActorKind {
    Survivor(Option<Weapon>),
    Monster(MonsterKind, u8),
}

pub(crate) struct ActorVisual {
    pub kind: ActorKind,
    pub facing: Vec2,
    pub stride: Vec2,
    pub phase: f32,
    pub recoil: f32,
    pub swing_progress: Option<f32>,
    pub hurt: f32,
    pub hit_flash: f32,
    pub quiet_walk: bool,
    pub hp: u8,
    pub max_hp: u8,
    pub awareness: AwarenessIcon,
    pub show_alert_badge: bool,
    pub alert_age_ms: u64,
    pub telegraph_progress: Option<f32>,
    pub swelling: bool,
    pub light: CellLight,
    pub elapsed_ms: u64,
}

type Ink = [u8; 4];

const fn rgb(r: u8, g: u8, b: u8) -> Ink {
    [r, g, b, 255]
}

const EDGE: Ink = rgb(31, 32, 44);
const BOOT: Ink = rgb(61, 54, 59);
const SOLE: Ink = rgb(108, 97, 94);
const DENIM: Ink = rgb(60, 75, 87);
const SEAM: Ink = rgb(98, 116, 122);
const BAG: Ink = rgb(74, 56, 48);
const CANVAS: Ink = rgb(139, 119, 80);
const ZIP: Ink = rgb(210, 182, 124);

// Standard humanoid head and torso templates.
const FACE: &[&str] = &[
    "  ###  ", " #hhH# ", " #HHHh#", " #sSSs#", "  #s#S#", "  sSSs#", "   ss# ",
];
const PROFILE: &[&str] = &[
    "  ###  ", " #hhH# ", " #HHHh#", " #hsSS#", "  hss#S", "   #SSs", "    ss ",
];
const NAPE: &[&str] = &[
    "  ###  ", " #hhH# ", " #HHHh#", " #hhhH#", "  #hhs#", "  #ss# ", "   ss  ",
];
const COAT: &[&str] = &[
    "   ##ss#  ",
    "  #LL#sD# ",
    " #LLLMDML#",
    " #LMLDbMM#",
    "  LMMD#MM#",
    "  MMLD#MD#",
    "  MLMDDMD#",
    "  #MMDbDD#",
    " #LMMD#MD#",
    " #DDD###D#",
];
const SIDE: &[&str] = &[
    "   ##ss#  ",
    "  #LLLsD# ",
    " #CLLLMDM#",
    " #CBLMMDb#",
    " #BBMMLMM#",
    " #BBMLMMD#",
    "  BBMMDMD#",
    "  #BMD#DD#",
    "  #MMMDDD#",
    "  #DD###D#",
];
const BACK: &[&str] = &[
    "   ##ss#  ",
    "  #LLMLL# ",
    " #LCMMMCML",
    " #MBCCCBB#",
    "  #CZCCBB#",
    "  BCBBBCB#",
    "  BCCCCBB#",
    "  BCBCBCB#",
    " #MBBBBBMD",
    " #DDD###D#",
];
const RAGS: &[&str] = &[
    "    ss#   ",
    "   DsDs#  ",
    " #MLLsDM# ",
    "#LMLMDDMM#",
    " #MMMDMLM#",
    "  MMDssMD#",
    "  LMMsMMD#",
    "  MMDMDMD#",
    " #DD##DMD#",
    "  D   #D# ",
];
const RAG_BACK: &[&str] = &[
    "    ss#   ",
    "   #ssD#  ",
    " #MLLLLM# ",
    "#LMLLMMDM#",
    " #MMLMDMM#",
    "  MMMsDMD#",
    "  LMDsMMD#",
    "  MMDMDMD#",
    " #DD##DMD#",
    "  D   #D# ",
];

// Shooter upright dark tactical hood + trench coat.
const HOOD_FRONT: &[&str] = &[
    "  ##### ", " #MMMMM#", " #M###M#", " #MVVVM#", " #M###M#", "  #DDD# ", "   DDD  ",
];
const HOOD_PROFILE: &[&str] = &[
    "  ##### ", " #MMMMM#", " #MM### ", " #MMVVV#", " #MM### ", "  #DDD# ", "   DDD  ",
];
const HOOD_BACK: &[&str] = &[
    "  ##### ", " #MMMMM#", " #MLMMM#", " #MMMMD#", " #MMDD# ", "  #DDD# ", "   DDD  ",
];
const TACTICAL_COAT: &[&str] = &[
    "   #####  ",
    "  #LLMLD# ",
    " #LMMMMMM#",
    " #LMMZMMM#",
    " #LMM#MMD#",
    "  LMM#MDD#",
    "  MMM#MDD#",
    "  LMD#DDD#",
    " #MMD#DDD#",
    " #DD###DD#",
];
const TACTICAL_BACK: &[&str] = &[
    "   #####  ",
    "  #LLMLL# ",
    " #LMMMMML#",
    " #LMMMMMD#",
    " #MMMMMMD#",
    "  MMMDMDD#",
    "  LMMDMDD#",
    "  MMDDDDD#",
    " #MMDDDDD#",
    " #DD###DD#",
];

// Wide 14×12 Bloater hunched bruiser torso with chartreuse pustules ('P'/'p').
const BLOATER_FRONT: &[&str] = &[
    "   ########   ",
    "  #LLMMMPMM#  ",
    " #LLLMMppMMM# ",
    "#LMLMMpPPpMMD#",
    "#LMMMMMppMMMD#",
    "#MMpPMMMMMMMD#",
    "#MpPPpMMMDMMD#",
    "#MMppMMpPMMDD#",
    " #MMMMLpPMDD# ",
    " #DDMMMMDDDD# ",
    "  #DDDDDDDD#  ",
    "   ###  ###   ",
];
const BLOATER_BACK: &[&str] = &[
    "   ########   ",
    "  #LLMMMMMM#  ",
    " #LLLpPMMMMM# ",
    "#LMLpPPpMMMMD#",
    "#LMMMpPMMMMMD#",
    "#MMMMMMMpPMMD#",
    "#LMMMMMpPPpMD#",
    "#MMMMMMMpPMDD#",
    " #MMMDMMMMDD# ",
    " #DDDDDDDDDD# ",
    "  #DDDDDDDD#  ",
    "   ###  ###   ",
];
const BLOATER_HEAD: &[&str] = &[
    "  #####  ",
    " #sSSsS# ",
    "#sSSPPSS#",
    "#sS#ss#S#",
    " #sssss# ",
    "  #####  ",
];

// Retro outlined 5×7 pixel awareness badges.
const BADGE_QUESTION: &[&str] = &[
    " ### ", "#AAA#", "#A#A#", " ##A#", "  #A#", "  ###", "  #A#", "  ###",
];
const BADGE_ALERT: &[&str] = &[
    " ### ", " #R# ", " #Y# ", " #R# ", " #Y# ", " ### ", " #Y# ", " ### ",
];

#[inline]
fn flash_blend(c: Ink, flash: f32) -> Ink {
    if flash <= 0.01 {
        return c;
    }
    let t = (flash.clamp(0.0, 1.0) * 200.0) as u32;
    let inv = 256 - t;
    [
        ((252 * t + c[0] as u32 * inv) >> 8) as u8,
        ((248 * t + c[1] as u32 * inv) >> 8) as u8,
        ((232 * t + c[2] as u32 * inv) >> 8) as u8,
        c[3],
    ]
}

struct Sprite<'a, 'b> {
    c: &'a mut Canvas<'b>,
    x: i32,
    y: i32,
    mirror: i32,
    edge: Ink,
    colors: [Ink; 7],
    bag: Ink,
    canvas: Ink,
    zip: Ink,
    visor: Ink,
    pustule_lit: Ink,
    pustule_dim: Ink,
}

impl Sprite<'_, '_> {
    #[inline]
    fn px(&mut self, x: i32, y: i32, color: Ink) {
        self.c.put(self.x + self.mirror * x, self.y + y, color);
    }

    fn stamp(&mut self, x: i32, y: i32, rows: &[&str]) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, s) in row.bytes().enumerate() {
                let ink = match s {
                    b'#' => self.edge,
                    b'L' => self.colors[0],
                    b'M' => self.colors[1],
                    b'D' => self.colors[2],
                    b's' => self.colors[3],
                    b'S' => self.colors[4],
                    b'h' => self.colors[5],
                    b'H' => self.colors[6],
                    b'B' => self.bag,
                    b'C' => self.canvas,
                    b'Z' | b'b' => self.zip,
                    b'V' => self.visor,
                    b'P' => self.pustule_lit,
                    b'p' => self.pustule_dim,
                    _ => continue,
                };
                self.px(x + dx as i32, y + dy as i32, ink);
            }
        }
    }

    fn limb(&mut self, a: (i32, i32), b: (i32, i32), width: i32, lit: Ink, shade: Ink) {
        let steps = (a.0 - b.0).abs().max((a.1 - b.1).abs()).max(1);
        for i in 0..=steps {
            let x = a.0 + (b.0 - a.0) * i / steps;
            let y = a.1 + (b.1 - a.1) * i / steps;
            self.px(x - 1, y, lit);
            for w in 0..width - 1 {
                self.px(x + w, y, shade);
            }
            self.px(x + width - 1, y, self.edge);
        }
    }
}

pub(crate) fn draw_actor(c: &mut Canvas<'_>, x: f32, y: f32, v: ActorVisual) {
    match v.kind {
        ActorKind::Monster(MonsterKind::Bloater, variant) => draw_bloater(c, x, y, &v, variant),
        ActorKind::Monster(MonsterKind::Shooter, variant) => draw_shooter(c, x, y, &v, variant),
        _ => draw_humanoid(c, x, y, &v),
    }
}

fn draw_humanoid(c: &mut Canvas<'_>, x: f32, y: f32, v: &ActorVisual) {
    let (weapon, monster_kind, variant) = match v.kind {
        ActorKind::Survivor(w) => (w, None, 0u8),
        ActorKind::Monster(k, var) => (None, Some(k), var),
    };
    let sick = monster_kind.is_some();
    let brisk = monster_kind == Some(MonsterKind::Brisk);

    let raw_colors = match monster_kind {
        None => [
            rgb(242, 177, 83),
            rgb(186, 102, 56),
            rgb(115, 62, 53),
            rgb(180, 113, 79),
            rgb(244, 183, 122),
            rgb(77, 55, 51),
            rgb(119, 83, 62),
        ],
        Some(MonsterKind::Brisk) => [
            rgb(206, 134, 82), // ragged ochre/rust highlight
            rgb(154, 82, 56),  // rust midtone
            rgb(88, 48, 44),   // dark rust shade
            rgb(154, 152, 132),
            rgb(218, 214, 184),
            rgb(66, 58, 62),
            rgb(104, 92, 92),
        ],
        Some(_) => {
            let (l, m, d) = match variant % 3 {
                0 => (rgb(164, 178, 159), rgb(111, 134, 127), rgb(68, 86, 86)),
                1 => (rgb(176, 156, 169), rgb(128, 108, 131), rgb(78, 74, 92)),
                _ => (rgb(174, 165, 134), rgb(133, 131, 101), rgb(80, 87, 76)),
            };
            [
                l,
                m,
                d,
                rgb(143, 148, 122),
                rgb(212, 211, 171),
                rgb(85, 78, 79),
                rgb(122, 116, 110),
            ]
        }
    };

    let f = v.hit_flash;
    let l = v.light;
    let pal = |ink: Ink| flash_blend(tint(ink, l), f);
    let colors = [
        pal(raw_colors[0]),
        pal(raw_colors[1]),
        pal(raw_colors[2]),
        pal(raw_colors[3]),
        pal(raw_colors[4]),
        pal(raw_colors[5]),
        pal(raw_colors[6]),
    ];
    let edge = pal(EDGE);
    let denim = pal(DENIM);
    let seam = pal(SEAM);
    let boot = pal(BOOT);
    let sole = pal(SOLE);

    let mirror = if v.facing.x < -0.05 { -1 } else { 1 };
    let direction = v.facing.y.atan2(v.facing.x.abs()).to_degrees();
    let rear = direction < -23.0;
    let side = direction.abs() < 68.0;
    let moving = v.stride.x.abs() + v.stride.y.abs() > 0.01;
    let cycle = v.phase * std::f32::consts::TAU;
    let mut wave = if moving { cycle.sin() } else { 0.0 };
    if sick && !brisk {
        wave = wave.signum() * wave.abs().powi(2);
    }
    let gait = if brisk {
        3.9
    } else if sick || v.quiet_walk {
        2.4
    } else {
        3.4
    };
    let bob = if moving {
        (wave.abs() * if v.quiet_walk { 0.9 } else { 1.45 }) as i32
    } else {
        0
    };
    let crouch = if v.quiet_walk && !sick { 1 } else { 0 };
    let lean = if brisk {
        3
    } else if sick {
        1 + (variant as i32 % 2)
    } else {
        0
    } - (v.recoil * 1.7).round() as i32;

    c.ellipse(x, y, 7.0, 2.2, [20, 20, 30, 148]);
    let mut p = Sprite {
        c,
        x: x.round() as i32,
        y: y.round() as i32,
        mirror,
        edge,
        colors,
        bag: pal(BAG),
        canvas: pal(CANVAS),
        zip: pal(ZIP),
        visor: rgb(255, 86, 56),
        pustule_lit: rgb(216, 236, 94),
        pustule_dim: rgb(146, 178, 64),
    };

    for (k, hip) in [(-1.0, -2), (1.0, 2)] {
        let swing = k * wave;
        let lift = (cycle.cos() * k).max(0.0) * if moving { 2.0 } else { 0.0 };
        let stride_len = v.stride.x.hypot(v.stride.y).max(0.001);
        let fx = hip + (swing * gait * v.stride.x / stride_len * mirror as f32).round() as i32;
        let fy = (swing * gait * v.stride.y / stride_len * 0.55 - lift).round() as i32;
        let knee = (
            hip + (fx - hip) / 2 + if sick { 1 } else { 0 },
            -5 - bob + crouch,
        );
        p.limb((hip, -10 - bob + crouch), knee, 3, seam, denim);
        p.limb(knee, (fx, fy - 2), 2, denim, boot);
        p.limb(
            (fx, fy - 1),
            (fx + if side { 2 } else { 1 }, fy),
            2,
            sole,
            boot,
        );
    }

    let torso = if sick {
        if rear { RAG_BACK } else { RAGS }
    } else if rear {
        BACK
    } else if side {
        SIDE
    } else {
        COAT
    };
    let ty = -18 - bob + crouch + if sick { 1 } else { 0 };

    let draw_arm = |p: &mut Sprite<'_, '_>, near: bool| {
        let sign = if near { -1 } else { 1 };
        let shoulder = sign * 4 + lean;
        let swing = (wave * if brisk { 3.4 } else { 2.6 } * sign as f32).round() as i32;
        let aimed = !sick && weapon.is_some();
        let axe_swing = if !sick && weapon == Some(Weapon::Axe) {
            v.swing_progress
        } else {
            None
        };
        let shove = if !sick && weapon.is_none() {
            v.swing_progress
        } else {
            None
        };

        let (dx, dy) = if let Some(sp) = axe_swing {
            // Brisk sweeping arc across body during Axe swing!
            let sweep = (sp * std::f32::consts::PI).sin();
            (
                (v.facing.x.abs() * 7.0 + sweep * 4.0).round() as i32,
                (-3.0 + sp * 7.0 + v.facing.y * 3.0).round() as i32,
            )
        } else if let Some(sp) = shove {
            let thrust = (sp * std::f32::consts::PI).sin();
            (
                (v.facing.x.abs() * 6.0 * thrust).round() as i32,
                (1.0 + v.facing.y * 3.0).round() as i32,
            )
        } else if aimed {
            let reach = match weapon {
                Some(Weapon::RocketLauncher) => 5.0,
                Some(Weapon::Shotgun) => 7.0,
                Some(Weapon::Axe) => 6.0,
                _ => 8.0,
            };
            (
                (v.facing.x.abs() * reach).round() as i32,
                -1 + (v.facing.y * 4.0).round() as i32,
            )
        } else if sick {
            let reach = if brisk { 5 } else { 3 };
            (
                reach + if near { swing } else { 0 },
                1 + (variant as i32 % 2),
            )
        } else {
            (swing, 7)
        };

        let hand = (
            if aimed || shove.is_some() {
                lean + dx
            } else {
                shoulder + dx
            },
            ty + 3 + dy,
        );
        let elbow = (shoulder + dx / 3, ty + if aimed { 5 } else { 7 });
        p.limb((shoulder, ty + 2), elbow, 2, colors[0], colors[1]);
        p.limb(
            elbow,
            hand,
            2,
            if sick { colors[3] } else { colors[1] },
            if sick { colors[4] } else { colors[2] },
        );
        p.px(hand.0, hand.1 + 1, colors[4]);

        if near
            && !sick
            && let Some(w) = weapon
        {
            draw_survivor_weapon(p, w, hand, shoulder, ty, v, &pal);
        }
    };

    draw_arm(&mut p, false);
    if rear {
        draw_arm(&mut p, true);
    }
    p.stamp(lean - 5, ty, torso);
    let hx = lean + if sick { 1 } else { 0 } + (v.hurt * 2.0).round() as i32;
    p.stamp(
        hx - 3,
        ty - 6,
        if rear {
            NAPE
        } else if side {
            PROFILE
        } else {
            FACE
        },
    );
    if sick && !rear {
        p.px(
            hx + 1,
            ty - 2,
            if brisk {
                rgb(232, 92, 64)
            } else {
                pal(rgb(107, 54, 64))
            },
        );
    }
    if v.hurt > 0.4 {
        p.px(hx, ty - 2, rgb(255, 210, 167));
    }
    if !rear {
        draw_arm(&mut p, true);
    }

    let head_top_y = y.round() as i32 + ty - 7;
    if sick {
        draw_monster_hud(c, x.round() as i32, head_top_y, 12, v);
    }
}

fn draw_survivor_weapon(
    p: &mut Sprite<'_, '_>,
    weapon: Weapon,
    hand: (i32, i32),
    shoulder: i32,
    ty: i32,
    v: &ActorVisual,
    pal: &impl Fn(Ink) -> Ink,
) {
    let fx = v.facing.x.abs();
    let fy = v.facing.y;
    let rec = (v.recoil * 2.0).round() as i32;
    match weapon {
        Weapon::Axe => {
            // Hickory wooden handle + gleaming steel axe head!
            let swing_ang = v.swing_progress.map_or(0.0, |sp| (sp - 0.35) * 1.9);
            let (tip_dx, tip_dy) = if v.swing_progress.is_some() {
                (
                    (5.0 + swing_ang.cos() * 4.0).round() as i32,
                    (-4.0 + swing_ang.sin() * 6.0).round() as i32,
                )
            } else {
                (
                    (fx * 5.0 + 2.0).round() as i32,
                    (fy * 3.0 - 5.0).round() as i32,
                )
            };
            let tip = (hand.0 + tip_dx, hand.1 + tip_dy);
            p.limb(hand, tip, 1, pal(rgb(172, 124, 74)), pal(rgb(118, 78, 44)));
            // Steel blade head at tip
            let steel = pal(rgb(206, 218, 226));
            let glint = rgb(242, 248, 252);
            p.px(tip.0, tip.1 - 1, steel);
            p.px(tip.0 + 1, tip.1 - 1, glint);
            p.px(tip.0 + 1, tip.1, glint);
            p.px(tip.0 + 1, tip.1 + 1, steel);
            p.px(tip.0, tip.1 + 1, p.edge);
        }
        Weapon::Pistol => {
            let muzzle = (
                hand.0 + (fx * 5.0).round() as i32,
                hand.1 + (fy * 4.0).round() as i32 - rec,
            );
            p.limb(hand, muzzle, 2, pal(rgb(164, 176, 180)), p.edge);
            p.px(hand.0, hand.1 + 1, pal(BOOT));
        }
        Weapon::Shotgun => {
            // Two-handed walnut stock + long steel barrel!
            let stock = (hand.0 - 3, hand.1 + 1);
            let barrel = (
                hand.0 + (fx * 8.0 + 1.0).round() as i32,
                hand.1 + (fy * 5.0).round() as i32 - rec,
            );
            p.limb(
                stock,
                hand,
                2,
                pal(rgb(164, 102, 58)),
                pal(rgb(106, 62, 38)),
            );
            p.limb(
                hand,
                barrel,
                2,
                pal(rgb(168, 178, 186)),
                pal(rgb(78, 88, 98)),
            );
            // Pump forend
            let pump = ((hand.0 + barrel.0) / 2, (hand.1 + barrel.1) / 2 + 1);
            p.px(pump.0, pump.1, pal(rgb(144, 88, 52)));
        }
        Weapon::RocketLauncher => {
            // Unmistakable shoulder-mounted olive tube with yellow hazard stripe!
            let back_x = shoulder - (fx * 5.0 + 2.0).round() as i32;
            let back_y = ty - (fy * 2.0).round() as i32;
            let front_x = shoulder + (fx * 8.0 + 3.0).round() as i32;
            let front_y = ty + (fy * 4.0).round() as i32 - rec;
            p.limb(
                (back_x, back_y),
                (front_x, front_y),
                3,
                pal(rgb(122, 144, 92)),
                pal(rgb(78, 98, 62)),
            );
            // Yellow hazard stripe near front!
            let sx = shoulder + (fx * 4.0 + 1.0).round() as i32;
            let sy = ty + (fy * 2.0).round() as i32 - rec / 2;
            let yellow = rgb(242, 206, 72);
            p.px(sx, sy - 1, yellow);
            p.px(sx, sy, yellow);
            p.px(sx, sy + 1, yellow);
            // Dark exhaust bell at rear
            p.px(back_x - 1, back_y, p.edge);
            p.px(back_x - 1, back_y + 1, p.edge);
        }
    }
}

fn draw_bloater(c: &mut Canvas<'_>, x: f32, y: f32, v: &ActorVisual, _variant: u8) {
    let f = v.hit_flash;
    let l = v.light;
    let pal = |ink: Ink| flash_blend(tint(ink, l), f);

    // Pulsing chartreuse-yellow pustules (faster & brighter when swelling!).
    let pulse_speed = if v.swelling { 0.032 } else { 0.008 };
    let pulse = ((v.elapsed_ms as f32 * pulse_speed).sin() + 1.0) * 0.5;
    let pustule_lit = if v.swelling && pulse > 0.5 {
        rgb(248, 255, 168)
    } else {
        rgb(
            (198.0 + 36.0 * pulse) as u8,
            (224.0 + 26.0 * pulse) as u8,
            (76.0 + 32.0 * pulse) as u8,
        )
    };
    let pustule_dim = rgb(146, 186, 60);

    let colors = [
        pal(rgb(138, 148, 108)), // bruised olive highlight
        pal(rgb(104, 96, 114)),  // necrotic olive-purple mid
        pal(rgb(64, 60, 78)),    // deep bruised shadow
        pal(rgb(132, 138, 108)), // head shade
        pal(rgb(184, 192, 148)), // head lit
        pal(rgb(74, 72, 82)),
        pal(rgb(104, 102, 110)),
    ];
    let edge = pal(EDGE);
    let leg_lit = pal(rgb(84, 92, 98));
    let leg_dark = pal(rgb(52, 54, 68));

    let mirror = if v.facing.x < -0.05 { -1 } else { 1 };
    let direction = v.facing.y.atan2(v.facing.x.abs()).to_degrees();
    let rear = direction < -23.0;
    let moving = v.stride.x.abs() + v.stride.y.abs() > 0.01;
    let cycle = v.phase * std::f32::consts::TAU;
    let wave = if moving { cycle.sin() } else { 0.0 };
    let bob = if moving { (wave.abs() * 1.1) as i32 } else { 0 };
    let swell_jitter = if v.swelling {
        (((v.elapsed_ms / 40) & 1) as i32) * 2 - 1
    } else {
        0
    };

    // Wide heavy contact shadow.
    c.ellipse(x, y + 1.0, 10.5, 3.3, [20, 22, 28, 165]);

    let mut p = Sprite {
        c,
        x: x.round() as i32 + swell_jitter,
        y: y.round() as i32,
        mirror,
        edge,
        colors,
        bag: edge,
        canvas: edge,
        zip: edge,
        visor: edge,
        pustule_lit,
        pustule_dim,
    };

    // Thick bruiser legs.
    for (k, hip) in [(-1.0, -3), (1.0, 3)] {
        let swing = k * wave;
        let stride_len = v.stride.x.hypot(v.stride.y).max(0.001);
        let fx = hip + (swing * 1.9 * v.stride.x / stride_len * mirror as f32).round() as i32;
        let fy = (swing * 1.4 * v.stride.y / stride_len).round() as i32;
        p.limb((hip, -9 - bob), (fx, fy - 1), 4, leg_lit, leg_dark);
    }

    let ty = -20 - bob;
    let slam_windup = v.telegraph_progress.unwrap_or(0.0);

    // Thick arms: raised high overhead during slam windup!
    let draw_heavy_arm = |p: &mut Sprite<'_, '_>, near: bool| {
        let sign = if near { -1 } else { 1 };
        let sh = (sign * 6, ty + 3);
        let hand = if slam_windup > 0.01 {
            let raise = (slam_windup * 11.0).round() as i32;
            (sign * 5 + 1, ty + 1 - raise)
        } else {
            let sw = (wave * 2.0 * sign as f32).round() as i32;
            (sign * 7 + 2 + sw, ty + 9)
        };
        p.limb(sh, hand, 3, colors[0], colors[1]);
        p.px(hand.0, hand.1, pustule_lit);
    };

    draw_heavy_arm(&mut p, false);
    if rear {
        draw_heavy_arm(&mut p, true);
    }

    // Wide 14×12 hunched bruiser torso!
    p.stamp(-7, ty, if rear { BLOATER_BACK } else { BLOATER_FRONT });
    // Swollen pustule bulge when swelling!
    if v.swelling {
        p.px(-6, ty + 4, pustule_lit);
        p.px(7, ty + 5, pustule_lit);
        p.px(0, ty + 2, pustule_lit);
    }

    // Low-set hunched head.
    p.stamp(-3, ty - 4, BLOATER_HEAD);
    if !rear {
        draw_heavy_arm(&mut p, true);
    }

    let head_top_y = y.round() as i32 + ty - 6;
    draw_monster_hud(c, x.round() as i32, head_top_y, 16, v);
}

fn draw_shooter(c: &mut Canvas<'_>, x: f32, y: f32, v: &ActorVisual, _variant: u8) {
    let f = v.hit_flash;
    let l = v.light;
    let pal = |ink: Ink| flash_blend(tint(ink, l), f);

    // Glowing crimson-amber visor (un-dimmed emissive so it pops clearly in dusk!).
    let aim_hot = v.telegraph_progress.is_some_and(|p| p >= 0.65);
    let visor = if aim_hot && ((v.elapsed_ms / 45) & 1) == 0 {
        rgb(255, 224, 126)
    } else {
        rgb(255, 86, 56)
    };

    let colors = [
        pal(rgb(88, 102, 128)), // slate-navy coat lit
        pal(rgb(56, 68, 92)),   // slate-navy mid
        pal(rgb(34, 42, 60)),   // dark shadow
        pal(rgb(138, 142, 134)),
        pal(rgb(188, 192, 178)),
        pal(rgb(48, 56, 76)),
        pal(rgb(74, 86, 112)),
    ];
    let edge = pal(rgb(20, 22, 32));
    let pant_lit = pal(rgb(64, 72, 88));
    let pant_dark = pal(rgb(36, 40, 52));
    let boot = pal(rgb(42, 40, 48));

    let mirror = if v.facing.x < -0.05 { -1 } else { 1 };
    let direction = v.facing.y.atan2(v.facing.x.abs()).to_degrees();
    let rear = direction < -23.0;
    let side = direction.abs() < 68.0;
    let moving = v.stride.x.abs() + v.stride.y.abs() > 0.01;
    let cycle = v.phase * std::f32::consts::TAU;
    let wave = if moving { cycle.sin() } else { 0.0 };
    let bob = if moving { (wave.abs() * 1.2) as i32 } else { 0 };

    c.ellipse(x, y, 7.0, 2.2, [20, 20, 30, 150]);
    let mut p = Sprite {
        c,
        x: x.round() as i32,
        y: y.round() as i32,
        mirror,
        edge,
        colors,
        bag: edge,
        canvas: edge,
        zip: pal(rgb(166, 176, 190)),
        visor,
        pustule_lit: visor,
        pustule_dim: visor,
    };

    for (k, hip) in [(-1.0, -2), (1.0, 2)] {
        let swing = k * wave;
        let lift = (cycle.cos() * k).max(0.0) * if moving { 1.8 } else { 0.0 };
        let stride_len = v.stride.x.hypot(v.stride.y).max(0.001);
        let fx = hip + (swing * 3.0 * v.stride.x / stride_len * mirror as f32).round() as i32;
        let fy = (swing * 3.0 * v.stride.y / stride_len * 0.55 - lift).round() as i32;
        p.limb((hip, -10 - bob), (fx, fy - 2), 2, pant_lit, pant_dark);
        p.limb((fx, fy - 1), (fx + 1, fy), 2, pant_lit, boot);
    }

    // Crisp upright posture!
    let ty = -19 - bob;
    let draw_rifle_arm = |p: &mut Sprite<'_, '_>, near: bool| {
        let sign = if near { -1 } else { 1 };
        let sh = (sign * 4, ty + 2);
        let hand = (
            (v.facing.x.abs() * 6.0).round() as i32 + if near { 0 } else { 2 },
            ty + 3 + (v.facing.y * 3.0).round() as i32,
        );
        p.limb(sh, hand, 2, colors[0], colors[1]);
        if near {
            // Raised 9 px tactical rifle!
            let stock = (hand.0 - 3, hand.1 + 1);
            let muzzle = (
                hand.0 + (v.facing.x.abs() * 9.0 + 1.0).round() as i32,
                hand.1 + (v.facing.y * 5.0).round() as i32,
            );
            p.limb(stock, muzzle, 2, pal(rgb(138, 150, 166)), edge);
            if v.telegraph_progress.is_some() {
                p.px(muzzle.0, muzzle.1, visor);
                p.px(muzzle.0 + 1, muzzle.1, rgb(255, 224, 138));
            }
        }
    };

    draw_rifle_arm(&mut p, false);
    if rear {
        draw_rifle_arm(&mut p, true);
    }
    p.stamp(-5, ty, if rear { TACTICAL_BACK } else { TACTICAL_COAT });
    p.stamp(
        -4,
        ty - 7,
        if rear {
            HOOD_BACK
        } else if side {
            HOOD_PROFILE
        } else {
            HOOD_FRONT
        },
    );
    if !rear {
        // Bright center visor glint.
        p.px(if side { 2 } else { 0 }, ty - 4, rgb(255, 212, 132));
        draw_rifle_arm(&mut p, true);
    }

    let head_top_y = y.round() as i32 + ty - 8;
    draw_monster_hud(c, x.round() as i32, head_top_y, 12, v);
}

fn draw_monster_hud(c: &mut Canvas<'_>, cx: i32, head_top_y: i32, bar_w: i32, v: &ActorVisual) {
    let mut top_y = head_top_y - 3;

    // Compact framed HP bar ONLY when 0 < hp < max_hp.
    if v.hp > 0 && v.hp < v.max_hp && v.max_hp > 0 {
        let bx = cx - bar_w / 2;
        let by = top_y - 3;
        c.rect_xywh(bx - 1, by - 1, bar_w + 2, 4, [20, 22, 30, 235]);
        c.rect_xywh(bx, by, bar_w, 2, [52, 36, 40, 235]);
        let fill = ((v.hp as i32 * bar_w + v.max_hp as i32 - 1) / v.max_hp as i32).clamp(1, bar_w);
        let pct = (v.hp as u32 * 100) / v.max_hp as u32;
        let fill_col = if pct > 50 {
            rgb(104, 218, 108)
        } else if pct > 25 {
            rgb(246, 186, 72)
        } else {
            rgb(238, 76, 66)
        };
        c.rect_xywh(bx, by, fill, 2, fill_col);
        top_y = by - 2;
    }

    // Retro outlined pixel awareness badges (`?` Investigating, popping `!` Alert for 800 ms).
    match v.awareness {
        AwarenessIcon::Investigating => {
            let bob = (((v.elapsed_ms / 240) & 1) as i32) - 1;
            draw_badge(c, cx - 2, top_y - 9 + bob, BADGE_QUESTION);
        }
        AwarenessIcon::Alert if v.show_alert_badge => {
            let pop = if v.alert_age_ms < 150 {
                -(((150 - v.alert_age_ms) as f32 / 150.0) * 3.0).round() as i32
            } else {
                0
            };
            draw_badge(c, cx - 2, top_y - 9 + pop, BADGE_ALERT);
        }
        _ => {}
    }
}

fn draw_badge(c: &mut Canvas<'_>, x: i32, y: i32, rows: &[&str]) {
    for (dy, row) in rows.iter().enumerate() {
        for (dx, b) in row.bytes().enumerate() {
            let col = match b {
                b'#' => [20, 22, 32, 245],
                b'A' => [252, 198, 72, 255],
                b'R' => [255, 78, 62, 255],
                b'Y' => [255, 220, 96, 255],
                _ => continue,
            };
            c.put(x + dx as i32, y + dy as i32, col);
        }
    }
}

/// Crimson visor & dark hooded silhouette peeking inside a closed upper-story window!
pub(crate) fn draw_peeking_shooter(c: &mut Canvas<'_>, cx: f32, cy: f32, elapsed_ms: u64) {
    let ix = cx.round() as i32;
    let iy = cy.round() as i32;
    // Dark hood & shoulders silhouette inside window glass.
    c.rect_xywh(ix - 3, iy - 4, 7, 7, [26, 30, 44, 240]);
    c.rect_xywh(ix - 2, iy - 6, 5, 3, [34, 40, 58, 245]);
    // Glowing crimson-amber visor!
    let pulse = ((elapsed_ms / 180) & 1) == 0;
    let visor = if pulse {
        [255, 86, 56, 255]
    } else {
        [255, 136, 72, 255]
    };
    c.put(ix - 1, iy - 3, visor);
    c.put(ix, iy - 3, [255, 212, 128, 255]);
    c.put(ix + 1, iy - 3, visor);
    // Tiny rifle barrel glint resting on sill.
    c.line(cx + 1.0, cy + 1.0, cx + 5.0, cy + 2.0, [132, 146, 162, 230]);
}

/// Fallen corpse (standard humanoid or wider fallen Bloater carcass).
pub(crate) fn draw_fallen(
    c: &mut Canvas<'_>,
    x: f32,
    y: f32,
    kind: MonsterKind,
    variant: u8,
    progress: f32,
    light: CellLight,
) {
    let fade = progress.clamp(0.0, 1.0);
    let height = (3.0 * (1.0 - fade)).round() as i32;
    let pal = |ink: Ink| tint(ink, light);
    let edge = pal(EDGE);

    if kind == MonsterKind::Bloater {
        c.ellipse(x, y, 13.5, 4.0, [86, 128, 46, 155]);
        c.ellipse(x, y, 10.5, 2.8, [46, 56, 44, 185]);
        let mut p = Sprite {
            c,
            x: x.round() as i32,
            y: y.round() as i32,
            mirror: if variant.is_multiple_of(2) { 1 } else { -1 },
            edge,
            colors: [edge; 7],
            bag: edge,
            canvas: edge,
            zip: edge,
            visor: edge,
            pustule_lit: rgb(194, 228, 76),
            pustule_dim: rgb(134, 174, 58),
        };
        p.limb(
            (-11, 0),
            (-3, -2 - height),
            4,
            pal(rgb(78, 86, 94)),
            pal(rgb(48, 52, 64)),
        );
        p.limb(
            (-4, -3 - height),
            (6, -3 - height),
            6,
            pal(rgb(122, 132, 98)),
            pal(rgb(88, 82, 102)),
        );
        p.limb(
            (6, -3 - height),
            (9, -2 - height),
            4,
            pal(rgb(152, 162, 124)),
            pal(rgb(96, 102, 84)),
        );
        p.px(-1, -4 - height, rgb(194, 228, 76));
        p.px(3, -3 - height, rgb(164, 206, 68));
        return;
    }

    c.ellipse(x, y, 10.0, 2.4, [44, 30, 38, 175]);
    let (torso_lit, torso_dark) = match kind {
        MonsterKind::Brisk => (pal(rgb(174, 106, 68)), pal(rgb(108, 60, 48))),
        MonsterKind::Shooter => (pal(rgb(76, 88, 112)), pal(rgb(44, 52, 72))),
        _ => (pal(rgb(139, 143, 127)), pal(rgb(90, 100, 96))),
    };
    let mut p = Sprite {
        c,
        x: x.round() as i32,
        y: y.round() as i32,
        mirror: if variant.is_multiple_of(2) { 1 } else { -1 },
        edge,
        colors: [edge; 7],
        bag: edge,
        canvas: edge,
        zip: edge,
        visor: edge,
        pustule_lit: edge,
        pustule_dim: edge,
    };
    p.limb((-9, 0), (-2, -2 - height), 3, pal(SEAM), pal(DENIM));
    p.limb((-7, 1), (0, -height), 2, pal(BOOT), pal(DENIM));
    p.limb(
        (-2, -2 - height),
        (4, -2 - height),
        4,
        torso_lit,
        torso_dark,
    );
    p.limb(
        (5, -3 - height),
        (7, -2 - height),
        3,
        pal(rgb(176, 176, 144)),
        pal(rgb(108, 108, 97)),
    );
    p.limb((2, -height), (4, 1), 2, torso_lit, pal(BOOT));
}
