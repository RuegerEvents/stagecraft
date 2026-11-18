//! What people see of a command: an English title (catalog key `cmd.<name>`, or the
//! entry's own key for presets), a group, an icon, a shortcut, whether it is a technical
//! command for AIs (hidden in the palette unless the query starts with `>`), and what it
//! needs before it can run (a selection of some kind, sheets, something to undo …).
//!
//! The table lives in the UI crate because titles, icons and shortcuts are GUI matters;
//! the registries (core and `ui.*`) stay as they are, so MCP and the CLI keep their IDs.
//! One row per registered command (the coverage test checks that), plus presets: a
//! command with fixed arguments (`ui.set_view {"view": "3d"}` is "3D View" with ⌘2),
//! the drawing tools, and the `app.*` actions of the menus that need file dialogs.

use serde_json::Value;
use stagecraft_core::Document;

use crate::i18n::{t, tf};

/// The palette's top level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    File,
    Edit,
    View,
    Draw,
    Sketch,
    Stage,
    Rigging,
    Lighting,
    Sheets,
    Underlays,
    Library,
    Organise,
    Help,
    /// Technical commands for AIs and scripts; only with `>`.
    Advanced,
}

impl Group {
    pub const ALL: [Group; 14] = [
        Group::File,
        Group::Edit,
        Group::View,
        Group::Draw,
        Group::Sketch,
        Group::Stage,
        Group::Rigging,
        Group::Lighting,
        Group::Sheets,
        Group::Underlays,
        Group::Library,
        Group::Organise,
        Group::Help,
        Group::Advanced,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Group::File => "group.file",
            Group::Edit => "group.edit",
            Group::View => "group.view",
            Group::Draw => "group.draw",
            Group::Sketch => "group.sketch",
            Group::Stage => "group.stage",
            Group::Rigging => "group.rigging",
            Group::Lighting => "group.lighting",
            Group::Sheets => "group.sheets",
            Group::Underlays => "group.underlays",
            Group::Library => "group.library",
            Group::Organise => "group.organise",
            Group::Help => "group.help",
            Group::Advanced => "group.advanced",
        }
    }

    pub fn title(self) -> &'static str {
        t(self.key())
    }

    pub fn icon(self) -> &'static str {
        match self {
            Group::File => "file",
            Group::Edit => "pencil",
            Group::View => "eye",
            Group::Draw => "pen-tool",
            Group::Sketch => "spline",
            Group::Stage => "portal",
            Group::Rigging => "hoist",
            Group::Lighting => "fixture",
            Group::Sheets => "sheet",
            Group::Underlays => "image",
            Group::Library => "library",
            Group::Organise => "layers",
            Group::Help => "circle-help",
            Group::Advanced => "terminal",
        }
    }
}

/// What a command needs before it can run, checked without running it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    Nothing,
    /// Selected objects, all of the entry's `applies` kinds when it names any.
    Selection,
    /// Exactly one selected object of the `applies` kinds.
    One,
    Undo,
    Redo,
    Sheets,
    Viewports,
    Tags,
    FixtureTypes,
    Fixtures,
    LabelTemplates,
    TitleBlocks,
    Dimensions,
}

#[derive(Clone, Copy, Debug)]
pub struct Meta {
    /// Command name, or an `app.*` action.
    pub id: &'static str,
    /// Fixed arguments (JSON) for a preset; `None` for the command itself.
    pub args: Option<&'static str>,
    /// Catalog key of the title; empty = `cmd.<id>`.
    title: &'static str,
    pub group: Group,
    pub icon: &'static str,
    pub hidden: bool,
    pub shortcut: Option<&'static str>,
    pub need: Need,
    /// Object kinds it works on: ranks it first when the selection is of these kinds.
    /// `*` = any object.
    pub applies: &'static [&'static str],
    /// Run at once with the arguments filled in from the selection (no form).
    pub direct: bool,
}

const fn m(id: &'static str, group: Group, icon: &'static str) -> Meta {
    Meta {
        id,
        args: None,
        title: "",
        group,
        icon,
        hidden: false,
        shortcut: None,
        need: Need::Nothing,
        applies: &[],
        direct: false,
    }
}

impl Meta {
    const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
    const fn key(mut self, s: &'static str) -> Self {
        self.shortcut = Some(s);
        self
    }
    const fn need(mut self, n: Need) -> Self {
        self.need = n;
        self
    }
    const fn on(mut self, kinds: &'static [&'static str]) -> Self {
        self.applies = kinds;
        self
    }
    /// Needs a selection of these kinds.
    const fn sel(self, kinds: &'static [&'static str]) -> Self {
        self.on(kinds).need(Need::Selection)
    }
    /// Needs one selected object of these kinds.
    const fn one(self, kinds: &'static [&'static str]) -> Self {
        self.on(kinds).need(Need::One)
    }
    const fn direct(mut self) -> Self {
        self.direct = true;
        self
    }
    /// A preset: this command with these arguments, under its own title.
    const fn preset(mut self, args: &'static str, title: &'static str) -> Self {
        self.args = Some(args);
        self.title = title;
        self
    }
    const fn titled(mut self, title: &'static str) -> Self {
        self.title = title;
        self
    }

    /// Catalog key of the title.
    pub fn title_key(&self) -> String {
        if self.title.is_empty() { format!("cmd.{}", self.id) } else { self.title.to_owned() }
    }

    pub fn title(&self) -> &'static str {
        t(&self.title_key())
    }

    /// Stable key for recents: id plus preset arguments.
    pub fn key_str(&self) -> String {
        match self.args {
            Some(a) => format!("{} {a}", self.id),
            None => self.id.to_owned(),
        }
    }

    pub fn args_value(&self) -> Option<Value> {
        self.args.map(|a| serde_json::from_str(a).expect("preset arguments are JSON"))
    }

    /// The registry's one-line summary, for the description line.
    pub fn summary(&self) -> &'static str {
        let own = format!("{}.desc", self.title_key());
        if crate::i18n::has(&own) {
            return t(&own);
        }
        if self.args.is_some() {
            return "";
        }
        if let Some(e) = crate::commands::ui_registry().get(self.id) {
            return e.summary;
        }
        stagecraft_core::registry().get(self.id).map_or("", |e| e.summary)
    }
}

use Group::*;

const ANY: &[&str] = &["*"];
const FIX: &[&str] = &["fixture"];
const HANGERS: &[&str] = &["fixture", "truss", "truss_run", "truss_node", "batten", "curtain", "hoist", "model"];
const SKETCHED: &[&str] = &["wall", "floor"];

/// Every entry. Base rows (no preset arguments) first for each command.
pub static META: &[Meta] = &[
    // File
    m("app.new", File, "file-plus").titled("cmd.app.new").key("⌘N"),
    m("app.open", File, "folder-open").titled("cmd.app.open").key("⌘O"),
    m("app.save", File, "save").titled("cmd.app.save").key("⌘S"),
    m("app.save_as", File, "save").titled("cmd.app.save_as").key("⇧⌘S"),
    m("app.import_mvr", File, "file-input").titled("cmd.app.import_mvr"),
    m("app.import_model", File, "file-input").titled("cmd.app.import_model"),
    m("app.export_mvr", File, "file-output").titled("cmd.app.export_mvr"),
    m("app.export_pdf", File, "printer").titled("cmd.app.export_pdf").need(Need::Sheets),
    m("doc.new", File, "file-plus").hidden(),
    m("doc.open", File, "folder-open").hidden(),
    m("doc.save", File, "save").hidden(),
    m("doc.info", File, "info"),
    m("mvr.import", File, "file-input").hidden(),
    m("mvr.export", File, "file-output").hidden(),
    m("model.import", File, "file-input").hidden(),
    m("sheet.export_pdf", File, "printer").hidden(),
    // Edit
    m("history.undo", Edit, "undo").key("⌘Z").need(Need::Undo),
    m("history.redo", Edit, "redo").key("⇧⌘Z").need(Need::Redo),
    m("history.list", Edit, "history").hidden(),
    m("object.delete", Edit, "trash-2").key("⌫").sel(ANY).direct(),
    m("object.duplicate", Edit, "copy").sel(ANY),
    m("object.move", Edit, "move").sel(ANY),
    m("object.rotate", Edit, "rotate").sel(ANY),
    m("object.rename", Edit, "text-cursor-input").one(ANY),
    m("object.set_placement", Edit, "crosshair").one(ANY),
    m("object.attach", Edit, "link").sel(HANGERS),
    m("object.detach", Edit, "unlink").sel(ANY).direct(),
    m("object.set_attrs", Edit, "braces").sel(ANY).hidden(),
    m("object.set_style", Edit, "pen-tool").sel(ANY),
    m("object.get", Edit, "info").hidden(),
    m("object.list", Edit, "list").hidden(),
    m("app.select_all", Edit, "square-dashed-mouse-pointer").titled("cmd.app.select_all").key("⌘A"),
    m("selection.clear", Edit, "x").need(Need::Selection).on(ANY),
    m("selection.set", Edit, "square-dashed-mouse-pointer").hidden(),
    m("selection.add", Edit, "square-dashed-mouse-pointer").hidden(),
    m("selection.get", Edit, "square-dashed-mouse-pointer").hidden(),
    m("ui.set_tool", Draw, "select").hidden(),
    m("ui.set_tool", Edit, "select").preset(r#"{"tool":"select"}"#, "cmd.tool.select").key("V"),
    m("ui.set_tool", Edit, "move").preset(r#"{"tool":"move"}"#, "cmd.tool.move").key("M"),
    m("ui.set_tool", Edit, "rotate").preset(r#"{"tool":"rotate"}"#, "cmd.tool.rotate").key("R"),
    // View
    m("ui.set_view", View, "eye").hidden(),
    m("ui.set_view", View, "square").preset(r#"{"view":"plan"}"#, "cmd.view.plan").key("⌘1"),
    m("ui.set_view", View, "box").preset(r#"{"view":"3d"}"#, "cmd.view.3d").key("⌘2"),
    m("ui.set_view", View, "columns-2").preset(r#"{"view":"split"}"#, "cmd.view.split").key("⌘3"),
    m("ui.set_view", Sheets, "sheet").preset(r#"{"view":"sheets"}"#, "cmd.view.sheets").key("⌘4"),
    m("ui.zoom_to_fit", View, "fit").key("⌘0"),
    m("ui.zoom_to_fit", View, "scan-search").preset(r#"{"selection":true}"#, "cmd.view.zoom_selection").sel(ANY),
    m("ui.zoom", View, "zoom-in"),
    m("ui.pan", View, "hand").hidden(),
    m("ui.camera", View, "camera").hidden(),
    m("ui.camera", View, "camera").preset(r#"{"view":"top"}"#, "cmd.camera.top"),
    m("ui.camera", View, "camera").preset(r#"{"view":"front"}"#, "cmd.camera.front"),
    m("ui.camera", View, "camera").preset(r#"{"view":"back"}"#, "cmd.camera.back"),
    m("ui.camera", View, "camera").preset(r#"{"view":"side"}"#, "cmd.camera.side"),
    m("ui.camera", View, "camera").preset(r#"{"view":"left"}"#, "cmd.camera.left"),
    m("ui.camera", View, "camera").preset(r#"{"view":"iso"}"#, "cmd.camera.iso"),
    m("ui.section", View, "scissors"),
    m("ui.section", View, "scissors").preset(r#"{"axis":"x"}"#, "cmd.section.x"),
    m("ui.section", View, "scissors").preset(r#"{"axis":"y"}"#, "cmd.section.y"),
    m("ui.section", View, "scissors").preset(r#"{"axis":"z"}"#, "cmd.section.z"),
    m("ui.section", View, "scissors").preset(r#"{"off":true}"#, "cmd.section.off"),
    m("ui.set_theme", View, "sun-moon").hidden(),
    m("ui.set_theme", View, "sun").preset(r#"{"theme":"light"}"#, "cmd.theme.light"),
    m("ui.set_theme", View, "moon").preset(r#"{"theme":"dark"}"#, "cmd.theme.dark"),
    m("ui.set_theme", View, "sun-moon").preset(r#"{"theme":"system"}"#, "cmd.theme.system"),
    m("ui.toggle_panel", View, "app-window").hidden(),
    m("ui.toggle_panel", View, "app-window").preset(r#"{"panel":"tools"}"#, "cmd.panel.tools"),
    m("ui.toggle_panel", View, "layers").preset(r#"{"panel":"layers"}"#, "cmd.panel.layers"),
    m("ui.toggle_panel", View, "tags").preset(r#"{"panel":"tags"}"#, "cmd.panel.tags"),
    m("ui.toggle_panel", View, "sheet").preset(r#"{"panel":"sheets"}"#, "cmd.panel.sheets"),
    m("ui.toggle_panel", View, "library").preset(r#"{"panel":"library"}"#, "cmd.panel.library"),
    m("ui.toggle_panel", View, "cloud-download").preset(r#"{"panel":"share"}"#, "cmd.panel.share"),
    m("ui.toggle_panel", View, "sliders").preset(r#"{"panel":"inspector"}"#, "cmd.panel.inspector"),
    m("ui.toggle_panel", View, "table").preset(r#"{"panel":"table"}"#, "cmd.panel.table"),
    m("ui.reset_layout", View, "app-window"),
    m("ui.layout", Advanced, "braces").hidden(),
    m("ui.float_panel", Advanced, "app-window").hidden(),
    m("ui.set_snap", Draw, "magnet"),
    m("ui.set_tool_options", Draw, "settings-2").hidden(),
    // Draw
    m("ui.set_tool", Draw, "wall").preset(r#"{"tool":"wall"}"#, "cmd.tool.wall").key("W"),
    m("ui.set_tool", Draw, "floor").preset(r#"{"tool":"floor"}"#, "cmd.tool.floor").key("F"),
    m("ui.set_tool", Draw, "dimension").preset(r#"{"tool":"dimension"}"#, "cmd.tool.dimension").key("D"),
    m("wall.create", Draw, "wall").hidden(),
    m("wall.update", Draw, "wall").one(&["wall"]),
    m("floor.create", Draw, "floor").hidden(),
    m("floor.update", Draw, "floor").one(&["floor"]),
    // Sketch
    m("ui.sketch", Sketch, "spline"),
    m("sketch.adopt", Sketch, "spline").sel(SKETCHED),
    m("sketch.unlink", Sketch, "unlink").sel(SKETCHED).direct(),
    m("sketch.solve", Sketch, "refresh-cw"),
    m("sketch.status", Sketch, "info"),
    m("sketch.link", Sketch, "link").sel(&["proscenium", "batten", "curtain", "truss"]).hidden(),
    m("sketch.make_wall", Sketch, "wall").hidden(),
    m("sketch.make_floor", Sketch, "floor").hidden(),
    m("sketch.add_point", Sketch, "map-pin").hidden(),
    m("sketch.add_line", Sketch, "spline").hidden(),
    m("sketch.add_arc", Sketch, "spline").hidden(),
    m("sketch.add_polyline", Sketch, "spline").hidden(),
    m("sketch.add_constraint", Sketch, "lock").hidden(),
    m("sketch.remove_constraint", Sketch, "lock").hidden(),
    m("sketch.remove", Sketch, "trash-2").hidden(),
    m("sketch.drag", Sketch, "move").hidden(),
    m("sketch.draw", Sketch, "pen-tool").hidden(),
    m("sketch.draw_line", Sketch, "pen-tool").hidden(),
    m("sketch.edit", Sketch, "braces").hidden(),
    m("sketch.get", Sketch, "braces").hidden(),
    m("dimension.set_value", Sketch, "dimension").need(Need::Dimensions),
    m("dimension.name", Sketch, "dimension").need(Need::Dimensions),
    m("dimension.set_driving", Sketch, "dimension").need(Need::Dimensions),
    m("dimension.list", Sketch, "list").hidden(),
    m("dimension.add", Sketch, "dimension").hidden(),
    m("dimension.add_reference", Sketch, "dimension").hidden(),
    m("dimension.move_label", Sketch, "dimension").hidden(),
    m("dimension.remove", Sketch, "dimension").hidden(),
    m("ui.dimension", Sketch, "dimension").hidden(),
    m("ui.sketch_pick", Sketch, "spline").hidden(),
    // Stage
    m("ui.set_tool", Stage, "portal").preset(r#"{"tool":"proscenium"}"#, "cmd.tool.proscenium"),
    m("ui.set_tool", Stage, "curtain").preset(r#"{"tool":"curtain"}"#, "cmd.tool.curtain").key("C"),
    m("ui.set_tool", Stage, "wings").preset(r#"{"tool":"wings"}"#, "cmd.tool.wings"),
    m("ui.set_tool", Stage, "platform").preset(r#"{"tool":"platform"}"#, "cmd.tool.platform").key("P"),
    m("ui.set_tool", Stage, "ceiling").preset(r#"{"tool":"ceiling"}"#, "cmd.tool.ceiling"),
    m("proscenium.create", Stage, "portal"),
    m("proscenium.update", Stage, "portal").one(&["proscenium"]),
    m("curtain.create", Stage, "curtain"),
    m("curtain.update", Stage, "curtain").one(&["curtain"]),
    m("wingset.create", Stage, "wings"),
    m("masking.update", Stage, "wings").one(&["masking"]),
    m("platform.create", Stage, "platform"),
    m("platform.update", Stage, "platform").one(&["platform"]),
    m("ceiling.create", Stage, "ceiling").hidden(),
    m("ceiling.update", Stage, "ceiling").one(&["ceiling"]),
    // Rigging
    m("ui.set_tool", Rigging, "batten").preset(r#"{"tool":"batten"}"#, "cmd.tool.batten").key("B"),
    m("ui.set_tool", Rigging, "truss").preset(r#"{"tool":"truss"}"#, "cmd.tool.truss").key("T"),
    m("ui.set_tool", Rigging, "run").preset(r#"{"tool":"run"}"#, "cmd.tool.run").key("U"),
    m("ui.set_tool", Rigging, "hoist").preset(r#"{"tool":"hoist"}"#, "cmd.tool.hoist").key("H"),
    m("ui.set_tool", Rigging, "support").preset(r#"{"tool":"support"}"#, "cmd.tool.support"),
    m("truss.create", Rigging, "truss"),
    m("truss.run", Rigging, "run").hidden(),
    m("truss.node", Rigging, "truss"),
    m("truss.join", Rigging, "combine").sel(&["truss", "truss_node"]).direct(),
    m("truss.update", Rigging, "truss").one(&["truss"]),
    m("batten.create", Rigging, "batten"),
    m("batten.update", Rigging, "batten").one(&["batten"]),
    m("batten.list", Rigging, "list").hidden(),
    m("hoist.create", Rigging, "hoist"),
    m("hoist.update", Rigging, "hoist").one(&["hoist"]),
    m("support.create", Rigging, "support"),
    m("support.update", Rigging, "support").one(&["support"]),
    m("load.report", Rigging, "weight").on(&["truss", "truss_run", "batten", "hoist"]),
    // Lighting & Patch
    m("ui.set_tool", Lighting, "fixture").preset(r#"{"tool":"fixture"}"#, "cmd.tool.fixture").key("L"),
    m("fixture.create", Lighting, "fixture"),
    m("fixture.update", Lighting, "fixture").one(FIX),
    m("fixture.set_type", Lighting, "fixture").sel(FIX).need(Need::FixtureTypes),
    m("fixture.number", Lighting, "hash").sel(FIX),
    m("patch.auto", Lighting, "wand-sparkles").on(FIX).need(Need::Fixtures),
    m("patch.check", Lighting, "triangle-alert").need(Need::Fixtures),
    m("ui.set_label_template", Lighting, "tag"),
    // Sheets
    m("sheet.create", Sheets, "plus"),
    m("sheet.update", Sheets, "sheet").need(Need::Sheets),
    m("sheet.delete", Sheets, "trash-2").need(Need::Sheets),
    m("sheet.export_svg", Sheets, "file-output").need(Need::Sheets),
    m("sheet.get", Sheets, "braces").hidden(),
    m("sheet.list", Sheets, "list").hidden(),
    m("ui.sheet", Sheets, "sheet").need(Need::Sheets),
    m("viewport.create", Sheets, "frame").need(Need::Sheets),
    m("viewport.update", Sheets, "frame").need(Need::Viewports),
    m("viewport.delete", Sheets, "trash-2").need(Need::Viewports),
    m("doc.set_meta", Sheets, "layout-template"),
    m("label.create", Sheets, "tag"),
    m("label.update", Sheets, "tag").need(Need::LabelTemplates),
    m("label.delete", Sheets, "trash-2").need(Need::LabelTemplates),
    m("label.list", Sheets, "list").hidden(),
    m("label.move", Sheets, "move").hidden(),
    m("titleblock.create", Sheets, "layout-template"),
    m("titleblock.update", Sheets, "layout-template").need(Need::TitleBlocks),
    m("titleblock.delete", Sheets, "trash-2").need(Need::TitleBlocks),
    m("titleblock.list", Sheets, "list").hidden(),
    // Underlays & Notes
    m("app.import_underlay", Underlays, "image").titled("cmd.app.import_underlay"),
    m("ui.set_tool", Underlays, "calibrate").preset(r#"{"tool":"calibrate"}"#, "cmd.tool.calibrate").key("K"),
    m("underlay.import", Underlays, "image").hidden(),
    m("underlay.update", Underlays, "image").one(&["underlay"]),
    m("underlay.calibrate", Underlays, "calibrate").hidden(),
    m("underlay.align", Underlays, "move").hidden(),
    m("note.create", Underlays, "sticky-note"),
    m("note.update", Underlays, "sticky-note").one(&["note"]),
    // Library & GDTF
    m("app.import_gdtf", Library, "file-input").titled("cmd.app.import_gdtf"),
    m("ui.share_search", Library, "cloud-download"),
    m("ui.share_add", Library, "cloud-download").hidden(),
    m("gdtf.import", Library, "file-input").hidden(),
    m("gdtf.library", Library, "library").hidden(),
    m("gdtf.remove", Library, "trash-2").need(Need::FixtureTypes),
    m("symbol.assign", Library, "fixture").need(Need::FixtureTypes),
    m("symbol.list", Library, "list").hidden(),
    // Tags & Layers
    m("layer.create", Organise, "plus"),
    m("layer.update", Organise, "layers"),
    m("layer.delete", Organise, "trash-2"),
    m("layer.list", Organise, "list").hidden(),
    m("ui.set_active_layer", Organise, "layers"),
    m("object.set_layer", Organise, "layers").sel(ANY),
    m("tag.create", Organise, "plus"),
    m("tag.update", Organise, "tag").need(Need::Tags),
    m("tag.delete", Organise, "trash-2").need(Need::Tags),
    m("tag.list", Organise, "list").hidden(),
    m("object.tag", Organise, "tag").sel(ANY),
    m("object.untag", Organise, "tag").sel(ANY),
    m("ui.set_tag_visible", Organise, "eye").need(Need::Tags),
    // Help
    m("ui.tutorial", Help, "graduation-cap").hidden(),
    m("ui.tutorial", Help, "graduation-cap").preset(r#"{"action":"start"}"#, "cmd.tutorial.start"),
    m("app.about", Help, "info").titled("cmd.app.about"),
    m("ui.palette", Help, "search").hidden(),
    // Advanced
    m("batch", Advanced, "braces").hidden(),
    m("journal.list", Advanced, "history").hidden(),
    m("ui.state", Advanced, "braces").hidden(),
    m("ui.screenshot", Advanced, "camera").hidden(),
    m("ui.file_dialog", Advanced, "folder-open").hidden(),
];

/// The base row of a command (no preset arguments).
pub fn of(id: &str) -> Option<&'static Meta> {
    META.iter().find(|m| m.id == id && m.args.is_none())
}

/// What the palette knows about the document and selection this frame, to tell what can
/// run without running it.
pub struct Context {
    /// Selected objects' kinds (`type_name`), one per object.
    pub kinds: Vec<&'static str>,
    pub undo: bool,
    pub redo: bool,
    pub sheets: usize,
    pub viewports: usize,
    pub tags: usize,
    pub fixture_types: usize,
    pub fixtures: usize,
    pub label_templates: usize,
    pub title_blocks: usize,
    pub dimensions: bool,
}

impl Context {
    pub fn of(s: &crate::Studio) -> Self {
        let doc: &Document = s.engine.doc();
        let kinds =
            s.engine.selection().iter().filter_map(|id| doc.objects.get(id)).map(|o| o.kind.type_name()).collect();
        Context {
            kinds,
            undo: s.engine.can_undo(),
            redo: s.engine.can_redo(),
            sheets: doc.sheets.len(),
            viewports: doc.sheets.values().map(|sh| sh.viewports.len()).sum(),
            tags: doc.tags.len(),
            fixture_types: doc.fixture_types.len(),
            fixtures: doc.objects.values().filter(|o| o.kind.type_name() == "fixture").count(),
            label_templates: doc.label_templates.len(),
            title_blocks: doc.title_blocks.len(),
            dimensions: !doc.dimensions.is_empty() || doc.sketches.values().any(|sk| !sk.dimensions.is_empty()),
        }
    }

    /// The selection is non-empty and every object is of one of `kinds` (`*` = any).
    pub fn selection_is(&self, kinds: &[&str]) -> bool {
        !self.kinds.is_empty() && (kinds.contains(&"*") || self.kinds.iter().all(|k| kinds.contains(k)))
    }

    /// The kind every selected object has, if they share one.
    pub fn common_kind(&self) -> Option<&'static str> {
        let first = *self.kinds.first()?;
        self.kinds.iter().all(|k| *k == first).then_some(first)
    }
}

fn kind_plural(kind: &str) -> String {
    t(&format!("type.{kind}.plural")).to_lowercase()
}

fn kinds_text(kinds: &[&str]) -> Option<String> {
    if kinds.is_empty() || kinds.contains(&"*") {
        return None;
    }
    let names: Vec<String> = kinds.iter().take(2).map(|k| kind_plural(k)).collect();
    Some(names.join(t("palette.need.or")))
}

/// `Ok` when the entry can run now, else why not ("Select fixtures first").
pub fn available(m: &Meta, c: &Context) -> Result<(), String> {
    let miss = |key: &str| Err(t(key).to_owned());
    match m.need {
        Need::Nothing => Ok(()),
        Need::Selection if c.selection_is(m.applies) || (m.applies.is_empty() && !c.kinds.is_empty()) => Ok(()),
        Need::Selection => match kinds_text(m.applies) {
            Some(k) => Err(tf("palette.need.kinds", &[("kinds", &k)])),
            None => miss("palette.need.objects"),
        },
        Need::One if c.kinds.len() == 1 && c.selection_is(if m.applies.is_empty() { &["*"] } else { m.applies }) => {
            Ok(())
        }
        Need::One => match m.applies.first().filter(|k| **k != "*") {
            Some(k) => Err(tf("palette.need.one", &[("kind", &t(&format!("type.{k}")).to_lowercase())])),
            None => miss("palette.need.one_object"),
        },
        Need::Undo if c.undo => Ok(()),
        Need::Undo => miss("palette.need.undo"),
        Need::Redo if c.redo => Ok(()),
        Need::Redo => miss("palette.need.redo"),
        Need::Sheets if c.sheets > 0 => Ok(()),
        Need::Sheets => miss("palette.need.sheets"),
        Need::Viewports if c.viewports > 0 => Ok(()),
        Need::Viewports => miss("palette.need.viewports"),
        Need::Tags if c.tags > 0 => Ok(()),
        Need::Tags => miss("palette.need.tags"),
        Need::FixtureTypes if c.fixture_types > 0 => match m.applies.first() {
            Some(_) if !c.selection_is(m.applies) => Err(tf(
                "palette.need.kinds",
                &[("kinds", &kinds_text(m.applies).unwrap_or_else(|| t("palette.need.objects").to_owned()))],
            )),
            _ => Ok(()),
        },
        Need::FixtureTypes => miss("palette.need.types"),
        Need::Fixtures if c.fixtures > 0 => Ok(()),
        Need::Fixtures => miss("palette.need.fixtures"),
        Need::LabelTemplates if c.label_templates > 0 => Ok(()),
        Need::LabelTemplates => miss("palette.need.labels"),
        Need::TitleBlocks if c.title_blocks > 0 => Ok(()),
        Need::TitleBlocks => miss("palette.need.titleblocks"),
        Need::Dimensions if c.dimensions => Ok(()),
        Need::Dimensions => miss("palette.need.dimensions"),
    }
}

/// Whether the entry is about what is selected: it names the selection's kinds.
pub fn fits_selection(m: &Meta, c: &Context) -> bool {
    !m.applies.is_empty() && !m.applies.contains(&"*") && c.selection_is(m.applies)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered() -> Vec<&'static str> {
        let mut v: Vec<&str> = stagecraft_core::registry().entries().iter().map(|e| e.name).collect();
        v.extend(crate::commands::ui_registry().entries().iter().map(|e| e.name));
        v
    }

    /// Every registered command has a title and a group; every row's title and icon exist.
    #[test]
    fn every_command_has_a_title_and_a_group() {
        let mut missing = Vec::new();
        for name in registered() {
            match of(name) {
                None => missing.push(format!("{name}: no row in META")),
                Some(m) if !crate::i18n::has(&m.title_key()) => missing.push(format!("{name}: no {}", m.title_key())),
                Some(_) => {}
            }
        }
        let icons: Vec<&str> = crate::icons::names().collect();
        for m in META {
            if !crate::i18n::has(&m.title_key()) {
                missing.push(format!("{}: no title {}", m.key_str(), m.title_key()));
            }
            if !icons.contains(&m.icon) {
                missing.push(format!("{}: no icon {}", m.key_str(), m.icon));
            }
            if let Some(a) = m.args {
                assert!(serde_json::from_str::<Value>(a).is_ok(), "{a}");
            }
            let known = m.id.starts_with("app.")
                || stagecraft_core::registry().get(m.id).is_some()
                || crate::commands::ui_registry().get(m.id).is_some();
            if !known {
                missing.push(format!("{}: not a command", m.id));
            }
        }
        for g in Group::ALL {
            if !crate::i18n::has(g.key()) || !icons.contains(&g.icon()) {
                missing.push(format!("group {g:?}"));
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    /// A visible command whose schema needs object ids says so, so the palette greys it
    /// out instead of failing.
    #[test]
    fn commands_on_objects_need_a_selection() {
        for m in META.iter().filter(|m| !m.hidden && m.args.is_none() && !m.id.starts_with("app.")) {
            let schema = crate::palette::schema_of(m.id);
            let req: Vec<&str> =
                schema["required"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let uuid = |f: &str| {
                let p = &schema["properties"][f];
                p["format"] == "uuid" || p["items"]["format"] == "uuid"
            };
            if req.contains(&"ids") && uuid("ids") {
                assert_eq!(m.need, Need::Selection, "{}", m.id);
            }
            if req.contains(&"id") && uuid("id") {
                assert_eq!(m.need, Need::One, "{}", m.id);
            }
        }
    }

    #[test]
    fn availability_follows_the_selection() {
        let mut c = Context {
            kinds: vec![],
            undo: false,
            redo: false,
            sheets: 0,
            viewports: 0,
            tags: 0,
            fixture_types: 1,
            fixtures: 8,
            label_templates: 0,
            title_blocks: 0,
            dimensions: false,
        };
        let number = of("fixture.number").unwrap();
        assert_eq!(available(number, &c), Err("Select fixtures first".into()));
        c.kinds = vec!["fixture"; 8];
        assert!(available(number, &c).is_ok());
        assert!(fits_selection(number, &c));
        assert_eq!(available(of("batten.update").unwrap(), &c), Err("Select one batten first".into()));
        assert_eq!(available(of("history.undo").unwrap(), &c), Err("Nothing to undo".into()));
        assert!(available(of("patch.auto").unwrap(), &c).is_ok());
    }
}
