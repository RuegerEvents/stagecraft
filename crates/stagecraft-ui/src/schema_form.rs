//! Argument forms generated from a command's JSON Schema (schemars, draft 2020-12).
//! Each property becomes a text field, a checkbox or a choice; the form turns the
//! filled-in text back into JSON arguments.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    Integer,
    Bool,
    /// One of these strings.
    Choice(Vec<String>),
    /// A list of ids or names, typed comma-separated or as JSON.
    List,
    /// Anything else, typed as JSON (or plain text, taken as a string).
    Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub kind: FieldKind,
    pub required: bool,
    pub description: String,
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

fn kind_of(root: &Value, v: &Value) -> FieldKind {
    let v = resolve(root, v);
    // Option<T> as anyOf [T, null].
    if let Some(any) = v.get("anyOf").and_then(Value::as_array) {
        let non_null: Vec<&Value> = any.iter().filter(|x| x.get("type") != Some(&json!("null"))).collect();
        if non_null.len() == 1 {
            return kind_of(root, non_null[0]);
        }
        return FieldKind::Json;
    }
    if let Some(one) = v.get("oneOf").and_then(Value::as_array) {
        let consts: Vec<String> =
            one.iter().filter_map(|x| x.get("const").and_then(Value::as_str).map(String::from)).collect();
        if consts.len() == one.len() {
            return FieldKind::Choice(consts);
        }
        return FieldKind::Json;
    }
    if let Some(e) = v.get("enum").and_then(Value::as_array) {
        return FieldKind::Choice(e.iter().filter_map(|x| x.as_str().map(String::from)).collect());
    }
    match types(v).first().map(String::as_str) {
        Some("string") => FieldKind::Text,
        Some("number") => FieldKind::Number,
        Some("integer") => FieldKind::Integer,
        Some("boolean") => FieldKind::Bool,
        Some("array") => {
            let item = v.get("items").map(|i| resolve(root, i));
            if item.is_some_and(|i| types(i) == ["string"]) { FieldKind::List } else { FieldKind::Json }
        }
        _ => FieldKind::Json,
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
                .map(|(name, v)| FieldSpec {
                    name: name.clone(),
                    kind: kind_of(schema, v),
                    required: required.contains(&name.as_str()),
                    description: v
                        .get("description")
                        .or_else(|| resolve(schema, v).get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|f| !f.required);
    out
}

/// Turn the typed text into arguments. Empty optional fields are left out.
pub fn build_args(specs: &[FieldSpec], values: &BTreeMap<String, String>) -> Result<Value, String> {
    let mut m = Map::new();
    for f in specs {
        let text = values.get(&f.name).map(|s| s.trim()).unwrap_or_default();
        if text.is_empty() {
            if f.required && f.kind != FieldKind::Bool {
                return Err(format!("{} is required", f.name));
            }
            continue;
        }
        let v = match &f.kind {
            FieldKind::Text | FieldKind::Choice(_) => json!(text),
            FieldKind::Number => {
                json!(text.replace(',', ".").parse::<f64>().map_err(|_| format!("{}: not a number", f.name))?)
            }
            FieldKind::Integer => json!(text.parse::<i64>().map_err(|_| format!("{}: not a whole number", f.name))?),
            FieldKind::Bool => json!(text == "true"),
            FieldKind::List => match serde_json::from_str::<Value>(text) {
                Ok(v @ Value::Array(_)) => v,
                _ => json!(text.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
            },
            // Lists, objects and whole numbers as JSON; "1.001" stays text (an address).
            FieldKind::Json
                if text.starts_with(['[', '{']) || text.parse::<i64>().is_ok() || text == "true" || text == "false" =>
            {
                serde_json::from_str(text).map_err(|e| format!("{}: {e}", f.name))?
            }
            FieldKind::Json => json!(text),
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
        assert_eq!(by("position").kind, FieldKind::Json);
        assert_eq!(by("address").kind, FieldKind::Json);
        assert!(!by("fid").required);
        let d = fields(&schema("object.delete"));
        assert_eq!(d[0].name, "ids");
        assert!(d[0].required);
        assert_eq!(
            d.iter().find(|x| x.name == "children").unwrap().kind,
            FieldKind::Choice(vec!["detach".into(), "delete".into()])
        );
        let l = fields(&schema("layer.create"));
        assert_eq!(l.iter().find(|x| x.name == "elevation").unwrap().kind, FieldKind::Number);
    }

    #[test]
    fn args_from_text() {
        let f = fields(&schema("fixture.create"));
        let mut v = BTreeMap::new();
        v.insert("fid".to_string(), "101".to_string());
        v.insert("address".to_string(), "1.001".to_string());
        v.insert("position".to_string(), "[1000, 0, 0]".to_string());
        v.insert("tags".to_string(), "light, front".to_string());
        let a = build_args(&f, &v).unwrap();
        assert_eq!(a, json!({ "fid": 101, "address": "1.001", "position": [1000, 0, 0], "tags": ["light", "front"] }));
        let l = fields(&schema("layer.create"));
        assert!(build_args(&l, &BTreeMap::new()).is_err());
    }
}
