//! Triangle meshes for the 3D view. Block 3 fills them in.

use serde::{Deserialize, Serialize};

/// Level of detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lod {
    /// Boxes and simple extrusions, for far views and many objects.
    Low,
    #[default]
    High,
}

/// What a surface is made of, mapped to a look by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialClass {
    #[default]
    Generic,
    Floor,
    Wall,
    Metal,
    Fixture,
}

/// Triangles in the object's local space, mm. Counter-clockwise faces point outwards.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Three per triangle, into `positions`/`normals`.
    pub indices: Vec<u32>,
    pub material: MaterialClass,
}

impl Mesh {
    pub fn empty(material: MaterialClass) -> Self {
        Self { material, ..Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}
