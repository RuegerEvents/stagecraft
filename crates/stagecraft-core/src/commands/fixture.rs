//! Fixtures, the GDTF library, plan symbols and the DMX patch.

use std::collections::BTreeSet;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::create::{Common, insert};
use super::{ensure_exists, object};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::fixture_type::{self, FixtureType, SYMBOLS};
use crate::model::{Document, Id, ObjectKind, P3, Placement, format_address, parse_address};
use crate::patch;

pub(super) fn register(r: &mut Registry) {
    r.add(
        "fixture.create",
        Kind::Edit,
        "Create a fixture of a GDTF type and mode, optionally hung on a parent",
        create,
    );
    r.add("fixture.update", Kind::Edit, "Change FID, CID, type, mode, address(es) or weight of one fixture", update);
    r.add("fixture.number", Kind::Edit, "Give fixtures consecutive FIDs in the given order", number);
    r.add(
        "fixture.set_type",
        Kind::Edit,
        "Swap the GDTF type (and mode) of fixtures, keeping FID, address and position",
        set_type,
    );
    r.add("gdtf.import", Kind::Edit, "Import a .gdtf file into the document's fixture type library", import);
    r.add(
        "gdtf.library",
        Kind::Query,
        "List the fixture types in the document: modes, footprints, weight, symbol",
        library,
    );
    r.add("gdtf.remove", Kind::Edit, "Remove a fixture type from the library (refuses while fixtures use it)", remove);
    r.add("symbol.assign", Kind::Edit, "Assign a built-in plan symbol to a fixture type, or clear it", assign_symbol);
    r.add("symbol.list", Kind::Query, "The built-in plan symbols", list_symbols);
    r.add("patch.auto", Kind::Edit, "Patch fixtures one after another from a start address, skipping used slots", auto);
    r.add(
        "patch.check",
        Kind::Query,
        "Patch problems: overlapping addresses, universe overruns, unpatched fixtures",
        check,
    );
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum AddressArg {
    Absolute(u32),
    Text(String),
}

impl AddressArg {
    fn resolve(&self) -> Result<u32, CmdError> {
        let n = match self {
            AddressArg::Absolute(n) => (*n >= 1).then_some(*n),
            AddressArg::Text(s) => parse_address(s),
        };
        n.ok_or_else(|| CmdError::invalid_args("address must be like 1.001 or an absolute number ≥ 1"))
    }

    /// `None` for "unpatch" (0 or empty text).
    fn resolve_or_clear(&self) -> Result<Option<u32>, CmdError> {
        match self {
            AddressArg::Absolute(0) => Ok(None),
            AddressArg::Text(s) if s.trim().is_empty() => Ok(None),
            x => x.resolve().map(Some),
        }
    }
}

fn check_fid(doc: &Document, fid: u32, except: Option<Id>) -> Result<(), CmdError> {
    let taken = doc
        .objects
        .values()
        .find(|o| Some(o.id) != except && matches!(o.kind, ObjectKind::Fixture { fid: Some(f), .. } if f == fid));
    match taken {
        Some(o) => Err(CmdError::invalid(format!("FID {fid} is already used by {} ({})", o.name, o.id))),
        None => Ok(()),
    }
}

fn library_type<'d>(doc: &'d Document, key: &str) -> Result<&'d FixtureType, CmdError> {
    doc.fixture_types.get(key).ok_or_else(|| {
        let known: Vec<&str> = doc.fixture_types.keys().map(String::as_str).collect();
        CmdError::not_found(
            "fixture type",
            format!("{key} (import it with gdtf.import; the library has: {})", known.join(", ")),
        )
    })
}

/// The mode to use: `want` (must exist), else `keep` if the type has it, else the
/// first mode.
fn pick_mode(t: &FixtureType, want: Option<&str>, keep: Option<&str>) -> Result<Option<String>, CmdError> {
    if let Some(w) = want {
        return match t.mode(w) {
            Some(m) => Ok(Some(m.name.clone())),
            None => Err(CmdError::invalid_args(format!(
                "{} has no mode `{w}`; modes: {}",
                t.key,
                t.modes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        };
    }
    Ok(keep.and_then(|k| t.mode(k)).or(t.modes.first()).map(|m| m.name.clone()))
}

#[derive(Deserialize, JsonSchema)]
struct CreateArgs {
    #[serde(flatten)]
    common: Common,
    /// mm, relative to the parent or layer. The origin is the hanging point; the
    /// GDTF body hangs below it.
    #[serde(default)]
    position: P3,
    /// Degrees about x, y, z (Rz · Ry · Rx). A fixture standing on the floor is
    /// turned over: `[180, 0, 0]`.
    #[serde(default)]
    rotation: P3,
    /// Fixture ID, unique in the document.
    fid: Option<u32>,
    /// Custom ID (MVR CustomId).
    cid: Option<String>,
    /// DMX address of the first break: `1.001`, `2.17` or an absolute number.
    address: Option<AddressArg>,
    /// Addresses of further DMX breaks (multi-break modes), in break order.
    #[serde(default)]
    extra_addresses: Vec<AddressArg>,
    /// Fixture type key from `gdtf.library` (`Manufacturer@Name`).
    #[serde(alias = "gdtf_type", alias = "type")]
    gdtf: Option<String>,
    /// DMX mode of the type. Default: its first mode.
    mode: Option<String>,
    /// Weight in kg, for load sums. Default: the GDTF's weight.
    weight_kg: Option<f64>,
}

fn create(ctx: &mut Ctx, a: CreateArgs) -> CmdResult {
    let doc = ctx.doc();
    if let Some(f) = a.fid {
        check_fid(doc, f, None)?;
    }
    let address = a.address.as_ref().map(AddressArg::resolve).transpose()?;
    let extra =
        a.extra_addresses.iter().map(|x| x.resolve_or_clear().map(|v| v.unwrap_or(0))).collect::<Result<_, _>>()?;
    if a.weight_kg.is_some_and(|w| !(w >= 0.0 && w.is_finite())) {
        return Err(CmdError::invalid_args("weight_kg must be zero or more"));
    }
    let (gdtf, mode, weight) = match &a.gdtf {
        Some(k) if !k.is_empty() => {
            let t = library_type(doc, k)?;
            (Some(t.key.clone()), pick_mode(t, a.mode.as_deref(), None)?, a.weight_kg.or(t.weight_kg))
        }
        _ => (None, a.mode.clone(), a.weight_kg),
    };
    let name = match a.fid {
        Some(f) => format!("Fixture {f}"),
        None => "Fixture".into(),
    };
    let p = Placement { position: a.position, rotation: a.rotation };
    let kind =
        ObjectKind::Fixture { fid: a.fid, cid: a.cid, gdtf, mode, address, extra_addresses: extra, weight_kg: weight };
    insert(ctx, a.common, &name, p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    id: Id,
    /// New FID; 0 clears it.
    fid: Option<u32>,
    /// New CID; empty string clears it.
    cid: Option<String>,
    /// Address of the first break: `1.001`, absolute number, or empty string / 0 to
    /// unpatch.
    address: Option<AddressArg>,
    /// Addresses of further breaks, in break order (0 / empty = unpatched).
    extra_addresses: Option<Vec<AddressArg>>,
    /// Fixture type key; empty string clears it. Same as `fixture.set_type`.
    #[serde(alias = "gdtf_type", alias = "type")]
    gdtf: Option<String>,
    /// DMX mode of the type.
    mode: Option<String>,
    /// Weight in kg; 0 or less clears it.
    weight_kg: Option<f64>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    let o = object(ctx.doc(), a.id)?;
    if !matches!(o.kind, ObjectKind::Fixture { .. }) {
        return Err(CmdError::invalid(format!("{} is not a fixture", a.id)));
    }
    if let Some(f) = a.fid.filter(|&f| f != 0) {
        check_fid(ctx.doc(), f, Some(a.id))?;
    }
    let address = a.address.as_ref().map(AddressArg::resolve_or_clear).transpose()?;
    let extra = match &a.extra_addresses {
        Some(v) => Some(v.iter().map(|x| x.resolve_or_clear().map(|v| v.unwrap_or(0))).collect::<Result<Vec<_>, _>>()?),
        None => None,
    };
    let current_type = match &o.kind {
        ObjectKind::Fixture { gdtf, .. } => gdtf.clone(),
        _ => None,
    };
    if a.gdtf.is_some() || a.mode.is_some() {
        let key = a.gdtf.clone().or(current_type);
        apply_type(ctx.doc_mut(), a.id, key.as_deref(), a.mode.as_deref())?;
    }
    let o = ctx.doc_mut().objects.get_mut(&a.id).expect("checked");
    let ObjectKind::Fixture { fid, cid, address: addr, extra_addresses, weight_kg, .. } = &mut o.kind else {
        unreachable!()
    };
    if let Some(w) = a.weight_kg {
        *weight_kg = (w > 0.0 && w.is_finite()).then_some(w);
    }
    if let Some(f) = a.fid {
        *fid = (f != 0).then_some(f);
    }
    if let Some(c) = a.cid {
        *cid = (!c.is_empty()).then_some(c);
    }
    if let Some(x) = address {
        *addr = x;
    }
    if let Some(x) = extra {
        *extra_addresses = x;
    }
    let doc = ctx.doc();
    Ok(fixture_json(doc, a.id))
}

/// What commands report about one fixture.
fn fixture_json(doc: &Document, id: Id) -> Value {
    let o = &doc.objects[&id];
    let ObjectKind::Fixture { fid, cid, gdtf, mode, address, weight_kg, .. } = &o.kind else { return json!({}) };
    let problems = patch::conflicts(doc).remove(&id).unwrap_or_default();
    json!({
        "id": id, "fid": fid, "cid": cid, "gdtf": gdtf, "mode": mode,
        "address": address.map(format_address),
        "addresses": patch::address_text(o),
        "footprint": patch::footprint(doc, o),
        "weight_kg": weight_kg,
        "problems": problems,
    })
}

/// Set a fixture's type and mode. The mode falls back to the old mode's name, then
/// the first mode; the weight follows the type unless it was set by hand; the
/// address stays and the list of further break addresses fits the new mode.
fn apply_type(doc: &mut Document, id: Id, key: Option<&str>, mode: Option<&str>) -> Result<(), CmdError> {
    let new_type = match key {
        Some(k) if !k.is_empty() => Some(library_type(doc, k)?.clone()),
        _ => None,
    };
    let old_weight = patch::fixture_type(doc, &doc.objects[&id]).and_then(|t| t.weight_kg);
    let o = doc.objects.get_mut(&id).expect("checked by caller");
    let ObjectKind::Fixture { gdtf, mode: m, extra_addresses, weight_kg, .. } = &mut o.kind else {
        return Err(CmdError::invalid(format!("{id} is not a fixture")));
    };
    let followed_type = weight_kg.is_none() || *weight_kg == old_weight;
    match &new_type {
        Some(t) => {
            *m = pick_mode(t, mode, m.as_deref())?;
            *gdtf = Some(t.key.clone());
            if followed_type {
                *weight_kg = t.weight_kg;
            }
            let breaks = t.mode_or_first(m.as_deref()).map_or(1, |x| x.footprint.len().max(1));
            extra_addresses.resize(breaks - 1, 0);
            while extra_addresses.last() == Some(&0) {
                extra_addresses.pop();
            }
        }
        None => {
            *gdtf = None;
            *m = mode.map(str::to_owned);
            extra_addresses.clear();
            if followed_type {
                *weight_kg = None;
            }
        }
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct SetTypeArgs {
    /// Fixtures to change. Default: the selection.
    #[serde(default)]
    ids: Vec<Id>,
    /// Fixture type key from `gdtf.library`; empty string removes the type.
    #[serde(alias = "gdtf", alias = "type")]
    gdtf_type: String,
    /// Mode of the new type. Default: a mode with the old mode's name, else the first.
    mode: Option<String>,
}

fn set_type(ctx: &mut Ctx, a: SetTypeArgs) -> CmdResult {
    let ids = if a.ids.is_empty() { ctx.selection().to_vec() } else { a.ids };
    ensure_exists(ctx.doc(), &ids)?;
    let doc = ctx.doc_mut();
    for id in &ids {
        apply_type(doc, *id, Some(&a.gdtf_type), a.mode.as_deref())?;
    }
    let doc = ctx.doc();
    let conflicts = patch::conflicts(doc);
    let hit: Vec<&Id> = ids.iter().filter(|id| conflicts.contains_key(id)).collect();
    Ok(json!({ "changed": ids.len(), "conflicts": hit }))
}

#[derive(Deserialize, JsonSchema)]
struct NumberArgs {
    /// Fixtures in the order they get numbers.
    ids: Vec<Id>,
    start: u32,
    /// Default 1.
    #[serde(default = "one")]
    step: u32,
}

fn one() -> u32 {
    1
}

fn number(ctx: &mut Ctx, a: NumberArgs) -> CmdResult {
    let doc = ctx.doc();
    ensure_exists(doc, &a.ids)?;
    if a.start == 0 || a.step == 0 {
        return Err(CmdError::invalid_args("start and step must be at least 1"));
    }
    let fids: Vec<u32> = (0..a.ids.len() as u32).map(|i| a.start + i * a.step).collect();
    for (id, f) in a.ids.iter().zip(&fids) {
        if !matches!(doc.objects[id].kind, ObjectKind::Fixture { .. }) {
            return Err(CmdError::invalid(format!("{id} is not a fixture")));
        }
        // Clashes with fixtures outside the renumbered set are errors.
        if let Some(o) = doc
            .objects
            .values()
            .find(|o| !a.ids.contains(&o.id) && matches!(o.kind, ObjectKind::Fixture { fid: Some(x), .. } if x == *f))
        {
            return Err(CmdError::invalid(format!("FID {f} is already used by {} ({})", o.name, o.id)));
        }
    }
    let doc = ctx.doc_mut();
    for (id, f) in a.ids.iter().zip(&fids) {
        if let ObjectKind::Fixture { fid, .. } = &mut doc.objects.get_mut(id).expect("checked").kind {
            *fid = Some(*f);
        }
    }
    Ok(json!({ "fids": fids }))
}

#[derive(Deserialize, JsonSchema)]
struct ImportArgs {
    /// Path to a `.gdtf` file.
    path: Option<PathBuf>,
    /// The file's bytes, base64, instead of `path` (browser, scripts).
    data_base64: Option<String>,
    /// File name to record with `data_base64`. Default `fixture.gdtf`.
    file_name: Option<String>,
    /// Library key. Default `Manufacturer@Name` from the file. Importing a key that
    /// exists replaces that type (its fixtures follow; the symbol stays).
    key: Option<String>,
}

fn import(ctx: &mut Ctx, a: ImportArgs) -> CmdResult {
    let (file_name, bytes) = match (&a.path, &a.data_base64) {
        (Some(p), None) => {
            let bytes = std::fs::read(p).map_err(|e| CmdError::io(format!("{}: {e}", p.display())))?;
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "fixture.gdtf".into());
            (name, bytes)
        }
        (None, Some(b)) => {
            let bytes =
                crate::sheet::base64_decode(b).ok_or_else(|| CmdError::invalid_args("data_base64 is not base64"))?;
            (a.file_name.clone().unwrap_or_else(|| "fixture.gdtf".into()), bytes)
        }
        _ => return Err(CmdError::invalid_args("give either `path` or `data_base64`")),
    };
    import_bytes(ctx, &file_name, bytes, a.key.as_deref())
}

/// Put a GDTF into the library (shared by `gdtf.import`).
pub(crate) fn import_bytes(ctx: &mut Ctx, file_name: &str, bytes: Vec<u8>, key: Option<&str>) -> CmdResult {
    let (mut t, asset) = fixture_type::read_gdtf(file_name, bytes, key)?;
    let doc = ctx.doc();
    if let Some(old) = doc.fixture_types.get(&t.key) {
        if old.asset == t.asset {
            return Ok(json!({ "key": t.key, "unchanged": true, "type": type_json(doc, old) }));
        }
        t.symbol = old.symbol.clone();
    }
    let key = t.key.clone();
    let doc = ctx.doc_mut();
    let old = doc.fixture_types.insert(key.clone(), t.clone());
    if !doc.assets.contains_key(&asset.hash) {
        doc.assets.insert(asset.hash.clone(), asset);
    }
    if let Some(old) = old {
        drop_unused_asset(doc, &old.asset);
        // Fixtures on a mode the new revision lacks move to its first mode.
        let ids: Vec<Id> = doc
            .objects
            .values()
            .filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(k), .. } if *k == key))
            .map(|o| o.id)
            .collect();
        for id in ids {
            apply_type(doc, id, Some(&key), None)?;
        }
    }
    let doc = ctx.doc();
    Ok(json!({ "key": key, "type": type_json(doc, &doc.fixture_types[&key]) }))
}

fn drop_unused_asset(doc: &mut Document, hash: &str) {
    if !doc.fixture_types.values().any(|t| t.asset == hash) {
        doc.assets.remove(hash);
    }
}

/// Which plan symbol a type gets: `assigned:<name>`, `gdtf_svg`, or `silhouette`.
pub fn symbol_source(t: &FixtureType) -> String {
    match (&t.symbol, t.has_plan_svg) {
        (Some(s), _) => format!("assigned:{s}"),
        (None, true) => "gdtf_svg".into(),
        (None, false) => "silhouette".into(),
    }
}

fn type_json(doc: &Document, t: &FixtureType) -> Value {
    let count = doc
        .objects
        .values()
        .filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(k), .. } if *k == t.key))
        .count();
    let modes: Vec<Value> = t
        .modes
        .iter()
        .map(|m| json!({ "name": m.name, "footprint": m.footprint_text(), "breaks": m.footprint, "channels": m.channels }))
        .collect();
    json!({
        "key": t.key, "manufacturer": t.manufacturer, "name": t.name, "long_name": t.long_name,
        "modes": modes, "weight_kg": t.weight_kg, "power_w": t.power_w, "size_mm": t.size_mm,
        "symbol": symbol_source(t), "suggested_symbol": t.suggested_symbol, "fixtures": count,
        "file": doc.assets.get(&t.asset).map(|a| a.file_name.clone()),
        "bytes": doc.assets.get(&t.asset).map(|a| a.size),
        "warnings": t.warnings.len(),
    })
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn library(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    let types: Vec<Value> = doc.fixture_types.values().map(|t| type_json(doc, t)).collect();
    Ok(json!({ "types": types }))
}

#[derive(Deserialize, JsonSchema)]
struct RemoveArgs {
    /// Fixture type key.
    #[serde(alias = "gdtf_type", alias = "key")]
    r#type: String,
    /// Remove it even if fixtures use it; they lose their type.
    #[serde(default)]
    force: bool,
}

fn remove(ctx: &mut Ctx, a: RemoveArgs) -> CmdResult {
    let doc = ctx.doc();
    library_type(doc, &a.r#type)?;
    let users: Vec<Id> = doc
        .objects
        .values()
        .filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(k), .. } if *k == a.r#type))
        .map(|o| o.id)
        .collect();
    if !users.is_empty() && !a.force {
        return Err(CmdError::invalid(format!("{} fixtures use {}; pass force: true", users.len(), a.r#type)));
    }
    let doc = ctx.doc_mut();
    for id in &users {
        apply_type(doc, *id, None, None)?;
    }
    let t = doc.fixture_types.remove(&a.r#type).expect("checked");
    drop_unused_asset(doc, &t.asset);
    Ok(json!({ "removed": a.r#type, "fixtures": users.len() }))
}

#[derive(Deserialize, JsonSchema)]
struct AssignArgs {
    /// Fixture type key.
    #[serde(alias = "type", alias = "gdtf")]
    gdtf_type: String,
    /// Built-in symbol name (`symbol.list`), `suggested` for the type's suggestion,
    /// or empty to clear (the GDTF's own top view or the silhouette then draws).
    #[serde(default)]
    symbol: String,
}

fn assign_symbol(ctx: &mut Ctx, a: AssignArgs) -> CmdResult {
    let t = library_type(ctx.doc(), &a.gdtf_type)?;
    let name = match a.symbol.trim() {
        "" => None,
        "suggested" => Some(t.suggested_symbol.clone().ok_or_else(|| CmdError::invalid("the type has no suggestion"))?),
        s if SYMBOLS.iter().any(|(n, _)| *n == s) => Some(s.to_owned()),
        s => {
            let names: Vec<&str> = SYMBOLS.iter().map(|(n, _)| *n).collect();
            return Err(CmdError::invalid_args(format!("no symbol `{s}`; symbols: {}", names.join(", "))));
        }
    };
    let t = ctx.doc_mut().fixture_types.get_mut(&a.gdtf_type).expect("checked");
    t.symbol = name;
    Ok(json!({ "type": a.gdtf_type, "symbol": symbol_source(t) }))
}

fn list_symbols(_: &mut Ctx, _: NoArgs) -> CmdResult {
    Ok(json!({ "symbols": SYMBOLS.iter().map(|(n, _)| *n).collect::<Vec<_>>() }))
}

#[derive(Deserialize, JsonSchema)]
struct AutoArgs {
    /// Fixtures in patch order. Default: the selection, in the order it was made.
    #[serde(default)]
    ids: Vec<Id>,
    /// First address: `1.001`, `2.1` or absolute. Default 1.001.
    start: Option<AddressArg>,
    /// Free slots left after each fixture. Default 0.
    #[serde(default)]
    gap: u32,
}

fn auto(ctx: &mut Ctx, a: AutoArgs) -> CmdResult {
    let ids = if a.ids.is_empty() { ctx.selection().to_vec() } else { a.ids };
    let doc = ctx.doc();
    ensure_exists(doc, &ids)?;
    let ids: Vec<Id> =
        ids.into_iter().filter(|id| matches!(doc.objects[id].kind, ObjectKind::Fixture { .. })).collect();
    if ids.is_empty() {
        return Err(CmdError::invalid_args("no fixtures given or selected"));
    }
    let start = a.start.as_ref().map(AddressArg::resolve).transpose()?.unwrap_or(1);
    let except: BTreeSet<Id> = ids.iter().copied().collect();
    let mut used = patch::used(doc, &except);
    let mut cursor = start;
    let mut plan: Vec<(Id, Vec<u32>)> = Vec::new();
    for id in &ids {
        let fp = patch::footprint(doc, &doc.objects[id]);
        let mut addrs = Vec::new();
        for n in fp {
            let at = patch::next_free(&used, cursor, n)
                .ok_or_else(|| CmdError::invalid("no free DMX range left for all fixtures"))?;
            addrs.push(at);
            used.push((at, at + n - 1));
            used.sort();
            cursor = at + n + a.gap;
        }
        plan.push((*id, addrs));
    }
    let doc = ctx.doc_mut();
    for (id, addrs) in &plan {
        if let ObjectKind::Fixture { address, extra_addresses, .. } =
            &mut doc.objects.get_mut(id).expect("checked").kind
        {
            *address = addrs.first().copied();
            *extra_addresses = addrs[1..].to_vec();
        }
    }
    let doc = ctx.doc();
    let out: Vec<Value> = plan
        .iter()
        .map(|(id, _)| json!({ "id": id, "fid": fid_of(doc, *id), "address": patch::address_text(&doc.objects[id]) }))
        .collect();
    Ok(json!({ "patched": out, "next": cursor }))
}

fn fid_of(doc: &Document, id: Id) -> Option<u32> {
    match doc.objects.get(&id)?.kind {
        ObjectKind::Fixture { fid, .. } => fid,
        _ => None,
    }
}

fn check(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    let conflicts: Vec<Value> = patch::conflicts(doc)
        .into_iter()
        .map(|(id, problems)| {
            let o = &doc.objects[&id];
            json!({ "id": id, "fid": fid_of(doc, id), "name": o.name, "address": patch::address_text(o), "problems": problems })
        })
        .collect();
    let fixtures: Vec<_> = doc.objects.values().filter(|o| matches!(o.kind, ObjectKind::Fixture { .. })).collect();
    let unpatched: Vec<Id> =
        fixtures.iter().filter(|o| patch::addresses(o).iter().all(Option::is_none)).map(|o| o.id).collect();
    let partly: Vec<Id> = fixtures
        .iter()
        .filter(|o| {
            let a = patch::addresses(o);
            let n = patch::footprint(doc, o).len();
            a.iter().any(Option::is_some) && (a.len() < n || a.iter().take(n).any(Option::is_none))
        })
        .map(|o| o.id)
        .collect();
    let untyped = fixtures.iter().filter(|o| patch::fixture_type(doc, o).is_none()).count();
    Ok(json!({
        "ok": conflicts.is_empty(),
        "conflicts": conflicts,
        "unpatched": unpatched,
        "partly_patched": partly,
        "fixtures": fixtures.len(),
        "without_type": untyped,
    }))
}
