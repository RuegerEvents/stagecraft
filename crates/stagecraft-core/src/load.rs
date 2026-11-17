//! Load sums: what each object weighs and what hangs on it. Sums only; how a load
//! splits between several hoists under one truss is not worked out here.
//!
//! Own weight comes from the catalogue (truss kg/m, node kg, platform kg/m²), from the
//! object (fixture `weight_kg`, hoist, support, batten kg/m) or from fabric weights
//! per m² of cloth. Everything attached counts towards its parent, all the way up.

use serde::Serialize;

use crate::catalog;
use crate::model::{Document, Id, Object, ObjectKind};

/// Stage velour for curtains, kg per m² of cloth.
pub const VELOUR_KG_M2: f64 = 0.55;
/// Molton for legs and borders, kg per m² of cloth.
pub const MOLTON_KG_M2: f64 = 0.32;

/// The object's own weight in kg (nothing attached). `None` for fixtures without a
/// weight yet.
pub fn own_weight(o: &Object) -> Option<f64> {
    let m = |mm: f64| mm / 1000.0;
    Some(match &o.kind {
        ObjectKind::Truss { length, profile } => catalog::truss_profile_or_fallback(profile).kg_per_m * m(*length),
        ObjectKind::TrussNode { profile, .. } => catalog::truss_profile_or_fallback(profile).node_weight(),
        ObjectKind::Fixture { weight_kg, .. } => return *weight_kg,
        ObjectKind::Batten { length, kg_per_m, .. } => kg_per_m * m(*length),
        ObjectKind::Curtain { width, height, fullness, .. } => {
            m(*width) * (1.0 + fullness / 100.0) * m(*height) * VELOUR_KG_M2
        }
        ObjectKind::Masking { width, height, fullness, .. } => {
            m(*width) * (1.0 + fullness / 100.0) * m(*height) * MOLTON_KG_M2
        }
        ObjectKind::Platform { width, depth, system, .. } => {
            let per = system
                .as_deref()
                .and_then(catalog::platform_system)
                .or_else(|| catalog::platform_system("Generic"))
                .map_or(25.0, |s| s.kg_per_m2);
            m(*width) * m(*depth) * per
        }
        ObjectKind::Support { weight_kg, .. } | ObjectKind::Hoist { weight_kg, .. } => *weight_kg,
        _ => 0.0,
    })
}

/// Weight of one object and everything below it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Load {
    /// The object itself, kg.
    pub own_kg: f64,
    /// Everything attached to it (children, their children …), kg.
    pub attached_kg: f64,
    /// Fixtures in the subtree without a weight; the sums leave them out.
    pub unknown: usize,
}

impl Load {
    pub fn total_kg(&self) -> f64 {
        self.own_kg + self.attached_kg
    }
}

/// The load of `id` and its subtree.
pub fn load_of(doc: &Document, id: Id) -> Load {
    let mut l = Load::default();
    for (i, d) in doc.subtree(id).into_iter().enumerate() {
        let Some(o) = doc.objects.get(&d) else { continue };
        let w = own_weight(o);
        if w.is_none() {
            l.unknown += 1;
        }
        let w = w.unwrap_or(0.0);
        if i == 0 {
            l.own_kg = w;
        } else {
            l.attached_kg += w;
        }
    }
    l
}

/// For a run: the truss in it (pieces and nodes) counts as own weight.
fn run_load(doc: &Document, id: Id) -> Load {
    let mut l = Load::default();
    for d in doc.subtree(id).into_iter().skip(1) {
        let Some(o) = doc.objects.get(&d) else { continue };
        let w = own_weight(o);
        if w.is_none() {
            l.unknown += 1;
        }
        let truss_of_run = matches!(o.kind, ObjectKind::Truss { .. } | ObjectKind::TrussNode { .. })
            && doc.objects.get(&o.parent.unwrap_or_default()).is_some_and(|p| p.id == id);
        if truss_of_run {
            l.own_kg += w.unwrap_or(0.0);
        } else {
            l.attached_kg += w.unwrap_or(0.0);
        }
    }
    l
}

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Batten number, for battens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    pub own_kg: f64,
    pub attached_kg: f64,
    pub total_kg: f64,
    pub unknown_weights: usize,
    /// Batten max load or hoist capacity, kg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_kg: Option<f64>,
    /// The payload (attached) is above the limit.
    pub over: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub trusses: Vec<Row>,
    pub runs: Vec<Row>,
    pub battens: Vec<Row>,
    pub hoists: Vec<Row>,
    /// Everything suspended: subtrees of battens and hoists, plus truss and runs that
    /// hang on neither, kg.
    pub suspended_kg: f64,
}

fn row(o: &Object, l: Load, limit: Option<f64>) -> Row {
    let r = |x: f64| (x * 10.0).round() / 10.0;
    Row {
        id: o.id,
        name: o.name.clone(),
        kind: o.kind.type_name(),
        number: match o.kind {
            ObjectKind::Batten { number, .. } => Some(number),
            _ => None,
        },
        own_kg: r(l.own_kg),
        attached_kg: r(l.attached_kg),
        total_kg: r(l.total_kg()),
        unknown_weights: l.unknown,
        limit_kg: limit,
        over: limit.is_some_and(|m| l.attached_kg > m + 1e-9),
    }
}

/// Weight per truss piece, per run, per batten and per hoist. `only` limits the rows
/// to those objects (an empty slice means all).
pub fn report(doc: &Document, only: &[Id]) -> Report {
    let mut rep = Report::default();
    let wanted = |id: &Id| only.is_empty() || only.contains(id);
    for o in doc.objects.values().filter(|o| wanted(&o.id)) {
        match &o.kind {
            ObjectKind::Truss { .. } => rep.trusses.push(row(o, load_of(doc, o.id), None)),
            ObjectKind::TrussRun { .. } => rep.runs.push(row(o, run_load(doc, o.id), None)),
            ObjectKind::Batten { max_load, .. } => rep.battens.push(row(o, load_of(doc, o.id), Some(*max_load))),
            ObjectKind::Hoist { capacity_kg, .. } => {
                rep.hoists.push(row(o, load_of(doc, o.id), Some(*capacity_kg)));
            }
            _ => {}
        }
    }
    rep.battens.sort_by_key(|r| r.number);
    // Suspended: tops of hanging things, counted once.
    let hangs = |o: &Object| {
        matches!(
            o.kind,
            ObjectKind::Batten { .. }
                | ObjectKind::Hoist { .. }
                | ObjectKind::TrussRun { .. }
                | ObjectKind::Truss { .. }
        )
    };
    let mut total = 0.0;
    for o in doc.objects.values().filter(|o| hangs(o)) {
        let above_hangs = {
            let mut cur = o.parent;
            let mut found = false;
            while let Some(p) = cur.and_then(|p| doc.objects.get(&p)) {
                // Under something that hangs (counted there) or standing on a support.
                if hangs(p) || matches!(p.kind, ObjectKind::Support { .. }) {
                    found = true;
                    break;
                }
                cur = p.parent;
            }
            found
        };
        if !above_hangs {
            total += load_of(doc, o.id).total_kg();
        }
    }
    rep.suspended_kg = (total * 10.0).round() / 10.0;
    rep
}
