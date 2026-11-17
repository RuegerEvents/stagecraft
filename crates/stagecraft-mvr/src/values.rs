//! Parsing and formatting of MVR value types (UUID, Float, Vector, CIE colour).

use uuid::Uuid;

/// Namespace for UUIDs derived from unparseable ones, so the same bad text always
/// maps to the same UUID and references stay linked.
const BAD_UUID_NAMESPACE: Uuid = Uuid::from_u128(0x6d76_7273_7461_6765_6372_6166_7462_6164);

/// Parse an MVR UUID. Accepts the standard form, upper or lower case, braces,
/// missing dashes and the space-separated hex bytes the spec example shows.
/// Returns `None` for text that holds no 32 hex digits.
pub fn parse_uuid_strict(text: &str) -> Option<Uuid> {
    let hex: String = text.chars().filter(|c| !matches!(c, '-' | ' ' | '{' | '}' | '\t' | '\n' | '\r')).collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok().map(Uuid::from_u128)
}

/// A UUID for text that is not one, derived deterministically (UUID v5).
pub fn derived_uuid(text: &str) -> Uuid {
    Uuid::new_v5(&BAD_UUID_NAMESPACE, text.as_bytes())
}

/// Format a UUID as MVR writes it (upper case, dashed).
pub fn fmt_uuid(u: &Uuid) -> String {
    u.hyphenated().encode_upper(&mut Uuid::encode_buffer()).to_string()
}

/// Shortest decimal text that reads back to the same `f64`. Non-finite values,
/// which MVR forbids, are written as 0.
pub fn fmt_f64(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    if v == 0.0 {
        return "0".into(); // also folds -0
    }
    format!("{v}")
}

/// Parse a Float. Accepts a decimal comma when there is no full stop, since some
/// exporters write locale-formatted numbers.
pub fn parse_f64(text: &str) -> Option<f64> {
    let t = text.trim();
    let v = t.parse::<f64>().ok().or_else(|| t.replace(',', ".").parse::<f64>().ok())?;
    v.is_finite().then_some(v)
}

/// Parse an Integer; tolerates a fractional part (`"12.0"`) by truncating.
pub fn parse_i64(text: &str) -> Option<i64> {
    let t = text.trim();
    t.parse::<i64>().ok().or_else(|| parse_f64(t).filter(|f| f.fract() == 0.0).map(|f| f as i64))
}

/// Parse a Bool (`true`/`false`, also `1`/`0`, `on`/`off`, any case).
pub fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Parse three comma-separated floats.
pub fn parse_vec3(text: &str) -> Option<[f64; 3]> {
    let parts: Vec<f64> =
        text.split(',').map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect::<Option<_>>()?;
    parts.try_into().ok()
}

pub fn fmt_vec3(v: [f64; 3]) -> String {
    format!("{},{},{}", fmt_f64(v[0]), fmt_f64(v[1]), fmt_f64(v[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_forms() {
        let a = parse_uuid_strict("8bf13dd7-cbf4-415b-99e4-625fe4d2daf6").unwrap();
        assert_eq!(fmt_uuid(&a), "8BF13DD7-CBF4-415B-99E4-625FE4D2DAF6");
        let b = parse_uuid_strict("8B F1 3D D7 CB F4 41 5B 99 E4 62 5F E4 D2 DA F6").unwrap();
        assert_eq!(a, b);
        assert!(parse_uuid_strict("12345").is_none());
        assert_eq!(derived_uuid("x"), derived_uuid("x"));
        assert_ne!(derived_uuid("x"), derived_uuid("y"));
    }

    #[test]
    fn floats() {
        assert_eq!(fmt_f64(1.0), "1");
        assert_eq!(fmt_f64(-0.0), "0");
        assert_eq!(fmt_f64(f64::NAN), "0");
        let v = 6020.939200000001;
        assert_eq!(fmt_f64(v).parse::<f64>().unwrap(), v);
        assert_eq!(parse_f64("3,5"), Some(3.5));
        assert_eq!(parse_i64("12.0"), Some(12));
        assert_eq!(parse_i64("12.5"), None);
    }
}
