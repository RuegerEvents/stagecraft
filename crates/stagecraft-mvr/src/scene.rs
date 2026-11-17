//! Queries on a [`Scene`]: world transforms, traversal, symbol expansion, counts.

use crate::matrix::Matrix;
use crate::model::*;
use std::collections::HashMap;
use uuid::Uuid;

/// Symbols nested deeper than this are treated as a cycle.
const MAX_SYMBOL_DEPTH: usize = 32;

/// One placed model file, after symbols are expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryInstance {
    /// The object the geometry belongs to (or the symdef chain root object).
    pub object: Uuid,
    pub file_name: String,
    /// Model space → scene space, mm.
    pub world: Matrix,
}

/// Counts for summaries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub layers: usize,
    pub groups: usize,
    pub fixtures: usize,
    pub trusses: usize,
    pub supports: usize,
    pub scene_objects: usize,
    pub focus_points: usize,
    pub video_screens: usize,
    pub projectors: usize,
    pub symdefs: usize,
    /// Symbol instances in object geometries and symdefs.
    pub symbols: usize,
    pub geometry3d: usize,
    pub classes: usize,
    pub positions: usize,
}

impl Scene {
    /// Recompute `world` of every object from the local matrices (parent first).
    /// Call after changing any `matrix`.
    pub fn resolve_world(&mut self) {
        fn walk(objs: &mut [Object], parent: &Matrix) {
            for o in objs {
                o.world = parent.then(&o.matrix);
                let w = o.world;
                walk(&mut o.children, &w);
            }
        }
        for l in &mut self.layers {
            let m = l.matrix;
            walk(&mut l.children, &m);
        }
    }

    /// Every object, depth first, with the layer it lives in.
    pub fn objects(&self) -> Vec<(&Layer, &Object)> {
        fn walk<'a>(l: &'a Layer, objs: &'a [Object], out: &mut Vec<(&'a Layer, &'a Object)>) {
            for o in objs {
                out.push((l, o));
                walk(l, &o.children, out);
            }
        }
        let mut out = Vec::new();
        for l in &self.layers {
            walk(l, &l.children, &mut out);
        }
        out
    }

    /// Every object of one kind.
    pub fn objects_of(&self, kind: ObjectKind) -> impl Iterator<Item = &Object> {
        self.objects().into_iter().map(|(_, o)| o).filter(move |o| o.kind == kind)
    }

    /// The object with this UUID (the first, if a file repeats UUIDs).
    pub fn find(&self, uuid: Uuid) -> Option<&Object> {
        self.objects().into_iter().map(|(_, o)| o).find(|o| o.uuid == uuid)
    }

    pub fn symdef(&self, uuid: Uuid) -> Option<&Symdef> {
        self.aux.symdefs.iter().find(|s| s.uuid == uuid)
    }

    pub fn class(&self, uuid: Uuid) -> Option<&Class> {
        self.aux.classes.iter().find(|c| c.uuid == uuid)
    }

    pub fn position(&self, uuid: Uuid) -> Option<&Position> {
        self.aux.positions.iter().find(|p| p.uuid == uuid)
    }

    /// All model files placed in the scene, with world transforms, expanding
    /// symbol instances recursively (cycles and missing symdefs are skipped).
    pub fn geometry_instances(&self) -> Vec<GeometryInstance> {
        let symdefs: HashMap<Uuid, &Symdef> = self.aux.symdefs.iter().map(|s| (s.uuid, s)).collect();
        let mut out = Vec::new();
        for (_, o) in self.objects() {
            for g in &o.geometries {
                expand(&symdefs, o.uuid, g, &o.world, 0, &mut out);
            }
        }
        out
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats {
            layers: self.layers.len(),
            symdefs: self.aux.symdefs.len(),
            classes: self.aux.classes.len(),
            positions: self.aux.positions.len(),
            ..Default::default()
        };
        let count_geo = |gs: &[Geometry], s: &mut Stats| {
            for g in gs {
                match g {
                    Geometry::Geometry3D { .. } => s.geometry3d += 1,
                    Geometry::Symbol { .. } => s.symbols += 1,
                }
            }
        };
        for sd in &self.aux.symdefs {
            count_geo(&sd.geometries, &mut s);
        }
        for (_, o) in self.objects() {
            count_geo(&o.geometries, &mut s);
            match o.kind {
                ObjectKind::SceneObject => s.scene_objects += 1,
                ObjectKind::GroupObject => s.groups += 1,
                ObjectKind::FocusPoint => s.focus_points += 1,
                ObjectKind::Fixture => s.fixtures += 1,
                ObjectKind::Truss => s.trusses += 1,
                ObjectKind::Support => s.supports += 1,
                ObjectKind::VideoScreen => s.video_screens += 1,
                ObjectKind::Projector => s.projectors += 1,
            }
        }
        s
    }
}

fn expand(
    symdefs: &HashMap<Uuid, &Symdef>,
    object: Uuid,
    g: &Geometry,
    parent: &Matrix,
    depth: usize,
    out: &mut Vec<GeometryInstance>,
) {
    match g {
        Geometry::Geometry3D { file_name, matrix } => {
            out.push(GeometryInstance { object, file_name: file_name.clone(), world: parent.then(matrix) })
        }
        Geometry::Symbol { symdef, matrix, .. } => {
            if depth >= MAX_SYMBOL_DEPTH {
                return;
            }
            if let Some(sd) = symdefs.get(symdef) {
                let m = parent.then(matrix);
                for c in &sd.geometries {
                    expand(symdefs, object, c, &m, depth + 1, out);
                }
            }
        }
    }
}
