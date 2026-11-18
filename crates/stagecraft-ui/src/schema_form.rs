//! Argument forms generated from a command's JSON Schema (schemars, draft 2020-12).
//! Each property becomes a field: text, number with its unit (mm, °, kg), checkbox,
//! drop-down for enums, x/y/z boxes for points, a picker for objects, layers, tags,
//! sheets, viewports and fixture types, or (only for nested structures) JSON. The form
//! turns the filled-in text back into JSON arguments.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use stagecraft_core::{Document, Id};

/// What a picker field chooses from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Layer,
    Tag,
    Sheet,
    Viewport,
    FixtureType,
    LabelTemplate,
    TitleBlock,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Bool,
    /// One of these strings.
    Choice(Vec<String>),
    /// A list of names, typed comma-separated.
    List,
    /// Nested structures, typed as JSON (or plain text, taken as a string).
    Json,
    /// A value of several simple forms (an address `1.001` or `17`, a view name): typed
    /// as text, numbers stay numbers.
    Value,
    /// A fixed number of numbers: a point (x, y[, z]) or a rotation.
    Point(usize),
    /// One object (UUID), picked from the document.
    Object,
    /// Objects (UUIDs): the selection.
    Objects,
    /// A layer, tag, sheet … by id or name.
    Ref(RefKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
    pub description: String,
    /// `mm`, `°` or `kg`, read from the description and name.
    pub unit: Option<&'static str>,
    /// The schema's default, as the field's text.
    pub default: Option<String>,
    /// `null` is allowed: an empty field means "leave unchanged".
    pub nullable: bool,
}

impl FieldSpec {
    /// Structures only an expert types (shown under "More options" when optional).
    pub fn is_advanced(&self) -> bool {
        matches!(self.kind, FieldKind::Json)
    }
}

fn resolve<'a>(root: &'a Value, v: &'a Value) -> &'a Value {
    match v.get("$ref").and_then(Value::as_str) {
        Some(r) => r.strip_prefix("#/$defs/").and_then(|name| root.pointer(&format!("/$defs/{name}"))).unwrap_or(v),
        None => v,
    }
}

/// The type names of a schema node, without "null".
fn types(v: &Value) -> Vec<String> {
    match v.get("type") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => {
            a.iter().filter_map(|x| x.as_str()).filter(|s| *s != "null").map(String::from).collect()
        }
        _ => Vec::new(),
    }
}

fn nullable(v: &Value) -> bool {
    match v.get("type") {
        Some(Value::Array(a)) => a.iter().any(|x| x == "null"),
        _ => v
            .get("anyOf")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|x| x.get("type") == Some(&json!("null")))),
    }
}

fn kind_of(root: &Value, v: &Value) -> FieldKind {
    let v = resolve(root, v);
    // Option<T> as anyOf [T, null].
    if let Some(any) = v.get("anyOf").and_then(Value::as_array) {
        let non_null: Vec<&Value> = any.iter().filter(|x| x.get("type") != Some(&json!("null"))).collect();
        if non_null.len() == 1 {
            return kind_of(root, non_null[0]);
        }
        if non_null.iter().any(|x| types(resolve(root, x)) == ["string"] || types(resolve(root, x)) == ["integer"]) {
            return FieldKind::Value;
        }
        return FieldKind::Json;
    }
    if let Some(one) = v.get("oneOf").and_then(Value::as_array) {
        let consts: Vec<String> = one
            .iter()
            .filter_map(|x| {
                x.get("const")
                    .and_then(Value::as_str)
                    .or_else(|| x.get("enum").and_then(|e| e.get(0)).and_then(Value::as_str))
                    .map(String::from)
            })
            .collect();
        if consts.len() == one.len() {
            return FieldKind::Choice(consts);
        }
        return FieldKind::Json;
    }
    if let Some(e) = v.get("enum").and_then(Value::as_array) {
        return FieldKind::Choice(e.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    }
    let uuid = v.get("format") == Some(&json!("uuid"));
    match types(v).first().map(String::as_str) {
        Some("string") if uuid => FieldKind::Object,
        Some("string") => FieldKind::Text,
        Some("number") => FieldKind::Number,
        Some("integer") => FieldKind::Integer,
        Some("boolean") => FieldKind::Bool,
        Some("array") => {
            let item = v.get("items").map(|i| resolve(root, i));
            let n = v.get("maxItems").and_then(Value::as_u64);
            match item {
                Some(i) if types(i) == ["string"] && i.get("format") == Some(&json!("uuid")) => FieldKind::Objects,
                Some(i) if types(i) == ["string"] => FieldKind::List,
                Some(i)
                    if types(i) == ["number"]
                        && n.is_some_and(|n| (2..=3).contains(&n))
                        && v.get("minItems").and_then(Value::as_u64) == n =>
                {
                    FieldKind::Point(n.unwrap_or(3) as usize)
                }
                _ => FieldKind::Json,
            }
        }
        _ => FieldKind::Json,
    }
}

fn unit_of(name: &str, kind: &FieldKind, description: &str) -> Option<&'static str> {
    let d = description.to_lowercase();
    if !matches!(kind, FieldKind::Number | FieldKind::Integer | FieldKind::Point(_)) {
        return None;
    }
    if name.contains("kg") || d.starts_with("kg") || d.contains(" kg") {
        Some("kg")
    } else if name.contains("rotation") || name == "angle" || d.contains("degree") || d.contains('°') {
        Some("°")
    } else if d.contains("mm") {
        Some("mm")
    } else {
        None
    }
}

fn default_text(v: &Value) -> Option<String> {
    match v.get("default")? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        }),
        Value::Bool(b) => Some(b.to_string()),
        Value::Array(a) if !a.is_empty() && a.iter().all(Value::is_number) => {
            Some(a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
        }
        _ => None,
    }
}

/// The fields of a command's argument schema, required ones first.
pub fn fields(schema: &Value) -> Vec<FieldSpec> {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut out: Vec<FieldSpec> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .map(|(name, v)| {
                    let description = v
                        .get("description")
                        .or_else(|| resolve(schema, v).get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let kind = kind_of(schema, v);
                    let kind = match (name.as_str(), kind) {
                        ("layer" | "move_objects_to", FieldKind::Text) => FieldKind::Ref(RefKind::Layer),
                        ("tag", FieldKind::Text) => FieldKind::Ref(RefKind::Tag),
                        ("sheet", FieldKind::Text) => FieldKind::Ref(RefKind::Sheet),
                        ("viewport", FieldKind::Object) => FieldKind::Ref(RefKind::Viewport),
                        ("gdtf" | "gdtf_type", FieldKind::Text) => FieldKind::Ref(RefKind::FixtureType),
                        (_, k) => k,
                    };
                    FieldSpec {
                        unit: unit_of(name, &kind, &description),
                        name: name.clone(),
                        required: required.contains(&name.as_str()),
                        default: default_text(v),
                        nullable: nullable(v),
                        description,
                        kind,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|f| (!f.required, rank(&f.name)));
    out
}

/// Where a field goes in the form: what it acts on, then names and geometry, then sizes,
/// then the rest; layer, parent and tags last.
fn rank(name: &str) -> usize {
    const FIRST: &[&str] = &[
        "ids",
        "id",
        "template",
        "tag",
        "sheet",
        "viewport",
        "layer_id",
        "type",
        "gdtf_type",
        "name",
        "text",
        "view",
        "start",
        "end",
        "points",
        "outline",
        "path",
        "position",
        "at",
        "x",
        "y",
        "z",
        "delta",
        "angle",
        "pivot",
        "offset",
        "rotation",
        "width",
        "depth",
        "height",
        "length",
        "trim",
        "thickness",
        "opening",
        "frame",
        "profile",
        "preset",
        "system",
        "gdtf",
        "mode",
        "fid",
        "cid",
        "address",
        "step",
        "gap",
        "count",
        "spacing",
    ];
    const LAST: &[&str] = &["layer", "parent", "tags"];
    if let Some(i) = FIRST.iter().position(|n| *n == name) {
        i
    } else if let Some(i) = LAST.iter().position(|n| *n == name) {
        1000 + i
    } else {
        500
    }
}

/// [`fields`] with what only the command's name tells: `template` is a label template
/// for `label.*` and a title block for `titleblock.*`, `type` a fixture type for
/// `gdtf.remove`.
pub fn fields_for(command: &str, schema: &Value) -> Vec<FieldSpec> {
    let mut f = fields(schema);
    for x in &mut f {
        x.kind = match (command.split('.').next(), x.name.as_str(), &x.kind) {
            (Some("label"), "template", FieldKind::Text) => FieldKind::Ref(RefKind::LabelTemplate),
            (Some("titleblock"), "template", FieldKind::Text) => FieldKind::Ref(RefKind::TitleBlock),
            (Some("gdtf"), "type", FieldKind::Text) => FieldKind::Ref(RefKind::FixtureType),
            _ => x.kind.clone(),
        };
    }
    f
}

/// What a picker offers: (value, shown text).
pub fn options(doc: &Document, kind: RefKind) -> Vec<(String, String)> {
    match kind {
        RefKind::Layer => doc.layers.values().map(|l| (l.name.clone(), l.name.clone())).collect(),
        RefKind::Tag => {
            let mut v: Vec<_> = doc.tags.values().map(|t| (t.name.clone(), t.name.clone())).collect();
            v.sort_by_key(|a| a.1.to_lowercase());
            v
        }
        RefKind::Sheet => doc
            .sheets
            .values()
            .map(|s| {
                (
                    s.id.to_string(),
                    if s.number.is_empty() { s.name.clone() } else { format!("{} {}", s.number, s.name) },
                )
            })
            .collect(),
        RefKind::Viewport => doc
            .sheets
            .values()
            .flat_map(|s| s.viewports.iter().map(move |v| (v.id.to_string(), format!("{} · {}", s.number, v.name))))
            .collect(),
        RefKind::FixtureType => doc.fixture_types.keys().map(|k| (k.clone(), k.replace('@', " "))).collect(),
        RefKind::LabelTemplate => doc.label_templates.values().map(|l| (l.id.to_string(), l.name.clone())).collect(),
        RefKind::TitleBlock => doc.title_blocks.values().map(|b| (b.id.to_string(), b.name.clone())).collect(),
    }
}

/// An object's name for pickers: its name, or kind and FID.
pub fn object_label(doc: &Document, id: Id) -> String {
    let Some(o) = doc.objects.get(&id) else { return id.to_string() };
    let kind = crate::i18n::t(&format!("type.{}", o.kind.type_name()));
    let fid = match &o.kind {
        stagecraft_core::ObjectKind::Fixture { fid: Some(f), .. } => format!(" · {f}"),
        _ => String::new(),
    };
    if o.name.is_empty() { format!("{kind}{fid}") } else { format!("{}{fid}", o.name) }
}

/// Objects a `parent`/`id` field may pick: things others hang on first.
pub fn object_options(doc: &Document, name: &str, exclude: &[Id]) -> Vec<(String, String)> {
    let carriers = ["truss", "truss_run", "batten", "hoist", "truss_node", "platform"];
    let mut v: Vec<(String, String)> = doc
        .objects
        .values()
        .filter(|o| !exclude.contains(&o.id))
        .filter(|o| name != "parent" || carriers.contains(&o.kind.type_name()))
        .map(|o| (o.id.to_string(), object_label(doc, o.id)))
        .collect();
    v.sort_by_key(|a| a.1.to_lowercase());
    v.truncate(400);
    v
}

/// The field's label: from the catalog (`arg.<name>`), else the name in words.
pub fn label(name: &str) -> String {
    let key = format!("arg.{name}");
    if crate::i18n::has(&key) {
        return crate::i18n::t(&key).to_owned();
    }
    let words = name.replace('_', " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Turn the typed text into arguments. Empty optional fields are left out.
pub fn build_args(specs: &[FieldSpec], values: &BTreeMap<String, String>) -> Result<Value, String> {
    let mut m = Map::new();
    for f in specs {
        let text = values.get(&f.name).map(|s| s.trim()).unwrap_or_default();
        let empty = match f.kind {
            FieldKind::Point(_) => text.split(',').all(|p| p.trim().is_empty()),
            _ => text.is_empty(),
        };
        if empty {
            if f.required && f.kind != FieldKind::Bool {
                return Err(crate::i18n::tf("form.required", &[("field", &label(&f.name))]));
            }
            continue;
        }
        let num = |t: &str| t.trim().replace(',', ".").parse::<f64>();
        let v = match &f.kind {
            FieldKind::Text | FieldKind::Choice(_) | FieldKind::Object | FieldKind::Ref(_) => json!(text),
            FieldKind::Number => {
                json!(num(text).map_err(|_| crate::i18n::tf("form.not_number", &[("field", &label(&f.name))]))?)
            }
            FieldKind::Integer => json!(
                text.parse::<i64>().map_err(|_| crate::i18n::tf("form.not_integer", &[("field", &label(&f.name))]))?
            ),
            FieldKind::Bool => json!(text == "true"),
            FieldKind::Point(n) => {
                let parts: Vec<&str> = text.split(',').collect();
                let mut out = Vec::with_capacity(*n);
                for i in 0..*n {
                    let p = parts.get(i).map(|s| s.trim()).unwrap_or("");
                    let x = if p.is_empty() {
                        0.0
                    } else {
                        num(p).map_err(|_| crate::i18n::tf("form.not_number", &[("field", &label(&f.name))]))?
                    };
                    out.push(x);
                }
                json!(out)
            }
            FieldKind::List | FieldKind::Objects => match serde_json::from_str::<Value>(text) {
                Ok(v @ Value::Array(_)) => v,
                _ => json!(text.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
            },
            // Lists, objects and whole numbers as JSON; "1.001" stays text (an address).
            FieldKind::Json | FieldKind::Value
                if text.starts_with(['[', '{']) || text.parse::<i64>().is_ok() || text == "true" || text == "false" =>
            {
                serde_json::from_str(text).map_err(|e| format!("{}: {e}", label(&f.name)))?
            }
            FieldKind::Json | FieldKind::Value => json!(text),
        };
        m.insert(f.name.clone(), v);
    }
    Ok(Value::Object(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(name: &str) -> Value {
        stagecraft_core::registry().get(name).unwrap().schema()
    }

    #[test]
    fn create_commands_get_typed_fields() {
        let f = fields(&schema("fixture.create"));
        let by = |n: &str| f.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(by("fid").kind, FieldKind::Integer);
        assert_eq!(by("name").kind, FieldKind::Text);
        assert_eq!(by("tags").kind, FieldKind::List);
        assert_eq!(by("position").kind, FieldKind::Point(3));
        assert_eq!(by("position").unit, Some("mm"));
        assert_eq!(by("rotation").unit, Some("°"));
        assert_eq!(by("address").kind, FieldKind::Value);
        assert_eq!(by("layer").kind, FieldKind::Ref(RefKind::Layer));
        assert_eq!(by("gdtf").kind, FieldKind::Ref(RefKind::FixtureType));
        assert_eq!(by("parent").kind, FieldKind::Object);
        assert!(!by("fid").required);
        let d = fields(&schema("object.delete"));
        assert_eq!(d[0].name, "ids");
        assert_eq!(d[0].kind, FieldKind::Objects);
        assert!(d[0].required);
        assert_eq!(
            d.iter().find(|x| x.name == "children").unwrap().kind,
            FieldKind::Choice(vec!["detach".into(), "delete".into()])
        );
        let l = fields(&schema("layer.create"));
        assert_eq!(l.iter().find(|x| x.name == "elevation").unwrap().kind, FieldKind::Number);
        let t = fields(&schema("truss.create"));
        assert_eq!(t.iter().find(|x| x.name == "profile").unwrap().default.as_deref(), Some("F34"));
        let lt = fields_for("label.update", &stagecraft_core::registry().get("label.update").unwrap().schema());
        assert_eq!(lt.iter().find(|x| x.name == "template").unwrap().kind, FieldKind::Ref(RefKind::LabelTemplate));
    }

    #[test]
    fn args_from_text() {
        let f = fields(&schema("fixture.create"));
        let mut v = BTreeMap::new();
        v.insert("fid".to_string(), "101".to_string());
        v.insert("address".to_string(), "1.001".to_string());
        v.insert("position".to_string(), "1000, , 0".to_string());
        v.insert("tags".to_string(), "light, front".to_string());
        let a = build_args(&f, &v).unwrap();
        assert_eq!(
            a,
            json!({ "fid": 101, "address": "1.001", "position": [1000.0, 0.0, 0.0], "tags": ["light", "front"] })
        );
        let l = fields(&schema("layer.create"));
        assert_eq!(build_args(&l, &BTreeMap::new()), Err("Name is required".into()));
    }
}
