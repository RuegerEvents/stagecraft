//! Closed loops (faces) of the sketch, for floors.
//!
//! The non-construction edges form a planar graph on the points (points joined by
//! `Coincident` constraints count as one vertex). Dangling edges are pruned, then every
//! face is traced: arriving at a vertex, continue with the next edge clockwise from the
//! one we came in on. Bounded faces come out counter-clockwise (positive area); the
//! unbounded face of each component comes out clockwise and is dropped.

use std::collections::HashMap;
use std::f64::consts::TAU;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{ConstraintKind, EdgeGeom, EdgeId, PointId, Sketch, Vec2};

/// One edge of a loop, with the direction it is walked in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LoopEdge {
    pub edge: EdgeId,
    /// Walked from its end point to its start point.
    pub reversed: bool,
}

/// A closed loop, counter-clockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Loop {
    pub edges: Vec<LoopEdge>,
    /// The point each edge starts at, in walking order.
    pub points: Vec<PointId>,
    /// Enclosed area in mm² (positive; arcs included).
    pub area: f64,
}

/// Arc helpers.
pub(crate) fn arc_params(c: Vec2, s: Vec2, e: Vec2) -> (f64, f64, f64) {
    let r = s.distance(c);
    let a0 = (s - c).angle();
    let a1 = (e - c).angle();
    let mut sweep = (a1 - a0).rem_euclid(TAU);
    if sweep <= 1e-12 {
        sweep = TAU;
    }
    (r, a0, sweep)
}

struct Half {
    edge: EdgeId,
    reversed: bool,
    from: usize,
    to: usize,
    angle: f64,
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Points of an edge walked in the given direction, without the final point.
/// Arcs are split so that no piece spans more than `max_step` radians.
fn walk(s: &Sketch, geom: EdgeGeom, reversed: bool, max_step: f64) -> Vec<Vec2> {
    match geom {
        EdgeGeom::Line { a, b } => vec![if reversed { s.pos(b) } else { s.pos(a) }],
        EdgeGeom::Arc { center, start, end } => {
            let c = s.pos(center);
            let (r, a0, sweep) = arc_params(c, s.pos(start), s.pos(end));
            let n = ((sweep / max_step).ceil() as usize).max(1);
            (0..n)
                .map(|i| {
                    let t = if reversed { n - i } else { i } as f64 / n as f64;
                    c + Vec2::new(r, 0.0).rotated(a0 + sweep * t)
                })
                .collect()
        }
    }
}

fn signed_area(poly: &[Vec2]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].cross(poly[(i + 1) % n])).sum::<f64>() * 0.5
}

impl Sketch {
    /// All bounded faces formed by the non-construction edges, counter-clockwise.
    /// Islands (a loop inside another, unconnected) are reported as separate loops; the
    /// caller decides about holes.
    pub fn closed_loops(&self) -> Vec<Loop> {
        // vertices: points merged through coincident constraints
        let idx: HashMap<PointId, usize> = self.points.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
        let mut parent: Vec<usize> = (0..self.points.len()).collect();
        for c in &self.constraints {
            if let ConstraintKind::Coincident { a, b } = c.kind
                && let (Some(&i), Some(&j)) = (idx.get(&a), idx.get(&b))
            {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }

        let mut edges: Vec<(EdgeId, EdgeGeom, usize, usize)> = Vec::new();
        for e in self.edges.iter().filter(|e| !e.construction) {
            if e.geom.points().iter().any(|p| !idx.contains_key(p)) {
                continue;
            }
            let (a, b) = e.geom.ends();
            let (va, vb) = (find(&mut parent, idx[&a]), find(&mut parent, idx[&b]));
            if va != vb {
                edges.push((e.id, e.geom, va, vb));
            }
        }

        // prune dangling edges
        let mut alive = vec![true; edges.len()];
        loop {
            let mut degree: HashMap<usize, usize> = HashMap::new();
            for (k, e) in edges.iter().enumerate() {
                if alive[k] {
                    *degree.entry(e.2).or_default() += 1;
                    *degree.entry(e.3).or_default() += 1;
                }
            }
            let mut changed = false;
            for (k, e) in edges.iter().enumerate() {
                if alive[k] && (degree[&e.2] < 2 || degree[&e.3] < 2) {
                    alive[k] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // half-edges with their leaving direction (a point slightly along the edge, so
        // a line and a tangent arc still sort apart)
        let mut halves: Vec<Half> = Vec::new();
        for (k, &(id, geom, va, vb)) in edges.iter().enumerate() {
            if !alive[k] {
                continue;
            }
            for reversed in [false, true] {
                let (from, to) = if reversed { (vb, va) } else { (va, vb) };
                let dir = match geom {
                    EdgeGeom::Line { a, b } => {
                        let d = self.pos(b) - self.pos(a);
                        if reversed { -d } else { d }
                    }
                    EdgeGeom::Arc { center, start, end } => {
                        let c = self.pos(center);
                        let (r, a0, sweep) = arc_params(c, self.pos(start), self.pos(end));
                        let delta = (sweep * 0.05).min(2f64.to_radians());
                        let (t0, t1) = if reversed { (a0 + sweep, a0 + sweep - delta) } else { (a0, a0 + delta) };
                        Vec2::new(r, 0.0).rotated(t1) - Vec2::new(r, 0.0).rotated(t0)
                    }
                };
                halves.push(Half { edge: id, reversed, from, to, angle: dir.angle() });
            }
        }

        // outgoing half-edges per vertex, sorted counter-clockwise
        let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
        for (h, half) in halves.iter().enumerate() {
            out.entry(half.from).or_default().push(h);
        }
        for list in out.values_mut() {
            list.sort_by(|&a, &b| halves[a].angle.total_cmp(&halves[b].angle));
        }
        let twin = |h: usize| h ^ 1; // pushed in pairs
        let next = |h: usize| -> usize {
            let list = &out[&halves[h].to];
            let k = list.iter().position(|&x| x == twin(h)).expect("twin present");
            list[(k + list.len() - 1) % list.len()]
        };

        let mut seen = vec![false; halves.len()];
        let mut loops = Vec::new();
        for start in 0..halves.len() {
            if seen[start] {
                continue;
            }
            let mut cycle = Vec::new();
            let mut h = start;
            while !seen[h] {
                seen[h] = true;
                cycle.push(h);
                h = next(h);
            }
            if h != start {
                continue; // cannot happen on a consistent rotation system
            }
            let mut poly = Vec::new();
            for &h in &cycle {
                let half = &halves[h];
                let geom = self.edge(half.edge).expect("edge").geom;
                poly.extend(walk(self, geom, half.reversed, 5f64.to_radians()));
            }
            let area = signed_area(&poly);
            if area > 1e-6 {
                let points = cycle
                    .iter()
                    .map(|&h| {
                        let half = &halves[h];
                        let (a, b) = self.edge(half.edge).expect("edge").geom.ends();
                        if half.reversed { b } else { a }
                    })
                    .collect();
                loops.push(Loop {
                    edges: cycle
                        .iter()
                        .map(|&h| LoopEdge { edge: halves[h].edge, reversed: halves[h].reversed })
                        .collect(),
                    points,
                    area,
                });
            }
        }
        loops
    }

    /// Outline of a loop as a polygon (counter-clockwise, not closed), arcs split into
    /// pieces of at most `max_step_deg` degrees.
    pub fn loop_outline(&self, l: &Loop, max_step_deg: f64) -> Vec<Vec2> {
        let step = max_step_deg.max(0.1).to_radians();
        let mut poly = Vec::new();
        for le in &l.edges {
            if let Some(e) = self.edge(le.edge) {
                poly.extend(walk(self, e.geom, le.reversed, step));
            }
        }
        poly
    }
}
