//! Plan primitives and their style classes.

use serde::{Deserialize, Serialize};
use stagecraft_core::model::P2;

use crate::math;

/// What a line means. Sheets map classes to line weights; the canvas to pixel widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineClass {
    Thin,
    Medium,
    Thick,
    /// Dashed: edges hidden behind or above something.
    Hidden,
    /// Dash-dot: axes and centre lines.
    Centre,
}

/// What a filled area is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillClass {
    Floor,
    /// Cut walls: dark solid in plan.
    Wall,
    /// Bodies of equipment (fixtures …).
    Body,
    /// Masking cloth (legs): dark.
    Masking,
    /// Platform decks.
    Deck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAnchor {
    #[default]
    Center,
    Left,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "prim", rename_all = "snake_case")]
pub enum PlanPrim {
    Polyline {
        points: Vec<P2>,
        closed: bool,
        line: LineClass,
    },
    Polygon {
        points: Vec<P2>,
        fill: FillClass,
        line: Option<LineClass>,
    },
    Circle {
        center: P2,
        radius: f64,
        fill: Option<FillClass>,
        line: Option<LineClass>,
    },
    /// Counter-clockwise from `start` degrees through `sweep` degrees.
    Arc {
        center: P2,
        radius: f64,
        start: f64,
        sweep: f64,
        line: LineClass,
    },
    /// Text with its cap height in world mm, upright unless `rotation` (degrees) is set.
    Text {
        pos: P2,
        text: String,
        height: f64,
        rotation: f64,
        anchor: TextAnchor,
    },
    /// A picture from the document's assets (PNG or JPEG) stretched over a
    /// parallelogram: `corners` are where its lower left, lower right, upper right and
    /// upper left corners land, world mm.
    Image {
        corners: [P2; 4],
        /// SHA-256 of the asset.
        asset: String,
        /// 0 to 1.
        opacity: f64,
    },
}

impl PlanPrim {
    pub fn line(points: Vec<P2>, line: LineClass) -> Self {
        PlanPrim::Polyline { points, closed: false, line }
    }

    /// Every point that outlines the primitive (circles and arcs by their box).
    pub fn extent(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        match self {
            PlanPrim::Polyline { points, .. } | PlanPrim::Polygon { points, .. } => {
                points.iter().for_each(|p| b.add(*p));
            }
            PlanPrim::Image { corners, .. } => corners.iter().for_each(|p| b.add(*p)),
            PlanPrim::Circle { center, radius, .. } | PlanPrim::Arc { center, radius, .. } => {
                b.add([center[0] - radius, center[1] - radius]);
                b.add([center[0] + radius, center[1] + radius]);
            }
            PlanPrim::Text { pos, text, height, anchor, .. } => {
                // Rough: glyphs are about 0.6 of the height wide.
                let w = text.chars().count() as f64 * height * 0.6;
                let x0 = match anchor {
                    TextAnchor::Center => pos[0] - w / 2.0,
                    TextAnchor::Left => pos[0],
                };
                b.add([x0, pos[1] - height / 2.0]);
                b.add([x0 + w, pos[1] + height / 2.0]);
            }
        }
        b
    }

    /// Distance from `p` to the primitive in mm; 0 inside filled areas.
    pub fn distance(&self, p: P2) -> f64 {
        match self {
            PlanPrim::Polyline { points, closed, .. } => math::polyline_distance(points, *closed, p),
            PlanPrim::Polygon { points, .. } => {
                if math::point_in_polygon(points, p) {
                    0.0
                } else {
                    math::polyline_distance(points, true, p)
                }
            }
            PlanPrim::Circle { center, radius, fill, .. } => {
                let d = math::dist(*center, p);
                if fill.is_some() && d <= *radius { 0.0 } else { (d - radius).abs() }
            }
            PlanPrim::Arc { center, radius, .. } => (math::dist(*center, p) - radius).abs(),
            PlanPrim::Text { .. } => self.extent().distance(p),
            PlanPrim::Image { corners, .. } => {
                if math::point_in_polygon(corners, p) {
                    0.0
                } else {
                    math::polyline_distance(corners, true, p)
                }
            }
        }
    }

    /// The same primitive moved by `d`.
    pub fn translated(&self, d: P2) -> Self {
        self.mapped(|p| [p[0] + d[0], p[1] + d[1]], 0.0)
    }

    /// The same primitive rotated by `angle` degrees counter-clockwise about `pivot`.
    pub fn rotated(&self, pivot: P2, angle: f64) -> Self {
        self.mapped(|p| math::rotate_about(p, pivot, angle), angle)
    }

    fn mapped(&self, f: impl Fn(P2) -> P2, angle: f64) -> Self {
        match self.clone() {
            PlanPrim::Polyline { points, closed, line } => {
                PlanPrim::Polyline { points: points.into_iter().map(&f).collect(), closed, line }
            }
            PlanPrim::Polygon { points, fill, line } => {
                PlanPrim::Polygon { points: points.into_iter().map(&f).collect(), fill, line }
            }
            PlanPrim::Circle { center, radius, fill, line } => {
                PlanPrim::Circle { center: f(center), radius, fill, line }
            }
            PlanPrim::Arc { center, radius, start, sweep, line } => {
                PlanPrim::Arc { center: f(center), radius, start: start + angle, sweep, line }
            }
            PlanPrim::Text { pos, text, height, rotation, anchor } => {
                PlanPrim::Text { pos: f(pos), text, height, rotation, anchor }
            }
            PlanPrim::Image { corners, asset, opacity } => PlanPrim::Image { corners: corners.map(&f), asset, opacity },
        }
    }
}

/// How much to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// Outlines only: for zoomed-out views and overviews.
    Low,
    #[default]
    High,
}

/// View-wide drawing options. Per-object overrides join here when the model has them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanStyle {
    pub detail: Detail,
    /// Draw axes of truss and walls as centre lines.
    pub centre_lines: bool,
    /// Draw FIDs and names next to symbols.
    pub labels: bool,
}

impl Default for PlanStyle {
    fn default() -> Self {
        Self { detail: Detail::High, centre_lines: false, labels: true }
    }
}

/// An axis-aligned box in plan, mm.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: P2,
    pub max: P2,
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds { min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2] };

    pub fn from_points(a: P2, b: P2) -> Self {
        let mut r = Self::EMPTY;
        r.add(a);
        r.add(b);
        r
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1]
    }

    pub fn add(&mut self, p: P2) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }

    pub fn union(&self, o: &Bounds) -> Bounds {
        if o.is_empty() {
            return *self;
        }
        let mut r = *self;
        r.add(o.min);
        r.add(o.max);
        r
    }

    pub fn contains_bounds(&self, o: &Bounds) -> bool {
        !o.is_empty()
            && o.min[0] >= self.min[0]
            && o.min[1] >= self.min[1]
            && o.max[0] <= self.max[0]
            && o.max[1] <= self.max[1]
    }

    pub fn intersects(&self, o: &Bounds) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && o.min[0] <= self.max[0]
            && o.max[0] >= self.min[0]
            && o.min[1] <= self.max[1]
            && o.max[1] >= self.min[1]
    }

    pub fn center(&self) -> P2 {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }

    pub fn size(&self) -> P2 {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }

    /// Distance from `p` to the box; 0 inside.
    pub fn distance(&self, p: P2) -> f64 {
        let dx = (self.min[0] - p[0]).max(p[0] - self.max[0]).max(0.0);
        let dy = (self.min[1] - p[1]).max(p[1] - self.max[1]).max(0.0);
        dx.hypot(dy)
    }
}
