//! Seating: straight rows of seats, one at a time or a block of rows.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::create::{Common, insert, kind_mut, positive};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Id, ObjectKind, P3, Placement};

pub(super) fn register(r: &mut Registry) {
    r.add("seating.row", Kind::Edit, "Create a straight row of seats with a seat count", row);
    r.add(
        "seating.block",
        Kind::Edit,
        "Create a block of seat rows: seats per row, row spacing and a step up per row",
        block,
    );
    r.add("seating.update", Kind::Edit, "Change the seat count, seat width or depth of a seat row", update);
    r.add("seating.count", Kind::Query, "Count the seats: in total, per layer and per row", count);
}

fn d10() -> u32 {
    10
}
fn d500() -> f64 {
    500.0
}
fn d900() -> f64 {
    900.0
}

#[derive(Deserialize, JsonSchema)]
struct RowArgs {
    #[serde(flatten)]
    common: Common,
    /// Middle of the row on the floor, mm, relative to the parent or layer.
    #[serde(default)]
    position: P3,
    /// Degrees about z. Default 0: the seats face +y (upstage).
    #[serde(default)]
    rotation: f64,
    /// Number of seats. Default 10.
    #[serde(default = "d10")]
    count: u32,
    /// Centre-to-centre distance of the seats, mm. Default 500.
    #[serde(default = "d500")]
    seat_width: f64,
    /// Front to back of a seat, mm. Default 500.
    #[serde(default = "d500")]
    depth: f64,
}

fn check(count: u32, seat_width: f64, depth: f64) -> Result<(), CmdError> {
    if count == 0 {
        return Err(CmdError::invalid_args("count must be at least 1"));
    }
    positive("seat_width", seat_width)?;
    positive("depth", depth)
}

fn row(ctx: &mut Ctx, a: RowArgs) -> CmdResult {
    check(a.count, a.seat_width, a.depth)?;
    let p = Placement { position: a.position, rotation: [0.0, 0.0, a.rotation] };
    let kind = ObjectKind::SeatRow { count: a.count, seat_width: a.seat_width, depth: a.depth };
    insert(ctx, a.common, "Seat row", p, kind)
}

/// Where the rows of a block line up.
#[derive(Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Align {
    /// Rows centred on the block's centre line.
    #[default]
    Centre,
    /// Rows start at the block's left end (seen from the seats).
    Left,
    /// Rows end at the block's right end.
    Right,
}

#[derive(Deserialize, JsonSchema)]
struct BlockArgs {
    #[serde(flatten)]
    common: Common,
    /// The first (front) row's reference point on the floor, mm: its middle, or its
    /// left/right end for `align` left/right.
    #[serde(default)]
    position: P3,
    /// Degrees about z. Default 0: the seats face +y; further rows lie behind (−y).
    #[serde(default)]
    rotation: f64,
    /// Seats per row, front row first, e.g. [8, 9, 10, 11].
    counts: Vec<u32>,
    /// Row to row, mm. Default 900.
    #[serde(default = "d900")]
    spacing: f64,
    /// Each row stands this much higher than the one in front, mm. Default 0.
    #[serde(default)]
    step: f64,
    /// Centre-to-centre distance of the seats, mm. Default 500.
    #[serde(default = "d500")]
    seat_width: f64,
    /// Front to back of a seat, mm. Default 500.
    #[serde(default = "d500")]
    depth: f64,
    /// centre (default), left or right.
    #[serde(default)]
    align: Align,
}

fn block(ctx: &mut Ctx, a: BlockArgs) -> CmdResult {
    if a.counts.is_empty() {
        return Err(CmdError::invalid_args("counts needs at least one row"));
    }
    for c in &a.counts {
        check(*c, a.seat_width, a.depth)?;
    }
    positive("spacing", a.spacing)?;
    let (s, c) = a.rotation.to_radians().sin_cos();
    let base = a.common.name.clone().unwrap_or_else(|| "Seats".into());
    let mut ids: Vec<Id> = Vec::new();
    for (i, n) in a.counts.iter().enumerate() {
        let half = *n as f64 * a.seat_width / 2.0;
        // local offsets: along the row (x) to its middle, back (−y) by the spacing
        let along = match a.align {
            Align::Centre => 0.0,
            Align::Left => half,
            Align::Right => -half,
        };
        let back = -a.spacing * i as f64;
        let pos = [
            a.position[0] + along * c - back * s,
            a.position[1] + along * s + back * c,
            a.position[2] + a.step * i as f64,
        ];
        let common = Common { name: Some(format!("{base} row {}", i + 1)), ..a.common.clone() };
        let p = Placement { position: pos, rotation: [0.0, 0.0, a.rotation] };
        let kind = ObjectKind::SeatRow { count: *n, seat_width: a.seat_width, depth: a.depth };
        let out = insert(ctx, common, "Seat row", p, kind)?;
        ids.push(serde_json::from_value(out["id"].clone()).map_err(|e| CmdError::invalid(e.to_string()))?);
    }
    let seats: u32 = a.counts.iter().sum();
    Ok(json!({ "ids": ids, "seats": seats }))
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    /// The seat row to change.
    id: Id,
    count: Option<u32>,
    /// mm.
    seat_width: Option<f64>,
    /// mm.
    depth: Option<f64>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    let o = kind_mut(ctx, a.id, "seat_row")?;
    let ObjectKind::SeatRow { count, seat_width, depth } = &mut o.kind else { unreachable!() };
    let (n, w, d) = (a.count.unwrap_or(*count), a.seat_width.unwrap_or(*seat_width), a.depth.unwrap_or(*depth));
    check(n, w, d)?;
    (*count, *seat_width, *depth) = (n, w, d);
    Ok(json!(o.kind))
}

#[derive(Deserialize, JsonSchema)]
struct CountArgs {
    /// Count only these rows. Default: every seat row.
    ids: Option<Vec<Id>>,
}

fn count(ctx: &mut Ctx, a: CountArgs) -> CmdResult {
    let doc = ctx.doc();
    let mut total = 0u32;
    let mut per_layer: std::collections::BTreeMap<String, u32> = Default::default();
    let mut rows: Vec<Value> = Vec::new();
    for o in doc.objects.values() {
        let ObjectKind::SeatRow { count, .. } = o.kind else { continue };
        if a.ids.as_ref().is_some_and(|ids| !ids.contains(&o.id)) {
            continue;
        }
        total += count;
        let layer = doc.layers.get(&o.layer).map(|l| l.name.clone()).unwrap_or_default();
        *per_layer.entry(layer).or_default() += count;
        rows.push(json!({ "id": o.id, "name": o.name, "seats": count }));
    }
    Ok(json!({ "seats": total, "layers": per_layer, "rows": rows }))
}
