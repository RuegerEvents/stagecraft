//! Edits: the serialisable operations a command layer maps onto (`sketch.*`,
//! `dimension.*`). Each edit is applied to a copy and solved; on error the input is
//! untouched.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::draw::{Axis, Endpoint, LinePlan};
use crate::error::SolveError;
use crate::model::{ConstraintId, ConstraintKind, EdgeGeom, EdgeId, ItemRef, Measure, PointId, Sketch, Vec2};
use crate::solve::{DragTarget, SolveOptions, Solved, solve_sketch};

/// One edit of a sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Solve without changing anything.
    Resolve,
    /// Drag points; each is soft-fixed to its target.
    Drag {
        targets: Vec<DragTarget>,
    },
    /// Change the value of a driving dimension.
    SetDimension {
        id: ConstraintId,
        value: f64,
    },
    /// Turn a dimension into a driving or a reference dimension.
    SetDriving {
        id: ConstraintId,
        driving: bool,
    },
    /// Move a dimension's label.
    SetLabelOffset {
        id: ConstraintId,
        offset: Vec2,
    },
    /// Rename a dimension (parameter name).
    SetDimensionName {
        id: ConstraintId,
        name: Option<String>,
    },
    AddPoint {
        at: Vec2,
    },
    AddEdge {
        geom: EdgeGeom,
        #[serde(default)]
        construction: bool,
        #[serde(default)]
        tag: Option<String>,
    },
    AddConstraint {
        kind: ConstraintKind,
    },
    /// Add a dimension. `value: None` takes the current measurement.
    AddDimension {
        measure: Measure,
        #[serde(default)]
        value: Option<f64>,
        #[serde(default = "yes")]
        driving: bool,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        label_offset: Vec2,
    },
    /// Remove a point, edge, constraint or dimension (and what depends on it).
    Remove {
        item: ItemRef,
    },
    SetEdgeTag {
        id: EdgeId,
        tag: Option<String>,
    },
    SetConstruction {
        id: EdgeId,
        construction: bool,
    },
    /// Draw a line from a plan made by [`infer_line`](crate::infer_line): new or
    /// snapped end points, point-on-line, horizontal/vertical and a length dimension.
    DrawLine {
        plan: LinePlan,
    },
    /// Several edits as one step (solved once at the end).
    Batch {
        edits: Vec<Edit>,
    },
}

fn yes() -> bool {
    true
}

#[derive(Default)]
struct Collected {
    drags: Vec<DragTarget>,
    culprits: Vec<ConstraintId>,
    created: Vec<ItemRef>,
}

fn apply_structural(s: &mut Sketch, edit: &Edit, c: &mut Collected) -> Result<(), SolveError> {
    match edit {
        Edit::Resolve => {}
        Edit::Drag { targets } => c.drags.extend(targets.iter().copied()),
        Edit::SetDimension { id, value } => {
            let d = s.dimension_mut(*id).ok_or(SolveError::Missing(ItemRef::Constraint(*id)))?;
            if !d.driving {
                return Err(SolveError::NotDriving(*id));
            }
            if !value.is_finite() || *value < 0.0 {
                return Err(SolveError::BadValue { dimension: *id, value: *value });
            }
            d.value = *value;
            c.culprits.push(*id);
        }
        Edit::SetDriving { id, driving } => {
            let d = s.dimension_mut(*id).ok_or(SolveError::Missing(ItemRef::Constraint(*id)))?;
            d.driving = *driving;
            c.culprits.push(*id);
        }
        Edit::SetLabelOffset { id, offset } => {
            s.dimension_mut(*id).ok_or(SolveError::Missing(ItemRef::Constraint(*id)))?.label_offset = *offset;
        }
        Edit::SetDimensionName { id, name } => {
            s.dimension_mut(*id).ok_or(SolveError::Missing(ItemRef::Constraint(*id)))?.name = name.clone();
        }
        Edit::AddPoint { at } => {
            let id = s.add_point(*at);
            c.created.push(ItemRef::Point(id));
        }
        Edit::AddEdge { geom, construction, tag } => {
            for p in geom.points() {
                if s.point(p).is_none() {
                    return Err(SolveError::Missing(ItemRef::Point(p)));
                }
            }
            let id = s.add_edge(*geom);
            let e = s.edge_mut(id).expect("just added");
            e.construction = *construction;
            e.tag = tag.clone();
            c.created.push(ItemRef::Edge(id));
        }
        Edit::AddConstraint { kind } => {
            let id = s.add_constraint(*kind);
            c.culprits.push(id);
            c.created.push(ItemRef::Constraint(id));
        }
        Edit::AddDimension { measure, value, driving, name, label_offset } => {
            let id = s.add_dimension_full(*measure, *value, *driving, name.clone());
            s.dimension_mut(id).expect("just added").label_offset = *label_offset;
            c.culprits.push(id);
            c.created.push(ItemRef::Constraint(id));
        }
        Edit::Remove { item } => {
            let ok = match *item {
                ItemRef::Point(id) => s.remove_point(id),
                ItemRef::Edge(id) => s.remove_edge(id),
                ItemRef::Constraint(id) => s.remove_constraint(id),
            };
            if !ok {
                return Err(SolveError::Missing(*item));
            }
        }
        Edit::SetEdgeTag { id, tag } => {
            s.edge_mut(*id).ok_or(SolveError::Missing(ItemRef::Edge(*id)))?.tag = tag.clone();
        }
        Edit::SetConstruction { id, construction } => {
            s.edge_mut(*id).ok_or(SolveError::Missing(ItemRef::Edge(*id)))?.construction = *construction;
        }
        Edit::DrawLine { plan } => draw_line(s, plan, c)?,
        Edit::Batch { edits } => {
            for e in edits {
                apply_structural(s, e, c)?;
            }
        }
    }
    Ok(())
}

impl Sketch {
    /// Applies an edit to a copy of the sketch and solves it, starting from the current
    /// positions. On error `self` is unchanged and nothing needs rolling back.
    pub fn apply(&self, edit: &Edit, opts: &SolveOptions) -> Result<Solved, SolveError> {
        let mut s = self.clone();
        let mut c = Collected::default();
        apply_structural(&mut s, edit, &mut c)?;
        let mut solved = solve_sketch(&s, &c.drags, &c.culprits, opts)?;
        solved.created = c.created;
        Ok(solved)
    }

    /// Like [`Sketch::apply`], but replaces `self` on success.
    pub fn apply_in_place(&mut self, edit: &Edit, opts: &SolveOptions) -> Result<Solved, SolveError> {
        let solved = self.apply(edit, opts)?;
        *self = solved.sketch.clone();
        Ok(solved)
    }
}

fn endpoint(s: &mut Sketch, e: &Endpoint, c: &mut Collected) -> Result<PointId, SolveError> {
    match *e {
        Endpoint::Existing { point } => {
            if s.point(point).is_none() {
                return Err(SolveError::Missing(ItemRef::Point(point)));
            }
            Ok(point)
        }
        Endpoint::New { at, on_line } => {
            if let Some(line) = on_line
                && s.line_ends(line).is_none()
            {
                return Err(SolveError::Missing(ItemRef::Edge(line)));
            }
            let id = s.add_point(at);
            c.created.push(ItemRef::Point(id));
            if let Some(line) = on_line {
                let k = s.add_constraint(ConstraintKind::PointOnLine { point: id, line });
                c.created.push(ItemRef::Constraint(k));
                c.culprits.push(k);
            }
            Ok(id)
        }
    }
}

fn draw_line(s: &mut Sketch, plan: &LinePlan, c: &mut Collected) -> Result<(), SolveError> {
    let a = endpoint(s, &plan.start, c)?;
    let b = endpoint(s, &plan.end, c)?;
    let line = s.add_line(a, b);
    c.created.push(ItemRef::Edge(line));
    let constrain = |s: &mut Sketch, c: &mut Collected, kind| {
        let k = s.add_constraint(kind);
        c.created.push(ItemRef::Constraint(k));
        c.culprits.push(k);
    };
    match plan.axis {
        Some(Axis::Horizontal) => constrain(s, c, ConstraintKind::Horizontal { a, b }),
        Some(Axis::Vertical) => constrain(s, c, ConstraintKind::Vertical { a, b }),
        None => {}
    }
    if let Some(len) = plan.length {
        let k = s.add_dimension(Measure::Length { line }, len);
        c.created.push(ItemRef::Constraint(k));
        c.culprits.push(k);
    }
    Ok(())
}
