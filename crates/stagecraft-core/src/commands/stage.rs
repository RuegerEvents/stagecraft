//! Stage objects: proscenium, curtains, wing sets and masking, battens, platforms,
//! supports, hoists, ceilings.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::create::{Common, insert, kind_mut, positive};
use super::object;
use crate::catalog;
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::geom::{local_for_world, world};
use crate::model::{
    Document, Gather, Id, MaskingRole, ObjectKind, P2, P3, Placement, SupportStyle, Understructure, parse_color,
};

pub(super) fn register(r: &mut Registry) {
    r.add("proscenium.create", Kind::Edit, "Create a proscenium (portal) with its opening and frame", proscenium);
    r.add(
        "proscenium.update",
        Kind::Edit,
        "Change opening, depth, frame or iron line of a proscenium",
        proscenium_update,
    );
    r.add(
        "curtain.create",
        Kind::Edit,
        "Create a curtain (main curtain, drape); hangs on a batten if given as parent",
        curtain,
    );
    r.add(
        "curtain.update",
        Kind::Edit,
        "Change size, fullness, opening, gather style or colour of a curtain",
        curtain_update,
    );
    r.add("wingset.create", Kind::Edit, "Generate legs and borders for several wing depths in one go", wingset);
    r.add("masking.update", Kind::Edit, "Change size, fullness or colour of one leg or border", masking_update);
    r.add(
        "batten.create",
        Kind::Edit,
        "Create a fly bar (Zug) at a position from the setting line and a trim height",
        batten,
    );
    r.add("batten.update", Kind::Edit, "Change number, position, trim, length or limits of a batten", batten_update);
    r.add(
        "batten.list",
        Kind::Query,
        "The fly bar list (Zug-Liste): battens by number with position, trim and load",
        batten_list,
    );
    r.add(
        "platform.create",
        Kind::Edit,
        "Create a stage platform (Podest), free size or a preset, optionally from a system",
        platform,
    );
    r.add(
        "platform.update",
        Kind::Edit,
        "Change size, height, understructure or system of a platform",
        platform_update,
    );
    r.add("support.create", Kind::Edit, "Create a base plate, stand or ground support base", support);
    r.add("support.update", Kind::Edit, "Change a base plate, stand or ground support base", support_update);
    r.add("hoist.create", Kind::Edit, "Create a chain hoist or rigging point", hoist);
    r.add("hoist.update", Kind::Edit, "Change capacity or weight of a hoist", hoist_update);
    r.add("ceiling.create", Kind::Edit, "Create a ceiling: a closed outline at a height", ceiling);
    r.add(
        "ceiling.update",
        Kind::Edit,
        "Change outline, height, thickness or plan visibility of a ceiling",
        ceiling_update,
    );
}

fn color(c: Option<String>, default: &str) -> Result<String, CmdError> {
    let c = c.unwrap_or_else(|| default.to_owned());
    match parse_color(&c) {
        Some(_) => Ok(c.trim().to_lowercase()),
        None => Err(CmdError::invalid_args(format!("colour `{c}` is not #RRGGBB"))),
    }
}

fn opt_positive(fields: &[(&str, Option<f64>)]) -> Result<(), CmdError> {
    for (what, v) in fields {
        if let Some(v) = v {
            positive(what, *v)?;
        }
    }
    Ok(())
}

fn non_negative(what: &str, v: f64) -> Result<(), CmdError> {
    if v >= 0.0 && v.is_finite() { Ok(()) } else { Err(CmdError::invalid_args(format!("{what} must be zero or more"))) }
}

fn set<T>(slot: &mut T, v: Option<T>) {
    if let Some(v) = v {
        *slot = v;
    }
}

// ---------------------------------------------------------------- proscenium

#[derive(Deserialize, JsonSchema)]
struct ProsceniumArgs {
    #[serde(flatten)]
    common: Common,
    /// Centre of the opening on the floor at the downstage face, mm. Default the
    /// origin (centre line on the setting line).
    #[serde(default)]
    position: P3,
    /// Degrees about z. Default 0 (opening facing the audience at −y).
    #[serde(default)]
    rotation: f64,
    /// Clear opening width, mm. Default 10000.
    #[serde(default = "d10000")]
    width: f64,
    /// Clear opening height, mm. Default 6000.
    #[serde(default = "d6000")]
    height: f64,
    /// Depth of the frame upstage, mm. Default 600.
    #[serde(default = "d600")]
    depth: f64,
    /// Width of the side pillars and height of the header above the opening, mm.
    /// Default 1000.
    #[serde(default = "d1000")]
    frame: f64,
    /// y of the iron (fire curtain) line from the downstage face, mm. Given = shown.
    /// Default: no iron line.
    iron: Option<f64>,
    /// Cut the opening's upper corners at 45° by this much, mm. Default 0 (square).
    #[serde(default)]
    chamfer: f64,
    /// Cut the frame's outer upper corners at 45° by this much, mm. Default 0.
    #[serde(default)]
    outer_chamfer: f64,
    /// Keep-clear strip under the iron (safety curtain), centred on its line, mm.
    /// Default 0 (none).
    #[serde(default)]
    keep_clear: f64,
}

fn d10000() -> f64 {
    10000.0
}
fn d6000() -> f64 {
    6000.0
}
fn d600() -> f64 {
    600.0
}
fn d1000() -> f64 {
    1000.0
}

fn proscenium(ctx: &mut Ctx, a: ProsceniumArgs) -> CmdResult {
    for (w, v) in [("width", a.width), ("height", a.height), ("depth", a.depth), ("frame", a.frame)] {
        positive(w, v)?;
    }
    non_negative("chamfer", a.chamfer)?;
    non_negative("outer_chamfer", a.outer_chamfer)?;
    non_negative("keep_clear", a.keep_clear)?;
    let p = Placement { position: a.position, rotation: [0.0, 0.0, a.rotation] };
    let kind = ObjectKind::Proscenium {
        width: a.width,
        height: a.height,
        depth: a.depth,
        frame: a.frame,
        iron: a.iron.unwrap_or(a.depth + 150.0),
        show_iron: a.iron.is_some(),
        chamfer: a.chamfer,
        outer_chamfer: a.outer_chamfer,
        keep_clear: a.keep_clear,
    };
    insert(ctx, a.common, "Proscenium", p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct ProsceniumUpdateArgs {
    id: Id,
    /// mm.
    width: Option<f64>,
    /// mm.
    height: Option<f64>,
    /// mm.
    depth: Option<f64>,
    /// mm.
    frame: Option<f64>,
    /// y of the iron line from the downstage face, mm (also shows it).
    iron: Option<f64>,
    /// Show or hide the iron line.
    show_iron: Option<bool>,
    /// 45° cut of the opening's upper corners, mm (0 = square).
    chamfer: Option<f64>,
    /// 45° cut of the frame's outer upper corners, mm (0 = square).
    outer_chamfer: Option<f64>,
    /// Keep-clear strip under the iron, mm (0 = none).
    keep_clear: Option<f64>,
}

fn proscenium_update(ctx: &mut Ctx, a: ProsceniumUpdateArgs) -> CmdResult {
    opt_positive(&[("width", a.width), ("height", a.height), ("depth", a.depth), ("frame", a.frame)])?;
    for (what, v) in [("chamfer", a.chamfer), ("outer_chamfer", a.outer_chamfer), ("keep_clear", a.keep_clear)] {
        if let Some(v) = v {
            non_negative(what, v)?;
        }
    }
    let o = kind_mut(ctx, a.id, "proscenium")?;
    let ObjectKind::Proscenium { width, height, depth, frame, iron, show_iron, chamfer, outer_chamfer, keep_clear } =
        &mut o.kind
    else {
        unreachable!()
    };
    set(keep_clear, a.keep_clear);
    set(chamfer, a.chamfer);
    set(outer_chamfer, a.outer_chamfer);
    set(width, a.width);
    set(height, a.height);
    set(depth, a.depth);
    set(frame, a.frame);
    if let Some(v) = a.iron {
        *iron = v;
        *show_iron = true;
    }
    set(show_iron, a.show_iron);
    // A portal, batten, curtain or truss piece that follows a sketch line keeps the
    // line's length and position.
    crate::sketch::sync_objects(ctx.doc_mut(), &[a.id]);
    Ok(json!(ctx.doc().objects[&a.id].kind))
}

// ---------------------------------------------------------------- curtains

/// Shortcut for the opening.
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum CurtainState {
    /// Opening 0.
    Closed,
    /// Opening 100, gather style unchanged.
    Open,
    /// Opening 100, Italian (bunched up).
    Gathered,
}

#[derive(Deserialize, JsonSchema)]
struct CurtainArgs {
    #[serde(flatten)]
    common: Common,
    /// Centre of the top edge, mm, relative to the parent (a batten: [0,0,0] hangs it
    /// centred on the pipe) or the layer.
    #[serde(default)]
    position: P3,
    /// Degrees about z. Default 0: the curtain runs along x.
    #[serde(default)]
    rotation: f64,
    /// mm. Default 12000.
    #[serde(default = "d12000")]
    width: f64,
    /// mm, top edge to hem. Default 7000.
    #[serde(default = "d7000")]
    height: f64,
    /// Extra fabric in percent of the width. Default 100 (fabric twice the width).
    #[serde(default = "d100")]
    fullness: f64,
    /// Two panels meeting in the middle. Default true.
    #[serde(default = "yes")]
    split: bool,
    /// 0 closed … 100 fully open. Default 0.
    #[serde(default)]
    opening: f64,
    /// tab (sideways, also `greek`) or italian (bunched up). Default tab.
    #[serde(default)]
    gather: Gather,
    /// Sets opening (and gather for `gathered`); overrides `opening`.
    state: Option<CurtainState>,
    /// `#RRGGBB`. Default #8b1a1a (theatre red).
    color: Option<String>,
}

fn d12000() -> f64 {
    12000.0
}
fn d7000() -> f64 {
    7000.0
}
fn d100() -> f64 {
    100.0
}
fn yes() -> bool {
    true
}

fn apply_state(state: Option<CurtainState>, opening: &mut f64, gather: &mut Gather) {
    match state {
        Some(CurtainState::Closed) => *opening = 0.0,
        Some(CurtainState::Open) => *opening = 100.0,
        Some(CurtainState::Gathered) => {
            *opening = 100.0;
            *gather = Gather::Italian;
        }
        None => {}
    }
}

fn check_curtain(fullness: f64, opening: f64) -> Result<(), CmdError> {
    if !(0.0..=400.0).contains(&fullness) {
        return Err(CmdError::invalid_args("fullness must be 0–400 %"));
    }
    if !(0.0..=100.0).contains(&opening) {
        return Err(CmdError::invalid_args("opening must be 0–100 %"));
    }
    Ok(())
}

fn curtain(ctx: &mut Ctx, a: CurtainArgs) -> CmdResult {
    positive("width", a.width)?;
    positive("height", a.height)?;
    let (mut opening, mut gather) = (a.opening, a.gather);
    apply_state(a.state, &mut opening, &mut gather);
    check_curtain(a.fullness, opening)?;
    let kind = ObjectKind::Curtain {
        width: a.width,
        height: a.height,
        fullness: a.fullness,
        split: a.split,
        opening,
        gather,
        color: color(a.color, "#8b1a1a")?,
    };
    let p = Placement { position: a.position, rotation: [0.0, 0.0, a.rotation] };
    insert(ctx, a.common, "Curtain", p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct CurtainUpdateArgs {
    id: Id,
    /// mm.
    width: Option<f64>,
    /// mm.
    height: Option<f64>,
    /// Percent extra fabric.
    fullness: Option<f64>,
    split: Option<bool>,
    /// 0 closed … 100 open.
    opening: Option<f64>,
    gather: Option<Gather>,
    state: Option<CurtainState>,
    /// `#RRGGBB`.
    color: Option<String>,
}

fn curtain_update(ctx: &mut Ctx, a: CurtainUpdateArgs) -> CmdResult {
    opt_positive(&[("width", a.width), ("height", a.height)])?;
    let new_color = a.color.map(|c| color(Some(c), "")).transpose()?;
    let o = kind_mut(ctx, a.id, "curtain")?;
    let ObjectKind::Curtain { width, height, fullness, split, opening, gather, color } = &mut o.kind else {
        unreachable!()
    };
    let (mut op, mut ga, mut fu) = (*opening, *gather, *fullness);
    set(&mut op, a.opening);
    set(&mut ga, a.gather);
    set(&mut fu, a.fullness);
    apply_state(a.state, &mut op, &mut ga);
    check_curtain(fu, op)?;
    (*opening, *gather, *fullness) = (op, ga, fu);
    set(width, a.width);
    set(height, a.height);
    set(split, a.split);
    set(color, new_color);
    // A portal, batten, curtain or truss piece that follows a sketch line keeps the
    // line's length and position.
    crate::sketch::sync_objects(ctx.doc_mut(), &[a.id]);
    Ok(json!(ctx.doc().objects[&a.id].kind))
}

// ---------------------------------------------------------------- wing sets

#[derive(Deserialize, JsonSchema)]
struct WingSetArgs {
    #[serde(flatten)]
    common: Common,
    /// Centre line x, mm. Default 0.
    #[serde(default)]
    x: f64,
    /// y of the first wing (from the setting line), mm. Default 1500.
    #[serde(default = "d1500")]
    y: f64,
    /// Number of wings (depths). Default 3.
    #[serde(default = "three")]
    count: u32,
    /// Distance between wings along y, mm. Default 2000.
    #[serde(default = "d2000")]
    spacing: f64,
    /// Clear width between the legs, mm. Default 10000.
    #[serde(default = "d10000")]
    opening: f64,
    /// Width of each leg, mm. Default 1500.
    #[serde(default = "d1500")]
    leg_width: f64,
    /// Leg height (top edge above the floor), mm. Default 7000.
    #[serde(default = "d7000")]
    height: f64,
    /// Drop of each border, mm. Default 1500.
    #[serde(default = "d1500")]
    border_height: f64,
    /// Height of the borders' bottom edge above the floor, mm. Default: 500 below the
    /// top of the legs.
    border_trim: Option<f64>,
    /// Percent extra fabric for legs and borders. Default 0 (flat).
    #[serde(default)]
    fullness: f64,
    /// `#RRGGBB`. Default #1a1a1a (black molton).
    color: Option<String>,
    /// Make borders (one per wing above the opening). Default true; false for legs only.
    #[serde(default = "yes")]
    borders: bool,
    /// Hang each border on the batten nearest its wing (within 400 mm in y) instead of
    /// keeping it in the set. Default true.
    #[serde(default = "yes")]
    hang_borders: bool,
}

fn d1500() -> f64 {
    1500.0
}
fn d2000() -> f64 {
    2000.0
}
fn three() -> u32 {
    3
}

/// The batten whose world y is nearest `y` within `tol` mm.
fn batten_near(doc: &Document, y: f64, tol: f64) -> Option<Id> {
    doc.objects
        .values()
        .filter(|o| matches!(o.kind, ObjectKind::Batten { .. }))
        .filter_map(|o| Some((o.id, (world(doc, o.id)?.translation.y - y).abs())))
        .filter(|(_, d)| *d <= tol)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|x| x.0)
}

fn wingset(ctx: &mut Ctx, a: WingSetArgs) -> CmdResult {
    for (w, v) in [
        ("opening", a.opening),
        ("leg_width", a.leg_width),
        ("height", a.height),
        ("border_height", a.border_height),
        ("spacing", a.spacing),
    ] {
        positive(w, v)?;
    }
    if a.count == 0 || a.count > 20 {
        return Err(CmdError::invalid_args("count must be 1–20"));
    }
    check_curtain(a.fullness, 0.0)?;
    let colour = color(a.color, "#1a1a1a")?;
    let trim = a.border_trim.unwrap_or(a.height - 500.0);
    let layer = a.common.layer.clone();
    let tags = a.common.tags.clone();
    let p = Placement { position: [a.x, a.y, 0.0], ..Default::default() };
    let kind = ObjectKind::WingSet {
        count: a.count,
        spacing: a.spacing,
        opening: a.opening,
        leg_width: a.leg_width,
        height: a.height,
        border_height: a.border_height,
        color: colour.clone(),
    };
    let set_id = super::id_of(&insert(ctx, a.common, "Wing set", p, kind)?);
    let mut legs = Vec::new();
    let mut borders = Vec::new();
    let leg_x = a.opening / 2.0 + a.leg_width / 2.0;
    let border_width = a.opening + 2.0 * a.leg_width;
    for i in 0..a.count {
        let y = i as f64 * a.spacing;
        let n = i + 1;
        for (side, sx) in [("left", -leg_x), ("right", leg_x)] {
            let c = Common {
                name: Some(format!("Leg {n} {side}")),
                layer: layer.clone(),
                tags: tags.clone(),
                parent: Some(set_id),
            };
            let kind = ObjectKind::Masking {
                role: MaskingRole::Leg,
                width: a.leg_width,
                height: a.height,
                fullness: a.fullness,
                color: colour.clone(),
            };
            let pl = Placement { position: [sx, y, a.height], ..Default::default() };
            legs.push(super::id_of(&insert(ctx, c, "Leg", pl, kind)?));
        }
        if !a.borders {
            continue;
        }
        let c = Common {
            name: Some(format!("Border {n}")),
            layer: layer.clone(),
            tags: tags.clone(),
            parent: Some(set_id),
        };
        let kind = ObjectKind::Masking {
            role: MaskingRole::Border,
            width: border_width,
            height: a.border_height,
            fullness: a.fullness,
            color: colour.clone(),
        };
        let pl = Placement { position: [0.0, y, trim + a.border_height], ..Default::default() };
        borders.push(super::id_of(&insert(ctx, c, "Border", pl, kind)?));
    }
    let mut hung = Vec::new();
    if a.hang_borders {
        for &b in &borders {
            let doc = ctx.doc();
            let w = world(doc, b).expect("just made");
            let Some(batten) = batten_near(doc, w.translation.y, 400.0) else { continue };
            let layer = doc.objects[&b].layer;
            let local = local_for_world(doc, &w, Some(batten), layer);
            let o = ctx.doc_mut().objects.get_mut(&b).expect("just made");
            o.parent = Some(batten);
            o.placement = local;
            hung.push(json!({ "border": b, "batten": batten }));
        }
    }
    Ok(json!({ "id": set_id, "legs": legs, "borders": borders, "hung": hung }))
}

#[derive(Deserialize, JsonSchema)]
struct MaskingUpdateArgs {
    id: Id,
    /// mm.
    width: Option<f64>,
    /// mm.
    height: Option<f64>,
    /// Percent extra fabric.
    fullness: Option<f64>,
    /// `#RRGGBB`.
    color: Option<String>,
    role: Option<MaskingRole>,
}

fn masking_update(ctx: &mut Ctx, a: MaskingUpdateArgs) -> CmdResult {
    opt_positive(&[("width", a.width), ("height", a.height)])?;
    if let Some(f) = a.fullness {
        check_curtain(f, 0.0)?;
    }
    let new_color = a.color.map(|c| color(Some(c), "")).transpose()?;
    let o = kind_mut(ctx, a.id, "masking")?;
    let ObjectKind::Masking { role, width, height, fullness, color } = &mut o.kind else { unreachable!() };
    set(width, a.width);
    set(height, a.height);
    set(fullness, a.fullness);
    set(color, new_color);
    set(role, a.role);
    Ok(json!(o.kind))
}

// ---------------------------------------------------------------- battens

#[derive(Deserialize, JsonSchema)]
struct BattenArgs {
    #[serde(flatten)]
    common: Common,
    /// Batten number (Zug). Default: one more than the highest. Unique.
    number: Option<u32>,
    /// Distance from the setting line upstage, mm. Default 0.
    #[serde(default)]
    y: f64,
    /// Centre of the pipe across the stage, mm. Default 0.
    #[serde(default)]
    x: f64,
    /// Trim height of the pipe above the layer, mm. Default 8000.
    #[serde(default = "d8000")]
    trim: f64,
    /// mm. Default 14000.
    #[serde(default = "d14000")]
    length: f64,
    /// Allowed payload, kg. Default 500.
    #[serde(default = "d500")]
    max_load: f64,
    /// Own weight of the bar, kg/m. Default 4.5 (Ø48.3 pipe with fittings).
    #[serde(default = "d4_5")]
    kg_per_m: f64,
}

fn d8000() -> f64 {
    8000.0
}
fn d14000() -> f64 {
    14000.0
}
fn d500() -> f64 {
    500.0
}
fn d4_5() -> f64 {
    4.5
}

fn batten_numbers(doc: &Document, except: Option<Id>) -> Vec<(u32, Id)> {
    doc.objects
        .values()
        .filter(|o| Some(o.id) != except)
        .filter_map(|o| match o.kind {
            ObjectKind::Batten { number, .. } => Some((number, o.id)),
            _ => None,
        })
        .collect()
}

fn check_number(doc: &Document, n: u32, except: Option<Id>) -> Result<(), CmdError> {
    match batten_numbers(doc, except).into_iter().find(|(m, _)| *m == n) {
        Some((_, id)) => Err(CmdError::invalid(format!("batten number {n} is already used by {id}"))),
        None => Ok(()),
    }
}

fn batten(ctx: &mut Ctx, a: BattenArgs) -> CmdResult {
    positive("length", a.length)?;
    non_negative("max_load", a.max_load)?;
    non_negative("kg_per_m", a.kg_per_m)?;
    let number = match a.number {
        Some(n) => {
            check_number(ctx.doc(), n, None)?;
            n
        }
        None => batten_numbers(ctx.doc(), None).iter().map(|x| x.0).max().unwrap_or(0) + 1,
    };
    let p = Placement { position: [a.x, a.y, a.trim], ..Default::default() };
    let kind = ObjectKind::Batten { number, length: a.length, max_load: a.max_load, kg_per_m: a.kg_per_m };
    let mut out = insert(ctx, a.common, &format!("Batten {number}"), p, kind)?;
    out["number"] = json!(number);
    Ok(out)
}

#[derive(Deserialize, JsonSchema)]
struct BattenUpdateArgs {
    id: Id,
    number: Option<u32>,
    /// Distance from the setting line, mm (the placement's y).
    y: Option<f64>,
    /// Trim height, mm (the placement's z). Attached curtains, borders and truss move
    /// with it.
    trim: Option<f64>,
    /// mm.
    length: Option<f64>,
    /// kg.
    max_load: Option<f64>,
    /// kg/m.
    kg_per_m: Option<f64>,
}

fn batten_update(ctx: &mut Ctx, a: BattenUpdateArgs) -> CmdResult {
    opt_positive(&[("length", a.length)])?;
    if let Some(n) = a.number {
        check_number(ctx.doc(), n, Some(a.id))?;
    }
    for (w, v) in [("max_load", a.max_load), ("kg_per_m", a.kg_per_m)] {
        if let Some(v) = v {
            non_negative(w, v)?;
        }
    }
    let o = kind_mut(ctx, a.id, "batten")?;
    set(&mut o.placement.position[1], a.y);
    set(&mut o.placement.position[2], a.trim);
    let ObjectKind::Batten { number, length, max_load, kg_per_m } = &mut o.kind else { unreachable!() };
    set(number, a.number);
    set(length, a.length);
    set(max_load, a.max_load);
    set(kg_per_m, a.kg_per_m);
    // A portal, batten, curtain or truss piece that follows a sketch line keeps the
    // line's length and position.
    crate::sketch::sync_objects(ctx.doc_mut(), &[a.id]);
    let o = &ctx.doc().objects[&a.id];
    Ok(json!({ "kind": o.kind, "placement": o.placement }))
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn batten_list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    let rep = crate::load::report(doc, &[]);
    let rows: Vec<Value> = rep
        .battens
        .iter()
        .map(|r| {
            let o = &doc.objects[&r.id];
            let ObjectKind::Batten { length, .. } = o.kind else { unreachable!() };
            let attached: Vec<String> = doc.children(o.id).iter().map(|c| doc.objects[c].name.clone()).collect();
            json!({
                "id": r.id, "number": r.number, "name": r.name,
                "y": o.placement.position[1], "trim": o.placement.position[2], "length": length,
                "own_kg": r.own_kg, "load_kg": r.attached_kg, "total_kg": r.total_kg,
                "max_load_kg": r.limit_kg, "over": r.over, "unknown_weights": r.unknown_weights,
                "attached": attached,
            })
        })
        .collect();
    Ok(json!(rows))
}

// ---------------------------------------------------------------- platforms

#[derive(Deserialize, JsonSchema)]
struct PlatformArgs {
    #[serde(flatten)]
    common: Common,
    /// Centre of the footprint at its base, mm. z is the level it stands on.
    #[serde(default)]
    position: P3,
    /// Degrees about z.
    #[serde(default)]
    rotation: f64,
    /// Size preset in metres: `2x1`, `1x1`, `2x0.5` (any `WxD`). Overrides width and
    /// depth.
    preset: Option<String>,
    /// mm. Default 2000.
    #[serde(default = "d2000")]
    width: f64,
    /// mm. Default 1000.
    #[serde(default = "d1000")]
    depth: f64,
    /// Deck top above the base, mm. Default 400.
    #[serde(default = "d400")]
    height: f64,
    /// legs, scissor or none. Default: the system's, else legs.
    under: Option<Understructure>,
    /// Platform system from the catalogue: Bütec, Nivoflex, Generic … The height must
    /// lie in its range.
    system: Option<String>,
    /// Stand on top of this platform: z becomes its deck top (stacking).
    on: Option<Id>,
}

fn d400() -> f64 {
    400.0
}

/// `2x1` → [2000, 1000] (metres in, mm out).
fn parse_preset(s: &str) -> Option<[f64; 2]> {
    let s = s.to_lowercase().replace(['×', '*'], "x").replace(',', ".");
    let (w, d) = s.split_once('x')?;
    let w: f64 = w.trim().trim_end_matches('m').parse().ok()?;
    let d: f64 = d.trim().trim_end_matches('m').parse().ok()?;
    (w > 0.0 && d > 0.0).then_some([w * 1000.0, d * 1000.0])
}

fn check_system(system: Option<&str>, height: f64) -> Result<Option<catalog::PlatformSystem>, CmdError> {
    let Some(name) = system.filter(|s| !s.trim().is_empty()) else { return Ok(None) };
    let s = catalog::platform_system(name)
        .ok_or_else(|| CmdError::invalid_args(format!("no platform system `{name}` in the catalogue")))?;
    if height < s.min_height - 1e-9 || height > s.max_height + 1e-9 {
        return Err(CmdError::invalid_args(format!(
            "{} platforms are {}–{} mm high, not {height}",
            s.name, s.min_height, s.max_height
        )));
    }
    Ok(Some(s))
}

/// World z of a platform's deck top.
fn deck_top(doc: &Document, id: Id) -> Result<f64, CmdError> {
    let o = object(doc, id)?;
    let ObjectKind::Platform { height, .. } = o.kind else {
        return Err(CmdError::invalid(format!("{id} is not a platform")));
    };
    Ok(world(doc, id).expect("exists").transform_point3(glam::DVec3::new(0.0, 0.0, height)).z)
}

fn platform(ctx: &mut Ctx, a: PlatformArgs) -> CmdResult {
    let [width, depth] = match &a.preset {
        Some(p) => parse_preset(p).ok_or_else(|| CmdError::invalid_args(format!("preset `{p}` is not like 2x1")))?,
        None => [a.width, a.depth],
    };
    for (w, v) in [("width", width), ("depth", depth), ("height", a.height)] {
        positive(w, v)?;
    }
    let sys = check_system(a.system.as_deref(), a.height)?;
    let under = a.under.or(sys.as_ref().map(|s| s.understructure)).unwrap_or_default();
    let mut position = a.position;
    if let Some(on) = a.on {
        if a.common.parent.is_some() {
            return Err(CmdError::invalid_args("give either `on` or `parent`, not both"));
        }
        let top = deck_top(ctx.doc(), on)?;
        let layer = super::layer_or_default(ctx.doc(), a.common.layer.as_deref())?;
        position[2] = top - ctx.doc().layers[&layer].elevation;
    }
    let p = Placement { position, rotation: [0.0, 0.0, a.rotation] };
    let kind = ObjectKind::Platform { width, depth, height: a.height, under, system: sys.map(|s| s.name) };
    insert(ctx, a.common, "Platform", p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct PlatformUpdateArgs {
    id: Id,
    /// Size preset in metres, e.g. `2x1`.
    preset: Option<String>,
    /// mm.
    width: Option<f64>,
    /// mm.
    depth: Option<f64>,
    /// Deck top above the base, mm.
    height: Option<f64>,
    under: Option<Understructure>,
    /// Catalogue system name; empty string clears it.
    system: Option<String>,
}

fn platform_update(ctx: &mut Ctx, a: PlatformUpdateArgs) -> CmdResult {
    let size = a
        .preset
        .as_deref()
        .map(|p| parse_preset(p).ok_or_else(|| CmdError::invalid_args(format!("preset `{p}` is not like 2x1"))))
        .transpose()?;
    opt_positive(&[("width", a.width), ("depth", a.depth), ("height", a.height)])?;
    let o = kind_mut(ctx, a.id, "platform")?;
    let ObjectKind::Platform { width, depth, height, under, system } = &mut o.kind else { unreachable!() };
    let new_height = a.height.unwrap_or(*height);
    let new_system = match &a.system {
        Some(s) => (!s.trim().is_empty()).then(|| s.clone()),
        None => system.clone(),
    };
    let sys = check_system(new_system.as_deref(), new_height)?;
    if let Some([w, d]) = size {
        *width = w;
        *depth = d;
    }
    set(width, a.width);
    set(depth, a.depth);
    *height = new_height;
    if a.system.is_some()
        && let Some(s) = &sys
        && a.under.is_none()
    {
        *under = s.understructure;
    }
    set(under, a.under);
    *system = sys.map(|s| s.name);
    Ok(json!(o.kind))
}

// ---------------------------------------------------------------- supports, hoists

#[derive(Deserialize, JsonSchema)]
struct SupportArgs {
    #[serde(flatten)]
    common: Common,
    /// On the floor, mm.
    #[serde(default)]
    position: P3,
    /// Degrees about z.
    #[serde(default)]
    rotation: f64,
    /// Catalogue entry, e.g. `Base plate 600`, `Wind-up stand`, `Ground support base`.
    /// Fills style, size, height and weight; given fields override it.
    preset: Option<String>,
    /// base_plate, stand or ground_support. Default base_plate.
    style: Option<SupportStyle>,
    /// Plate edge or leg spread, mm. Default 600.
    size: Option<f64>,
    /// Attachment height above the floor, mm. Default: plate 10, stand 3000, ground
    /// support 1000.
    height: Option<f64>,
    /// kg. Default from the preset, else 20.
    weight_kg: Option<f64>,
}

fn support(ctx: &mut Ctx, a: SupportArgs) -> CmdResult {
    let pre = match &a.preset {
        Some(n) => Some(
            catalog::support(n).ok_or_else(|| CmdError::invalid_args(format!("no support `{n}` in the catalogue")))?,
        ),
        None => None,
    };
    let style = a.style.or(pre.as_ref().map(|p| p.style)).unwrap_or_default();
    let default_height = match style {
        SupportStyle::BasePlate => 10.0,
        SupportStyle::Stand => 3000.0,
        SupportStyle::GroundSupport => 1000.0,
    };
    let size = a.size.or(pre.as_ref().map(|p| p.size)).unwrap_or(600.0);
    let height = a.height.or(pre.as_ref().map(|p| p.height)).unwrap_or(default_height);
    let weight = a.weight_kg.or(pre.as_ref().map(|p| p.weight_kg)).unwrap_or(20.0);
    positive("size", size)?;
    positive("height", height)?;
    non_negative("weight_kg", weight)?;
    let name = pre.as_ref().map_or("Support", |p| p.name.as_str()).to_owned();
    let p = Placement { position: a.position, rotation: [0.0, 0.0, a.rotation] };
    insert(ctx, a.common, &name, p, ObjectKind::Support { style, size, height, weight_kg: weight })
}

#[derive(Deserialize, JsonSchema)]
struct SupportUpdateArgs {
    id: Id,
    style: Option<SupportStyle>,
    /// mm.
    size: Option<f64>,
    /// mm.
    height: Option<f64>,
    /// kg.
    weight_kg: Option<f64>,
}

fn support_update(ctx: &mut Ctx, a: SupportUpdateArgs) -> CmdResult {
    opt_positive(&[("size", a.size), ("height", a.height)])?;
    if let Some(w) = a.weight_kg {
        non_negative("weight_kg", w)?;
    }
    let o = kind_mut(ctx, a.id, "support")?;
    let ObjectKind::Support { style, size, height, weight_kg } = &mut o.kind else { unreachable!() };
    set(style, a.style);
    set(size, a.size);
    set(height, a.height);
    set(weight_kg, a.weight_kg);
    Ok(json!(o.kind))
}

#[derive(Deserialize, JsonSchema)]
struct HoistArgs {
    #[serde(flatten)]
    common: Common,
    /// The hook, mm. Default [0, 0, 9000].
    #[serde(default = "hook")]
    position: P3,
    /// Working load limit, kg. Default 500.
    #[serde(default = "d500")]
    capacity_kg: f64,
    /// Own weight with chain, kg. Default 30.
    #[serde(default = "d30")]
    weight_kg: f64,
}

fn hook() -> P3 {
    [0.0, 0.0, 9000.0]
}
fn d30() -> f64 {
    30.0
}

fn hoist(ctx: &mut Ctx, a: HoistArgs) -> CmdResult {
    positive("capacity_kg", a.capacity_kg)?;
    non_negative("weight_kg", a.weight_kg)?;
    let p = Placement { position: a.position, ..Default::default() };
    insert(ctx, a.common, "Hoist", p, ObjectKind::Hoist { capacity_kg: a.capacity_kg, weight_kg: a.weight_kg })
}

#[derive(Deserialize, JsonSchema)]
struct HoistUpdateArgs {
    id: Id,
    /// kg.
    capacity_kg: Option<f64>,
    /// kg.
    weight_kg: Option<f64>,
}

fn hoist_update(ctx: &mut Ctx, a: HoistUpdateArgs) -> CmdResult {
    opt_positive(&[("capacity_kg", a.capacity_kg)])?;
    if let Some(w) = a.weight_kg {
        non_negative("weight_kg", w)?;
    }
    let o = kind_mut(ctx, a.id, "hoist")?;
    let ObjectKind::Hoist { capacity_kg, weight_kg } = &mut o.kind else { unreachable!() };
    set(capacity_kg, a.capacity_kg);
    set(weight_kg, a.weight_kg);
    Ok(json!(o.kind))
}

// ---------------------------------------------------------------- ceilings

#[derive(Deserialize, JsonSchema)]
struct CeilingArgs {
    #[serde(flatten)]
    common: Common,
    /// Closed outline in plan, mm, at least 3 points.
    outline: Vec<P2>,
    /// Underside above the layer, mm. Default 9000.
    #[serde(default = "d9000")]
    height: f64,
    /// mm. Default 200.
    #[serde(default = "d200")]
    thickness: f64,
    /// Draw the outline (dashed) in plan. Default true.
    #[serde(default = "yes")]
    show_in_plan: bool,
}

fn d9000() -> f64 {
    9000.0
}
fn d200() -> f64 {
    200.0
}

fn ceiling(ctx: &mut Ctx, a: CeilingArgs) -> CmdResult {
    if a.outline.len() < 3 {
        return Err(CmdError::invalid_args("a ceiling outline needs at least 3 points"));
    }
    positive("thickness", a.thickness)?;
    let p = Placement { position: [0.0, 0.0, a.height], ..Default::default() };
    let kind = ObjectKind::Ceiling { outline: a.outline, thickness: a.thickness, show_in_plan: a.show_in_plan };
    insert(ctx, a.common, "Ceiling", p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct CeilingUpdateArgs {
    id: Id,
    /// Closed outline, mm, relative to the ceiling's placement.
    outline: Option<Vec<P2>>,
    /// Underside above the layer, mm.
    height: Option<f64>,
    /// mm.
    thickness: Option<f64>,
    show_in_plan: Option<bool>,
}

fn ceiling_update(ctx: &mut Ctx, a: CeilingUpdateArgs) -> CmdResult {
    if a.outline.as_ref().is_some_and(|o| o.len() < 3) {
        return Err(CmdError::invalid_args("a ceiling outline needs at least 3 points"));
    }
    opt_positive(&[("thickness", a.thickness)])?;
    let o = kind_mut(ctx, a.id, "ceiling")?;
    set(&mut o.placement.position[2], a.height);
    let ObjectKind::Ceiling { outline, thickness, show_in_plan } = &mut o.kind else { unreachable!() };
    set(outline, a.outline);
    set(thickness, a.thickness);
    set(show_in_plan, a.show_in_plan);
    Ok(json!(o.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_metres() {
        assert_eq!(parse_preset("2x1"), Some([2000.0, 1000.0]));
        assert_eq!(parse_preset("2 × 0,5"), Some([2000.0, 500.0]));
        assert_eq!(parse_preset("1x"), None);
    }
}
