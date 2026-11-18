//! The context inspector: only the fields that fit the selection. With several objects
//! selected, a field shows a value when all agree, and an edit applies to all of them
//! (one undo step through `batch`).

use egui::{RichText, Ui};
use serde_json::{Value, json};
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
    crate::widgets::prop_label(ui, t(key));
    let id = egui::Id::unique(("inspector", salt));
    let mixed = current.is_none();
    let hint = if mixed { t("inspector.mixed") } else { "" };
    let out =
        edit_field_hint(ui, id, &current.unwrap_or_default(), f32::INFINITY, &format!("Inspector {}", t(key)), hint);
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
    let sel: Vec<Id> = s.engine.selection().to_vec();
    let doc = s.engine.doc().clone();
    let objs: Vec<&Object> = sel.iter().filter_map(|id| doc.objects.get(id)).collect();
    if objs.is_empty() {
        if s.ui.sketch.selected.is_none() && s.ui.sketch.picked.is_empty() {
            ui.label(RichText::new(t("inspector.nothing")).weak());
            ui.add_space(8.0);
        }
        crate::sketch_ui::inspector(ui, s);
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
    crate::widgets::section(ui, title);
    ui.add_space(4.0);

    // Roots and objects whose parent is not selected: absolute edits apply to them,
    // their children follow.
    let tops: Vec<Id> =
        sel.iter().copied().filter(|id| !sel.iter().any(|o| o != id && doc.is_ancestor(*o, *id))).collect();
    let worlds: Vec<(Id, stagecraft_core::Placement)> =
        tops.iter().filter_map(|id| stagecraft_core::geom::world_placement(&doc, *id).map(|w| (*id, w))).collect();

    egui::Grid::new("inspector_common").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        let name = common(objs.iter().map(|o| o.name.clone()));
        if let Some(v) = field(ui, "inspector.name", "name", name) {
            batch(s, sel.iter().map(|id| step("object.rename", json!({ "id": id, "name": v }))).collect());
        }

        crate::widgets::prop_label(ui, t("inspector.layer"));
        let layer = common(objs.iter().map(|o| o.layer));
        let shown =
            layer.and_then(|l| doc.layers.get(&l)).map_or_else(|| t("inspector.mixed").to_owned(), |l| l.name.clone());
        let r = crate::widgets::combo_w(ui.available_width() - 12.0, "inspector_layer").selected_text(shown).show_ui(
            ui,
            |ui| {
                for l in doc.layers_sorted() {
                    if ui.selectable_label(layer == Some(l.id), &l.name).clicked() {
                        let _ = s.run("object.set_layer", json!({ "ids": sel, "layer": l.id }));
                    }
                }
            },
        );
        widgets::name(&r.response, egui::Role::ComboBox, format!("Inspector {}", t("inspector.layer")));
        ui.end_row();

        // Floors, walls and ceilings name their height themselves ("Top height" …) below.
        let own_height = ty.is_some_and(|t| matches!(t, "floor" | "wall" | "ceiling"));
        for (axis, key) in [(0usize, "inspector.x"), (1, "inspector.y"), (2, "inspector.z")] {
            if axis == 2 && own_height {
                continue;
            }
            // Heights are above the object's layer, as in the tool options.
            let shown = |id: &Id, w: &stagecraft_core::Placement| {
                if axis == 2 { w.position[2] - layer_elevation(&doc, *id) } else { w.position[axis] }
            };
            let cur = common(worlds.iter().map(|(id, w)| fmt_mm(shown(id, w))));
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
                        d[axis] = mm - shown(id, w);
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
        crate::widgets::rule(ui);
        egui::Grid::new("inspector_kind").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
            kind_fields(ui, s, ty, &objs, &sel);
        });
    }

    if let [o] = objs.as_slice() {
        load_section(ui, &doc, o);
    }

    tags_field(ui, s, &doc, &objs, &sel);

    let parents: Vec<Id> = objs.iter().filter_map(|o| o.parent).collect();
    if !parents.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let names: Vec<String> =
                parents.iter().filter_map(|p| doc.objects.get(p).map(|o| o.name.clone())).collect();
            let shown = common(names.iter().cloned()).unwrap_or_else(|| t("inspector.mixed").to_owned());
            ui.label(tf("inspector.attached_to", &[("name", &shown)]));
            if let Some(&p) = common(parents.iter())
                && button(ui, t("inspector.select_parent"), "Select parent").clicked()
            {
                let _ = s.run("selection.set", json!({ "ids": [p] }));
            }
            if button(ui, t("inspector.detach"), "Detach").clicked() {
                let ids: Vec<Id> = objs.iter().filter(|o| o.parent.is_some()).map(|o| o.id).collect();
                let _ = s.run("object.detach", json!({ "ids": ids }));
            }
        });
    }
}

/// Elevation of the object's layer: the datum its heights are measured from.
fn layer_elevation(doc: &stagecraft_core::Document, id: Id) -> f64 {
    doc.objects.get(&id).and_then(|o| doc.layers.get(&o.layer)).map_or(0.0, |l| l.elevation)
}

/// A height above the layer (world z minus the layer's elevation) of the selected
/// objects, edited as a vertical move: a floor's top, a wall's base.
fn height_row(ui: &mut Ui, s: &mut Studio, sel: &[Id], key: &str, salt: &str) {
    let doc = s.engine.doc();
    let above =
        |id: &Id| stagecraft_core::geom::world_placement(doc, *id).map(|w| w.position[2] - layer_elevation(doc, *id));
    let heights: Vec<(Id, f64)> = sel.iter().filter_map(|id| above(id).map(|z| (*id, z))).collect();
    let cur = common(heights.iter().map(|(_, z)| fmt_mm(*z)));
    if let Some(v) = field(ui, key, salt, cur)
        && let Some(mm) = mm_or_error(s, &v)
    {
        let steps = heights
            .iter()
            .filter(|(_, z)| (mm - z).abs() > 1e-9)
            .map(|(id, z)| step("object.move", json!({ "ids": [id], "delta": [0.0, 0.0, mm - z] })))
            .collect();
        batch(s, steps);
    }
}

/// Under a height row: where that is in the world when the layer is raised or lowered.
fn world_note(ui: &mut Ui, s: &Studio, objs: &[&Object]) {
    let doc = s.engine.doc();
    let Some(layer) = common(objs.iter().map(|o| o.layer)).and_then(|l| doc.layers.get(&l)) else { return };
    if layer.elevation == 0.0 {
        return;
    }
    let world = |o: &&Object| stagecraft_core::geom::world_placement(doc, o.id).map_or(0.0, |w| w.position[2]);
    let Some(z) = common(objs.iter().map(|o| fmt_mm(world(o)))) else { return };
    crate::widgets::prop_label(ui, "");
    let note = tf("inspector.world_height", &[("z", &z), ("layer", &layer.name), ("e", &fmt_mm(layer.elevation))]);
    ui.label(RichText::new(note).weak().small());
    ui.end_row();
}

fn kind_fields(ui: &mut Ui, s: &mut Studio, ty: &str, objs: &[&Object], sel: &[Id]) {
    let each = |cmd: &str, f: &dyn Fn(Id) -> Value| sel.iter().map(|id| step(cmd, f(*id))).collect::<Vec<_>>();
    match ty {
        "floor" => {
            height_row(ui, s, sel, "inspector.top_height", "floor_top");
            world_note(ui, s, objs);
            let th = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Floor { thickness, .. } => fmt_mm(*thickness),
                _ => String::new(),
            }));
            if let Some(v) = field(ui, "inspector.thickness", "floor_thickness", th)
                && let Some(mm) = mm_or_error(s, &v)
            {
                batch(s, each("floor.update", &|id| json!({ "id": id, "thickness": mm })));
            }
            crate::sketch_ui::link_row(ui, s, sel);
        }
        "wall" => {
            let get = |f: fn(&ObjectKind) -> f64| common(objs.iter().map(|o| fmt_mm(f(&o.kind))));
            let h = get(|k| if let ObjectKind::Wall { height, .. } = k { *height } else { 0.0 });
            let th = get(|k| if let ObjectKind::Wall { thickness, .. } = k { *thickness } else { 0.0 });
            height_row(ui, s, sel, "inspector.base_height", "wall_base");
            world_note(ui, s, objs);
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
            crate::sketch_ui::link_row(ui, s, sel);
        }
        "truss" => {
            let profile = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Truss { profile, .. } => profile.clone(),
                _ => String::new(),
            }));
            crate::widgets::prop_label(ui, t("inspector.profile"));
            let shown = profile.clone().unwrap_or_else(|| t("inspector.mixed").to_owned());
            let r = crate::widgets::combo_w(ui.available_width() - 12.0, "inspector_profile")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for p in stagecraft_geom::truss_profiles() {
                        if ui.selectable_label(profile.as_deref() == Some(&p.name), &p.name).clicked() {
                            batch(s, each("truss.update", &|id| json!({ "id": id, "profile": p.name })));
                        }
                    }
                });
            widgets::name(&r.response, egui::Role::ComboBox, format!("Inspector {}", t("inspector.profile")));
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
                crate::widgets::prop_label(ui, "");
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
            let doc = s.engine.doc().clone();
            let ty = get(|k| match k {
                ObjectKind::Fixture { gdtf, .. } => gdtf.clone().unwrap_or_default(),
                _ => String::new(),
            });
            let mode = common(objs.iter().map(|o| stagecraft_sheet::labels::mode_name(&doc, o)));
            let addr = common(objs.iter().map(|o| stagecraft_core::patch::address_text(o)));
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
            let mut types: Vec<(String, String)> = vec![(String::new(), t("inspector.no_type").to_owned())];
            types.extend(doc.fixture_types.values().map(|ft| (ft.key.clone(), ft.display_name())));
            choice(ui, s, sel, "inspector.fixture_type", "fixture_type", ty.clone(), &types, "fixture.update", "gdtf");
            if let Some(ft) = ty.as_deref().and_then(|k| doc.fixture_types.get(k)) {
                let modes: Vec<(String, String)> = ft
                    .modes
                    .iter()
                    .map(|m| (m.name.clone(), format!("{} · {} ch", m.name, m.footprint_text())))
                    .collect();
                choice(ui, s, sel, "inspector.mode", "fixture_mode", mode, &modes, "fixture.update", "mode");
            }
            if let Some(v) = field(ui, "inspector.address", "address", addr) {
                match crate::table::address_args(&v) {
                    Ok((first, extra)) => batch(
                        s,
                        each("fixture.update", &|id| json!({ "id": id, "address": first, "extra_addresses": extra })),
                    ),
                    Err(e) => s.ui.error(e),
                }
            }
            if sel.len() == 1 {
                let o = &doc.objects[&sel[0]];
                let fp = stagecraft_sheet::labels::footprint_text(&doc, o);
                if !fp.is_empty() {
                    crate::widgets::prop_label(ui, t("col.footprint"));
                    ui.label(fp);
                    ui.end_row();
                }
                if let Some(p) = stagecraft_core::patch::conflicts(&doc).remove(&sel[0]) {
                    crate::widgets::prop_label(ui, "");
                    ui.label(RichText::new(crate::table::problem_text(&doc, &p)).color(crate::theme::of(ui).warning));
                    ui.end_row();
                }
            }
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.weight",
                "fixture_weight",
                |o| match o.kind {
                    ObjectKind::Fixture { weight_kg, .. } => Some(weight_kg.unwrap_or(0.0)),
                    _ => None,
                },
                "fixture.update",
                "weight_kg",
            );
        }
        _ => crate::inspector::stage_fields(ui, s, ty, objs, sel),
    }
}

/// A number field over the selection: shown when all agree, applied to all.
#[allow(clippy::too_many_arguments)]
fn num(
    ui: &mut Ui,
    s: &mut Studio,
    objs: &[&Object],
    sel: &[Id],
    key: &str,
    salt: &str,
    get: impl Fn(&Object) -> Option<f64>,
    cmd: &str,
    arg: &str,
) {
    let cur = common(objs.iter().map(|o| get(o).map(fmt_mm).unwrap_or_default()));
    if let Some(v) = field(ui, key, salt, cur)
        && let Some(x) = mm_or_error(s, &v)
    {
        // Whole numbers go as integers so counts (batten numbers) parse too.
        let x = if x.fract() == 0.0 && x.abs() < 1e15 { json!(x as i64) } else { json!(x) };
        batch(s, sel.iter().map(|id| step(cmd, json!({ "id": id, arg: x }))).collect());
    }
}

/// A choice among fixed values (the JSON value and its label).
#[allow(clippy::too_many_arguments)]
fn choice(
    ui: &mut Ui,
    s: &mut Studio,
    sel: &[Id],
    key: &str,
    salt: &str,
    current: Option<String>,
    options: &[(String, String)],
    cmd: &str,
    arg: &str,
) {
    crate::widgets::prop_label(ui, t(key));
    let shown = current
        .as_ref()
        .and_then(|c| options.iter().find(|(v, _)| v == c).map(|o| o.1.clone()))
        .unwrap_or_else(|| t("inspector.mixed").to_owned());
    let r = crate::widgets::combo_w(ui.available_width() - 12.0, ("inspector_choice", salt))
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (v, label) in options {
                if ui.selectable_label(current.as_deref() == Some(v), label).clicked() {
                    batch(s, sel.iter().map(|id| step(cmd, json!({ "id": id, arg: v }))).collect());
                }
            }
        });
    widgets::name(&r.response, egui::Role::ComboBox, format!("Inspector {}", t(key)));
    ui.end_row();
}

fn flag(ui: &mut Ui, s: &mut Studio, sel: &[Id], key: &str, current: Option<bool>, cmd: &str, arg: &str) {
    crate::widgets::prop_label(ui, t(key));
    let mut on = current.unwrap_or(false);
    let r = ui.checkbox(&mut on, "");
    widgets::name(&r, egui::Role::CheckBox, format!("Inspector {}", t(key)));
    if r.changed() {
        batch(s, sel.iter().map(|id| step(cmd, json!({ "id": id, arg: on }))).collect());
    }
    ui.end_row();
}

fn colour(ui: &mut Ui, s: &mut Studio, sel: &[Id], current: Option<String>, cmd: &str) {
    crate::widgets::prop_label(ui, t("inspector.colour"));
    let mut rgb = current.as_deref().and_then(stagecraft_core::model::parse_color).unwrap_or([128, 128, 128]);
    let r = ui.color_edit_button_srgb(&mut rgb);
    widgets::name(&r, egui::Role::ColorWell, "Inspector colour");
    if r.changed() {
        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        batch(s, sel.iter().map(|id| step(cmd, json!({ "id": id, "color": hex }))).collect());
    }
    ui.end_row();
}

fn opts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(v, k)| ((*v).to_owned(), t(k).to_owned())).collect()
}

/// The value of a kind's enum field as its JSON string.
fn json_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Fields of the block-4 stage objects.
pub(crate) fn stage_fields(ui: &mut Ui, s: &mut Studio, ty: &str, objs: &[&Object], sel: &[Id]) {
    macro_rules! f {
        ($kind:pat => $v:expr) => {
            |o: &Object| match &o.kind {
                $kind => Some($v),
                _ => None,
            }
        };
    }
    let all_str = |f: &dyn Fn(&Object) -> Option<String>| common(objs.iter().map(|o| f(o).unwrap_or_default()));
    let all_bool = |f: &dyn Fn(&Object) -> Option<bool>| common(objs.iter().map(|o| f(o).unwrap_or(false)));
    match ty {
        "proscenium" => {
            let c = "proscenium.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.width",
                "pr_w",
                f!(ObjectKind::Proscenium { width, .. } => *width),
                c,
                "width",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.height",
                "pr_h",
                f!(ObjectKind::Proscenium { height, .. } => *height),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.depth",
                "pr_d",
                f!(ObjectKind::Proscenium { depth, .. } => *depth),
                c,
                "depth",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.frame",
                "pr_f",
                f!(ObjectKind::Proscenium { frame, .. } => *frame),
                c,
                "frame",
            );
            let show = all_bool(&f!(ObjectKind::Proscenium { show_iron, .. } => *show_iron));
            flag(ui, s, sel, "inspector.show_iron", show, c, "show_iron");
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.iron",
                "pr_i",
                f!(ObjectKind::Proscenium { iron, .. } => *iron),
                c,
                "iron",
            );
        }
        "curtain" => {
            let c = "curtain.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.width",
                "cu_w",
                f!(ObjectKind::Curtain { width, .. } => *width),
                c,
                "width",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.height",
                "cu_h",
                f!(ObjectKind::Curtain { height, .. } => *height),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.fullness",
                "cu_f",
                f!(ObjectKind::Curtain { fullness, .. } => *fullness),
                c,
                "fullness",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.opening",
                "cu_o",
                f!(ObjectKind::Curtain { opening, .. } => *opening),
                c,
                "opening",
            );
            let g = all_str(&f!(ObjectKind::Curtain { gather, .. } => json_str(gather)));
            let gathers = opts(&[("tab", "gather.tab"), ("italian", "gather.italian")]);
            choice(ui, s, sel, "inspector.gather", "cu_g", g, &gathers, c, "gather");
            let split = all_bool(&f!(ObjectKind::Curtain { split, .. } => *split));
            flag(ui, s, sel, "inspector.split", split, c, "split");
            colour(ui, s, sel, all_str(&f!(ObjectKind::Curtain { color, .. } => color.clone())), c);
        }
        "masking" => {
            let c = "masking.update";
            let r = all_str(&f!(ObjectKind::Masking { role, .. } => json_str(role)));
            let roles = opts(&[("leg", "masking.leg"), ("border", "masking.border")]);
            choice(ui, s, sel, "inspector.role", "ma_r", r, &roles, c, "role");
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.width",
                "ma_w",
                f!(ObjectKind::Masking { width, .. } => *width),
                c,
                "width",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.height",
                "ma_h",
                f!(ObjectKind::Masking { height, .. } => *height),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.fullness",
                "ma_f",
                f!(ObjectKind::Masking { fullness, .. } => *fullness),
                c,
                "fullness",
            );
            colour(ui, s, sel, all_str(&f!(ObjectKind::Masking { color, .. } => color.clone())), c);
        }
        "batten" => {
            let c = "batten.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.number",
                "ba_n",
                f!(ObjectKind::Batten { number, .. } => f64::from(*number)),
                c,
                "number",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.position_y",
                "ba_y",
                |o| matches!(o.kind, ObjectKind::Batten { .. }).then_some(o.placement.position[1]),
                c,
                "y",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.trim",
                "ba_t",
                |o| matches!(o.kind, ObjectKind::Batten { .. }).then_some(o.placement.position[2]),
                c,
                "trim",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.length",
                "ba_l",
                f!(ObjectKind::Batten { length, .. } => *length),
                c,
                "length",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.max_load",
                "ba_m",
                f!(ObjectKind::Batten { max_load, .. } => *max_load),
                c,
                "max_load",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.kg_per_m",
                "ba_k",
                f!(ObjectKind::Batten { kg_per_m, .. } => *kg_per_m),
                c,
                "kg_per_m",
            );
        }
        "platform" => {
            let c = "platform.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.width",
                "pl_w",
                f!(ObjectKind::Platform { width, .. } => *width),
                c,
                "width",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.depth",
                "pl_d",
                f!(ObjectKind::Platform { depth, .. } => *depth),
                c,
                "depth",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.height",
                "pl_h",
                f!(ObjectKind::Platform { height, .. } => *height),
                c,
                "height",
            );
            let u = all_str(&f!(ObjectKind::Platform { under, .. } => json_str(under)));
            let unders = opts(&[("legs", "under.legs"), ("scissor", "under.scissor"), ("none", "under.none")]);
            choice(ui, s, sel, "inspector.under", "pl_u", u, &unders, c, "under");
            let sys = all_str(&f!(ObjectKind::Platform { system, .. } => system.clone().unwrap_or_default()));
            let mut systems = vec![(String::new(), t("option.none").to_owned())];
            systems.extend(stagecraft_geom::platform_systems().into_iter().map(|p| (p.name.clone(), p.name)));
            choice(ui, s, sel, "inspector.system", "pl_s", sys, &systems, c, "system");
        }
        "support" => {
            let c = "support.update";
            let st = all_str(&f!(ObjectKind::Support { style, .. } => json_str(style)));
            let styles = opts(&[
                ("base_plate", "support.base_plate"),
                ("stand", "support.stand"),
                ("ground_support", "support.ground_support"),
            ]);
            choice(ui, s, sel, "inspector.style", "su_s", st, &styles, c, "style");
            num(ui, s, objs, sel, "inspector.size", "su_z", f!(ObjectKind::Support { size, .. } => *size), c, "size");
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.height",
                "su_h",
                f!(ObjectKind::Support { height, .. } => *height),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.weight",
                "su_w",
                f!(ObjectKind::Support { weight_kg, .. } => *weight_kg),
                c,
                "weight_kg",
            );
        }
        "hoist" => {
            let c = "hoist.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.capacity",
                "ho_c",
                f!(ObjectKind::Hoist { capacity_kg, .. } => *capacity_kg),
                c,
                "capacity_kg",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.weight",
                "ho_w",
                f!(ObjectKind::Hoist { weight_kg, .. } => *weight_kg),
                c,
                "weight_kg",
            );
        }
        "ceiling" => {
            let c = "ceiling.update";
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.underside",
                "ce_h",
                |o| matches!(o.kind, ObjectKind::Ceiling { .. }).then_some(o.placement.position[2]),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.thickness",
                "ce_t",
                f!(ObjectKind::Ceiling { thickness, .. } => *thickness),
                c,
                "thickness",
            );
            let show = all_bool(&f!(ObjectKind::Ceiling { show_in_plan, .. } => *show_in_plan));
            flag(ui, s, sel, "inspector.show_in_plan", show, c, "show_in_plan");
        }
        "underlay" => {
            let c = "underlay.update";
            let op = common(objs.iter().map(|o| match &o.kind {
                ObjectKind::Underlay { opacity, .. } => format!("{:.0}", opacity * 100.0),
                _ => String::new(),
            }));
            if let Some(v) = field(ui, "inspector.opacity", "ul_op", op) {
                match v.trim().trim_end_matches('%').trim().replace(',', ".").parse::<f64>() {
                    Ok(x) if (0.0..=100.0).contains(&x) => {
                        batch(s, sel.iter().map(|id| step(c, json!({ "id": id, "opacity": x / 100.0 }))).collect())
                    }
                    _ => s.ui.error(tf("status.bad_number", &[("text", &v)])),
                }
            }
            let locked = all_bool(&f!(ObjectKind::Underlay { locked, .. } => *locked));
            flag(ui, s, sel, "inspector.locked", locked, c, "locked");
            let visible = common(objs.iter().map(|o| !o.style.hidden));
            flag(ui, s, sel, "inspector.visible", visible, c, "visible");
            if let [o] = objs
                && let ObjectKind::Underlay { format, extent: [lo, hi], scale, .. } = &o.kind
            {
                let size =
                    format!("{:.2} × {:.2} m", (hi[0] - lo[0]) * scale / 1000.0, (hi[1] - lo[1]) * scale / 1000.0);
                crate::widgets::prop_label(ui, t("inspector.size"));
                ui.label(RichText::new(format!("{} · {size}", json_str(format))).weak());
                ui.end_row();
            }
            crate::widgets::prop_label(ui, "");
            let unlocked = all_bool(&f!(ObjectKind::Underlay { locked, .. } => *locked)) != Some(true);
            if ui.add_enabled(unlocked, egui::Button::new(t("inspector.calibrate"))).clicked() {
                s.ui.set_tool(crate::state::Tool::Calibrate);
            }
            ui.end_row();
        }
        "note" => {
            let c = "note.update";
            let text = all_str(&f!(ObjectKind::Note { text, .. } => text.replace('\n', " / ")));
            if let Some(v) = field(ui, "inspector.text", "note_text", text) {
                let v = v.replace(" / ", "\n");
                batch(s, sel.iter().map(|id| step(c, json!({ "id": id, "text": v }))).collect());
            }
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.text_height",
                "note_h",
                f!(ObjectKind::Note { height, .. } => *height),
                c,
                "height",
            );
            num(
                ui,
                s,
                objs,
                sel,
                "inspector.wrap_width",
                "note_w",
                f!(ObjectKind::Note { width, .. } => *width),
                c,
                "width",
            );
            let frame = all_bool(&f!(ObjectKind::Note { frame, .. } => *frame));
            flag(ui, s, sel, "inspector.frame", frame, c, "frame");
        }
        "truss_node" | "truss_run" | "wing_set" => {
            if let [o] = objs {
                let info = match &o.kind {
                    ObjectKind::TrussNode { profile, arms, down, up } => {
                        let n = arms.len() + usize::from(*down) + usize::from(*up);
                        tf("inspector.node_info", &[("profile", profile), ("n", &n)])
                    }
                    ObjectKind::TrussRun { profile } => {
                        tf("inspector.run_info", &[("profile", profile), ("n", &s.engine.doc().children(o.id).len())])
                    }
                    _ => tf("inspector.set_info", &[("n", &s.engine.doc().children(o.id).len())]),
                };
                crate::widgets::prop_label(ui, "");
                ui.label(RichText::new(info).weak());
                ui.end_row();
                if o.kind.is_group() {
                    crate::widgets::prop_label(ui, "");
                    if button(ui, t("inspector.select_members"), "Select members").clicked() {
                        let kids = s.engine.doc().children(o.id);
                        let _ = s.run("selection.set", json!({ "ids": kids }));
                    }
                    ui.end_row();
                }
            }
        }
        _ => {}
    }
}

/// Own weight, what hangs on it, and the limit, for one selected object.
fn load_section(ui: &mut Ui, doc: &stagecraft_core::Document, o: &Object) {
    let relevant = matches!(
        o.kind,
        ObjectKind::Truss { .. }
            | ObjectKind::TrussRun { .. }
            | ObjectKind::TrussNode { .. }
            | ObjectKind::Batten { .. }
            | ObjectKind::Hoist { .. }
            | ObjectKind::Support { .. }
    ) || !doc.children(o.id).is_empty();
    if !relevant {
        return;
    }
    let rep = stagecraft_core::load::report(doc, &[o.id]);
    let row = rep.trusses.iter().chain(&rep.runs).chain(&rep.battens).chain(&rep.hoists).next().cloned();
    let (own, attached, total, unknown, limit, over) = match row {
        Some(r) => (r.own_kg, r.attached_kg, r.total_kg, r.unknown_weights, r.limit_kg, r.over),
        None => {
            let l = stagecraft_core::load::load_of(doc, o.id);
            (l.own_kg, l.attached_kg, l.total_kg(), l.unknown, None, false)
        }
    };
    crate::widgets::section(ui, t("inspector.load"));
    let kg = |v: f64| format!("{v:.1} kg");
    egui::Grid::new("inspector_load").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        crate::widgets::prop_label(ui, t("load.own"));
        ui.label(kg(own));
        ui.end_row();
        crate::widgets::prop_label(ui, t("load.attached"));
        let text = RichText::new(kg(attached));
        ui.label(if over { text.color(ui.visuals().error_fg_color).strong() } else { text });
        ui.end_row();
        crate::widgets::prop_label(ui, t("load.total"));
        ui.label(RichText::new(kg(total)).strong());
        ui.end_row();
        if let Some(m) = limit {
            crate::widgets::prop_label(ui, t("load.limit"));
            ui.label(kg(m));
            ui.end_row();
        }
    });
    if over {
        ui.label(RichText::new(t("load.over")).color(ui.visuals().error_fg_color));
    }
    if unknown > 0 {
        ui.label(RichText::new(tf("load.unknown", &[("n", &unknown)])).weak());
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
    crate::widgets::section(ui, t("inspector.tags"));
    let mut all: Vec<Id> = objs.iter().flat_map(|o| o.tags.iter().copied()).collect();
    all.sort();
    all.dedup();
    ui.horizontal_wrapped(|ui| {
        for tag in &all {
            let Some(tg) = doc.tags.get(tag) else { continue };
            let on_all = objs.iter().all(|o| o.tags.contains(tag));
            let text = if on_all { tg.name.clone() } else { format!("{} ({})", tg.name, t("inspector.some")) };
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(10)
                .inner_margin(egui::Margin { left: 8, right: 2, top: 1, bottom: 1 })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(text);
                    let r = widgets::icon_button(ui, widgets::Icon::Close, &format!("Remove tag {}", tg.name));
                    if r.on_hover_text(t("inspector.remove_tag")).clicked() {
                        let _ = s.run("object.untag", json!({ "ids": sel, "tags": [tag] }));
                    }
                });
        }
    });
    let id = egui::Id::unique("inspector_add_tag");
    if let Some(name) = edit_field_hint(ui, id, "", f32::INFINITY, "Add tag", t("inspector.add_tag"))
        && !name.trim().is_empty()
    {
        let _ = s.run("object.tag", json!({ "ids": sel, "tags": [name.trim()] }));
    }
}

/// With nothing selected: the document's meta data.
fn document_fields(ui: &mut Ui, s: &mut Studio) {
    crate::widgets::section(ui, t("inspector.document"));
    let meta = s.engine.doc().meta.clone();
    egui::Grid::new("inspector_doc").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        for (key, field_name, v) in crate::sheets::meta_texts(&meta) {
            if let Some(v) = field(ui, key, field_name, Some(v)) {
                let _ = s.run("doc.set_meta", json!({ field_name: crate::sheets::meta_value(field_name, v) }));
            }
        }
        crate::sheets::sheet_text_rows(ui, s, "Inspector");
    });
}
