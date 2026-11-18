mod common;

use glam::{Mat4, Vec3};
use stagecraft_gdtf::{Gdtf, GdtfError, GeometryKind, Lod, PrimitiveType, SvgView, silhouette};

fn head() -> Gdtf {
    Gdtf::from_bytes(&common::moving_head()).expect("parses")
}

#[test]
fn metadata_and_physical() {
    let g = head();
    let f = &g.fixture;
    assert_eq!(f.name, "Test Head");
    assert_eq!(f.long_name, "Test Moving Head");
    assert_eq!(f.manufacturer, "Acme");
    assert_eq!(f.fixture_type_id, "11111111-2222-3333-4444-555555555555");
    assert_eq!(f.data_version, "1.2");
    assert!(!f.can_have_children);
    assert_eq!(f.physical.weight_kg, Some(12.5));
    assert_eq!(f.physical.leg_height_m, Some(0.02));
    assert_eq!(f.physical.operating_temperature_c, Some((-10.0, 45.0)));
    assert_eq!(f.power_w(), Some(350.0));
    assert_eq!(f.revisions.len(), 1);
    assert!(g.thumbnail_png().is_some());
    assert!(g.model_svg("Base", SvgView::Top).is_some());
    assert!(g.model_svg("Base", SvgView::Side).is_none());
    assert_eq!(g.plan_svgs(None).len(), 1);
}

#[test]
fn geometry_tree_and_beam() {
    let g = head();
    let f = &g.fixture;
    assert_eq!(f.geometries.len(), 2, "Base tree and Pixel template");
    let yoke = f.find_geometry("Yoke").unwrap();
    assert!(yoke.kind.is_axis());
    assert_eq!(yoke.position.transform_point3(Vec3::ZERO), Vec3::new(0.0, 0.0, -0.1));
    let beam = f.find_geometry("Beam").unwrap();
    let GeometryKind::Beam(b) = &beam.kind else { panic!() };
    assert_eq!(b.lamp_type, "LED");
    assert_eq!(b.beam_angle_deg, 12.0);
    assert_eq!(b.field_angle_deg, 18.0);
    assert_eq!(b.power_consumption_w, 300.0);
    assert_eq!(b.beam_type, "Spot");
    assert_eq!(f.model("Yoke").unwrap().primitive, PrimitiveType::Yoke);
    let r = f.find_geometry("Pixel 2").unwrap();
    let GeometryKind::Reference { geometry, breaks } = &r.kind else { panic!() };
    assert_eq!(geometry, "Pixel");
    assert_eq!(breaks.len(), 2);
    assert_eq!(breaks[0].dmx_offset, 6);
}

#[test]
fn footprints_with_references_and_breaks() {
    let g = head();
    let std = g.resolve_mode("Standard").unwrap();
    assert_eq!(std.footprint.get(&1), Some(&11), "{:?}", std.footprint);
    assert_eq!(std.footprint.get(&2), Some(&4));
    assert_eq!(std.virtual_channels.len(), 1);
    let red: Vec<_> = std.channels.iter().filter(|c| c.attribute == "ColorAdd_R").collect();
    assert_eq!(red.len(), 4);
    assert_eq!(red[1].geometry, "Pixel 2");
    assert_eq!(red[1].offsets, vec![6], "DMXOffset + Offset − 1");
    assert_eq!(red[1].name(), "Pixel 2_ColorAdd_R");
    let dim: Vec<_> = std.channels.iter().filter(|c| c.attribute == "Dimmer").collect();
    assert_eq!(dim.len(), 4);
    assert!(dim.iter().all(|c| c.dmx_break == 2), "Overwrite takes the last Break");
    assert_eq!(dim[3].offsets, vec![4]);
    assert!(std.warnings.is_empty(), "{:?}", std.warnings);
    let basic = g.resolve_mode("Basic").unwrap();
    assert_eq!(basic.footprint_break1(), 3);
    assert_eq!(basic.total_footprint(), 3);
}

#[test]
fn mesh_assembly_scaling_and_axes() {
    let g = head();
    let m = g.mesh(None, Lod::Default);
    // Base, Yoke, Head, Beam, 4 pixels, Dangling.
    assert_eq!(m.parts.len(), 9);
    let names: Vec<&str> = m.parts.iter().map(|p| p.instance.path.as_str()).collect();
    assert!(names.contains(&"Base/Yoke/Head/Pixel 3"), "{names:?}");
    // Pixel instances are beams (the template's kind) at the reference position.
    let px = m.parts.iter().find(|p| p.instance.name == "Pixel 1").unwrap();
    assert!(matches!(px.instance.kind, GeometryKind::Beam(_)));
    assert_eq!(px.instance.reference.as_deref(), Some("Pixel 1"));
    let at = px.instance.world.transform_point3(Vec3::ZERO);
    assert!((at - Vec3::new(50.0, 0.0, -450.0)).length() < 1e-3, "{at}");
    assert_eq!(m.axes().len(), 2);
    assert_eq!(m.beams().len(), 5);

    // Base: 3DS box in mm, already the right size; stays where it is.
    let base = m.parts[0].mesh.as_ref().unwrap();
    let (lo, hi) = base.bounds().unwrap();
    assert!((lo - Vec3::new(-200.0, -150.0, -100.0)).length() < 1e-3);
    assert!((hi - Vec3::new(200.0, 150.0, 0.0)).length() < 1e-3);
    // Head: GLB y-up, turned to z-up and scaled to 250 × 250 × 300 mm.
    let head = m.parts[2].mesh.as_ref().unwrap();
    let (lo, hi) = head.bounds().unwrap();
    assert!(((hi - lo) - Vec3::new(250.0, 250.0, 300.0)).length() < 1e-2, "{:?}", hi - lo);
    assert!(head.normals.len() == head.positions.len());

    let flat = m.flatten();
    assert_eq!(flat.groups.len(), 9);
    let (lo, _) = flat.bounds().unwrap();
    // Pixels hang at −450 (Base −100, Head −200, Pixel −150) and are 5 mm cubes
    // centred there.
    assert!((lo.z + 452.5).abs() < 1e-2, "{lo}");

    // Missing file falls back to the primitive with a warning.
    assert!(m.warnings.iter().any(|w| w.message.contains("nothere")), "{:?}", m.warnings);

    // Tilting the head 90° about x moves the beam off the z axis.
    let head_i = m.axes()[1];
    let posed = m.posed_transforms(&|i, _| {
        if i == head_i { Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2) } else { Mat4::IDENTITY }
    });
    let beam_i = m.parts.iter().position(|p| p.instance.name == "Beam").unwrap();
    let beam_at = posed[beam_i].transform_point3(Vec3::ZERO);
    assert!((beam_at - Vec3::new(0.0, 150.0, -300.0)).length() < 1e-2, "{beam_at}");
}

#[test]
fn silhouette_from_mesh() {
    let g = head();
    let m = g.mesh(None, Lod::Default);
    let s = silhouette::silhouette(&m);
    let (lo, hi) = s.bounds().unwrap();
    assert!((lo[0] + 200.0).abs() < 1.0 && (hi[0] - 200.0).abs() < 1.0, "{lo:?} {hi:?}");
    assert!((lo[1] + 150.0).abs() < 1.0 && (hi[1] - 150.0).abs() < 1.0);
    assert!(s.area() > 0.0, "counter-clockwise");
    assert!(s.parts.len() >= 4);
}

#[test]
fn messy_input_warns_instead_of_failing() {
    let xml = r#"<?xml version="1.0"?>
<GDTF DataVersion="1.1"><FixtureType Name="Messy" Manufacturer="X">
  <Models><Model Name="M" Length="abc" Width="0,2" Height="0.1" PrimitiveType="Blob"/></Models>
  <Geometries>
    <Geometry Name="Body" Model="M" Position="{garbage}">
      <Beam Name="B" Model="NoSuchModel"/>
      <Weird Name="W"/>
      <GeometryReference Name="R" Geometry="Nope"/>
    </Geometry>
  </Geometries>
  <DMXModes>
    <DMXMode Name="Mode" Geometry="NotThere">
      <DMXChannels>
        <DMXChannel DMXBreak="x" Offset="1,zz" Geometry="Ghost"><LogicalChannel Attribute="Dimmer"/></DMXChannel>
      </DMXChannels>
    </DMXMode>
  </DMXModes>
</FixtureType></GDTF>"#;
    let g = Gdtf::from_bytes(&common::zip(&[("description.xml", xml.as_bytes())])).unwrap();
    assert!(g.warnings.len() >= 6, "{:#?}", g.warnings);
    assert_eq!(g.fixture.model("M").unwrap().width_m, 0.2, "decimal comma accepted");
    let r = g.resolve_mode("Mode").unwrap();
    assert_eq!(r.footprint_break1(), 1);
    assert!(!r.warnings.is_empty());
    let m = g.mesh(None, Lod::Default);
    assert!(!m.flatten().is_empty());
}

#[test]
fn fatal_errors() {
    assert!(matches!(Gdtf::from_bytes(b"not a zip"), Err(GdtfError::Zip(_))));
    let z = common::zip(&[("readme.txt", b"hi")]);
    assert_eq!(Gdtf::from_bytes(&z).unwrap_err(), GdtfError::NoDescription);
    let z = common::zip(&[("description.xml", b"<GDTF><oops")]);
    assert!(matches!(Gdtf::from_bytes(&z), Err(GdtfError::Xml(_))));
    let z = common::zip(&[("description.xml", b"<GDTF/>")]);
    assert_eq!(Gdtf::from_bytes(&z).unwrap_err(), GdtfError::NoFixtureType);
    // An archive that declares millions of entries is refused before it is indexed.
    let many = craft_zip::zip64_entry_count_sample(10_000_000);
    assert!(matches!(Gdtf::from_bytes(&many), Err(GdtfError::Zip(m)) if m.contains("entries")));
}
