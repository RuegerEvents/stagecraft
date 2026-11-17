//! The 3D orbit camera: a target point, a distance, and the direction the eye sits in
//! (yaw/pitch, z up). Perspective or orthographic; the orthographic view shows what the
//! perspective one shows at the target's depth, so switching keeps the size.

use egui::{Pos2, Rect, pos2, vec2};
use glam::{DMat4, DVec3, Mat4};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

/// Standard views. Front looks from the audience (−y) upstage, side from stage
/// right (+x), iso from front right above.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StdView {
    Top,
    Front,
    Back,
    Side,
    Left,
    Iso,
}

impl StdView {
    pub const ALL: [StdView; 6] =
        [StdView::Top, StdView::Front, StdView::Back, StdView::Side, StdView::Left, StdView::Iso];

    /// (yaw, pitch) in degrees of the eye around the target.
    pub fn angles(self) -> (f64, f64) {
        match self {
            StdView::Top => (-90.0, 90.0),
            StdView::Front => (-90.0, 0.0),
            StdView::Back => (90.0, 0.0),
            StdView::Side => (0.0, 0.0),
            StdView::Left => (180.0, 0.0),
            StdView::Iso => (-45.0, 35.264),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            StdView::Top => "view3d.top",
            StdView::Front => "view3d.front",
            StdView::Back => "view3d.back",
            StdView::Side => "view3d.side",
            StdView::Left => "view3d.left",
            StdView::Iso => "view3d.iso",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera3d {
    /// World mm the camera orbits around.
    pub target: DVec3,
    /// mm from the eye to the target.
    pub distance: f64,
    /// Degrees, counter-clockwise from +x, of the eye seen from the target.
    pub yaw: f64,
    /// Degrees above the horizon, −90…90.
    pub pitch: f64,
    /// Vertical field of view, degrees.
    pub fov: f64,
    pub projection: Projection,
    /// Where the view was drawn last frame, points.
    pub rect: Rect,
}

impl Default for Camera3d {
    fn default() -> Self {
        let (yaw, pitch) = StdView::Iso.angles();
        Self {
            target: DVec3::new(0.0, 3000.0, 1000.0),
            distance: 25_000.0,
            yaw,
            pitch,
            fov: 40.0,
            projection: Projection::Perspective,
            rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
        }
    }
}

pub const MIN_DISTANCE: f64 = 50.0;
pub const MAX_DISTANCE: f64 = 2_000_000.0;

/// A ray in world mm; `dir` is unit length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

impl Camera3d {
    /// Unit vector from the target to the eye.
    pub fn back(&self) -> DVec3 {
        let (y, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        DVec3::new(p.cos() * y.cos(), p.cos() * y.sin(), p.sin())
    }

    /// Screen right and up in world space. Right comes from the yaw alone, so looking
    /// straight down is no special case.
    pub fn right_up(&self) -> (DVec3, DVec3) {
        let y = self.yaw.to_radians();
        let right = DVec3::new(-y.sin(), y.cos(), 0.0);
        let up = self.back().cross(right);
        (right, up)
    }

    pub fn eye(&self) -> DVec3 {
        self.target + self.back() * self.distance
    }

    fn aspect(&self) -> f64 {
        (self.rect.width() / self.rect.height().max(1.0)) as f64
    }

    /// Half the visible height at the target's depth, mm.
    pub fn half_height(&self) -> f64 {
        self.distance * (self.fov.to_radians() / 2.0).tan()
    }

    pub fn view(&self) -> DMat4 {
        let (r, u) = self.right_up();
        let b = self.back();
        let e = self.eye();
        DMat4::from_cols(
            glam::DVec4::new(r.x, u.x, b.x, 0.0),
            glam::DVec4::new(r.y, u.y, b.y, 0.0),
            glam::DVec4::new(r.z, u.z, b.z, 0.0),
            glam::DVec4::new(-r.dot(e), -u.dot(e), -b.dot(e), 1.0),
        )
    }

    /// Reversed depth (near = 1, far = 0) for precision across a whole venue.
    /// `scene_radius` bounds the orthographic depth range.
    pub fn proj(&self, scene_radius: f64) -> DMat4 {
        let aspect = self.aspect();
        match self.projection {
            Projection::Perspective => {
                // Infinite far plane, depth = near / distance (1 at near, 0 far away).
                let near = (self.distance * 0.002).clamp(1.0, 200.0);
                let f = 1.0 / (self.fov.to_radians() / 2.0).tan();
                DMat4::from_cols(
                    glam::DVec4::new(f / aspect, 0.0, 0.0, 0.0),
                    glam::DVec4::new(0.0, f, 0.0, 0.0),
                    glam::DVec4::new(0.0, 0.0, 0.0, -1.0),
                    glam::DVec4::new(0.0, 0.0, near, 0.0),
                )
            }
            Projection::Orthographic => {
                let h = self.half_height();
                let w = h * aspect;
                // View z from +depth (behind the eye) to −depth maps to depth 1 … 0.
                let depth = self.distance + 2.0 * scene_radius + 10_000.0;
                DMat4::from_cols(
                    glam::DVec4::new(1.0 / w, 0.0, 0.0, 0.0),
                    glam::DVec4::new(0.0, 1.0 / h, 0.0, 0.0),
                    glam::DVec4::new(0.0, 0.0, 0.5 / depth, 0.0),
                    glam::DVec4::new(0.0, 0.0, 0.5, 1.0),
                )
            }
        }
    }

    pub fn view_proj(&self, scene_radius: f64) -> Mat4 {
        (self.proj(scene_radius) * self.view()).as_mat4()
    }

    /// The ray through a screen point.
    pub fn ray(&self, p: Pos2) -> Ray {
        let c = self.rect.center();
        let h = self.rect.height().max(1.0) as f64 / 2.0;
        let sx = (p.x - c.x) as f64 / h; // −aspect … aspect
        let sy = -(p.y - c.y) as f64 / h; // −1 … 1, up
        let (r, u) = self.right_up();
        let b = self.back();
        match self.projection {
            Projection::Perspective => {
                let t = (self.fov.to_radians() / 2.0).tan();
                let dir = (-b + r * sx * t + u * sy * t).normalize();
                Ray { origin: self.eye(), dir }
            }
            Projection::Orthographic => {
                let hh = self.half_height();
                let origin = self.target + r * sx * hh + u * sy * hh + b * (self.distance + 1e6);
                Ray { origin, dir: -b }
            }
        }
    }

    /// World point to screen points; `None` behind the eye.
    pub fn to_screen(&self, w: DVec3) -> Option<Pos2> {
        let v = self.view().transform_point3(w);
        if self.projection == Projection::Perspective && v.z >= -1e-6 {
            return None;
        }
        let h = self.rect.height() as f64 / 2.0;
        let scale = match self.projection {
            Projection::Perspective => h / ((self.fov.to_radians() / 2.0).tan() * -v.z),
            Projection::Orthographic => h / self.half_height(),
        };
        let c = self.rect.center();
        Some(pos2(c.x + (v.x * scale) as f32, c.y - (v.y * scale) as f32))
    }

    /// Screen pixels per mm at a world point (for picking detail levels).
    pub fn points_per_mm_at(&self, w: DVec3) -> f64 {
        let h = self.rect.height() as f64 / 2.0;
        match self.projection {
            Projection::Perspective => {
                let d = (w - self.eye()).dot(-self.back()).max(1.0);
                h / ((self.fov.to_radians() / 2.0).tan() * d)
            }
            Projection::Orthographic => h / self.half_height(),
        }
    }

    pub fn set_angles(&mut self, yaw: f64, pitch: f64) {
        self.yaw = yaw.rem_euclid(360.0);
        if self.yaw > 180.0 {
            self.yaw -= 360.0;
        }
        self.pitch = pitch.clamp(-90.0, 90.0);
    }

    /// Orbit by screen points (drag right turns the model to the right).
    pub fn orbit(&mut self, d: egui::Vec2) {
        self.set_angles(self.yaw - d.x as f64 * 0.35, self.pitch + d.y as f64 * 0.35);
    }

    /// Move target and eye by a screen offset (content follows the pointer).
    pub fn pan_screen(&mut self, d: egui::Vec2) {
        let mm_per_point = self.half_height() / (self.rect.height().max(1.0) as f64 / 2.0);
        let (r, u) = self.right_up();
        self.target += (-r * d.x as f64 + u * d.y as f64) * mm_per_point;
    }

    /// Move closer by `factor` (> 1 = closer), keeping the point under `at` (at the
    /// target's depth) where it is.
    pub fn zoom_at(&mut self, at: Pos2, factor: f64) {
        if factor <= 0.0 {
            return;
        }
        let ray = self.ray(at);
        // Where the ray meets the plane through the target, facing the camera.
        let b = self.back();
        let denom = ray.dir.dot(b);
        let p = if denom.abs() > 1e-9 {
            ray.origin + ray.dir * ((self.target - ray.origin).dot(b) / denom)
        } else {
            self.target
        };
        let new_distance = (self.distance / factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        let k = 1.0 - new_distance / self.distance;
        self.target += (p - self.target) * k;
        self.distance = new_distance;
    }

    /// Put the eye and target where asked; the distance follows.
    pub fn look(&mut self, eye: DVec3, target: DVec3) {
        let d = eye - target;
        let len = d.length();
        if len < 1e-6 {
            self.target = target;
            return;
        }
        self.target = target;
        self.distance = len.clamp(MIN_DISTANCE, MAX_DISTANCE);
        let pitch = (d.z / len).clamp(-1.0, 1.0).asin().to_degrees();
        // Straight down or up: keep the current yaw, so "up" on screen stays.
        let yaw = if d.x.hypot(d.y) < 1e-9 * len { self.yaw } else { d.y.atan2(d.x).to_degrees() };
        self.set_angles(yaw, pitch);
    }

    /// Show the box `min`–`max` whole, from the current direction.
    pub fn fit(&mut self, min: DVec3, max: DVec3) {
        let (min, max) = if min.x > max.x {
            (DVec3::new(-10_000.0, -2_000.0, 0.0), DVec3::new(10_000.0, 12_000.0, 6_000.0))
        } else {
            (min, max)
        };
        let centre = (min + max) / 2.0;
        let (r, u) = self.right_up();
        let b = self.back();
        let aspect = self.aspect().max(0.1);
        let t = (self.fov.to_radians() / 2.0).tan();
        let corners = (0..8).map(|i| {
            DVec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            ) - centre
        });
        let margin = 1.06;
        self.target = centre;
        self.distance = match self.projection {
            // Each corner must be inside the frustum: its sideways offset within the
            // view's half size at its own depth.
            Projection::Perspective => corners
                .map(|c| {
                    let depth = c.dot(b);
                    (c.dot(u).abs() * margin / t).max(c.dot(r).abs() * margin / (t * aspect)) + depth
                })
                .fold(500.0, f64::max),
            Projection::Orthographic => {
                let half = corners.map(|c| c.dot(u).abs().max(c.dot(r).abs() / aspect)).fold(250.0, f64::max);
                half * margin / t
            }
        }
        .clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera3d {
        Camera3d { rect: Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0)), ..Default::default() }
    }

    #[test]
    fn basis_is_orthonormal_in_every_standard_view() {
        let mut c = cam();
        for v in StdView::ALL {
            let (y, p) = v.angles();
            c.set_angles(y, p);
            let (r, u) = c.right_up();
            let b = c.back();
            assert!((r.length() - 1.0).abs() < 1e-9 && (u.length() - 1.0).abs() < 1e-9, "{v:?}");
            assert!(r.dot(u).abs() < 1e-9 && r.dot(b).abs() < 1e-9 && u.dot(b).abs() < 1e-9, "{v:?}");
            assert!(r.cross(u).distance(b) < 1e-9, "{v:?} right-handed");
        }
        c.set_angles(-90.0, 90.0);
        assert!(
            c.right_up().0.distance(DVec3::X) < 1e-9 && c.right_up().1.distance(DVec3::Y) < 1e-9,
            "top: x right, y up"
        );
        c.set_angles(-90.0, 0.0);
        assert!(c.eye().y < c.target.y && c.right_up().0.distance(DVec3::X) < 1e-9, "front: from the audience");
    }

    #[test]
    fn screen_and_ray_agree() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let mut c = cam();
            c.projection = projection;
            let w = DVec3::new(1200.0, 3400.0, 900.0);
            let s = c.to_screen(w).unwrap();
            let ray = c.ray(s);
            let closest = ray.origin + ray.dir * (w - ray.origin).dot(ray.dir);
            assert!(closest.distance(w) < 1e-3, "{projection:?}: {}", closest.distance(w));
            // The view-projection matrix lands on the same pixel.
            let ndc = c.view_proj(20_000.0).project_point3(w.as_vec3());
            let px = c.rect.center().x + ndc.x * c.rect.width() / 2.0;
            assert!((px - s.x).abs() < 0.05, "{projection:?}");
            assert!(ndc.z > 0.0 && ndc.z <= 1.0, "reversed depth in range: {}", ndc.z);
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut c = cam();
        let at = pos2(300.0, 200.0);
        let b = c.back();
        let ray = c.ray(at);
        let p = ray.origin + ray.dir * ((c.target - ray.origin).dot(b) / ray.dir.dot(b));
        c.zoom_at(at, 2.0);
        let s = c.to_screen(p).unwrap();
        assert!((s - at).length() < 0.01, "{s:?}");
        assert!((c.distance - 12_500.0).abs() < 1e-6);
    }

    #[test]
    fn look_and_fit() {
        let mut c = cam();
        c.look(DVec3::new(0.0, -10_000.0, 0.0), DVec3::ZERO);
        assert!((c.yaw + 90.0).abs() < 1e-9 && c.pitch.abs() < 1e-9 && (c.distance - 10_000.0).abs() < 1e-9);
        c.fit(DVec3::new(-5000.0, -1000.0, 0.0), DVec3::new(5000.0, 1000.0, 3000.0));
        for corner in [DVec3::new(-5000.0, -1000.0, 0.0), DVec3::new(5000.0, -1000.0, 3000.0)] {
            let s = c.to_screen(corner).unwrap();
            assert!(c.rect.contains(s), "{corner} at {s:?}");
        }
    }
}
