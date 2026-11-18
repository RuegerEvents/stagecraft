//! Plan annotations: underlays (pictures and DXF linework under the plan) and notes.
//! Neither has a mesh.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::DAffine3;
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Object, ObjectKind};
use stagecraft_underlay::Linework;

use super::to_plan_2d;
use crate::{LineClass, PlanPrim, PlanStyle, TextAnchor};

/// Parsed DXF linework per asset hash.
fn linework(doc: &Document, hash: &str) -> Option<Arc<Linework>> {
    static C: OnceLock<Mutex<HashMap<String, Option<Arc<Linework>>>>> = OnceLock::new();
    let cache = C.get_or_init(Default::default);
    if let Some(l) = cache.lock().ok()?.get(hash) {
        return l.clone();
    }
    let l = doc.assets.get(hash).and_then(|a| Linework::from_json(a.data.as_slice())).map(Arc::new);
    let mut c = cache.lock().ok()?;
    if c.len() > 32 {
        c.clear();
    }
    c.insert(hash.to_owned(), l.clone());
    l
}

/// The corners of an underlay's content in its local frame (mm): lower left, lower
/// right, upper right, upper left.
pub fn underlay_corners(obj: &Object) -> Option<[P2; 4]> {
    let ObjectKind::Underlay { extent: [lo, hi], scale, .. } = &obj.kind else { return None };
    let s = *scale;
    Some([[lo[0] * s, lo[1] * s], [hi[0] * s, lo[1] * s], [hi[0] * s, hi[1] * s], [lo[0] * s, hi[1] * s]])
}

pub fn underlay_plan(doc: &Document, w: &DAffine3, obj: &Object) -> Vec<PlanPrim> {
    let ObjectKind::Underlay { raster, linework: lines, scale, opacity, .. } = &obj.kind else { return Vec::new() };
    let corners = underlay_corners(obj).expect("underlay").map(|c| to_plan_2d(w, c));
    let mut out = Vec::new();
    if let Some(r) = raster {
        out.push(PlanPrim::Image { corners, asset: r.clone(), opacity: *opacity });
    }
    if let Some(l) = lines.as_deref().and_then(|h| linework(doc, h)) {
        let s = *scale;
        let at = |p: [f64; 2]| to_plan_2d(w, [p[0] * s, p[1] * s]);
        let turn = w.matrix3.x_axis.y.atan2(w.matrix3.x_axis.x).to_degrees();
        for p in &l.paths {
            out.push(PlanPrim::Polyline {
                points: p.points.iter().map(|q| at(*q)).collect(),
                closed: p.closed,
                line: LineClass::Thin,
            });
        }
        for t in &l.texts {
            // DXF texts sit on their baseline; plan text is placed by its middle.
            let r = (t.rotation).to_radians();
            let up = [-r.sin() * t.height / 2.0, r.cos() * t.height / 2.0];
            out.push(PlanPrim::Text {
                pos: at([t.pos[0] + up[0], t.pos[1] + up[1]]),
                text: t.text.clone(),
                height: t.height * s,
                rotation: t.rotation + turn,
                anchor: TextAnchor::Left,
            });
        }
    }
    out
}

/// Glyph width as a share of the cap height, for wrapping and frames (Roboto averages
/// about 0.55 of the em, the cap height is 0.71 of it).
const CHAR_WIDTH: f64 = 0.78;
/// Line pitch as a multiple of the cap height.
pub const LINE_PITCH: f64 = 1.7;

/// The note's lines after wrapping at `width` (mm; 0 = no wrapping).
pub fn note_lines(text: &str, height: f64, width: f64) -> Vec<String> {
    let max_chars = if width > 0.0 { ((width / (height * CHAR_WIDTH)).floor() as usize).max(8) } else { usize::MAX };
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max_chars {
                out.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push(line);
    }
    out
}

/// Size of a note's text block (local mm): width, height.
pub fn note_size(text: &str, height: f64, width: f64) -> [f64; 2] {
    let lines = note_lines(text, height, width);
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * height * CHAR_WIDTH;
    [w, lines.len() as f64 * height * LINE_PITCH]
}

pub fn note_plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Note { text, height, width, frame, leader } = &obj.kind else { return Vec::new() };
    let h = *height;
    let turn = w.matrix3.x_axis.y.atan2(w.matrix3.x_axis.x).to_degrees();
    let mut out = Vec::new();
    let lines = note_lines(text, h, *width);
    for (i, l) in lines.iter().enumerate() {
        if l.is_empty() {
            continue;
        }
        let y = -(i as f64 + 0.5) * h * LINE_PITCH;
        out.push(PlanPrim::Text {
            pos: to_plan_2d(w, [0.0, y]),
            text: l.clone(),
            height: h,
            rotation: turn,
            anchor: TextAnchor::Left,
        });
    }
    let [bw, bh] = note_size(text, h, *width);
    let pad = h * 0.6;
    let rect = [[-pad, pad], [bw + pad, pad], [bw + pad, -bh - pad * 0.5], [-pad, -bh - pad * 0.5]];
    if *frame {
        out.push(PlanPrim::Polyline {
            points: rect.iter().map(|p| to_plan_2d(w, *p)).collect(),
            closed: true,
            line: LineClass::Thin,
        });
    }
    if let Some(end) = leader {
        // from the nearest point of the box edge
        let start = [end[0].clamp(rect[0][0], rect[1][0]), end[1].clamp(rect[2][1], rect[0][1])];
        let (a, b) = (to_plan_2d(w, start), to_plan_2d(w, *end));
        out.push(PlanPrim::line(vec![a, b], LineClass::Thin));
        out.push(PlanPrim::Circle { center: b, radius: h * 0.25, fill: Some(crate::FillClass::Masking), line: None });
    }
    out
}

/// Snap points of an underlay: its corners, and the DXF's path points.
pub fn underlay_snaps(doc: &Document, w: &DAffine3, obj: &Object) -> (Vec<P2>, Vec<[P2; 2]>) {
    let ObjectKind::Underlay { linework: lines, scale, .. } = &obj.kind else { return Default::default() };
    let corners: Vec<P2> = underlay_corners(obj).expect("underlay").iter().map(|c| to_plan_2d(w, *c)).collect();
    let mut pts = corners.clone();
    let mut segs: Vec<[P2; 2]> = (0..4).map(|i| [corners[i], corners[(i + 1) % 4]]).collect();
    if let Some(l) = lines.as_deref().and_then(|h| linework(doc, h)) {
        let s = *scale;
        for p in &l.paths {
            let v: Vec<P2> = p.points.iter().map(|q| to_plan_2d(w, [q[0] * s, q[1] * s])).collect();
            segs.extend(v.windows(2).map(|x| [x[0], x[1]]));
            pts.extend(v);
        }
    }
    (pts, segs)
}
