//! Heads-up dimension input, as in Fusion 360: while a tool draws something with a
//! length, small boxes next to the rubber band show the live length (and angle). Typing
//! a digit puts it straight into the length box; Tab cycles length → angle → length; a
//! box whose text parses locks its quantity (lock icon), and the point follows the
//! pointer only in the free one. Enter or a click takes the point and empties the boxes
//! for the next segment. Shift (square to the axes) still turns the pointer's
//! direction, so a typed length lands along the snapped direction.
//!
//! The text lives in [`Gesture::dim`](crate::state::Gesture); `tools.rs` feeds it from
//! the keyboard and commits points; this module holds the pure rules (what a tool takes,
//! where the point lands) and paints the boxes.

use egui::{Align2, Pos2, Rect, Role, Sense, Shape, Stroke, Ui, Vec2, vec2};
use stagecraft_core::model::P2;
use stagecraft_core::units::{LengthUnit, Units};
use stagecraft_geom::math;

use crate::app::Studio;
use crate::i18n::t;
use crate::state::{DimField, DimInput, Tool};
use crate::theme::{self, Palette};

/// Which boxes a tool shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Length and angle (walls, floors, ceilings, runs, truss, curtains, move).
    LengthAngle,
    /// A length only (calibrate, dimension value).
    Length,
    /// An angle only (rotate).
    Angle,
}

impl Kind {
    pub fn first_field(self) -> DimField {
        if self == Kind::Angle { DimField::Angle } else { DimField::Length }
    }
}

/// The boxes the active tool shows now, if any.
pub fn kind(s: &Studio) -> Option<Kind> {
    let g = &s.ui.gesture;
    let n = g.points.len();
    match s.ui.tool {
        Tool::Wall | Tool::Floor | Tool::Ceiling | Tool::Run | Tool::Truss | Tool::Curtain | Tool::Move if n >= 1 => {
            Some(Kind::LengthAngle)
        }
        Tool::Rotate if n >= 1 => Some(Kind::Angle),
        Tool::Calibrate if n >= 1 => Some(Kind::Length),
        Tool::Dimension if !g.picks.is_empty() => Some(Kind::Length),
        _ => None,
    }
}

/// Whether keys go into the boxes now (the dimension tool takes its value once both
/// points are picked).
pub fn takes_keys(s: &Studio) -> bool {
    match s.ui.tool {
        Tool::Dimension => s.ui.gesture.picks.len() == 2,
        _ => kind(s).is_some(),
    }
}

/// Characters a length or angle may hold. Letters (units) only after the first
/// character, so `m` or `f` before any digit stay tool shortcuts.
pub fn accepts(c: char, started: bool) -> bool {
    c.is_ascii_digit()
        || matches!(c, '.' | ',' | '-' | '+')
        || (started && matches!(c, 'm' | 'c' | 'f' | 't' | 'i' | 'n' | '\'' | '"' | '°' | ' ' | '/'))
}

/// Whether `text` (one text event) may go into a box; `started` = a box already has
/// the keys.
pub fn accepts_text(text: &str, started: bool) -> bool {
    let mut started = started;
    !text.is_empty()
        && text.chars().all(|c| {
            let ok = accepts(c, started);
            started |= ok;
            ok
        })
}

/// Direction of the last segment of `points`, degrees, or 0 (+x).
pub fn prev_dir(points: &[P2]) -> f64 {
    match points {
        [.., a, b] if math::dist(*a, *b) > 1e-6 => math::angle_deg(math::sub(*b, *a)),
        _ => 0.0,
    }
}

/// Where the next point lands from `from` with the pointer at `cursor` (already
/// snapped, squared by Shift) and what is typed: a typed length and angle fix the
/// point; a typed length alone goes along the pointer's direction; a typed angle alone
/// follows the pointer along that ray. Nothing typed: the pointer.
pub fn constrained(from: P2, cursor: P2, dim: Option<&DimInput>, fallback_dir: f64) -> P2 {
    let Some(dim) = dim else { return cursor };
    let d = math::sub(cursor, from);
    let free_dir = if math::length(d) > 1e-6 { math::angle_deg(d) } else { fallback_dir };
    match (dim.length_mm(), dim.angle_deg()) {
        (None, None) => cursor,
        (Some(l), None) => math::polar(from, l, free_dir),
        (Some(l), Some(a)) => math::polar(from, l, a),
        (None, Some(a)) => {
            let u = math::polar([0.0, 0.0], 1.0, a);
            math::add(from, math::scale(u, math::dot(d, u).max(0.0)))
        }
    }
}

/// The segment the boxes measure for a length tool: from the last point to where the
/// next one lands. `None` when the tool shows no length.
pub fn segment(s: &Studio) -> Option<(P2, P2)> {
    let g = &s.ui.gesture;
    let cursor = g.cursor.map(|c| c.point)?;
    match (s.ui.tool, kind(s)?) {
        (Tool::Dimension, _) => match g.picks.as_slice() {
            [a] => Some((a.at(), cursor)),
            [a, b] => Some((a.at(), b.at())),
            _ => None,
        },
        (Tool::Calibrate, _) => {
            let a = g.points[0];
            Some((a, g.points.get(1).copied().unwrap_or(cursor)))
        }
        (_, Kind::LengthAngle) => {
            let from = match s.ui.tool {
                Tool::Truss | Tool::Curtain | Tool::Move => g.points[0],
                _ => *g.points.last()?,
            };
            Some((from, constrained(from, cursor, g.dim.as_ref(), prev_dir(&g.points))))
        }
        _ => None,
    }
}

/// The rotate tool: pivot, reference direction and the angle turned (typed, else from
/// the pointer once a reference is set).
pub fn rotation(s: &Studio) -> Option<(P2, f64, Option<f64>)> {
    let g = &s.ui.gesture;
    if s.ui.tool != Tool::Rotate {
        return None;
    }
    let pivot = *g.points.first()?;
    let reference = g.points.get(1).map_or(0.0, |r| math::angle_deg(math::sub(*r, pivot)));
    let typed = g.dim.as_ref().and_then(DimInput::angle_deg);
    let live = match (g.points.get(1), g.cursor) {
        (Some(_), Some(c)) if math::dist(c.point, pivot) > 1e-6 => {
            Some(norm180(math::angle_deg(math::sub(c.point, pivot)) - reference))
        }
        _ => None,
    };
    Some((pivot, reference, typed.or(live)))
}

/// An angle in (-180, 180].
pub fn norm180(a: f64) -> f64 {
    let a = a.rem_euclid(360.0);
    if a > 180.0 { a - 360.0 } else { a }
}

fn has_unit(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_alphabetic() || matches!(c, '\'' | '"'))
}

/// One box: what it shows and how.
struct BoxSpec {
    /// "Length" / "Angle": the accessible name's start.
    what: &'static str,
    /// Typed text, or the live value.
    text: String,
    /// Drawn dimmer after the text (" m", "°").
    suffix: String,
    locked: bool,
    focused: bool,
    /// The next key replaces the text (drawn selected).
    replace: bool,
}

impl BoxSpec {
    fn length(dim: Option<&DimInput>, live_mm: f64, u: &Units) -> Self {
        let typed = dim.filter(|d| !d.length.is_empty());
        let focused = dim.is_some_and(|d| d.field == DimField::Length);
        // ft-in values carry their marks; a bare typed number there is inches.
        let unit = match u.unit {
            LengthUnit::FtIn => "\"".to_owned(),
            _ => format!(" {}", u.label()),
        };
        match typed {
            Some(d) => BoxSpec {
                what: "dim.length",
                text: d.length.clone(),
                suffix: if has_unit(&d.length) { String::new() } else { unit },
                locked: d.length_mm().is_some(),
                focused,
                replace: focused && d.replace,
            },
            None => BoxSpec {
                what: "dim.length",
                text: u.fixed(live_mm),
                suffix: if u.unit == LengthUnit::FtIn { String::new() } else { unit },
                locked: false,
                focused,
                replace: false,
            },
        }
    }

    fn angle(dim: Option<&DimInput>, live_deg: f64) -> Self {
        let typed = dim.filter(|d| !d.angle.is_empty());
        let focused = dim.is_some_and(|d| d.field == DimField::Angle);
        match typed {
            Some(d) => BoxSpec {
                what: "dim.angle",
                text: d.angle.clone(),
                suffix: if d.angle.ends_with('°') { String::new() } else { "°".to_owned() },
                locked: d.angle_deg().is_some(),
                focused,
                replace: focused && d.replace,
            },
            None => BoxSpec {
                what: "dim.angle",
                text: format!("{live_deg:.1}"),
                suffix: "°".to_owned(),
                locked: false,
                focused,
                replace: false,
            },
        }
    }

    /// The accessible name: "Length: 4.5 m (locked, typing)".
    fn name(&self) -> String {
        let mut flags = Vec::new();
        if self.locked {
            flags.push("locked");
        }
        if self.focused {
            flags.push("typing");
        }
        let flags = if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) };
        format!("{}: {}{}{flags}", t(self.what), self.text, self.suffix)
    }
}

const BOX_H: f32 = 20.0;
const PAD: f32 = 6.0;
const LOCK: f32 = 12.0;

/// The size a box needs.
fn box_size(ui: &Ui, b: &BoxSpec) -> Vec2 {
    let font = theme::proportional(theme::BODY);
    let w = ui.fonts_mut(|f| {
        f.layout_no_wrap(b.text.clone(), font.clone(), egui::Color32::WHITE).size().x
            + f.layout_no_wrap(b.suffix.clone(), font.clone(), egui::Color32::WHITE).size().x
    });
    let lock = if b.locked { LOCK + 3.0 } else { 0.0 };
    vec2((PAD * 2.0 + lock + w + 4.0).max(52.0).round(), BOX_H)
}

/// Paint a box centred at `center` and give it an accessible name.
fn paint_box(ui: &Ui, p: &egui::Painter, center: Pos2, b: &BoxSpec, id: &str) -> Rect {
    let c = theme::of(ui);
    let size = box_size(ui, b);
    let r = Rect::from_center_size(center.round(), size);
    p.rect_filled(r.translate(vec2(0.0, 1.0)), 4.0, c.shadow.gamma_multiply(0.6));
    p.rect_filled(r, 4.0, c.pal_bg);
    let border = if b.focused { Stroke::new(1.5, c.accent) } else { Stroke::new(1.0, c.pal_border) };
    p.rect_stroke(r, 4.0, border, egui::StrokeKind::Inside);
    let mut x = r.left() + PAD;
    if b.locked {
        let ir = Rect::from_center_size(Pos2::new(x + LOCK / 2.0, r.center().y), vec2(LOCK, LOCK));
        crate::icons::paint_with(p, ui.ctx(), ir, "lock", LOCK, c.accent);
        x += LOCK + 3.0;
    }
    let font = theme::proportional(theme::BODY);
    let ink = if b.locked || b.focused { c.text } else { c.text2 };
    let g = p.layout_no_wrap(b.text.clone(), font.clone(), ink);
    let text_rect = Align2::LEFT_CENTER.anchor_size(Pos2::new(x, r.center().y), g.size());
    if b.replace {
        p.rect_filled(text_rect.expand2(vec2(1.0, -1.0)), 2.0, c.select_bg);
    }
    let after = text_rect.right();
    p.galley(text_rect.min, g, ink);
    let caret = b.focused && !b.replace;
    if caret {
        // The caret after the typed text, before the unit hint.
        let cx = after.round() + 0.5;
        p.line_segment([Pos2::new(cx, r.top() + 4.0), Pos2::new(cx, r.bottom() - 4.0)], Stroke::new(1.0, c.text));
    }
    let sg = p.layout_no_wrap(b.suffix.to_owned(), font, c.text3);
    p.galley(Pos2::new(after + if caret { 2.0 } else { 0.0 }, text_rect.top()), sg, c.text3);
    let resp = ui.interact(r, egui::Id::unique(("hud_box", id)), Sense::hover());
    crate::widgets::name(&resp, Role::TextInput, b.name());
    r
}

/// The box's centre beside the segment `a`–`b` (screen), on the side `n` points to.
fn beside(a: Pos2, b: Pos2, n: Vec2, size: Vec2) -> Pos2 {
    let mid = a + (b - a) * 0.5;
    let off = 8.0 + (n.x.abs() * size.x + n.y.abs() * size.y) / 2.0;
    mid + n * off
}

/// Paint the boxes (and the angle arc) for the active tool.
pub fn draw(ui: &Ui, p: &egui::Painter, s: &Studio, pal: &Palette) {
    let Some(kind) = kind(s) else { return };
    let cam = &s.ui.camera;
    let dim = s.ui.gesture.dim.as_ref();
    if kind == Kind::Angle {
        if let Some((pivot, reference, turned)) = rotation(s) {
            let at = cam.to_screen(pivot);
            let b = BoxSpec::angle(dim, turned.unwrap_or(0.0));
            if let Some(a) = turned {
                arc(p, at, reference, a, pal);
            }
            let size = box_size(ui, &b);
            let mid = (reference + turned.unwrap_or(0.0) / 2.0).to_radians();
            let dir = vec2(mid.cos() as f32, -(mid.sin() as f32));
            let off = ARC_R + 10.0 + (dir.x.abs() * size.x + dir.y.abs() * size.y) / 2.0;
            paint_box(ui, p, at + dir * off, &b, "angle");
        }
        return;
    }
    let Some((a, b)) = segment(s) else { return };
    let len = math::dist(a, b);
    let (sa, sb) = (cam.to_screen(a), cam.to_screen(b));
    let px = (sb - sa).length();
    // The normal on the upper side of the rubber band (screen y grows down).
    let n = if px > 1.0 {
        let d = (sb - sa) / px;
        let n = vec2(d.y, -d.x);
        if n.y > 0.0 || (n.y == 0.0 && n.x > 0.0) { -n } else { n }
    } else {
        vec2(0.0, -1.0)
    };
    let u = crate::lengths::of(s);
    let lb = BoxSpec::length(dim.filter(|_| s.ui.tool != Tool::Dimension || s.ui.gesture.picks.len() == 2), len, &u);
    let size = box_size(ui, &lb);
    let center = if px > size.x * 0.6 { beside(sa, sb, n, size) } else { sb + vec2(18.0 + size.x / 2.0, -18.0) };
    paint_box(ui, p, center, &lb, "length");
    if kind == Kind::LengthAngle {
        let ang = if len > 1e-6 { math::angle_deg(math::sub(b, a)) } else { 0.0 };
        let ab = BoxSpec::angle(dim, norm180(ang));
        if px > 6.0 {
            arc(p, sa, 0.0, norm180(ang), pal);
        }
        // On the other side of the band from the length box, past the arc.
        let asize = box_size(ui, &ab);
        let half = (norm180(ang) / 2.0).to_radians();
        let dir = vec2(half.cos() as f32, -(half.sin() as f32));
        let off = ARC_R + 10.0 + (dir.x.abs() * asize.x + dir.y.abs() * asize.y) / 2.0;
        // Beside the arc, on the other side of the band from the length box.
        let c = sa + dir * off - n * (asize.y / 2.0 + 6.0);
        paint_box(ui, p, c, &ab, "angle");
    }
    // Where the typed values put the point, if that is not under the pointer.
    if let Some(c) = s.ui.gesture.cursor
        && dim.is_some()
        && (cam.to_screen(c.point) - sb).length() > 3.0
    {
        let st = Stroke::new(1.0, pal.preview.gamma_multiply(0.6));
        p.extend(Shape::dashed_line(&[sb, cam.to_screen(c.point)], st, 3.0, 3.0));
        p.circle_filled(sb, 3.0, pal.preview);
    }
}

const ARC_R: f32 = 34.0;

/// The angle arc at `at` from `from_deg` turning `turn` degrees, with a dashed
/// reference line.
fn arc(p: &egui::Painter, at: Pos2, from_deg: f64, turn: f64, pal: &Palette) {
    let st = Stroke::new(1.0, pal.preview.gamma_multiply(0.7));
    let dir = |deg: f64| vec2(deg.to_radians().cos() as f32, -(deg.to_radians().sin() as f32));
    p.extend(Shape::dashed_line(&[at, at + dir(from_deg) * (ARC_R + 14.0)], st, 4.0, 3.0));
    let n = ((turn.abs() / 6.0).ceil() as usize).max(2);
    let pts: Vec<Pos2> = (0..=n).map(|i| at + dir(from_deg + turn * i as f64 / n as f64) * ARC_R).collect();
    p.add(Shape::line(pts, st));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Typed in a metre project.
    fn dim(length: &str, angle: &str) -> DimInput {
        DimInput { length: length.into(), angle: angle.into(), units: Units::new(LengthUnit::M), ..Default::default() }
    }

    fn close(a: P2, b: P2) -> bool {
        math::dist(a, b) < 1e-6
    }

    #[test]
    fn a_typed_length_goes_along_the_pointer() {
        let p = constrained([1000.0, 0.0], [1000.0, 50.0], Some(&dim("2", "")), 0.0);
        assert!(close(p, [1000.0, 2000.0]), "{p:?}");
    }

    #[test]
    fn length_and_angle_fix_the_point() {
        let p = constrained([0.0, 0.0], [-500.0, -500.0], Some(&dim("1", "90")), 0.0);
        assert!(close(p, [0.0, 1000.0]), "{p:?}");
    }

    #[test]
    fn a_typed_angle_alone_follows_the_pointer_along_its_ray() {
        let p = constrained([0.0, 0.0], [300.0, 4000.0], Some(&dim("", "0")), 0.0);
        assert!(close(p, [300.0, 0.0]), "{p:?}");
        // Behind the start: stays on it.
        let p = constrained([0.0, 0.0], [-300.0, 10.0], Some(&dim("", "0")), 0.0);
        assert!(close(p, [0.0, 0.0]), "{p:?}");
    }

    #[test]
    fn pointer_on_the_start_uses_the_previous_direction() {
        let p = constrained([0.0, 0.0], [0.0, 0.0], Some(&dim("3", "")), 90.0);
        assert!(close(p, [0.0, 3000.0]), "{p:?}");
        assert_eq!(prev_dir(&[[0.0, 0.0], [0.0, -5.0]]), -90.0);
        assert_eq!(prev_dir(&[[0.0, 0.0]]), 0.0);
    }

    #[test]
    fn units_start_only_after_a_digit() {
        assert!(accepts_text("15m", false));
        assert!(!accepts_text("m", false), "a bare m stays the Move tool");
        assert!(accepts_text("m", true));
        assert!(accepts_text("2,5", false));
        assert!(accepts_text("6 1/2\"", false));
        assert!(!accepts_text("x", true));
    }

    #[test]
    fn the_length_box_speaks_the_project_unit() {
        let mm = Units::default();
        let b = BoxSpec::length(None, 4525.0, &mm);
        assert_eq!((b.text.as_str(), b.suffix.as_str()), ("4525", " mm"));
        let m = Units::new(LengthUnit::M);
        assert_eq!(BoxSpec::length(None, 4500.0, &m).text, "4.500");
        let typed = DimInput { length: "1500mm".into(), units: m, ..Default::default() };
        let b = BoxSpec::length(Some(&typed), 0.0, &m);
        assert!(b.locked && b.suffix.is_empty(), "an explicit unit stays as typed");
        let ft = Units::new(LengthUnit::FtIn);
        assert_eq!(BoxSpec::length(None, 6155.0, &ft).text, "20' 2 5/16\"");
        // Plain mm in a mm project: 1500 is 1.5 m, not 1500 m.
        let typed = DimInput { length: "1500".into(), ..Default::default() };
        assert_eq!(typed.length_mm(), Some(1500.0));
    }

    #[test]
    fn angles_normalise() {
        assert_eq!(norm180(270.0), -90.0);
        assert_eq!(norm180(-180.0), 180.0);
    }
}
