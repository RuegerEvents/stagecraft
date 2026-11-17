//! The MVR 4x3 transform.

/// An MVR transform: three axis vectors and an offset, in millimetres.
///
/// A local point `p` maps to `o + p.x * u + p.y * v + p.z * w` in the parent
/// space. MVR axes: x right (seen from the audience), y upstage, z up,
/// right-handed. The XML form is `{u1,u2,u3}{v1,v2,v3}{w1,w2,w3}{o1,o2,o3}`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub w: [f64; 3],
    /// Offset in mm.
    pub o: [f64; 3],
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix =
        Matrix { u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], w: [0.0, 0.0, 1.0], o: [0.0, 0.0, 0.0] };

    /// A pure translation (mm).
    pub fn translation(x: f64, y: f64, z: f64) -> Self {
        Matrix { o: [x, y, z], ..Self::IDENTITY }
    }

    /// Rotation about z by `deg` degrees (counter-clockwise seen from above).
    pub fn rotation_z(deg: f64) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Matrix { u: [c, s, 0.0], v: [-s, c, 0.0], ..Self::IDENTITY }
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// Transform a direction (no offset).
    pub fn apply_vector(&self, p: [f64; 3]) -> [f64; 3] {
        let mut r = [0.0; 3];
        for (i, ri) in r.iter_mut().enumerate() {
            *ri = self.u[i] * p[0] + self.v[i] * p[1] + self.w[i] * p[2];
        }
        r
    }

    /// Transform a point (mm).
    pub fn apply_point(&self, p: [f64; 3]) -> [f64; 3] {
        let r = self.apply_vector(p);
        [r[0] + self.o[0], r[1] + self.o[1], r[2] + self.o[2]]
    }

    /// `self * local`: the transform of something placed with `local` inside the
    /// space `self` describes. World = parent.then(local) all the way down.
    pub fn then(&self, local: &Matrix) -> Matrix {
        Matrix {
            u: self.apply_vector(local.u),
            v: self.apply_vector(local.v),
            w: self.apply_vector(local.w),
            o: self.apply_point(local.o),
        }
    }

    /// Determinant of the 3x3 part (negative means mirrored).
    pub fn determinant(&self) -> f64 {
        let [a, b, c] = [self.u, self.v, self.w];
        a[0] * (b[1] * c[2] - b[2] * c[1]) - b[0] * (a[1] * c[2] - a[2] * c[1]) + c[0] * (a[1] * b[2] - a[2] * b[1])
    }

    /// Inverse, if the 3x3 part is not singular.
    pub fn inverse(&self) -> Option<Matrix> {
        let det = self.determinant();
        if det.abs() < 1e-12 || !det.is_finite() {
            return None;
        }
        // Columns u, v, w form M; the inverse rows are cross products / det.
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
        };
        let r0 = cross(self.v, self.w);
        let r1 = cross(self.w, self.u);
        let r2 = cross(self.u, self.v);
        let inv = Matrix {
            u: [r0[0] / det, r1[0] / det, r2[0] / det],
            v: [r0[1] / det, r1[1] / det, r2[1] / det],
            w: [r0[2] / det, r1[2] / det, r2[2] / det],
            o: [0.0; 3],
        };
        let t = inv.apply_vector(self.o);
        Some(Matrix { o: [-t[0], -t[1], -t[2]], ..inv })
    }

    /// Whether all values are within `eps` of `other`.
    pub fn approx_eq(&self, other: &Matrix, eps: f64) -> bool {
        let a = self.to_array();
        let b = other.to_array();
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= eps)
    }

    /// The twelve values in file order u, v, w, o.
    pub fn to_array(&self) -> [f64; 12] {
        let mut r = [0.0; 12];
        r[0..3].copy_from_slice(&self.u);
        r[3..6].copy_from_slice(&self.v);
        r[6..9].copy_from_slice(&self.w);
        r[9..12].copy_from_slice(&self.o);
        r
    }

    pub fn from_array(a: [f64; 12]) -> Matrix {
        Matrix { u: [a[0], a[1], a[2]], v: [a[3], a[4], a[5]], w: [a[6], a[7], a[8]], o: [a[9], a[10], a[11]] }
    }

    /// Parse the XML text form. Tolerant: any separators, any number format.
    /// Twelve numbers give the full matrix; nine give a rotation with zero
    /// offset; anything else is an error message.
    pub fn parse(text: &str) -> Result<Matrix, String> {
        let nums: Vec<f64> = text
            .split(|c: char| c == '{' || c == '}' || c == ',' || c == ';' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>().map_err(|_| format!("bad number {s:?} in matrix")))
            .collect::<Result<_, _>>()?;
        if nums.iter().any(|n| !n.is_finite()) {
            return Err("non-finite value in matrix".into());
        }
        match nums.len() {
            12 => Ok(Matrix::from_array(nums.try_into().unwrap())),
            9 => {
                let mut a = [0.0; 12];
                a[..9].copy_from_slice(&nums);
                Ok(Matrix::from_array(a))
            }
            n => Err(format!("matrix has {n} values, expected 12")),
        }
    }

    /// The XML text form. Shortest representation that reads back exactly.
    pub fn to_mvr_string(&self) -> String {
        let f = crate::values::fmt_f64;
        let v = |a: [f64; 3]| format!("{{{},{},{}}}", f(a[0]), f(a[1]), f(a[2]));
        format!("{}{}{}{}", v(self.u), v(self.v), v(self.w), v(self.o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        let m = Matrix::parse("{0.5,0,0}{0,1,0}{0,0,1}{1000.25,-2,3e2}").unwrap();
        assert_eq!(m.u, [0.5, 0.0, 0.0]);
        assert_eq!(m.o, [1000.25, -2.0, 300.0]);
        assert_eq!(Matrix::parse(&m.to_mvr_string()).unwrap(), m);
        assert!(Matrix::parse("{1,0}{0,1}").is_err());
        assert!(Matrix::parse("{a,b,c}").is_err());
    }

    #[test]
    fn composition_and_inverse() {
        let parent = Matrix::translation(1000.0, 0.0, 5000.0).then(&Matrix::rotation_z(90.0));
        let local = Matrix::translation(100.0, 0.0, 0.0);
        let world = parent.then(&local);
        let p = world.apply_point([0.0; 3]);
        assert!((p[0] - 1000.0).abs() < 1e-9 && (p[1] - 100.0).abs() < 1e-9 && p[2] == 5000.0);
        let back = parent.inverse().unwrap().then(&world);
        assert!(back.approx_eq(&local, 1e-9));
    }
}
