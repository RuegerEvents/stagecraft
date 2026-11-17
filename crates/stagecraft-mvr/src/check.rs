//! Consistency checks after import: dangling references and missing files.
//! Repeated problems (e.g. 150 fixtures pointing at one missing GDTF) give one
//! warning with a count.

use crate::archive::Resources;
use crate::model::*;
use crate::values::fmt_uuid;
use crate::warning::Warning;
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

/// Check references and files of a scene. Pure; returns the warnings.
pub fn check(scene: &Scene, res: &Resources) -> Vec<Warning> {
    let mut grouped: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut add = |ctx: &str, msg: String| *grouped.entry((ctx.to_string(), msg)).or_default() += 1;

    let symdefs: HashSet<Uuid> = scene.aux.symdefs.iter().map(|s| s.uuid).collect();
    let classes: HashSet<Uuid> = scene.aux.classes.iter().map(|c| c.uuid).collect();
    let positions: HashSet<Uuid> = scene.aux.positions.iter().map(|p| p.uuid).collect();
    let mapping_defs: HashSet<Uuid> = scene.aux.mapping_definitions.iter().map(|m| m.uuid).collect();
    let objects = scene.objects();
    let focus_points: HashSet<Uuid> =
        objects.iter().filter(|(_, o)| o.kind == ObjectKind::FocusPoint).map(|(_, o)| o.uuid).collect();
    let all_objects: HashSet<Uuid> = objects.iter().map(|(_, o)| o.uuid).collect();

    // Duplicate identities among layers, objects, symdefs, classes, positions.
    let mut seen: HashMap<Uuid, usize> = HashMap::new();
    let ids = scene
        .layers
        .iter()
        .map(|l| l.uuid)
        .chain(objects.iter().map(|(_, o)| o.uuid))
        .chain(scene.aux.symdefs.iter().map(|s| s.uuid))
        .chain(scene.aux.classes.iter().map(|s| s.uuid))
        .chain(scene.aux.positions.iter().map(|s| s.uuid));
    for id in ids {
        *seen.entry(id).or_default() += 1;
    }
    let mut dups: Vec<_> = seen.into_iter().filter(|(_, n)| *n > 1).collect();
    dups.sort();
    for (id, n) in dups {
        add("scene", format!("uuid {} used {n} times", fmt_uuid(&id)));
    }

    let check_geo = |ctx: &str, gs: &[Geometry], add: &mut dyn FnMut(&str, String)| {
        for g in gs {
            match g {
                Geometry::Geometry3D { file_name, .. } => {
                    if res.resolve(file_name).is_none() {
                        add(ctx, format!("geometry file {file_name:?} missing from archive"));
                    }
                }
                Geometry::Symbol { symdef, .. } => {
                    if !symdefs.contains(symdef) {
                        add(ctx, format!("symbol refers to unknown symdef {}", fmt_uuid(symdef)));
                    }
                }
            }
        }
    };
    for sd in &scene.aux.symdefs {
        check_geo("AUXData", &sd.geometries, &mut add);
    }
    for (layer, o) in &objects {
        let ctx = format!("Layer {:?}", layer.name);
        check_geo(&ctx, &o.geometries, &mut add);
        if let Some(c) = o.classing
            && !classes.contains(&c)
        {
            add(&ctx, format!("classing refers to unknown class {}", fmt_uuid(&c)));
        }
        if let Some(p) = o.position
            && !positions.contains(&p)
        {
            add(&ctx, format!("position refers to unknown position {}", fmt_uuid(&p)));
        }
        if let Some(f) = o.focus
            && !focus_points.contains(&f)
        {
            add(&ctx, format!("focus refers to unknown focus point {}", fmt_uuid(&f)));
        }
        if let Some(m) = o.multipatch
            && !all_objects.contains(&m)
        {
            add(&ctx, format!("multipatch parent {} not in scene", fmt_uuid(&m)));
        }
        if let Some(spec) = &o.gdtf_spec
            && res.resolve_gdtf(spec).is_none()
        {
            add("archive", format!("GDTF {spec:?} missing"));
        }
        if let Some(g) = &o.gobo
            && res.resolve(&g.file_name).is_none()
        {
            add(&ctx, format!("gobo image {:?} missing", g.file_name));
        }
        for m in &o.mappings {
            if !mapping_defs.contains(&m.linked_def) {
                add(&ctx, format!("mapping refers to unknown definition {}", fmt_uuid(&m.linked_def)));
            }
        }
        let mut breaks = HashSet::new();
        for a in &o.addresses {
            if !breaks.insert(a.dmx_break) {
                add(&ctx, format!("{} {:?} has DMX break {} twice", o.kind.tag(), o.name, a.dmx_break));
            }
        }
    }

    grouped
        .into_iter()
        .map(|((ctx, msg), n)| if n > 1 { Warning::new(ctx, format!("{msg} ({n}×)")) } else { Warning::new(ctx, msg) })
        .collect()
}
