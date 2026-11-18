//! Stage objects: proscenium, curtains, masking, battens, platforms, supports, hoists,
//! ceilings. Plan symbols in world mm, meshes in local mm.

use glam::{DAffine3, DVec3};
use stagecraft_core::model::{Gather, MaskingRole, P2, SupportStyle, Understructure, parse_color};
use stagecraft_core::{Object, ObjectKind};

use super::fabric::{self, CLOTH, Drape};
use super::{to_plan, to_plan_2d};
use crate::{Detail, FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle, TextAnchor};

/// Cap height of batten numbers and platform heights in plan, mm.
pub const LABEL_HEIGHT: f64 = 150.0;
/// Diameter of a batten pipe, mm.
pub const PIPE: f64 = 48.3;

fn rect(w: &DAffine3, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P2> {
    vec![to_plan_2d(w, [x0, y0]), to_plan_2d(w, [x1, y0]), to_plan_2d(w, [x1, y1]), to_plan_2d(w, [x0, y1])]
}

fn label(pos: P2, text: String, height: f64) -> PlanPrim {
    PlanPrim::Text { pos, text, height, rotation: 0.0, anchor: TextAnchor::Center }
}

// ---------------------------------------------------------------- proscenium

/// Batten numbers, platform heights and the iron label are drawn unless the object
/// turns labels off: the canvas already drops text too small to read, and these are
/// bigger than FIDs, so they stay readable when fixture labels are switched off.
fn own_labels(obj: &Object) -> bool {
    obj.style.labels != Some(false)
}

pub fn proscenium_plan(
    doc: &stagecraft_core::Document,
    w: &DAffine3,
    obj: &Object,
    _style: &PlanStyle,
) -> Vec<PlanPrim> {
    let ObjectKind::Proscenium { width, depth, frame, iron, show_iron, keep_clear, .. } = obj.kind else {
        return Vec::new();
    };
    let (hw, f) = (width / 2.0, frame);
    let mut out = vec![
        PlanPrim::Polygon {
            points: rect(w, -hw - f, 0.0, -hw, depth),
            fill: FillClass::Wall,
            line: Some(LineClass::Thick),
        },
        PlanPrim::Polygon {
            points: rect(w, hw, 0.0, hw + f, depth),
            fill: FillClass::Wall,
            line: Some(LineClass::Thick),
        },
        // The header is above the plan cut: dashed.
        PlanPrim::Polyline { points: rect(w, -hw, 0.0, hw, depth), closed: true, line: LineClass::Hidden },
    ];
    if show_iron {
        out.push(PlanPrim::line(
            vec![to_plan_2d(w, [-hw - f, iron]), to_plan_2d(w, [hw + f, iron])],
            LineClass::Centre,
        ));
        // the keep-clear strip under it, hatched at 45°, across the opening
        if keep_clear > 0.0 {
            let (y0, y1) = (iron - keep_clear / 2.0, iron + keep_clear / 2.0);
            out.push(PlanPrim::Polyline { points: rect(w, -hw, y0, hw, y1), closed: true, line: LineClass::Thin });
            let step = 200.0;
            let mut c = -keep_clear;
            while c < 2.0 * hw {
                // the line x = -hw + c + (y - y0), clipped to the strip
                let (mut a, mut b) = ([-hw + c, y0], [-hw + c + keep_clear, y1]);
                if a[0] < -hw {
                    a = [-hw, y0 + (-hw - a[0])];
                }
                if b[0] > hw {
                    b = [hw, y1 - (b[0] - hw)];
                }
                if b[0] > a[0] {
                    out.push(PlanPrim::line(vec![to_plan_2d(w, a), to_plan_2d(w, b)], LineClass::Thin));
                }
                c += step;
            }
        }
        if own_labels(obj) {
            let key = if keep_clear > 0.0 { "iron_keep_clear" } else { "iron" };
            out.push(PlanPrim::Text {
                pos: to_plan_2d(w, [hw + f + 150.0, iron]),
                text: stagecraft_core::sheet::text::tr(doc.meta.sheet_language, key).into(),
                height: LABEL_HEIGHT,
                rotation: 0.0,
                anchor: TextAnchor::Left,
            });
        }
    }
    out
}

pub fn proscenium_mesh(obj: &Object) -> Mesh {
    let ObjectKind::Proscenium { width, height, depth, frame, chamfer, outer_chamfer, .. } = obj.kind else {
        return Mesh::empty();
    };
    let (hw, f) = (width / 2.0, frame);
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Wall);
    if chamfer <= 0.0 && outer_chamfer <= 0.0 {
        m.cuboid(DVec3::new(-hw - f, 0.0, 0.0), DVec3::new(-hw, depth, height + f));
        m.cuboid(DVec3::new(hw, 0.0, 0.0), DVec3::new(hw + f, depth, height + f));
        m.cuboid(DVec3::new(-hw, 0.0, height), DVec3::new(hw, depth, height + f));
        return m;
    }
    // Pieces in the front face (u = x, v = z), extruded upstage: the left pillar with
    // both chamfers, the header between the inner chamfers, the mirrored right pillar.
    let c = chamfer.clamp(0.0, (hw * 0.9).min(height));
    let co = outer_chamfer.clamp(0.0, (f + c).min(height + f));
    let (ho, top) = (hw + f, height + f);
    let left = [
        [-ho, 0.0],
        [-hw, 0.0],
        [-hw, height - c],
        [-hw + c, height],
        [-hw + c, top],
        [-ho + co, top],
        [-ho, top - co],
    ];
    // (u, v, w) → (u, −w, v): a rotation, so the extrusion's winding stays outward.
    let to_face = DAffine3::from_cols(DVec3::X, DVec3::Z, DVec3::NEG_Y, DVec3::ZERO);
    let mut add = |outline: &[[f64; 2]]| {
        let mut pts: Vec<[f64; 2]> = Vec::new();
        for p in outline {
            if pts.last().is_none_or(|q: &[f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-6) {
                pts.push(*p);
            }
        }
        while pts.len() > 1 && (pts[0][0] - pts[pts.len() - 1][0]).hypot(pts[0][1] - pts[pts.len() - 1][1]) < 1e-6 {
            pts.pop();
        }
        let mut piece = Mesh::empty();
        piece.set_material(MaterialClass::Wall);
        piece.extrude(&pts, -depth, 0.0);
        m.append(&piece.transformed(&to_face));
    };
    add(&left);
    let right: Vec<[f64; 2]> = left.iter().rev().map(|p| [-p[0], p[1]]).collect();
    add(&right);
    if hw - c > 1e-6 {
        add(&[[-hw + c, height], [hw - c, height], [hw - c, top], [-hw + c, top]]);
    }
    m
}

// ---------------------------------------------------------------- curtains and masking

/// The drape of a curtain or a piece of masking.
pub fn drape(kind: &ObjectKind) -> Option<Drape> {
    match *kind {
        ObjectKind::Curtain { width, height, fullness, split, opening, gather, .. } => {
            Some(Drape { width, height, fullness, split, opening, gather })
        }
        ObjectKind::Masking { width, height, fullness, .. } => {
            Some(Drape { width, height, fullness, split: false, opening: 0.0, gather: Gather::Tab })
        }
        _ => None,
    }
}

/// Points per pleat for a detail level (0 = straight).
fn per_wave(d: &Drape, lod: Lod) -> usize {
    if d.fullness <= 0.0 {
        return 0;
    }
    match lod {
        Lod::Full => 12,
        Lod::Simple => 4,
        Lod::Box => 0,
    }
}

/// Peak-to-peak depth of the pleats of a closed drape, mm (0 for flat ones).
pub fn pleat_depth(d: &Drape) -> f64 {
    fabric::panels(&Drape { opening: 0.0, ..*d }, 8).first().map_or(0.0, |p| p.depth)
}

pub fn curtain_plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let Some(d) = drape(&obj.kind) else { return Vec::new() };
    let pw = if style.detail == Detail::High && d.fullness > 0.0 { 8 } else { 0 };
    let mut out: Vec<PlanPrim> = fabric::panels(&d, pw)
        .into_iter()
        .map(|p| PlanPrim::line(p.line.iter().map(|q| to_plan_2d(w, *q)).collect(), LineClass::Medium))
        .collect();
    if style.detail == Detail::High {
        let hw = d.width / 2.0;
        out.push(PlanPrim::line(vec![to_plan_2d(w, [-hw, 0.0]), to_plan_2d(w, [hw, 0.0])], LineClass::Centre));
    }
    out
}

pub fn masking_plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Masking { role, width, .. } = obj.kind else { return Vec::new() };
    let hw = width / 2.0;
    match role {
        MaskingRole::Leg => vec![PlanPrim::Polygon {
            points: rect(w, -hw, -40.0, hw, 40.0),
            fill: FillClass::Masking,
            line: Some(LineClass::Medium),
        }],
        // Borders hang above the plan cut.
        MaskingRole::Border => {
            vec![PlanPrim::line(vec![to_plan_2d(w, [-hw, 0.0]), to_plan_2d(w, [hw, 0.0])], LineClass::Hidden)]
        }
    }
}

/// Curtains and masking: cloth slabs along the pleated panels, coloured by the object.
pub fn cloth_mesh(obj: &Object, lod: Lod) -> Mesh {
    let Some(d) = drape(&obj.kind) else { return Mesh::empty() };
    let (material, colour) = match &obj.kind {
        ObjectKind::Curtain { color, .. } => (MaterialClass::Curtain, color),
        ObjectKind::Masking { color, .. } => (MaterialClass::Masking, color),
        _ => unreachable!(),
    };
    let mut m = Mesh::empty();
    m.set_material(material);
    m.tint = parse_color(colour);
    let n = per_wave(&d, lod);
    let flat_depth = if n == 0 { pleat_depth(&d).max(if d.fullness > 0.0 { 0.0 } else { 20.0 }) } else { 0.0 };
    for p in fabric::panels(&d, n) {
        let t = if n == 0 { flat_depth.max(CLOTH) } else { CLOTH };
        fabric::slab(&mut m, &p.line, &p.hem, 0.0, t);
    }
    m
}

// ---------------------------------------------------------------- battens

pub fn batten_plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Batten { number, length, .. } = obj.kind else { return Vec::new() };
    let hl = length / 2.0;
    let mut out = vec![
        PlanPrim::line(vec![to_plan_2d(w, [-hl, 0.0]), to_plan_2d(w, [hl, 0.0])], LineClass::Medium),
        PlanPrim::line(vec![to_plan_2d(w, [-hl, -120.0]), to_plan_2d(w, [-hl, 120.0])], LineClass::Medium),
        PlanPrim::line(vec![to_plan_2d(w, [hl, -120.0]), to_plan_2d(w, [hl, 120.0])], LineClass::Medium),
    ];
    let c = to_plan_2d(w, [-hl - 260.0, 0.0]);
    out.push(PlanPrim::Circle { center: c, radius: 190.0, fill: Some(FillClass::Body), line: Some(LineClass::Thin) });
    if own_labels(obj) {
        out.push(label(c, number.to_string(), LABEL_HEIGHT));
    }
    out
}

pub fn batten_mesh(obj: &Object, lod: Lod) -> Mesh {
    let ObjectKind::Batten { length, .. } = obj.kind else { return Mesh::empty() };
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Metal);
    let (a, b) = (DVec3::new(-length / 2.0, 0.0, 0.0), DVec3::new(length / 2.0, 0.0, 0.0));
    match lod {
        Lod::Full => m.tube(a, b, PIPE / 2.0, 12),
        Lod::Simple => m.tube(a, b, PIPE / 2.0, 6),
        Lod::Box => m.cuboid(a - DVec3::new(0.0, PIPE / 2.0, PIPE / 2.0), b + DVec3::new(0.0, PIPE / 2.0, PIPE / 2.0)),
    }
    m
}

// ---------------------------------------------------------------- platforms

/// Deck thickness of a platform, mm (its system's, else 100), never above its height.
pub fn deck_thickness(obj: &Object) -> f64 {
    let ObjectKind::Platform { height, system, .. } = &obj.kind else { return 0.0 };
    let deck = system.as_deref().and_then(crate::platform_system).map_or(100.0, |s| s.deck);
    deck.min(*height)
}

pub fn platform_plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Platform { width, depth, height, .. } = obj.kind else { return Vec::new() };
    let (hw, hd) = (width / 2.0, depth / 2.0);
    let corners = rect(w, -hw, -hd, hw, hd);
    let mut out =
        vec![PlanPrim::Polygon { points: corners.clone(), fill: FillClass::Deck, line: Some(LineClass::Medium) }];
    if style.detail == Detail::High {
        out.push(PlanPrim::line(vec![corners[0], corners[2]], LineClass::Thin));
        out.push(PlanPrim::line(vec![corners[1], corners[3]], LineClass::Thin));
    }
    if own_labels(obj) {
        let top = w.transform_point3(DVec3::new(0.0, 0.0, height)).z;
        let h = LABEL_HEIGHT.min(depth / 4.0);
        let c = to_plan_2d(w, [0.0, 0.0]);
        out.push(PlanPrim::Polygon {
            points: vec![
                [c[0] - h * 2.2, c[1] - h * 0.8],
                [c[0] + h * 2.2, c[1] - h * 0.8],
                [c[0] + h * 2.2, c[1] + h * 0.8],
                [c[0] - h * 2.2, c[1] + h * 0.8],
            ],
            fill: FillClass::Deck,
            line: None,
        });
        out.push(label(c, level_text(top), h));
    }
    out
}

/// A level in metres with its sign, as on drawings: `+1.40`, `±0.00`, `-0.75`.
pub fn level_text(mm: f64) -> String {
    let m = (mm / 10.0).round() / 100.0;
    if m == 0.0 { "±0.00".to_owned() } else { format!("{m:+.2}") }
}

pub fn platform_mesh(obj: &Object, lod: Lod) -> Mesh {
    let ObjectKind::Platform { width, depth, height, under, .. } = obj.kind else { return Mesh::empty() };
    let (hw, hd) = (width / 2.0, depth / 2.0);
    let deck = deck_thickness(obj);
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Deck);
    if lod == Lod::Box {
        m.cuboid(DVec3::new(-hw, -hd, 0.0), DVec3::new(hw, hd, height));
        return m;
    }
    m.cuboid(DVec3::new(-hw, -hd, height - deck), DVec3::new(hw, hd, height));
    let leg_top = height - deck;
    if leg_top < 1.0 {
        return m;
    }
    m.set_material(MaterialClass::Metal);
    match under {
        Understructure::None => {}
        Understructure::Legs => {
            let s = 60.0;
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let c = DVec3::new(sx * (hw - 40.0 - s / 2.0), sy * (hd - 40.0 - s / 2.0), 0.0);
                m.cuboid(c - DVec3::new(s / 2.0, s / 2.0, 0.0), c + DVec3::new(s / 2.0, s / 2.0, leg_top));
            }
        }
        Understructure::Scissor => {
            // Scissor frames on the two long sides, with floor rails.
            let long_x = width >= depth;
            let (half_long, half_short) = if long_x { (hw, hd) } else { (hd, hw) };
            let at = |along: f64, across: f64, z: f64| {
                if long_x { DVec3::new(along, across, z) } else { DVec3::new(across, along, z) }
            };
            let r = 20.0;
            let segs = if lod == Lod::Full { 8 } else { 4 };
            let a = half_long - 80.0;
            for side in [-1.0, 1.0] {
                let y = side * (half_short - 60.0);
                m.tube(at(-a, y, r), at(a, y, leg_top - r), r, segs);
                m.tube(at(a, y, r), at(-a, y, leg_top - r), r, segs);
                m.tube(at(-a, y, r), at(a, y, r), r, segs);
            }
        }
    }
    m
}

// ---------------------------------------------------------------- supports

pub fn support_plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Support { style, size, .. } = obj.kind else { return Vec::new() };
    let h = size / 2.0;
    let c = to_plan_2d(w, [0.0, 0.0]);
    match style {
        SupportStyle::BasePlate => vec![
            PlanPrim::Polygon { points: rect(w, -h, -h, h, h), fill: FillClass::Body, line: Some(LineClass::Medium) },
            PlanPrim::Circle { center: c, radius: 30.0, fill: None, line: Some(LineClass::Thin) },
        ],
        SupportStyle::Stand => {
            let mut out = vec![PlanPrim::Circle {
                center: c,
                radius: 40.0,
                fill: Some(FillClass::Body),
                line: Some(LineClass::Medium),
            }];
            for a in [90.0f64, 210.0, 330.0] {
                let r = a.to_radians();
                out.push(PlanPrim::line(vec![c, to_plan_2d(w, [h * r.cos(), h * r.sin()])], LineClass::Medium));
            }
            out
        }
        SupportStyle::GroundSupport => {
            let mut out = vec![PlanPrim::Polygon {
                points: rect(w, -175.0, -175.0, 175.0, 175.0),
                fill: FillClass::Body,
                line: Some(LineClass::Medium),
            }];
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                out.push(PlanPrim::line(
                    vec![to_plan_2d(w, [175.0 * dx, 175.0 * dy]), to_plan_2d(w, [h * dx, h * dy])],
                    LineClass::Medium,
                ));
                let p = [h * dx, h * dy];
                out.push(PlanPrim::Polyline {
                    points: rect(w, p[0] - 100.0, p[1] - 100.0, p[0] + 100.0, p[1] + 100.0),
                    closed: true,
                    line: LineClass::Thin,
                });
            }
            out
        }
    }
}

pub fn support_mesh(obj: &Object, lod: Lod) -> Mesh {
    let ObjectKind::Support { style, size, height, .. } = obj.kind else { return Mesh::empty() };
    let h = size / 2.0;
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Metal);
    let segs = if lod == Lod::Full { 10 } else { 5 };
    match style {
        SupportStyle::BasePlate => m.cuboid(DVec3::new(-h, -h, 0.0), DVec3::new(h, h, height)),
        SupportStyle::Stand => {
            if lod == Lod::Box {
                // The mast only: a box round the tripod would read as a solid block.
                m.cuboid(DVec3::new(-30.0, -30.0, 0.0), DVec3::new(30.0, 30.0, height));
                return m;
            }
            let knee = (height * 0.3).min(900.0);
            for a in [90.0f64, 210.0, 330.0] {
                let r = a.to_radians();
                m.tube(DVec3::new(0.0, 0.0, knee), DVec3::new(h * r.cos(), h * r.sin(), 15.0), 15.0, segs);
            }
            m.tube(DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 0.0, height - 30.0), 25.0, segs);
            m.cuboid(DVec3::new(-200.0, -30.0, height - 30.0), DVec3::new(200.0, 30.0, height));
        }
        SupportStyle::GroundSupport => {
            m.cuboid(DVec3::new(-300.0, -300.0, 0.0), DVec3::new(300.0, 300.0, 15.0));
            m.cuboid(DVec3::new(-h, -60.0, 15.0), DVec3::new(h, 60.0, 135.0));
            m.cuboid(DVec3::new(-60.0, -h, 15.0), DVec3::new(60.0, -60.0, 135.0));
            m.cuboid(DVec3::new(-60.0, 60.0, 15.0), DVec3::new(60.0, h, 135.0));
            m.cuboid(DVec3::new(-175.0, -175.0, 135.0), DVec3::new(175.0, 175.0, height));
        }
    }
    m
}

// ---------------------------------------------------------------- hoists

pub fn hoist_plan(w: &DAffine3, _obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let c = to_plan_2d(w, [0.0, 0.0]);
    let r = 160.0;
    let d = r * 0.7;
    vec![
        PlanPrim::Circle { center: c, radius: r, fill: Some(FillClass::Body), line: Some(LineClass::Medium) },
        PlanPrim::line(vec![[c[0] - d, c[1] - d], [c[0] + d, c[1] + d]], LineClass::Medium),
        PlanPrim::line(vec![[c[0] - d, c[1] + d], [c[0] + d, c[1] - d]], LineClass::Medium),
    ]
}

pub fn hoist_mesh(_obj: &Object, lod: Lod) -> Mesh {
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Metal);
    m.cuboid(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 120.0));
    m.set_material(MaterialClass::Fixture);
    m.cuboid(DVec3::new(-130.0, -130.0, 120.0), DVec3::new(130.0, 130.0, 520.0));
    m.set_material(MaterialClass::Metal);
    let segs = if lod == Lod::Full { 8 } else { 4 };
    m.tube(DVec3::new(0.0, 0.0, 520.0), DVec3::new(0.0, 0.0, 1500.0), 10.0, segs);
    m
}

// ---------------------------------------------------------------- ceilings

pub fn ceiling_plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Ceiling { outline, show_in_plan, .. } = &obj.kind else { return Vec::new() };
    if !show_in_plan {
        return Vec::new();
    }
    vec![PlanPrim::Polyline {
        points: outline.iter().map(|p| to_plan_2d(w, *p)).collect(),
        closed: true,
        line: LineClass::Hidden,
    }]
}

pub fn ceiling_mesh(obj: &Object) -> Mesh {
    let ObjectKind::Ceiling { outline, thickness, .. } = &obj.kind else { return Mesh::empty() };
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Wall);
    m.extrude(outline, 0.0, thickness.max(1.0));
    m
}

/// Where things attach on a support: the top of its mast or plate, local mm.
pub fn support_top(obj: &Object) -> f64 {
    match obj.kind {
        ObjectKind::Support { height, .. } => height,
        _ => 0.0,
    }
}

/// The local point on the floor under the origin, for snapping.
pub fn origin(w: &DAffine3) -> P2 {
    to_plan(w, [0.0; 3])
}
