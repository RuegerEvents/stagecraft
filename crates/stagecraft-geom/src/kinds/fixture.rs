//! Fixtures. A generic symbol until block 5 brings GDTF silhouettes and SVG symbols:
//! a body box 260 × 340 mm with a thick lens edge at the front (local +y), and the FID.

use glam::{DAffine3, DVec3};
use stagecraft_core::{Object, ObjectKind};

use super::to_plan;
use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle, TextAnchor};

pub const BODY_WIDTH: f64 = 260.0;
pub const BODY_DEPTH: f64 = 340.0;
pub const LABEL_HEIGHT: f64 = 90.0;

pub fn plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Fixture { fid, .. } = &obj.kind else { return Vec::new() };
    let (hw, hd) = (BODY_WIDTH / 2.0, BODY_DEPTH / 2.0);
    let p = |x: f64, y: f64| to_plan(w, [x, y, 0.0]);
    let body = vec![p(-hw, -hd), p(hw, -hd), p(hw, hd), p(-hw, hd)];
    let mut out = vec![
        PlanPrim::Polygon { points: body, fill: FillClass::Body, line: Some(LineClass::Medium) },
        PlanPrim::line(vec![p(-hw, hd - 40.0), p(hw, hd - 40.0)], LineClass::Thick),
    ];
    if style.labels
        && let Some(f) = fid
    {
        out.push(PlanPrim::Text {
            pos: p(0.0, -20.0),
            text: f.to_string(),
            height: LABEL_HEIGHT,
            rotation: 0.0,
            anchor: TextAnchor::Center,
        });
    }
    out
}

/// Height of the yoke bar under the hanging point.
pub const YOKE_HEIGHT: f64 = 60.0;
/// Height of the body below the yoke.
pub const BODY_HEIGHT: f64 = 250.0;
pub const LENS_DIAMETER: f64 = 180.0;
pub const LENS_LENGTH: f64 = 50.0;

/// A placeholder until block 5 brings the GDTF model. GDTF convention: the origin is
/// the hanging point and the fixture hangs down (-z); floor fixtures are turned over
/// (rotation x 180°). A yoke bar, the body box under it, and a lens cylinder on the
/// front face (+y), as in the plan symbol.
pub fn mesh(_obj: &Object, _lod: Lod) -> Mesh {
    let (hw, hd) = (BODY_WIDTH / 2.0, BODY_DEPTH / 2.0);
    let body_top = -YOKE_HEIGHT;
    let body_bottom = body_top - BODY_HEIGHT;
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Fixture);
    m.cuboid(DVec3::new(-hw * 0.8, -40.0, body_top), DVec3::new(hw * 0.8, 40.0, 0.0));
    m.cuboid(DVec3::new(-hw, -hd, body_bottom), DVec3::new(hw, hd, body_top));
    m.set_material(MaterialClass::Lens);
    let zc = (body_top + body_bottom) / 2.0;
    m.tube(DVec3::new(0.0, hd, zc), DVec3::new(0.0, hd + LENS_LENGTH, zc), LENS_DIAMETER / 2.0, 20);
    m
}
