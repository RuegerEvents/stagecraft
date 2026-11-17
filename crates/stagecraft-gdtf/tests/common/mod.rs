//! Builders for synthetic GDTF archives.
#![allow(dead_code)]

use std::io::{Cursor, Write};

/// A zip archive with the given entries (stored, no compression).
pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A 3DS file holding one object with the given triangles.
pub fn three_ds(name: &str, verts: &[[f32; 3]], faces: &[[u16; 3]]) -> Vec<u8> {
    fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend(((body.len() + 6) as u32).to_le_bytes());
        v.extend(body);
        v
    }
    let mut vb = (verts.len() as u16).to_le_bytes().to_vec();
    for v in verts {
        for c in v {
            vb.extend(c.to_le_bytes());
        }
    }
    let mut fb = (faces.len() as u16).to_le_bytes().to_vec();
    for f in faces {
        for i in f {
            fb.extend(i.to_le_bytes());
        }
        fb.extend(0u16.to_le_bytes());
    }
    let mut tri = chunk(0x4110, &vb);
    tri.extend(chunk(0x4120, &fb));
    let mut obj = name.as_bytes().to_vec();
    obj.push(0);
    obj.extend(chunk(0x4100, &tri));
    chunk(0x4D4D, &chunk(0x3D3D, &chunk(0x4000, &obj)))
}

/// A unit-ish box from `lo` to `hi` as 8 vertices and 12 triangles.
pub fn box_tris(lo: [f32; 3], hi: [f32; 3]) -> (Vec<[f32; 3]>, Vec<[u16; 3]>) {
    let v = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect();
    let f = vec![
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    (v, f)
}

/// A GLB with one mesh (positions + indices, no normals) under a node with `scale`.
pub fn glb(verts: &[[f32; 3]], faces: &[[u16; 3]], scale: f32) -> Vec<u8> {
    let mut bin = Vec::new();
    for v in verts {
        for c in v {
            bin.extend(c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    for f in faces {
        for i in f {
            bin.extend(i.to_le_bytes());
        }
    }
    let idx_len = bin.len() - pos_len;
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in verts {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
"nodes":[{{"mesh":0,"scale":[{s},{s},{s}]}}],
"meshes":[{{"name":"m","primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":{nv},"type":"VEC3","min":[{l0},{l1},{l2}],"max":[{h0},{h1},{h2}]}},
{{"bufferView":1,"componentType":5123,"count":{ni},"type":"SCALAR"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{pos_len}}},{{"buffer":0,"byteOffset":{pos_len},"byteLength":{idx_len}}}],
"buffers":[{{"byteLength":{bl}}}]}}"#,
        s = scale,
        nv = verts.len(),
        ni = faces.len() * 3,
        l0 = lo[0],
        l1 = lo[1],
        l2 = lo[2],
        h0 = hi[0],
        h1 = hi[1],
        h2 = hi[2],
        bl = bin.len(),
    );
    let mut j = json.into_bytes();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = Vec::new();
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((j.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(j);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(bin);
    out
}

/// A moving head with a pixel ring, two breaks and every addressing case:
///
/// - Mode "Standard": Pan (1,2) on Yoke, Tilt (3) on Head, Dimmer virtual on Head,
///   4 pixel refs × (R, G) starting at 4, 6, 8, 10 → footprint 11 in break 1;
///   an Overwrite channel (Offset 1) that the refs place in break 2 at 1..4.
/// - Mode "Basic": Pan, Tilt only → 3.
pub const MOVING_HEAD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GDTF DataVersion="1.2">
  <FixtureType Name="Test Head" ShortName="TH" LongName="Test Moving Head" Manufacturer="Acme"
      Description="synthetic" FixtureTypeID="11111111-2222-3333-4444-555555555555" Thumbnail="thumbnail" CanHaveChildren="No">
    <AttributeDefinitions/>
    <PhysicalDescriptions>
      <Properties>
        <OperatingTemperature Low="-10" High="45"/>
        <Weight Value="12.5"/>
        <PowerConsumption Value="350" PowerFactor="0.95" Connector="Power In" VoltageLow="100" VoltageHigh="240"/>
        <LegHeight Value="0.02"/>
      </Properties>
    </PhysicalDescriptions>
    <Models>
      <Model Name="Base" Length="0.4" Width="0.3" Height="0.1" PrimitiveType="Undefined" File="base"/>
      <Model Name="Yoke" Length="0.35" Width="0.1" Height="0.3" PrimitiveType="Yoke"/>
      <Model Name="Head" Length="0.25" Width="0.25" Height="0.3" PrimitiveType="Undefined" File="head"/>
      <Model Name="Beam" Length="0.1" Width="0.1" Height="0.001" PrimitiveType="Cylinder"/>
      <Model Name="Pixel" Length="0.02" Width="0.02" Height="0.005" PrimitiveType="Cube"/>
      <Model Name="Missing" Length="0.1" Width="0.1" Height="0.1" PrimitiveType="Cube" File="nothere"/>
    </Models>
    <Geometries>
      <Geometry Name="Base" Model="Base" Position="{1,0,0,0}{0,1,0,0}{0,0,1,0}{0,0,0,1}">
        <Axis Name="Yoke" Model="Yoke" Position="{1,0,0,0}{0,1,0,0}{0,0,1,-0.1}{0,0,0,1}">
          <Axis Name="Head" Model="Head" Position="{1,0,0,0}{0,1,0,0}{0,0,1,-0.2}{0,0,0,1}">
            <Beam Name="Beam" Model="Beam" Position="{1,0,0,0}{0,1,0,0}{0,0,1,-0.15}{0,0,0,1}"
                LampType="LED" PowerConsumption="300" LuminousFlux="9000" BeamAngle="12" FieldAngle="18" BeamType="Spot"/>
            <GeometryReference Name="Pixel 1" Geometry="Pixel" Position="{1,0,0,0.05}{0,1,0,0}{0,0,1,-0.15}{0,0,0,1}">
              <Break DMXBreak="1" DMXOffset="4"/><Break DMXBreak="2" DMXOffset="1"/>
            </GeometryReference>
            <GeometryReference Name="Pixel 2" Geometry="Pixel" Position="{1,0,0,-0.05}{0,1,0,0}{0,0,1,-0.15}{0,0,0,1}">
              <Break DMXBreak="1" DMXOffset="6"/><Break DMXBreak="2" DMXOffset="2"/>
            </GeometryReference>
            <GeometryReference Name="Pixel 3" Geometry="Pixel" Position="{1,0,0,0}{0,1,0,0.05}{0,0,1,-0.15}{0,0,0,1}">
              <Break DMXBreak="1" DMXOffset="8"/><Break DMXBreak="2" DMXOffset="3"/>
            </GeometryReference>
            <GeometryReference Name="Pixel 4" Geometry="Pixel" Position="{1,0,0,0}{0,1,0,-0.05}{0,0,1,-0.15}{0,0,0,1}">
              <Break DMXBreak="1" DMXOffset="10"/><Break DMXBreak="2" DMXOffset="4"/>
            </GeometryReference>
          </Axis>
        </Axis>
        <Geometry Name="Dangling" Model="Missing"/>
      </Geometry>
      <Beam Name="Pixel" Model="Pixel" LampType="LED" PowerConsumption="5" BeamAngle="60" FieldAngle="90"/>
    </Geometries>
    <DMXModes>
      <DMXMode Name="Standard" Geometry="Base">
        <DMXChannels>
          <DMXChannel DMXBreak="1" Offset="1,2" Geometry="Yoke"><LogicalChannel Attribute="Pan"><ChannelFunction Name="Pan" Attribute="Pan" DMXFrom="0/1" Default="32768/2" PhysicalFrom="-270" PhysicalTo="270"/></LogicalChannel></DMXChannel>
          <DMXChannel DMXBreak="1" Offset="3" Geometry="Head"><LogicalChannel Attribute="Tilt"/></DMXChannel>
          <DMXChannel DMXBreak="1" Offset="" Geometry="Head"><LogicalChannel Attribute="Dimmer"/></DMXChannel>
          <DMXChannel DMXBreak="1" Offset="1" Geometry="Pixel"><LogicalChannel Attribute="ColorAdd_R"/></DMXChannel>
          <DMXChannel DMXBreak="1" Offset="2" Geometry="Pixel"><LogicalChannel Attribute="ColorAdd_G"/></DMXChannel>
          <DMXChannel DMXBreak="Overwrite" Offset="1" Geometry="Pixel"><LogicalChannel Attribute="Dimmer"/></DMXChannel>
        </DMXChannels>
      </DMXMode>
      <DMXMode Name="Basic" Geometry="Base">
        <DMXChannels>
          <DMXChannel Offset="1,2" Geometry="Yoke"><LogicalChannel Attribute="Pan"/></DMXChannel>
          <DMXChannel Offset="3" Geometry="Head"><LogicalChannel Attribute="Tilt"/></DMXChannel>
        </DMXChannels>
      </DMXMode>
    </DMXModes>
    <Revisions><Revision Text="first" Date="2026-01-01T00:00:00" UserID="7"/></Revisions>
  </FixtureType>
</GDTF>
"#;

/// The moving head archive: base as 3DS in mm, head as GLB in metres (y-up),
/// a thumbnail and a top-view SVG for the base.
pub fn moving_head() -> Vec<u8> {
    let (bv, bf) = box_tris([-200.0, -150.0, -100.0], [200.0, 150.0, 0.0]);
    let base = three_ds("base", &bv, &bf);
    // Head in glTF y-up, mm with node scale 0.001: x 250, y(up) 300, z 250.
    let (hv, hf) = box_tris([-125.0, -150.0, -125.0], [125.0, 150.0, 125.0]);
    let head = glb(&hv, &hf, 0.001);
    zip(&[
        ("description.xml", MOVING_HEAD_XML.as_bytes()),
        ("thumbnail.png", b"\x89PNG fake"),
        ("models/3ds/base.3ds", &base),
        ("models/gltf/head.glb", &head),
        ("models/svg/base.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
    ])
}
