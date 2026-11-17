//! Hidden-line removal: split each edge into visible and hidden pieces against all
//! occluding triangles, found through a uniform screen-space grid.
//!
//! Per (edge, triangle) pair the hidden part is one parameter interval: the edge
//! clipped to the triangle's 2D projection (three half-planes, slightly enlarged so
//! an edge lying exactly on a triangle's outline counts as covered), then to where the
//! edge lies behind the triangle's plane by more than a depth tolerance. Both are linear
//! in the edge parameter. An edge on its own faces has depth difference zero, so it is
//! never hidden by them.

use glam::{DVec2, DVec3};

use crate::{LineClass, ObjectId, Options, Seg};

/// A classified 2D piece of an edge.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Piece {
    pub a: DVec2,
    pub b: DVec2,
    pub object: ObjectId,
    pub class: LineClass,
}

struct Occ {
    /// Inside when `e[k][0]·x + e[k][1]·y + e[k][2] ≥ −xy_eps` for all three edges.
    e: [[f64; 3]; 3],
    /// Depth plane: `z = z[0]·x + z[1]·y + z[2]`.
    z: [f64; 3],
    zmax: f64,
    lo: DVec2,
    hi: DVec2,
}

impl Occ {
    fn new(t: &[DVec3; 3]) -> Option<Self> {
        let [a, mut b, mut c] = *t;
        let n = (b - a).cross(c - a);
        let len = n.length();
        // Edge-on triangles cover no area on screen; their steep depth plane would also
        // be numerically useless.
        if len == 0.0 || n.z.abs() < 1e-6 * len {
            return None;
        }
        if (b - a).truncate().perp_dot((c - a).truncate()) < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        let mut e = [[0.0; 3]; 3];
        for (k, (p, q)) in [(a, b), (b, c), (c, a)].into_iter().enumerate() {
            let d = (q - p).truncate();
            let inward = DVec2::new(-d.y, d.x).normalize();
            e[k] = [inward.x, inward.y, -inward.dot(p.truncate())];
        }
        // n·(p − a) = 0 solved for z.
        let z = [-n.x / n.z, -n.y / n.z, a.z + (n.x * a.x + n.y * a.y) / n.z];
        let lo = a.truncate().min(b.truncate()).min(c.truncate());
        let hi = a.truncate().max(b.truncate()).max(c.truncate());
        Some(Self { e, z, zmax: a.z.max(b.z).max(c.z), lo, hi })
    }
}

/// A uniform screen-space grid layout.
#[derive(Clone, Copy)]
pub(crate) struct Geom {
    lo: DVec2,
    inv: DVec2,
    nx: usize,
    ny: usize,
}

struct Grid {
    geom: Geom,
    /// CSR: items of cell `i` are `items[start[i]..start[i + 1]]`.
    start: Vec<u32>,
    items: Vec<u32>,
}

/// x-range of a triangle (3 points) or segment (2 points) inside the strip y0..y1.
fn strip_range(pts: &[DVec2], y0: f64, y1: f64) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let m = pts.len();
    let edges = if m == 2 { 1 } else { m };
    for k in 0..edges {
        let (p, q) = (pts[k], pts[(k + 1) % m]);
        let dy = q.y - p.y;
        let (t0, t1) = if dy == 0.0 {
            if p.y < y0 || p.y > y1 {
                continue;
            }
            (0.0, 1.0)
        } else {
            let (s0, s1) = ((y0 - p.y) / dy, (y1 - p.y) / dy);
            (s0.min(s1).max(0.0), s0.max(s1).min(1.0))
        };
        if t0 > t1 {
            continue;
        }
        for t in [t0, t1] {
            let x = p.x + (q.x - p.x) * t;
            lo = lo.min(x);
            hi = hi.max(x);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

impl Grid {
    fn build(occ: &[Occ], tris: &[[DVec2; 3]]) -> Self {
        let mut lo = DVec2::splat(f64::INFINITY);
        let mut hi = DVec2::splat(f64::NEG_INFINITY);
        for o in occ {
            lo = lo.min(o.lo);
            hi = hi.max(o.hi);
        }
        if occ.is_empty() {
            lo = DVec2::ZERO;
            hi = DVec2::ONE;
        }
        let geom = Geom::new(lo, hi, occ.len());
        let (nx, ny) = (geom.nx, geom.ny);
        let mut start = vec![0u32; nx * ny + 1];
        for t in tris {
            geom.cells(t, |c| start[c + 1] += 1);
        }
        for i in 0..nx * ny {
            start[i + 1] += start[i];
        }
        let mut items = vec![0; start[nx * ny] as usize];
        let mut fill = start.clone();
        for (i, t) in tris.iter().enumerate() {
            geom.cells(t, |c| {
                items[fill[c] as usize] = i as u32;
                fill[c] += 1;
            });
        }
        Grid { geom, start, items }
    }
}

impl Geom {
    /// About `items` square cells over `lo..hi`, at most 4096 per side.
    pub(crate) fn new(lo: DVec2, hi: DVec2, items: usize) -> Self {
        let size = (hi - lo).max(DVec2::splat(1e-6));
        let target = (items as f64).clamp(1.0, 4.0e6);
        let cell = (size.x * size.y / target).sqrt().max(size.x.max(size.y) / 4096.0);
        let nx = ((size.x / cell).ceil() as usize).clamp(1, 4096);
        let ny = ((size.y / cell).ceil() as usize).clamp(1, 4096);
        let inv = DVec2::new(nx as f64 / size.x, ny as f64 / size.y);
        Geom { lo, inv, nx, ny }
    }

    pub(crate) fn len(&self) -> usize {
        self.nx * self.ny
    }


    /// Calls `f` with every cell the polygon (triangle or segment) touches.
    pub(crate) fn cells(&self, pts: &[DVec2], mut f: impl FnMut(usize)) {
        let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in pts {
            ylo = ylo.min(p.y);
            yhi = yhi.max(p.y);
        }
        let row = |y: f64| (((y - self.lo.y) * self.inv.y).floor().max(0.0) as usize).min(self.ny - 1);
        let col = |x: f64| (((x - self.lo.x) * self.inv.x).floor().max(0.0) as usize).min(self.nx - 1);
        if yhi < self.lo.y || ylo > self.lo.y + self.ny as f64 / self.inv.y {
            return;
        }
        for iy in row(ylo)..=row(yhi) {
            let y0 = self.lo.y + iy as f64 / self.inv.y;
            let y1 = self.lo.y + (iy + 1) as f64 / self.inv.y;
            let Some((x0, x1)) = strip_range(pts, y0.max(ylo), y1.min(yhi)) else { continue };
            for ix in col(x0)..=col(x1) {
                f(iy * self.nx + ix);
            }
        }
    }
}

struct Ctx<'a> {
    occ: &'a [Occ],
    grid: Grid,
    xy_eps: f64,
    z_eps: f64,
    opts: &'a Options,
}

impl Ctx<'_> {
    /// Hidden parameter intervals of `s`, merged and sorted.
    fn hidden(&self, s: &Seg, stamp: &mut [u32], generation: u32) -> Vec<(f64, f64)> {
        let (a, b) = (s.a, s.b);
        let d = b - a;
        let len = d.truncate().length();
        let tmin = (self.opts.min_length * 0.1 / len).min(1e-3);
        let zmin = a.z.min(b.z) + self.z_eps;
        let (slo, shi) = (a.truncate().min(b.truncate()), a.truncate().max(b.truncate()));
        let mut iv: Vec<(f64, f64)> = Vec::new();
        let pts = [a.truncate(), b.truncate()];
        let mut all = false;
        self.grid.geom.cells(&pts, |c| {
            if all {
                return;
            }
            let (from, to) = (self.grid.start[c] as usize, self.grid.start[c + 1] as usize);
            for &i in &self.grid.items[from..to] {
                if stamp[i as usize] == generation {
                    continue;
                }
                stamp[i as usize] = generation;
                let o = &self.occ[i as usize];
                if o.zmax <= zmin
                    || o.lo.x > shi.x + self.xy_eps
                    || o.hi.x < slo.x - self.xy_eps
                    || o.lo.y > shi.y + self.xy_eps
                    || o.hi.y < slo.y - self.xy_eps
                {
                    continue;
                }
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                let mut clip = |f0: f64, f1: f64| {
                    // Keep f0 + t·f1 ≥ 0.
                    if f1 == 0.0 {
                        if f0 < 0.0 {
                            hi = -1.0;
                        }
                    } else if f1 > 0.0 {
                        lo = lo.max(-f0 / f1);
                    } else {
                        hi = hi.min(-f0 / f1);
                    }
                };
                for e in &o.e {
                    clip(e[0] * a.x + e[1] * a.y + e[2] + self.xy_eps, e[0] * d.x + e[1] * d.y);
                }
                let g0 = o.z[0] * a.x + o.z[1] * a.y + o.z[2] - a.z - self.z_eps;
                let g1 = o.z[0] * d.x + o.z[1] * d.y - d.z;
                clip(g0, g1);
                if hi - lo > tmin {
                    if lo <= 0.0 && hi >= 1.0 {
                        all = true;
                        iv.clear();
                        iv.push((0.0, 1.0));
                        return;
                    }
                    iv.push((lo, hi));
                }
            }
        });
        if all {
            return iv;
        }
        iv.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(iv.len());
        for (lo, hi) in iv {
            match merged.last_mut() {
                Some(last) if lo <= last.1 + tmin => last.1 = last.1.max(hi),
                _ => merged.push((lo, hi)),
            }
        }
        merged
    }

    fn pieces(&self, s: &Seg, stamp: &mut [u32], generation: u32, out: &mut Vec<Piece>) {
        let (a2, b2) = (s.a.truncate(), s.b.truncate());
        let len = a2.distance(b2);
        if len < self.opts.min_length {
            return;
        }
        let hidden_class = match s.class {
            LineClass::Visible | LineClass::Cut | LineClass::Hidden => Some(LineClass::Hidden),
            LineClass::Centre => None,
        };
        let push = |out: &mut Vec<Piece>, t0: f64, t1: f64, class: Option<LineClass>| {
            if let Some(class) = class
                && (t1 - t0) * len >= self.opts.min_length
            {
                out.push(Piece { a: a2.lerp(b2, t0), b: a2.lerp(b2, t1), object: s.object, class });
            }
        };
        if !s.occluded {
            push(out, 0.0, 1.0, Some(s.class));
            return;
        }
        let hidden = self.hidden(s, stamp, generation);
        let hidden_class = if self.opts.hidden_lines { hidden_class } else { None };
        let mut t = 0.0;
        for (lo, hi) in hidden {
            let (lo, hi) = (lo.max(0.0), hi.min(1.0));
            if lo > t {
                push(out, t, lo, Some(s.class));
            }
            push(out, lo.max(t), hi, hidden_class);
            t = t.max(hi);
        }
        if t < 1.0 {
            push(out, t, 1.0, Some(s.class));
        }
    }
}

pub(crate) fn classify(segs: &[Seg], tris: &[[DVec3; 3]], opts: &Options) -> Vec<Piece> {
    let mut ext: f64 = 1.0;
    for t in tris {
        for p in t {
            ext = ext.max(p.abs().max_element());
        }
    }
    for s in segs {
        ext = ext.max(s.a.abs().max_element()).max(s.b.abs().max_element());
    }
    let (occ, flat): (Vec<Occ>, Vec<[DVec2; 3]>) =
        tris.iter().filter_map(|t| Some((Occ::new(t)?, t.map(|p| p.truncate())))).unzip();
    let grid = Grid::build(&occ, &flat);
    let ctx = Ctx { occ: &occ, grid, xy_eps: ext * 1e-10 + 1e-7, z_eps: (ext * 1e-7).max(1e-3), opts };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        segs.par_iter()
            .map_init(
                || (vec![0u32; occ.len()], 0u32),
                |(stamp, generation), s| {
                    *generation += 1;
                    let mut out = Vec::new();
                    ctx.pieces(s, stamp, *generation, &mut out);
                    out
                },
            )
            .flatten_iter()
            .collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        let mut stamp = vec![0u32; occ.len()];
        let mut out = Vec::new();
        for (i, s) in segs.iter().enumerate() {
            ctx.pieces(s, &mut stamp, i as u32 + 1, &mut out);
        }
        out
    }
}
