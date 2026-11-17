//! Join classified pieces into polylines per (object, class) and drop collinear
//! interior points.

use std::collections::{HashMap, HashSet};

use glam::DVec2;

use crate::hlr::Piece;
use crate::{LineClass, ObjectId, Polyline};

pub(crate) fn join(mut pieces: Vec<Piece>, tol: f64) -> Vec<Polyline> {
    let tol = tol.max(1e-6);
    pieces.sort_by_key(|p| (p.object, p.class));
    let mut out = Vec::new();
    let mut start = 0;
    while start < pieces.len() {
        let key = (pieces[start].object, pieces[start].class);
        let end = start + pieces[start..].iter().take_while(|p| (p.object, p.class) == key).count();
        join_group(&pieces[start..end], key.0, key.1, tol, &mut out);
        start = end;
    }
    out
}

fn join_group(pieces: &[Piece], object: ObjectId, class: LineClass, tol: f64, out: &mut Vec<Polyline>) {
    let q = |p: DVec2| ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
    let mut ids: HashMap<(i64, i64), u32> = HashMap::new();
    let mut nodes: Vec<DVec2> = Vec::new();
    let mut node = |p: DVec2| -> u32 {
        *ids.entry(q(p)).or_insert_with(|| {
            nodes.push(p);
            nodes.len() as u32 - 1
        })
    };
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for p in pieces {
        let (a, b) = (node(p.a), node(p.b));
        if a != b && seen.insert((a.min(b), a.max(b))) {
            edges.push((a, b));
        }
    }
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); nodes.len()];
    for (i, (a, b)) in edges.iter().enumerate() {
        adj[*a as usize].push(i as u32);
        adj[*b as usize].push(i as u32);
    }
    let mut used = vec![false; edges.len()];
    let other = |e: u32, n: u32| {
        let (a, b) = edges[e as usize];
        if a == n { b } else { a }
    };
    let walk = |first: u32, from: u32, used: &mut Vec<bool>| -> (Vec<u32>, bool) {
        let mut path = vec![from];
        let mut e = first;
        let mut cur = from;
        loop {
            used[e as usize] = true;
            cur = other(e, cur);
            if cur == from {
                return (path, true);
            }
            path.push(cur);
            if adj[cur as usize].len() != 2 {
                return (path, false);
            }
            match adj[cur as usize].iter().find(|&&f| !used[f as usize]) {
                Some(&f) => e = f,
                None => return (path, false),
            }
        }
    };
    let mut emit = |path: Vec<u32>, closed: bool| {
        let pts: Vec<DVec2> = path.iter().map(|&n| nodes[n as usize]).collect();
        let (points, closed) = simplify(pts, closed, tol);
        if points.len() >= 2 {
            out.push(Polyline { object, class, points, closed });
        }
    };
    for n in 0..nodes.len() as u32 {
        if adj[n as usize].len() == 2 {
            continue;
        }
        for &e in &adj[n as usize] {
            if !used[e as usize] {
                let (path, closed) = walk(e, n, &mut used);
                emit(path, closed);
            }
        }
    }
    for e in 0..edges.len() {
        if !used[e] {
            let (path, closed) = walk(e as u32, edges[e].0, &mut used);
            emit(path, closed);
        }
    }
}

/// Drop points that lie on the straight line between their neighbours (within `tol`).
fn simplify(pts: Vec<DVec2>, closed: bool, tol: f64) -> (Vec<DVec2>, bool) {
    let straight = |a: DVec2, p: DVec2, b: DVec2| {
        let d = b - a;
        let len = d.length();
        if len < tol {
            return false;
        }
        let t = (p - a).dot(d) / (len * len);
        (0.0..=1.0).contains(&t) && (d.perp_dot(p - a) / len).abs() <= tol
    };
    let mut out: Vec<DVec2> = Vec::with_capacity(pts.len());
    for p in pts {
        while out.len() >= 2 && straight(out[out.len() - 2], out[out.len() - 1], p) {
            out.pop();
        }
        out.push(p);
    }
    if closed {
        // Also the seam: the last and first points.
        while out.len() >= 3 && straight(out[out.len() - 2], out[out.len() - 1], out[0]) {
            out.pop();
        }
        while out.len() >= 3 && straight(out[out.len() - 1], out[0], out[1]) {
            out.remove(0);
        }
        if out.len() < 3 {
            return (out, false);
        }
    }
    (out, closed)
}
