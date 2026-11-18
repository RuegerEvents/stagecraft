//! Truss nodes and runs, load sums, per-object plan styles.

use glam::DVec3;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::create::{Common, insert, line_placement};
use super::{ensure_exists, id_of};
use crate::catalog;
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::geom::{local_for_world, world};
use crate::model::{Id, ObjectKind, P3, Placement};

pub(super) fn register(r: &mut Registry) {
    r.add("truss.node", Kind::Edit, "Create a truss corner or junction (2-, 3-, 4-way, 90° or 45°)", node);
    r.add(
        "truss.run",
        Kind::Edit,
        "Create a run of truss along points: corners at the bends, pieces in stock lengths, all in one run object",
        run,
    );
    r.add("truss.join", Kind::Edit, "Join truss pieces and nodes into one run object (they become its children)", join);
    r.add(
        "load.report",
        Kind::Query,
        "Weights per truss, run, batten and hoist: own weight plus everything attached (sums only)",
        load_report,
    );
    r.add(
        "object.set_style",
        Kind::Edit,
        "Override how objects draw in plan (line class, labels, detail, hidden)",
        set_style,
    );
}

/// Common node shapes.
#[derive(Clone, Copy, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
enum NodePreset {
    /// 2-way corner, arms at 0° and 90°.
    #[default]
    #[serde(rename = "corner_90")]
    Corner90,
    /// 2-way corner, arms 45° apart.
    #[serde(rename = "corner_45")]
    Corner45,
    /// 2-way corner, arms 135° apart (the run turns by 45°).
    #[serde(rename = "corner_135")]
    Corner135,
    /// 3-way T: arms at 0°, 90°, 180°.
    Tee,
    /// 3-way corner: arms at 0° and 90° and one down.
    #[serde(rename = "corner_3way")]
    Corner3way,
    /// 4-way cross: arms at 0°, 90°, 180°, 270°.
    Cross,
    /// 4-way T: arms at 0°, 90°, 180° and one down.
    #[serde(rename = "tee_4way")]
    Tee4way,
}

impl NodePreset {
    fn arms(self) -> (Vec<f64>, bool) {
        match self {
            NodePreset::Corner90 => (vec![0.0, 90.0], false),
            NodePreset::Corner45 => (vec![0.0, 45.0], false),
            NodePreset::Corner135 => (vec![0.0, 135.0], false),
            NodePreset::Tee => (vec![0.0, 90.0, 180.0], false),
            NodePreset::Corner3way => (vec![0.0, 90.0], true),
            NodePreset::Cross => (vec![0.0, 90.0, 180.0, 270.0], false),
            NodePreset::Tee4way => (vec![0.0, 90.0, 180.0], true),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct NodeArgs {
    #[serde(flatten)]
    common: Common,
    /// Node centre, mm.
    #[serde(default)]
    position: P3,
    /// Degrees about z.
    #[serde(default)]
    rotation: f64,
    /// Default F34.
    #[serde(default = "f34")]
    profile: String,
    /// Shape. Default corner_90. Ignored when `arms` is given.
    #[serde(default)]
    preset: NodePreset,
    /// Directions of horizontal arms, degrees from local +x (overrides the preset).
    arms: Option<Vec<f64>>,
    /// An arm pointing down. Default from the preset.
    down: Option<bool>,
    /// An arm pointing up. Default false.
    #[serde(default)]
    up: bool,
}

fn f34() -> String {
    "F34".into()
}

fn node(ctx: &mut Ctx, a: NodeArgs) -> CmdResult {
    let (preset_arms, preset_down) = a.preset.arms();
    let arms = a.arms.unwrap_or(preset_arms);
    let down = a.down.unwrap_or(preset_down);
    if arms.len() + usize::from(down) + usize::from(a.up) < 2 {
        return Err(CmdError::invalid_args("a node needs at least two arms"));
    }
    let p = Placement { position: a.position, rotation: [0.0, 0.0, a.rotation] };
    insert(ctx, a.common, "Truss node", p, ObjectKind::TrussNode { profile: a.profile, arms, down, up: a.up })
}

/// `total` mm in stock lengths, longest first; a rest above 1 mm becomes a custom piece.
pub fn split_length(total: f64, stock: &[f64]) -> Vec<f64> {
    let mut stock: Vec<f64> = stock.iter().copied().filter(|s| *s > 0.0).collect();
    stock.sort_by(|a, b| b.total_cmp(a));
    let mut out = Vec::new();
    let mut rest = total;
    while let Some(s) = stock.iter().copied().find(|s| *s <= rest + 0.5) {
        out.push(s);
        rest -= s;
    }
    if rest > 1.0 {
        out.push(rest);
    }
    out
}

#[derive(Deserialize, JsonSchema)]
struct RunArgs {
    #[serde(flatten)]
    common: Common,
    /// Corner points of the run's axis, mm, relative to the parent or layer; at least
    /// 2. Bends get corner nodes, open ends none.
    points: Vec<P3>,
    /// Default F34.
    #[serde(default = "f34")]
    profile: String,
    /// The last point joins the first (a closed rectangle of truss). Default false.
    #[serde(default)]
    closed: bool,
    /// Split straight stretches into the profile's stock lengths. Default true;
    /// false makes one piece per stretch.
    #[serde(default = "yes")]
    split: bool,
}

fn yes() -> bool {
    true
}

fn run(ctx: &mut Ctx, a: RunArgs) -> CmdResult {
    let mut pts: Vec<DVec3> = a.points.iter().map(|p| DVec3::from_array(*p)).collect();
    pts.dedup_by(|x, y| x.distance(*y) < 1.0);
    if a.closed && pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 1.0 {
        pts.pop();
    }
    if pts.len() < 2 || (a.closed && pts.len() < 3) {
        return Err(CmdError::invalid_args("a run needs at least 2 points (3 when closed)"));
    }
    let prof = catalog::truss_profile_or_fallback(&a.profile);
    let reach = prof.node_reach();
    let n = pts.len();
    let is_node = |i: usize| a.closed || (i > 0 && i < n - 1);
    let segs: Vec<(usize, usize)> =
        if a.closed { (0..n).map(|i| (i, (i + 1) % n)).collect() } else { (0..n - 1).map(|i| (i, i + 1)).collect() };
    for &(i, j) in &segs {
        let need = reach * (f64::from(u8::from(is_node(i))) + f64::from(u8::from(is_node(j))));
        if pts[i].distance(pts[j]) <= need + 1.0 {
            return Err(CmdError::invalid_args(format!(
                "stretch {i}–{j} is too short for its corners ({} mm needed)",
                need.round()
            )));
        }
    }
    let origin = pts[0];
    let common = a.common;
    let (layer, tags) = (common.layer.clone(), common.tags.clone());
    let p = Placement { position: origin.to_array(), ..Default::default() };
    let run_id = id_of(&insert(ctx, common, "Truss run", p, ObjectKind::TrussRun { profile: prof.name.clone() })?);
    let child =
        |name: String| Common { name: Some(name), layer: layer.clone(), tags: tags.clone(), parent: Some(run_id) };
    let plan_angle = |v: DVec3| v.y.atan2(v.x).to_degrees();
    let mut nodes = Vec::new();
    for i in (0..n).filter(|&i| is_node(i)) {
        let prev = pts[(i + n - 1) % n];
        let next = pts[(i + 1) % n];
        let arms = vec![plan_angle(prev - pts[i]), plan_angle(next - pts[i])];
        let pl = Placement { position: (pts[i] - origin).to_array(), ..Default::default() };
        let kind = ObjectKind::TrussNode { profile: prof.name.clone(), arms, down: false, up: false };
        nodes.push(id_of(&insert(ctx, child(format!("Corner {}", nodes.len() + 1)), "Corner", pl, kind)?));
    }
    let mut pieces = Vec::new();
    for &(i, j) in &segs {
        let dir = (pts[j] - pts[i]).normalize();
        let start = pts[i] + dir * if is_node(i) { reach } else { 0.0 };
        let end = pts[j] - dir * if is_node(j) { reach } else { 0.0 };
        let total = start.distance(end);
        let lengths = if a.split { split_length(total, &prof.stock_lengths) } else { vec![total] };
        let mut at = start;
        for l in lengths {
            let b = at + dir * l;
            let (pl, length) = line_placement((at - origin).to_array(), (b - origin).to_array());
            let kind = ObjectKind::Truss { length, profile: prof.name.clone() };
            let name = format!("{} {:.1} m", prof.name, length / 1000.0);
            pieces.push(id_of(&insert(ctx, child(name), "Truss", pl, kind)?));
            at = b;
        }
    }
    Ok(json!({ "id": run_id, "nodes": nodes, "pieces": pieces }))
}

#[derive(Deserialize, JsonSchema)]
struct JoinArgs {
    /// Truss pieces and nodes. They keep their place in the world.
    ids: Vec<Id>,
    name: Option<String>,
}

fn join(ctx: &mut Ctx, a: JoinArgs) -> CmdResult {
    let doc = ctx.doc();
    ensure_exists(doc, &a.ids)?;
    if a.ids.is_empty() {
        return Err(CmdError::invalid_args("nothing to join"));
    }
    let mut profile = None;
    for id in &a.ids {
        match &doc.objects[id].kind {
            ObjectKind::Truss { profile: p, .. } | ObjectKind::TrussNode { profile: p, .. } => {
                profile.get_or_insert_with(|| p.clone());
            }
            k => return Err(CmdError::invalid(format!("{id} is a {}, not truss", k.type_name()))),
        }
    }
    let first = &doc.objects[&a.ids[0]];
    let (layer, parent) = (first.layer, first.parent);
    let old_runs: Vec<Id> = a
        .ids
        .iter()
        .filter_map(|id| doc.objects[id].parent)
        .filter(|p| matches!(doc.objects.get(p).map(|o| &o.kind), Some(ObjectKind::TrussRun { .. })))
        .collect();
    // The run sits where the first piece starts, unrotated, beside it in the tree.
    let w0 = world(doc, a.ids[0]).expect("exists");
    let at = glam::DAffine3::from_translation(w0.translation);
    let parent = parent.filter(|p| !old_runs.contains(p));
    let placement = local_for_world(doc, &at, parent, layer);
    let run = crate::model::Object {
        id: uuid::Uuid::new_v4(),
        name: a.name.unwrap_or_else(|| "Truss run".into()),
        layer,
        tags: Vec::new(),
        parent,
        placement,
        kind: ObjectKind::TrussRun { profile: profile.unwrap_or_else(f34) },
        style: Default::default(),
        attrs: Default::default(),
        sketch: None,
    };
    let run_id = run.id;
    let doc = ctx.doc_mut();
    doc.objects.insert(run_id, run);
    for id in &a.ids {
        let w = world(doc, *id).expect("exists");
        let local = local_for_world(doc, &w, Some(run_id), layer);
        let o = doc.objects.get_mut(id).expect("exists");
        o.parent = Some(run_id);
        o.placement = local;
    }
    // Runs left without members go.
    let mut removed = Vec::new();
    for r in old_runs {
        if doc.children(r).is_empty() && doc.objects.remove(&r).is_some() {
            removed.push(r);
        }
    }
    Ok(json!({ "id": run_id, "removed_runs": removed }))
}

#[derive(Deserialize, JsonSchema)]
struct ReportArgs {
    /// Only these objects. Default: every truss, run, batten and hoist.
    #[serde(default)]
    ids: Vec<Id>,
}

fn load_report(ctx: &mut Ctx, a: ReportArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    Ok(json!(crate::load::report(ctx.doc(), &a.ids)))
}

#[derive(Deserialize, JsonSchema)]
struct StyleArgs {
    ids: Vec<Id>,
    /// thin, medium, thick, hidden or centre; empty string clears.
    line: Option<String>,
    /// true/false forces labels on/off.
    labels: Option<bool>,
    /// low or high; empty string clears.
    detail: Option<String>,
    /// Leave out of the plan.
    hidden: Option<bool>,
    /// Drop every override first.
    #[serde(default)]
    reset: bool,
}

fn set_style(ctx: &mut Ctx, a: StyleArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let pick = |v: &Option<String>, allowed: &[&str], what: &str| -> Result<Option<Option<String>>, CmdError> {
        match v.as_deref().map(str::trim) {
            None => Ok(None),
            Some("") => Ok(Some(None)),
            Some(s) if allowed.contains(&s) => Ok(Some(Some(s.to_owned()))),
            Some(s) => Err(CmdError::invalid_args(format!("{what} `{s}` is not one of {}", allowed.join(", ")))),
        }
    };
    let line = pick(&a.line, &["thin", "medium", "thick", "hidden", "centre"], "line")?;
    let detail = pick(&a.detail, &["low", "high"], "detail")?;
    let doc = ctx.doc_mut();
    for id in &a.ids {
        let s = &mut doc.objects.get_mut(id).expect("checked").style;
        if a.reset {
            *s = Default::default();
        }
        if let Some(l) = &line {
            s.line = l.clone();
        }
        if let Some(d) = &detail {
            s.detail = d.clone();
        }
        if let Some(l) = a.labels {
            s.labels = Some(l);
        }
        if let Some(h) = a.hidden {
            s.hidden = h;
        }
    }
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::split_length;

    #[test]
    fn runs_split_into_stock_lengths() {
        let stock = [3000.0, 2500.0, 2000.0, 1500.0, 1000.0, 500.0];
        assert_eq!(split_length(8000.0, &stock), vec![3000.0, 3000.0, 2000.0]);
        assert_eq!(split_length(3200.0, &stock), vec![3000.0, 200.0]);
        assert_eq!(split_length(400.0, &stock), vec![400.0]);
    }
}
