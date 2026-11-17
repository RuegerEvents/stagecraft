//! The context inspector: only the fields that fit the selection. With several objects
//! selected, a field shows a value when all agree, and an edit applies to all of them
//! (one undo step through `batch`).

use egui::{RichText, Ui, WidgetType};
use serde_json::{Value, json};
use stagecraft_core::model::format_address;
use stagecraft_core::{Id, Object, ObjectKind};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::panels::{fmt_mm, parse_mm};
use crate::widgets::{self, button, edit_field_hint};

/// The one value all items share, if they do.
pub fn common<T: PartialEq>(mut it: impl Iterator<Item = T>) -> Option<T> {
    let first = it.next()?;
    for x in it {
        if x != first {
            return None;
        }
    }
    Some(first)
}

/// A labelled field; `None` as current shows the "mixed" hint.
fn field(ui: &mut Ui, key: &str, salt: &str, current: Option<String>) -> Option<String> {
    ui.label(t(key));
    let id = egui::Id::new(("inspector", salt));
    let mixed = current.is_none();
    let hint = if mixed { t("inspector.mixed") } else { "" };
    let out = edit_field_hint(ui, id, &current.unwrap_or_default(), 140.0, &format!("Inspector {}", t(key)), hint);
    ui.end_row();
    out
}

fn batch(s: &mut Studio, steps: Vec<Value>) {
    match steps.len() {
        0 => {}
        1 => {
            let step = &steps[0];
            let _ = s.run(step["command"].as_str().unwrap_or_default(), step["args"].clone());
        }
        _ => {
            let _ = s.run("batch", json!({ "commands": steps }));
        }
    }
}

fn step(command: &str, args: Value) -> Value {
    json!({ "command": command, "args": args })
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    ui.heading(t("panel.inspector"));
    ui.add_space(4.0);
    let sel: Vec<Id> = s.engine.selection().to_vec();
    let doc = s.engine.doc().clone();
    let objs: Vec<&Object> = sel.iter().filter_map(|id| doc.objects.get(id)).collect();
    if objs.is_empty() {
        ui.label(RichText::new(t("inspector.nothing")).weak());
        ui.add_space(8.0);
        document_fields(ui, s);
        return;
    }
    let ty = common(objs.iter().map(|o| o.kind.type_name()));
    let title = match (objs.len(), ty) {
        (1, _) => format!("{} · {}", objs[0].name, t(&format!("type.{}", objs[0].kind.type_name()))),
        (n, Some(ty)) => tf("inspector.n_of_type", &[("n", &n), ("type", &t(&format!("type.{ty}.plural")))]),
        (n, None) => tf("inspector.n_objects", &[("n", &n)]),
    };
    ui.label(RichText::new(title).strong());
    ui.add_space(4.0);

    // Roots and objects whose parent is not selected: absolute edits apply to them,
    // their children follow.
    let tops: Vec<Id> =
        sel.iter().copied().filter(|id| !sel.iter().any(|o| o != id && doc.is_ancestor(*o, *id))).collect();
    let worlds: Vec<(Id, stagecraft_core::Placement)> =
        tops.iter().filter_map(|id| stagecraft_core::geom::world_placement(&doc, *id).map(|w| (*id, w))).collect();

    egui::Grid::new("inspector_common").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        let name = common(objs.iter().map(|o| o.name.clone()));
        if let Some(v) = field(ui, "inspector.name", "name", name) {
            batch(s, sel.iter().map(|id| step("object.rename", json!({ "id": id, "name": v }))).collect());
        }

        ui.label(t("inspector.layer"));
        let layer = common(objs.iter().map(|o| o.layer));
        let shown =
            layer.and_then(|l| doc.layers.get(&l)).map_or_else(|| t("inspector.mixed").to_owned(), |l| l.name.clone());
        egui::ComboBox::from_id_salt("inspector_layer").selected_text(shown).show_ui(ui, |ui| {
            for l in doc.layers_sorted() {
                if ui.selectable_label(layer == Some(l.id), &l.name).clicked() {
                    let _ = s.run("object.set_layer", json!({ "ids": sel, "layer": l.id }));
                }
            }
        });
        ui.end_row();

        for (axis, key) in [(0usize, "inspector.x"), (1, "inspector.y"), (2, "inspector.z")] {
            let cur = common(worlds.iter().map(|(_, w)| fmt_mm(w.position[axis])));
            let cur = cur.or_else(|| (worlds.is_empty()).then(String::new));
            if let Some(v) = field(ui, key, key, cur) {
                let Some(mm) = parse_mm(&v) else {
                    s.ui.error(tf("status.bad_number", &[("text", &v)]));
                    continue;
                };
                let steps = worlds
                    .iter()
                    .filter_map(|(id, w)| {
                        let mut d = [0.0; 3];
                        d[axis] = mm - w.position[axis];
                        (d[axis].abs() > 1e-9).then(|| step("object.move", json!({ "ids": [id], "delta": d })))
                    })
                    .collect();
                batch(s, steps);
            }
        }
        let rot = common(worlds.iter().map(|(_, w)| fmt_mm(w.rotation[2])));
        if let Some(v) = field(ui, "inspector.rotation", "rotation", rot) {
            match crate::tools::parse_angle(&v) {
                Some(a) => {
                    let steps = worlds
                        .iter()
                        .filter_map(|(id, w)| {
                            let d = a - w.rotation[2];
                            (d.abs() > 1e-9)
                                .then(|| step("object.rotate", json!({ "ids": [id], "angle": d, "pivot": w.position })))
                        })
                        .collect();
                    batch(s, steps);
                }
                None => s.ui.error(tf("status.bad_angle", &[("text", &v)])),
            }
        }
    });

    if let Some(ty) = ty {
        ui.add_space(6.0);
        ui.separator();
        egui::Grid::new("inspector_kind").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            kind_fields(ui, s, ty, &objs, &sel);
        });
    }

    ui.add_space(6.0);
    ui.separator();
    tags_field(ui, s, &doc, &objs, &sel);

    let parents: Vec<Id> = objs.iter().filter_map(|o| o.parent).collect();
    if !parents.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let names: Vec<String> =
                parents.iter().filter_map(|p| doc.objects.get(p).map(|o| o.name.clone())).collect();
            let shown = common(names.iter().cloned()).unwrap_or_else(|| t("inspector.mixed").to_owned());
            ui.label(tf("inspector.attached_to", &[("name", &shown)]));
            if button(ui, t("inspector.detach"), "Detach").clicked() {
                let ids: Vec<Id> = objs.iter().filter(|o| o.parent.is_some()).map(|o| o.id).collect();
                let _ = s.run("object.detach", json!({ "ids": ids }));
            }
        });
    }
}

fn kind_fields(ui: &mut Ui, s: &mut Studio, ty: &str, objs: &[&Object], sel: &[Id]) {
    let each = |cmd: &str, f: &dyn Fn(Id) -> Value| sel.iter().map(|id| step(cmd, f(*id))).collect::<Vec<_>>();
    match ty {
        "floor" => {
            let th = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Floor { thickness, .. } => fmt_mm(*thickness),
                _ => String::new(),
            }));
            if let Some(v) = field(ui, "inspector.thickness", "floor_thickness", th)
                && let Some(mm) = mm_or_error(s, &v)
            {
                batch(s, each("floor.update", &|id| json!({ "id": id, "thickness": mm })));
            }
        }
        "wall" => {
            let get = |f: fn(&ObjectKind) -> f64| common(objs.iter().map(|o| fmt_mm(f(&o.kind))));
            let h = get(|k| if let ObjectKind::Wall { height, .. } = k { *height } else { 0.0 });
            let th = get(|k| if let ObjectKind::Wall { thickness, .. } = k { *thickness } else { 0.0 });
            if let Some(v) = field(ui, "inspector.height", "wall_height", h)
                && let Some(mm) = mm_or_error(s, &v)
            {
                batch(s, each("wall.update", &|id| json!({ "id": id, "height": mm })));
            }
            if let Some(v) = field(ui, "inspector.thickness", "wall_thickness", th)
                && let Some(mm) = mm_or_error(s, &v)
            {
                batch(s, each("wall.update", &|id| json!({ "id": id, "thickness": mm })));
            }
        }
        "truss" => {
            let profile = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Truss { profile, .. } => profile.clone(),
                _ => String::new(),
            }));
            ui.label(t("inspector.profile"));
            let shown = profile.clone().unwrap_or_else(|| t("inspector.mixed").to_owned());
            egui::ComboBox::from_id_salt("inspector_profile").selected_text(shown).show_ui(ui, |ui| {
                for p in stagecraft_geom::truss_profiles() {
                    if ui.selectable_label(profile.as_deref() == Some(&p.name), &p.name).clicked() {
                        batch(s, each("truss.update", &|id| json!({ "id": id, "profile": p.name })));
                    }
                }
            });
            ui.end_row();
            let len = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Truss { length, .. } => fmt_mm(*length),
                _ => String::new(),
            }));
            if let Some(v) = field(ui, "inspector.length", "truss_length", len)
                && let Some(mm) = mm_or_error(s, &v)
            {
                batch(s, each("truss.update", &|id| json!({ "id": id, "length": mm })));
            }
            if let Some(p) = profile.as_deref().and_then(stagecraft_geom::truss_profile) {
                ui.label("");
                ui.label(
                    RichText::new(format!("{} · {}×{} mm · {} kg/m", p.maker, p.width, p.height, p.kg_per_m)).weak(),
                );
                ui.end_row();
            }
        }
        "fixture" => {
            let get = |f: fn(&ObjectKind) -> String| common(objs.iter().map(|o| f(&o.kind)));
            let fid = get(|k| match k {
                ObjectKind::Fixture { fid, .. } => fid.map(|f| f.to_string()).unwrap_or_default(),
                _ => String::new(),
            });
            let cid = get(|k| match k {
                ObjectKind::Fixture { cid, .. } => cid.clone().unwrap_or_default(),
                _ => String::new(),
            });
            let mode = get(|k| match k {
                ObjectKind::Fixture { mode, .. } => mode.clone().unwrap_or_default(),
                _ => String::new(),
            });
            let addr = get(|k| match k {
                ObjectKind::Fixture { address, .. } => address.map(format_address).unwrap_or_default(),
                _ => String::new(),
            });
            if let Some(v) = field(ui, "inspector.fid", "fid", fid) {
                let v = v.trim();
                if v.is_empty() {
                    batch(s, each("fixture.update", &|id| json!({ "id": id, "fid": 0 })));
                } else if let Ok(n) = v.parse::<u32>() {
                    // Several fixtures cannot share a FID: number them from here.
                    if sel.len() == 1 {
                        let _ = s.run("fixture.update", json!({ "id": sel[0], "fid": n }));
                    } else {
                        let order = crate::table::plan_order(s.engine.doc(), sel);
                        let _ = s.run("fixture.number", json!({ "ids": order, "start": n }));
                    }
                } else {
                    s.ui.error(tf("status.bad_number", &[("text", &v)]));
                }
            }
            if let Some(v) = field(ui, "inspector.cid", "cid", cid) {
                batch(s, each("fixture.update", &|id| json!({ "id": id, "cid": v })));
            }
            if let Some(v) = field(ui, "inspector.mode", "mode", mode) {
                batch(s, each("fixture.update", &|id| json!({ "id": id, "mode": v })));
            }
            if let Some(v) = field(ui, "inspector.address", "address", addr) {
                batch(s, each("fixture.update", &|id| json!({ "id": id, "address": v.trim() })));
            }
        }
        _ => {}
    }
}

fn mm_or_error(s: &mut Studio, v: &str) -> Option<f64> {
    let r = parse_mm(v);
    if r.is_none() {
        s.ui.error(tf("status.bad_number", &[("text", &v)]));
    }
    r
}

fn tags_field(ui: &mut Ui, s: &mut Studio, doc: &stagecraft_core::Document, objs: &[&Object], sel: &[Id]) {
    ui.label(RichText::new(t("inspector.tags")).strong());
    let mut all: Vec<Id> = objs.iter().flat_map(|o| o.tags.iter().copied()).collect();
    all.sort();
    all.dedup();
    ui.horizontal_wrapped(|ui| {
        for tag in &all {
            let Some(tg) = doc.tags.get(tag) else { continue };
            let on_all = objs.iter().all(|o| o.tags.contains(tag));
            let text = if on_all { tg.name.clone() } else { format!("{} ({})", tg.name, t("inspector.some")) };
            let r = ui.button(format!("{text}  ✕"));
            widgets::name(&r, WidgetType::Button, format!("Remove tag {}", tg.name));
            if r.on_hover_text(t("inspector.remove_tag")).clicked() {
                let _ = s.run("object.untag", json!({ "ids": sel, "tags": [tag] }));
            }
        }
    });
    let id = egui::Id::new("inspector_add_tag");
    if let Some(name) = edit_field_hint(ui, id, "", 140.0, "Add tag", t("inspector.add_tag"))
        && !name.trim().is_empty()
    {
        let _ = s.run("object.tag", json!({ "ids": sel, "tags": [name.trim()] }));
    }
}

/// With nothing selected: the document's meta data.
fn document_fields(ui: &mut Ui, s: &mut Studio) {
    ui.label(RichText::new(t("inspector.document")).strong());
    let meta = s.engine.doc().meta.clone();
    egui::Grid::new("inspector_doc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, field_name, v) in [
            ("meta.project", "project", meta.project),
            ("meta.venue", "venue", meta.venue),
            ("meta.drawn_by", "drawn_by", meta.drawn_by),
            ("meta.revision", "revision", meta.revision),
        ] {
            if let Some(v) = field(ui, key, field_name, Some(v)) {
                let _ = s.run("doc.set_meta", json!({ field_name: v }));
            }
        }
    });
}
