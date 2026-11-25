//! Clustered pixel sprites. Each row is one native scanline, shaded from above.
use super::{Canvas, Vec2};

pub(super) enum ActorKind {
    Survivor,
    Infected(usize),
}

pub(super) struct ActorVisual {
    pub facing: Vec2,
    pub stride: Vec2,
    pub phase: f32,
    pub armed: bool,
    pub recoil: f32,
    pub hurt: f32,
    pub kind: ActorKind,
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

// Symbols identify material and light planes across the head, collar, coat and pack.
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

struct Sprite<'a, 'b> {
    c: &'a mut Canvas<'b>,
    x: i32,
    y: i32,
    mirror: i32,
    colors: [Ink; 7],
}
impl Sprite<'_, '_> {
    fn px(&mut self, x: i32, y: i32, color: Ink) {
        self.c.put(self.x + self.mirror * x, self.y + y, color);
    }
    fn stamp(&mut self, x: i32, y: i32, rows: &[&str]) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, s) in row.bytes().enumerate() {
                let ink = match s {
                    b'#' => EDGE,
                    b'L' => self.colors[0],
                    b'M' => self.colors[1],
                    b'D' => self.colors[2],
                    b's' => self.colors[3],
                    b'S' => self.colors[4],
                    b'h' => self.colors[5],
                    b'H' => self.colors[6],
                    b'B' => BAG,
                    b'C' => CANVAS,
                    b'Z' | b'b' => ZIP,
                    _ => continue,
                };
                self.px(x + dx as i32, y + dy as i32, ink);
            }
        }
    }
    // Connected horizontal clusters taper at the cuff, knee and ankle.
    fn limb(&mut self, a: (i32, i32), b: (i32, i32), width: i32, lit: Ink, shade: Ink) {
        let steps = (a.0 - b.0).abs().max((a.1 - b.1).abs()).max(1);
        for i in 0..=steps {
            let x = a.0 + (b.0 - a.0) * i / steps;
            let y = a.1 + (b.1 - a.1) * i / steps;
            self.px(x - 1, y, lit);
            for w in 0..width - 1 {
                self.px(x + w, y, shade);
            }
            self.px(x + width - 1, y, EDGE);
        }
    }
}

pub(super) fn draw_actor(c: &mut Canvas<'_>, x: f32, y: f32, v: ActorVisual) {
    let id = match v.kind {
        ActorKind::Survivor => None,
        ActorKind::Infected(n) => Some(n),
    };
    let sick = id.is_some();
    let variant = id.unwrap_or(0) % 3;
    let colors = match id.map(|n| n % 3) {
        None => [
            rgb(242, 177, 83),
            rgb(186, 102, 56),
            rgb(115, 62, 53),
            rgb(180, 113, 79),
            rgb(244, 183, 122),
            rgb(77, 55, 51),
            rgb(119, 83, 62),
        ],
        Some(n) => {
            let (l, m, d) = match n {
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
    let mirror = if v.facing.x < -0.05 { -1 } else { 1 };
    let direction = v.facing.y.atan2(v.facing.x.abs()).to_degrees();
    let rear = direction < -23.;
    let side = direction.abs() < 68.;
    let moving = v.stride.x.abs() + v.stride.y.abs() > 0.01;
    let cycle = v.phase * std::f32::consts::TAU;
    let mut wave = if moving { cycle.sin() } else { 0. };
    if sick {
        wave = wave.signum() * wave.abs().powi(2);
    }
    let gait = if sick { 2.4 } else { 3.4 };
    let bob = if moving {
        (wave.abs() * 1.45) as i32
    } else {
        0
    };
    let lean = if sick { 1 + variant as i32 % 2 } else { 0 } - (v.recoil * 1.7).round() as i32;
    c.ellipse(x, y, 7., 2.2, [24, 23, 34, 142]);
    let mut p = Sprite {
        c,
        x: x.round() as i32,
        y: y.round() as i32,
        mirror,
        colors,
    };
    for (k, hip) in [(-1., -2), (1., 2)] {
        let swing = k * wave;
        let lift = (cycle.cos() * k).max(0.) * if moving { 2.0 } else { 0. };
        let stride_len = v.stride.x.hypot(v.stride.y).max(0.001);
        let fx = hip + (swing * gait * v.stride.x / stride_len * mirror as f32).round() as i32;
        let fy = (swing * gait * v.stride.y / stride_len * 0.55 - lift).round() as i32;
        let knee = (hip + (fx - hip) / 2 + if sick { 1 } else { 0 }, -5 - bob);
        p.limb((hip, -10 - bob), knee, 3, SEAM, DENIM);
        p.limb(knee, (fx, fy - 2), 2, DENIM, BOOT);
        p.limb(
            (fx, fy - 1),
            (fx + if side { 2 } else { 1 }, fy),
            2,
            SOLE,
            BOOT,
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
    let ty = -18 - bob + if sick { 1 } else { 0 };
    let arm = |p: &mut Sprite<'_, '_>, near: bool| {
        let sign = if near { -1 } else { 1 };
        let shoulder = sign * 4 + lean;
        let swing = (wave * 2.6 * sign as f32).round() as i32;
        let aimed = v.armed && !sick;
        let dy = if aimed {
            -1 + (v.facing.y * 4.).round() as i32
        } else if sick {
            2 + variant as i32
        } else {
            7
        };
        let dx = if aimed {
            (v.facing.x.abs() * 8.).round() as i32
        } else if sick {
            3 + if near { swing } else { 0 }
        } else {
            swing
        };
        let hand = (if aimed { lean + dx } else { shoulder + dx }, ty + 3 + dy);
        let elbow = (shoulder + dx / 3, ty + if aimed { 6 } else { 7 });
        p.limb((shoulder, ty + 2), elbow, 2, colors[0], colors[1]);
        p.limb(
            elbow,
            hand,
            2,
            if sick { colors[3] } else { colors[1] },
            if sick { colors[4] } else { colors[2] },
        );
        p.px(hand.0, hand.1 + 1, colors[4]);
        if near && aimed {
            let muzzle = (
                hand.0 + (v.facing.x.abs() * 5.).round() as i32,
                hand.1 + (v.facing.y * 4.).round() as i32 - (v.recoil * 2.).round() as i32,
            );
            p.limb(hand, muzzle, 2, rgb(157, 169, 172), EDGE);
            p.px(hand.0, hand.1 + 1, BOOT);
        }
    };
    arm(&mut p, false);
    if rear {
        arm(&mut p, true);
    }
    p.stamp(lean - 5, ty, torso);
    let hx = lean + if sick { 1 } else { 0 } + (v.hurt * 2.).round() as i32;
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
        p.px(hx + 1, ty - 2, rgb(107, 54, 64));
    }
    if v.hurt > 0.4 {
        p.px(hx, ty - 2, rgb(255, 210, 167));
    }
    if !rear {
        arm(&mut p, true);
    }
}

pub(super) fn draw_fallen(c: &mut Canvas<'_>, x: f32, y: f32, variant: usize, progress: f32) {
    let fade = progress.clamp(0., 1.);
    c.ellipse(x, y, 10., 2.4, [48, 33, 43, 170]);
    let mut p = Sprite {
        c,
        x: x.round() as i32,
        y: y.round() as i32,
        mirror: if variant.is_multiple_of(2) { 1 } else { -1 },
        colors: [EDGE; 7],
    };
    let height = (3. * (1. - fade)).round() as i32;
    p.limb((-9, 0), (-2, -2 - height), 3, SEAM, DENIM);
    p.limb((-7, 1), (0, -height), 2, BOOT, DENIM);
    p.limb(
        (-2, -2 - height),
        (4, -2 - height),
        4,
        rgb(139, 143, 127),
        rgb(90, 100, 96),
    );
    p.limb(
        (5, -3 - height),
        (7, -2 - height),
        3,
        rgb(176, 176, 144),
        rgb(108, 108, 97),
    );
    p.limb((2, -height), (4, 1), 2, rgb(120, 128, 116), BOOT);
}
