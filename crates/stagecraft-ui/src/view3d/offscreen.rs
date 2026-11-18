//! The 3D view rendered into images for sheets: every `rendered` viewport gets a
//! picture of the model from its camera (or an iso view fitted to what it shows),
//! handed to `stagecraft_sheet::set_raster` so the page and the PDF embed it.
//!
//! It uses the same pipelines and mesh cache as the 3D view, on the GUI's wgpu device
//! or on one created without a window ([`Offscreen::headless`]) for exports from the
//! command line. Without a GPU nothing is rendered and the sheet keeps its placeholder.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Pos2, Rect, Rgba, vec2};
use egui_wgpu::wgpu;
use glam::DVec3;
use stagecraft_core::sheet::{ViewKind, Viewport};
use stagecraft_core::{Document, Id};
use stagecraft_sheet::ImageData;

use super::{View3d, gpu, palette, uniforms};

/// Pixels per paper mm of a rendered viewport (200 dpi).
pub const PX_PER_MM: f64 = 200.0 / 25.4;
/// Longest side of a rendered image, px.
pub const MAX_PX: f64 = 3000.0;

struct Rendered {
    view: View3d,
    /// What the image was made from: document maps, viewport, size.
    key: Option<(Document, Viewport, [u32; 2])>,
    image: Option<ImageData>,
}

/// Renders `rendered` viewports to images.
pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: gpu::Gpu3d,
    views: HashMap<Id, Rendered>,
    /// Images made so far (tests check the cache with it).
    pub renders: u64,
}

impl Offscreen {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let gpu = gpu::Gpu3d::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        Self { device, queue, gpu, views: HashMap::new(), renders: 0 }
    }

    pub fn from_render_state(rs: &egui_wgpu::RenderState) -> Self {
        Self::new(rs.device.clone(), rs.queue.clone())
    }

    /// A device without a window, for command-line exports. `None` without a GPU.
    pub fn headless() -> Option<Self> {
        let setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        let instance = pollster::block_on(egui_wgpu::WgpuSetup::CreateNew(setup.clone()).new_instance());
        let config =
            egui_wgpu::WgpuConfiguration { wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup), ..Default::default() };
        let rs = pollster::block_on(egui_wgpu::RenderState::create(
            &config,
            &instance,
            None,
            egui_wgpu::RendererOptions::default(),
        ))
        .ok()?;
        Some(Self::from_render_state(&rs))
    }

    /// Render every `rendered` viewport of `sheets` that changed since its last image
    /// and register the images with the sheet crate.
    pub fn render_sheets(&mut self, doc: &Document, sheets: &[Id]) {
        for sid in sheets {
            let Some(sheet) = doc.sheets.get(sid) else { continue };
            for vp in &sheet.viewports {
                if let ViewKind::Rendered { .. } = vp.view
                    && let Some(img) = self.render(doc, vp)
                {
                    stagecraft_sheet::set_raster(vp.id, img);
                }
            }
        }
    }

    /// The image of one `rendered` viewport, from the cache when nothing it shows changed.
    pub fn render(&mut self, doc: &Document, vp: &Viewport) -> Option<ImageData> {
        let ViewKind::Rendered { eye, target } = vp.view else { return None };
        let crop = stagecraft_sheet::compose::effective_crop(doc, vp, stagecraft_sheet::default_source())?;
        let paper = [(crop[1][0] - crop[0][0]) / vp.scale, (crop[1][1] - crop[0][1]) / vp.scale];
        let k = (PX_PER_MM).min(MAX_PX / paper[0].max(paper[1]).max(1.0));
        let size = [(paper[0] * k).round().max(16.0) as u32, (paper[1] * k).round().max(16.0) as u32];
        let r = self.views.entry(vp.id).or_insert_with(|| Rendered { view: View3d::default(), key: None, image: None });
        let same = r.key.as_ref().is_some_and(|(d, v, s)| {
            d.objects.ptr_eq(&doc.objects)
                && d.layers.ptr_eq(&doc.layers)
                && d.fixture_types.ptr_eq(&doc.fixture_types)
                && d.tags.ptr_eq(&doc.tags)
                && v == vp
                && *s == size
        });
        if same && r.image.is_some() {
            return r.image.clone();
        }
        let ids: Vec<Id> = stagecraft_sheet::compose::shown_ids(doc, vp)
            .into_iter()
            .filter(|id| doc.objects.get(id).is_some_and(|o| !o.kind.is_group()))
            .collect();
        let v = &mut r.view;
        v.grid = false;
        v.camera.rect = Rect::from_min_size(Pos2::ZERO, vec2(size[0] as f32, size[1] as f32));
        v.scene.sync(doc, &ids, &[], &v.camera);
        match (eye, target) {
            (Some(e), t) => {
                let t = t.map(DVec3::from_array).or_else(|| v.scene.bounds().map(|(lo, hi)| (lo + hi) / 2.0));
                v.camera.look(DVec3::from_array(e), t.unwrap_or(DVec3::ZERO));
            }
            (None, _) => {
                let (yaw, pitch) = super::camera::StdView::Iso.angles();
                v.camera.set_angles(yaw, pitch);
                v.fit(None);
            }
        }
        // Detail levels follow the final camera.
        v.scene.sync(doc, &ids, &[], &v.camera);
        v.scene.update_caps(&[]);
        let pal = palette(false);
        let bg = Rgba::from(egui::Color32::WHITE);
        let frame = gpu::Frame3d {
            uniforms: uniforms(v, &pal, 1.0),
            size_px: size,
            clear: [bg.r() as f64, bg.g() as f64, bg.b() as f64, 1.0],
            mesh: v.scene.buffers.clone(),
            caps: v.scene.caps.clone(),
        };
        let (w, h, pixels) = self.gpu.render_to_rgba(&self.device, &self.queue, &frame)?;
        self.renders += 1;
        let img = ImageData::Rgba { width: w, height: h, pixels: Arc::new(pixels) };
        r.key = Some((doc.clone(), vp.clone(), size));
        r.image = Some(img.clone());
        Some(img)
    }
}
