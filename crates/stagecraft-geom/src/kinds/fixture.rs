//! Fixtures. A generic symbol until block 5 brings GDTF silhouettes and SVG symbols:
//! a body box 260 × 340 mm with a thick lens edge at the front (local +y), and the FID.

use glam::DAffine3;
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

pub fn mesh(_obj: &Object, _lod: Lod) -> Mesh {
    // TODO(block 3/5): a box now, the GDTF model later.
    Mesh::empty(MaterialClass::Fixture)
}
