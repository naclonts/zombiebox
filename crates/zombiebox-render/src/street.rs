//! Birch Street's small, flat decorative details and cutaway house.
use super::{Canvas, TILE_H, TILE_W};
use zombiebox_core::{ActionGame, Ground, House, Position};

fn hash(x: i32, y: i32) -> u32 {
    let mut n = (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb_352d);
    n ^ (n >> 15)
}

pub(super) fn draw_ground(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    project: &impl Fn(f32, f32) -> (f32, f32),
    visible: &[(i32, i32, Ground, f32, f32)],
) {
    let house = game.house();
    for &(x, y, ground, sx, sy) in visible {
        let h = hash(x, y);
        let inside =
            house.is_some_and(|b| x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height);
        let color = if inside {
            let v = (h % 7) as u8;
            [105 + v, 83 + v, 61 + v, 255]
        } else {
            match ground {
                Ground::Road => {
                    let v = (h % 6) as u8;
                    [50 + v, 62 + v, 72 + v, 255]
                }
                Ground::Grass => {
                    let v = (hash(x.div_euclid(3), y.div_euclid(3)) % 8) as u8;
                    [52 + v, 72 + v, 57 + v / 2, 255]
                }
                Ground::Gravel => [59, 75, 60, 255],
                Ground::Pavement => [90, 94, 88, 255],
                Ground::Field => [75, 80, 56, 255],
            }
        };
        c.diamond(sx, sy, TILE_W * 0.5, TILE_H * 0.5, color);
        if inside {
            // Boards follow the room rather than the screen.
            let a = project(x as f32 - 0.5, y as f32);
            let b = project(x as f32 + 0.5, y as f32);
            c.line(a.0, a.1, b.0, b.1, [63, 52, 43, 95]);
            if h.is_multiple_of(5) {
                c.line(sx - 4., sy - 1., sx + 3., sy + 2., [149, 119, 82, 95]);
            }
        } else if ground == Ground::Road {
            // Roads run on the x=8 or y=8 chunk axes. Paint follows that axis.
            let along_x = y.rem_euclid(16) == 8 && x.rem_euclid(16) != 8;
            let along_y = x.rem_euclid(16) == 8 && y.rem_euclid(16) != 8;
            if (along_x && x.rem_euclid(4) < 2) || (along_y && y.rem_euclid(4) < 2) {
                let (a, b) = if along_x {
                    (
                        project(x as f32 - 0.38, y as f32),
                        project(x as f32 + 0.38, y as f32),
                    )
                } else {
                    (
                        project(x as f32, y as f32 - 0.38),
                        project(x as f32, y as f32 + 0.38),
                    )
                };
                c.line(a.0, a.1, b.0, b.1, [192, 166, 105, 190]);
                c.line(a.0, a.1 + 1., b.0, b.1 + 1., [118, 105, 78, 150]);
            }
            if h.is_multiple_of(4) {
                c.line(sx - 7., sy + 2., sx - 2., sy, [29, 39, 46, 160]);
                c.line(sx - 2., sy, sx + 2., sy + 2., [29, 39, 46, 160]);
                c.rect(sx + 6., sy - 2., 2., 1., [133, 143, 142, 110]);
            }
        } else {
            if ground == Ground::Grass || ground == Ground::Field {
                c.line(sx - 5., sy + 2., sx - 4., sy, [112, 135, 88, 150]);
                c.line(sx + 5., sy - 1., sx + 6., sy - 3., [37, 63, 49, 180]);
                if h.is_multiple_of(3) {
                    c.rect(sx + 2., sy + 3., 2., 1., [151, 144, 93, 150]);
                }
            } else if h.is_multiple_of(3) {
                c.rect(sx - 3., sy, 3., 1., [153, 151, 129, 130]);
                c.rect(sx + 5., sy + 2., 2., 1., [48, 55, 52, 140]);
            }
            // A pale curb runs exactly along each road edge.
            if x.rem_euclid(16) == 7 || x.rem_euclid(16) == 9 {
                let edge = x as f32 + if x.rem_euclid(16) == 7 { 0.5 } else { -0.5 };
                let a = project(edge, y as f32 - 0.5);
                let b = project(edge, y as f32 + 0.5);
                c.line(a.0, a.1, b.0, b.1, [150, 151, 135, 210]);
            }
            if y.rem_euclid(16) == 7 || y.rem_euclid(16) == 9 {
                let edge = y as f32 + if y.rem_euclid(16) == 7 { 0.5 } else { -0.5 };
                let a = project(x as f32 - 0.5, edge);
                let b = project(x as f32 + 0.5, edge);
                c.line(a.0, a.1, b.0, b.1, [150, 151, 135, 210]);
            }
        }
    }
    if let Some(b) = house {
        for (ox, oy, crown_height) in [(-5, 5, 33.), (2, -4, 39.), (b.width + 2, 8, 29.)] {
            let position = Position::new(b.x + ox, b.y + oy);
            if game
                .world()
                .tile_at(position)
                .is_some_and(|t| matches!(t.ground, Ground::Grass | Ground::Gravel))
            {
                let (x, y) = project(position.x as f32, position.y as f32);
                draw_birch(c, x, y, crown_height);
            }
        }
        // Authored planting pockets give the lot a recognizable silhouette without
        // suggesting that the lawn is impassable. Skip roads and the house itself.
        for (ox, oy, kind) in [
            (-3, -2, 0),
            (-4, -2, 0),
            (-3, -3, 1),
            (b.width + 2, -2, 0),
            (b.width + 3, -2, 0),
            (-3, b.height + 2, 1),
            (-4, b.height + 3, 0),
            (b.width + 2, b.height + 2, 1),
            (b.width + 3, b.height + 3, 0),
        ] {
            let wx = b.x + ox;
            let wy = b.y + oy;
            if !game
                .world()
                .tile_at(Position::new(wx, wy))
                .is_some_and(|t| t.ground == Ground::Grass || t.ground == Ground::Gravel)
            {
                continue;
            }
            let (x, y) = project(wx as f32, wy as f32);
            c.ellipse(x, y + 2., 15., 5., [28, 48, 35, 130]);
            c.ellipse(x - 5., y, 9., 5., [40, 72, 48, 255]);
            c.ellipse(x + 4., y - 2., 10., 6., [48, 82, 52, 255]);
            c.ellipse(x + 1., y - 5., 6., 3., [78, 108, 67, 210]);
            c.rect(x - 6., y - 3., 3., 2., [143, 142, 87, 190]);
            if kind == 1 {
                c.rect(x + 6., y - 5., 2., 2., [195, 149, 107, 210]);
            }
        }
        // A small mailbox is a familiar landmark beside the front walk.
        let (mx, my) = project(b.door.x as f32 - 2., b.door.y as f32 + 2.);
        if game
            .world()
            .tile_at(Position::new(b.door.x - 2, b.door.y + 2))
            .is_some_and(|t| t.ground != Ground::Road)
        {
            c.ellipse(mx, my + 2., 5., 2., [22, 34, 30, 150]);
            c.rect(mx, my - 4., 2., 10., [103, 91, 72, 255]);
            c.rect(mx, my - 10., 9., 5., [93, 108, 108, 255]);
            c.line(mx - 4., my - 13., mx + 3., my - 13., [169, 180, 164, 255]);
            c.rect(mx + 4., my - 11., 2., 3., [189, 104, 72, 255]);
        }
        // A short walk leads from the actual opening, with planted beds at the sides.
        let (dx, dy) = if b.door.x == b.x {
            (-1, 0)
        } else if b.door.x == b.x + b.width - 1 {
            (1, 0)
        } else if b.door.y == b.y {
            (0, -1)
        } else {
            (0, 1)
        };
        for step in 1..=3 {
            let wx = b.door.x + dx * step;
            let wy = b.door.y + dy * step;
            if game
                .world()
                .tile_at(Position::new(wx, wy))
                .is_some_and(|t| t.ground != Ground::Road)
            {
                let (x, y) = project(wx as f32, wy as f32);
                c.diamond(x, y, 12., 6., [147, 143, 123, 210]);
                c.line(x - 10., y, x, y + 5., [89, 96, 86, 180]);
            }
        }
        for side in [-1., 1.] {
            let wx = b.door.x as f32 + dx as f32 * 1.1 + dy as f32 * side * 1.3;
            let wy = b.door.y as f32 + dy as f32 * 1.1 + dx as f32 * side * 1.3;
            let (x, y) = project(wx, wy);
            c.ellipse(x, y + 2., 9., 3., [29, 45, 35, 150]);
            c.ellipse(x, y - 1., 7., 4., [48, 79, 53, 255]);
            c.ellipse(x - 3., y - 3., 4., 3., [75, 105, 65, 255]);
            c.rect(x + 3., y - 3., 2., 2., [171, 151, 93, 210]);
        }
    }
}

fn draw_birch(c: &mut Canvas<'_>, x: f32, y: f32, height: f32) {
    c.ellipse(x + 8., y + 3., 19., 6., [23, 36, 33, 123]);
    c.line(x, y, x - 1., y - height, [104, 116, 108, 255]);
    c.line(x + 1., y, x, y - height, [172, 174, 143, 255]);
    c.line(x, y - 13., x + 7., y - height + 4., [128, 137, 114, 255]);
    c.line(x, y - 10., x - 7., y - height + 11., [111, 122, 104, 255]);
    for nick in [4., 9., 17.] {
        c.line(x - 1., y - nick, x + 1., y - nick, [57, 72, 65, 255]);
    }

    let top = y - height;
    c.polygon(
        &[
            (x - 17., top + 4.),
            (x - 14., top - 5.),
            (x - 5., top - 12.),
            (x + 6., top - 11.),
            (x + 14., top - 6.),
            (x + 19., top + 2.),
            (x + 14., top + 9.),
            (x + 4., top + 14.),
            (x - 8., top + 12.),
        ],
        [39, 65, 56, 255],
    );
    c.polygon(
        &[
            (x - 14., top),
            (x - 12., top - 6.),
            (x - 4., top - 11.),
            (x + 5., top - 9.),
            (x + 11., top - 3.),
            (x + 8., top + 5.),
            (x - 3., top + 7.),
        ],
        [69, 92, 65, 255],
    );
    for (dx, dy, width) in [(-8., -5., 5.), (1., -7., 6.), (8., -2., 5.), (-5., 1., 7.)] {
        c.rect(x + dx, top + dy, width, 2., [113, 123, 79, 210]);
    }
}

pub(super) fn draw_house(
    c: &mut Canvas<'_>,
    house: House,
    project: &impl Fn(f32, f32) -> (f32, f32),
) {
    let House {
        x,
        y,
        width: w,
        height: h,
        door,
    } = house;
    // Walls are centered on the existing collision tiles. Near faces are cut low.
    for yy in y..y + h {
        for xx in x..x + w {
            if xx == door.x && yy == door.y {
                continue;
            }
            if xx != x && xx != x + w - 1 && yy != y && yy != y + h - 1 {
                continue;
            }
            let horizontal = yy == y || yy == y + h - 1;
            let near = xx == x + w - 1 || yy == y + h - 1;
            let height = if near { 9. } else { 30. };
            let (a, b) = if horizontal {
                (
                    project(xx as f32 - 0.5, yy as f32),
                    project(xx as f32 + 0.5, yy as f32),
                )
            } else {
                (
                    project(xx as f32, yy as f32 - 0.5),
                    project(xx as f32, yy as f32 + 0.5),
                )
            };
            c.polygon(
                &[
                    (a.0, a.1),
                    (b.0, b.1),
                    (b.0, b.1 - height),
                    (a.0, a.1 - height),
                ],
                if horizontal {
                    [111, 112, 99, 255]
                } else {
                    [86, 98, 94, 255]
                },
            );
            c.line(a.0, a.1 - height, b.0, b.1 - height, [187, 176, 151, 255]);
            c.line(a.0, a.1 - 3., b.0, b.1 - 3., [65, 68, 62, 180]);
            c.line(a.0, a.1 - 6., b.0, b.1 - 6., [155, 148, 130, 150]);
            if !near {
                for course in [12., 18., 24.] {
                    c.line(a.0, a.1 - course, b.0, b.1 - course, [55, 67, 62, 120]);
                }
                if (yy == y && matches!(xx - x, 2 | 4)) || (xx == x && matches!(yy - y, 2 | 4)) {
                    draw_window(c, a, b);
                }
            }
        }
    }
    // A rug, low cabinet and upholstered bench establish a lived-in interior.
    // They sit along the back perimeter and are painted beneath every actor.
    let rug = [
        project(x as f32 + 1.2, y as f32 + 1.8),
        project(x as f32 + 3.0, y as f32 + 1.8),
        project(x as f32 + 3.0, y as f32 + 3.1),
        project(x as f32 + 1.2, y as f32 + 3.1),
    ];
    c.polygon(&rug, [105, 70, 62, 230]);
    for i in 0..4 {
        let a = rug[i];
        let b = rug[(i + 1) % 4];
        c.line(a.0, a.1, b.0, b.1, [188, 143, 104, 220]);
    }
    let (bx, by) = project(x as f32 + 1.4, y as f32 + 0.8);
    c.ellipse(bx, by + 3., 19., 6., [29, 29, 26, 130]);
    c.polygon(
        &[
            (bx - 17., by - 7.),
            (bx - 3., by - 14.),
            (bx + 15., by - 5.),
            (bx + 1., by + 2.),
        ],
        [92, 111, 103, 255],
    );
    c.polygon(
        &[
            (bx - 17., by - 7.),
            (bx + 1., by + 2.),
            (bx + 1., by + 8.),
            (bx - 17., by - 1.),
        ],
        [61, 78, 75, 255],
    );
    c.line(bx - 12., by - 8., bx + 2., by - 1., [157, 167, 145, 200]);
    let (cx, cy) = project(x as f32 + 0.8, y as f32 + 3.8);
    c.ellipse(cx, cy + 3., 13., 4., [30, 29, 25, 130]);
    c.polygon(
        &[
            (cx - 11., cy - 8.),
            (cx, cy - 13.),
            (cx + 11., cy - 8.),
            (cx, cy - 3.),
        ],
        [145, 112, 77, 255],
    );
    c.polygon(
        &[
            (cx - 11., cy - 8.),
            (cx, cy - 3.),
            (cx, cy + 4.),
            (cx - 11., cy - 1.),
        ],
        [99, 72, 52, 255],
    );
    c.polygon(
        &[
            (cx, cy - 3.),
            (cx + 11., cy - 8.),
            (cx + 11., cy - 1.),
            (cx, cy + 4.),
        ],
        [77, 60, 47, 255],
    );
    c.rect(cx - 2., cy - 14., 4., 3., [195, 172, 125, 255]);
    let (x, y) = project(door.x as f32, door.y as f32);
    c.diamond(x, y, 13., 6., [178, 139, 87, 255]);
    c.line(x - 12., y, x, y + 6., [222, 191, 128, 255]);
    let door_slope = if door.y == house.y || door.y == house.y + h - 1 {
        6.
    } else {
        -6.
    };
    for side in [-1., 1.] {
        let px = x + side * 13.;
        let py = y + side * door_slope;
        c.rect(px, py - 8., 4., 17., [100, 85, 66, 255]);
        c.rect(px - 1., py - 9., 1., 14., [160, 137, 98, 255]);
        c.rect(px, py - 17., 5., 2., [196, 170, 121, 255]);
    }
    // Light is small and local so the opening remains legible under actors.
    let light_y = y - door_slope - 13.;
    c.glow(x - 13., light_y, 24., [255, 184, 89], 62.);
    c.rect(x - 13., light_y, 5., 7., [71, 56, 44, 255]);
    c.rect(x - 13., light_y, 3., 4., [255, 213, 132, 255]);
}

fn draw_window(c: &mut Canvas<'_>, a: (f32, f32), b: (f32, f32)) {
    let left = (a.0 + (b.0 - a.0) * 0.16, a.1 + (b.1 - a.1) * 0.16);
    let right = (a.0 + (b.0 - a.0) * 0.84, a.1 + (b.1 - a.1) * 0.84);
    c.polygon(
        &[
            (left.0, left.1 - 9.),
            (right.0, right.1 - 9.),
            (right.0, right.1 - 24.),
            (left.0, left.1 - 24.),
        ],
        [39, 56, 65, 255],
    );
    c.line(
        left.0,
        left.1 - 23.,
        right.0,
        right.1 - 23.,
        [158, 167, 143, 255],
    );
    c.line(
        left.0,
        left.1 - 9.,
        right.0,
        right.1 - 9.,
        [181, 176, 149, 255],
    );
    c.line(
        left.0,
        left.1 - 16.,
        right.0,
        right.1 - 16.,
        [101, 131, 128, 255],
    );
    let mid = ((left.0 + right.0) * 0.5, (left.1 + right.1) * 0.5);
    c.line(mid.0, mid.1 - 10., mid.0, mid.1 - 23., [113, 140, 131, 255]);
}

pub(super) fn draw_cache(c: &mut Canvas<'_>, x: f32, y: f32, nearby: bool, elapsed_ms: u64) {
    let pulse = if nearby {
        ((elapsed_ms as f32 * 0.005).sin() + 1.) * 8.
    } else {
        0.
    };
    c.glow(x, y - 5., 27., [247, 169, 66], 55. + pulse);
    c.ellipse(x, y + 3., 9., 4., [17, 21, 19, 160]);
    c.polygon(
        &[
            (x - 8., y - 6.),
            (x, y - 10.),
            (x + 8., y - 6.),
            (x, y - 2.),
        ],
        [211, 155, 76, 255],
    );
    c.polygon(
        &[(x - 8., y - 6.), (x, y - 2.), (x, y + 5.), (x - 8., y + 1.)],
        [144, 91, 45, 255],
    );
    c.polygon(
        &[(x, y - 2.), (x + 8., y - 6.), (x + 8., y + 1.), (x, y + 5.)],
        [111, 68, 37, 255],
    );
    c.line(x - 4., y - 8., x + 4., y - 4., [255, 218, 134, 255]);
    c.rect(x, y - 2., 3., 3., [255, 222, 143, 255]);
}
