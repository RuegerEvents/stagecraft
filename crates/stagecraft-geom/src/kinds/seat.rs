//! Seat rows: one chair outline per seat in the plan, a simple chair per seat in 3D.

use glam::{DAffine3, DVec3};
use stagecraft_core::{Object, ObjectKind};

use super::to_plan_2d;
use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle};

/// Seat top above the floor, mm.
pub const SEAT_HEIGHT: f64 = 450.0;
/// Top of the backrest above the floor, mm.
pub const BACK_HEIGHT: f64 = 850.0;

/// Centres of the seats along local x.
pub fn seat_centres(count: u32, seat_width: f64) -> impl Iterator<Item = f64> {
    let first = -(count as f64 - 1.0) * seat_width / 2.0;
    (0..count).map(move |i| first + seat_width * i as f64)
}

/// A seat's half width: a little gap to the next one.
fn half(seat_width: f64) -> f64 {
    (seat_width / 2.0 - (seat_width * 0.06).min(30.0)).max(1.0)
}

pub fn plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::SeatRow { count, seat_width, depth } = obj.kind else { return Vec::new() };
    let (hd, hs) = (depth / 2.0, half(seat_width));
    let back = (depth * 0.15).min(60.0);
    let p = |x: f64, y: f64| to_plan_2d(w, [x, y]);
    let mut out = Vec::with_capacity(2 * count as usize);
    for c in seat_centres(count, seat_width) {
        // the seat, its back along the rear edge (−y: the seats face +y)
        out.push(PlanPrim::Polygon {
            points: vec![p(c - hs, -hd), p(c + hs, -hd), p(c + hs, hd), p(c - hs, hd)],
            fill: FillClass::Body,
            line: Some(LineClass::Thin),
        });
        out.push(PlanPrim::Polygon {
            points: vec![p(c - hs, -hd), p(c + hs, -hd), p(c + hs, -hd + back), p(c - hs, -hd + back)],
            fill: FillClass::Masking,
            line: Some(LineClass::Thin),
        });
    }
    out
}

pub fn mesh(obj: &Object, lod: Lod) -> Mesh {
    let ObjectKind::SeatRow { count, seat_width, depth } = obj.kind else { return Mesh::empty() };
    let (hd, hs) = (depth / 2.0, half(seat_width));
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Deck);
    if lod == Lod::Box {
        let hw = count as f64 * seat_width / 2.0;
        m.cuboid(DVec3::new(-hw, -hd, 0.0), DVec3::new(hw, hd, BACK_HEIGHT));
        return m;
    }
    let (seat, back) = (40.0, (depth * 0.12).min(50.0));
    for c in seat_centres(count, seat_width) {
        m.set_material(MaterialClass::Deck);
        m.cuboid(DVec3::new(c - hs, -hd, SEAT_HEIGHT - seat), DVec3::new(c + hs, hd, SEAT_HEIGHT));
        m.cuboid(DVec3::new(c - hs, -hd, SEAT_HEIGHT), DVec3::new(c + hs, -hd + back, BACK_HEIGHT));
        m.set_material(MaterialClass::Metal);
        if lod == Lod::Full {
            let l = 25.0;
            for (x, y) in [(c - hs + l, -hd + l), (c + hs - l, -hd + l), (c + hs - l, hd - l), (c - hs + l, hd - l)] {
                m.cuboid(
                    DVec3::new(x - l / 2.0, y - l / 2.0, 0.0),
                    DVec3::new(x + l / 2.0, y + l / 2.0, SEAT_HEIGHT - seat),
                );
            }
        } else {
            let l = 60.0;
            m.cuboid(DVec3::new(c - l / 2.0, -l / 2.0, 0.0), DVec3::new(c + l / 2.0, l / 2.0, SEAT_HEIGHT - seat));
        }
    }
    m
}
