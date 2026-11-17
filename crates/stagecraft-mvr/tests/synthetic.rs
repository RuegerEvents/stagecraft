//! Committed coverage with MVRs built in the tests (no client data).

use stagecraft_mvr::*;
use std::io::Write;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x1000_0000_0000_4000_8000_0000_0000_0000 + n)
}

/// A scene that uses every part of the model.
fn full_scene() -> Mvr {
    let mut scene = Scene::default();
    scene
        .user_data
        .push(xml::Element::new("Data").with_attr("provider", "Other").with_attr("ver", "2").with_text("opaque"));
    scene.aux.classes.push(Class { uuid: id(1), name: "Lighting".into() });
    scene.aux.positions.push(Position { uuid: id(2), name: "LX1".into() });
    scene.aux.symdefs.push(Symdef {
        uuid: id(3),
        name: "Inner".into(),
        geometries: vec![Geometry::Geometry3D {
            file_name: "box.glb".into(),
            matrix: Matrix::translation(0.0, 0.0, 10.0),
        }],
    });
    scene.aux.symdefs.push(Symdef {
        uuid: id(4),
        name: "Outer".into(),
        geometries: vec![
            Geometry::Symbol { uuid: id(5), symdef: id(3), matrix: Matrix::translation(100.0, 0.0, 0.0) },
            Geometry::Geometry3D { file_name: "pipe.3ds".into(), matrix: Matrix::IDENTITY },
        ],
    });
    scene.aux.mapping_definitions.push(MappingDefinition {
        uuid: id(6),
        name: "Map".into(),
        size_x: 1920,
        size_y: 1080,
        source: Some(Source {
            linked_geometry: "Display".into(),
            source_type: "File".into(),
            value: "movie.mov".into(),
        }),
        scale_handling: Some("UpScale".into()),
    });

    let mut layer = Layer::new(id(10), "Rig");
    layer.matrix = Matrix::translation(0.0, 0.0, 500.0);

    let mut focus = Object::new(ObjectKind::FocusPoint, id(11), "DS centre");
    focus.matrix = Matrix::translation(0.0, 2000.0, 0.0);

    let mut truss = Object::new(ObjectKind::Truss, id(12), "LX1 truss");
    truss.matrix = Matrix::translation(0.0, 3000.0, 6000.0).then(&Matrix::rotation_z(90.0));
    truss.classing = Some(id(1));
    truss.position = Some(id(2));
    truss.function = Some("Flown".into());
    truss.fixture_id = "T1".into();
    truss.fixture_id_numeric = 1;
    truss.geometries.push(Geometry::Symbol { uuid: id(13), symdef: id(4), matrix: Matrix::IDENTITY });

    let mut fx = Object::new(ObjectKind::Fixture, id(14), "Spot 1");
    fx.matrix = Matrix::translation(500.0, 0.0, -200.0);
    fx.gdtf_spec = Some("Generic@Spot".into()); // without .gdtf, as some exporters write
    fx.gdtf_mode = Some("Standard".into());
    fx.fixture_id = "101".into();
    fx.fixture_id_numeric = 101;
    fx.unit_number = 3;
    fx.custom_id = Some(7);
    fx.custom_id_type = Some(1);
    fx.focus = Some(id(11));
    fx.position = Some(id(2));
    fx.classing = Some(id(1));
    fx.cast_shadow = Some(true);
    fx.dmx_invert_pan = Some(false);
    fx.dmx_invert_tilt = Some(true);
    fx.color = Some(CieColor { x: 0.3127, y: 0.329, big_y: 100.0 });
    fx.gobo = Some(Gobo { file_name: "gobo.png".into(), rotation: 32.5 });
    fx.addresses = vec![DmxAddress::new(0, 2, 17), DmxAddress { dmx_break: 1, absolute: 1 }];
    fx.networks.push(Network {
        geometry: "ethernet_1".into(),
        ipv4: Some("10.0.0.5".into()),
        subnet_mask: Some("255.0.0.0".into()),
        dhcp: Some(false),
        ..Default::default()
    });
    fx.protocols.push(Protocol {
        geometry: Some("NetworkInOut_1".into()),
        protocol_type: Some("Art-Net".into()),
        ..Default::default()
    });
    fx.alignments.push(Alignment {
        geometry: Some("Beam".into()),
        up: Some([0.0, 0.0, 1.0]),
        direction: Some([0.0, 0.0, -1.0]),
    });
    fx.custom_commands = vec!["Body_Pan,f 50".into(), "Yoke_Tilt,f 50".into()];
    fx.overwrites.push(Overwrite {
        universal: "Universal Wheel 1.Universal Wheel Slot 1".into(),
        target: Some("Wheel 1.Slot".into()),
    });
    fx.connections.push(Connection { own: "Input".into(), other: "Output1".into(), to_object: id(12) });
    fx.mappings.push(Mapping { linked_def: id(6), ux: Some(10), uy: Some(10), ox: Some(5), oy: None, rz: Some(45.0) });
    fx.extra.push(xml::Element::new("FixtureTypeId").with_text("0"));
    let mut attachment = Object::new(ObjectKind::Fixture, id(15), "Attachment");
    attachment.multipatch = Some(id(14));
    attachment.child_position = Some("Base.Yoke.Head".into());
    attachment.gdtf_spec = Some("Generic@Spot.gdtf".into());
    attachment.gdtf_mode = Some("Standard".into());
    fx.children.push(attachment);
    truss.children.push(fx);

    let mut support = Object::new(ObjectKind::Support, id(16), "Motor");
    support.chain_length = Some(2500.5);
    let mut screen = Object::new(ObjectKind::VideoScreen, id(17), "LED wall");
    screen.sources.push(Source { linked_geometry: "Display".into(), source_type: "NDI".into(), value: "Cam 1".into() });
    let mut projector = Object::new(ObjectKind::Projector, id(18), "Beamer");
    projector.projections.push(Projection {
        source: Some(Source { linked_geometry: "Beam".into(), source_type: "File".into(), value: "movie.mov".into() }),
        scale_handling: Some("ScaleKeepRatio".into()),
    });
    let mut so = Object::new(ObjectKind::SceneObject, id(19), "Platform");
    so.geometries.push(Geometry::Geometry3D { file_name: "box.glb".into(), matrix: Matrix::IDENTITY });

    let mut group = Object::new(ObjectKind::GroupObject, id(20), "Stage");
    group.matrix = Matrix::rotation_z(180.0);
    group.children = vec![support, screen, projector, so];

    layer.children = vec![focus, truss, group];
    scene.layers.push(layer);
    scene.layers.push(Layer::new(id(21), "Empty"));
    scene.resolve_world();

    let mut resources = Resources::default();
    resources.insert("Generic@Spot.gdtf", b"PK-not-really-a-gdtf".to_vec());
    resources.insert("box.glb", b"glTF....".to_vec());
    resources.insert("pipe.3ds", vec![0x4d, 0x4d, 0, 0]);
    resources.insert("gobo.png", vec![0x89, b'P', b'N', b'G']);
    resources.insert("movie.mov", vec![1, 2, 3]);
    Mvr { header: Header::default(), scene, resources }
}

#[test]
fn full_scene_round_trips_without_warnings() {
    let mvr = full_scene();
    let bytes = write_mvr(&mvr).unwrap();
    let imp = read_mvr(&bytes).unwrap();
    assert!(imp.warnings.is_empty(), "{:#?}", imp.warnings);
    assert_eq!(imp.mvr.header.provider, "StageCraft");
    assert_eq!(imp.mvr.header.version, Version { major: 1, minor: 6 });
    assert_eq!(imp.mvr.resources, mvr.resources);
    assert_eq!(imp.mvr.scene, mvr.scene);
    // Deterministic: same input, same bytes.
    assert_eq!(bytes, write_mvr(&imp.mvr).unwrap());
}

#[test]
fn xml_has_spec_shape() {
    let xml = write_scene_xml(&full_scene().scene);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\""));
    assert!(xml.contains(r#"<GeneralSceneDescription verMajor="1" verMinor="6" provider="StageCraft""#));
    assert!(xml.contains("<Address break=\"0\">529</Address>"));
    assert!(xml.contains("<Symdef uuid="));
    assert!(xml.contains("<FixtureIDNumeric>101</FixtureIDNumeric>"));
    // Mandatory children are present even when empty.
    let support = xml.split("<Support ").nth(1).unwrap();
    assert!(support.contains("<Geometries/>"));
    assert!(support.contains("<FixtureID/>"));
}

#[test]
fn world_matrices_and_symbols() {
    let mvr = full_scene();
    let s = &mvr.scene;
    let fx = s.find(id(14)).unwrap();
    // Layer z 500, truss at (0,3000,6000) turned 90°, fixture 500 along the truss, 200 below.
    let p = fx.world_position();
    assert!((p[0] - 0.0).abs() < 1e-9 && (p[1] - 3500.0).abs() < 1e-9 && (p[2] - 6300.0).abs() < 1e-9, "{p:?}");
    // Local matrix is kept.
    assert_eq!(fx.matrix, Matrix::translation(500.0, 0.0, -200.0));
    // Child inherits.
    assert_eq!(s.find(id(15)).unwrap().world, fx.world);
    // Group rotated 180°: its children stay at the origin but turn.
    let support = s.find(id(16)).unwrap();
    assert!(support.world.approx_eq(&Matrix::translation(0.0, 0.0, 500.0).then(&Matrix::rotation_z(180.0)), 1e-9));

    let inst = s.geometry_instances();
    let truss_box: Vec<_> = inst.iter().filter(|g| g.object == id(12) && g.file_name == "box.glb").collect();
    assert_eq!(truss_box.len(), 1);
    // Outer symbol puts Inner at local x 100, Inner lifts the box 10. Truss x axis is world y.
    let o = truss_box[0].world.o;
    assert!((o[0]).abs() < 1e-9 && (o[1] - 3100.0).abs() < 1e-9 && (o[2] - 6510.0).abs() < 1e-9, "{o:?}");
    assert_eq!(inst.len(), 3); // box via symbols, pipe.3ds, platform box

    let st = s.stats();
    assert_eq!((st.layers, st.fixtures, st.trusses, st.groups, st.symbols), (2, 2, 1, 1, 2));
    assert_eq!(mvr.resources.resolve_gdtf("Generic@Spot"), Some("Generic@Spot.gdtf"));
}

#[test]
fn dmx_address_split() {
    let a = DmxAddress::new(0, 2, 17);
    assert_eq!((a.absolute, a.universe(), a.address()), (529, 2, 17));
    let b = DmxAddress { dmx_break: 0, absolute: 512 };
    assert_eq!((b.universe(), b.address()), (1, 512));
}

fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, d) in files {
        z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(d).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// A messy file in the style of real exporters.
const MESSY: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<GeneralSceneDescription verMajor="1" verMinor="5">
  <UserData>
    <Data provider="Vectorworks" ver="0.1"><VWEntry key="A">x.lit</VWEntry></Data>
  </UserData>
  <Scene>
    <AUXData>
      <Class name="C" uuid="not-a-uuid"/>
      <Symdef name="S" uuid="00000000-0000-0000-0000-000000000000">
        <ChildList><Geometry3D fileName="missing.glb"/></ChildList>
      </Symdef>
    </AUXData>
    <Layers>
      <Layer name="L">
        <Matrix>{1,0,0}{0,1,0}{0,0,1}{0,0,1000}</Matrix>
        <ChildList>
          <Fixture name="F1" uuid="aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee">
            <Matrix>{1,0,0}{0,1}</Matrix>
            <Classing>not-a-uuid</Classing>
            <GDTFSpec>Vendor@Thing</GDTFSpec>
            <GDTFMode>Mode 1</GDTFMode>
            <Addresses><Address break="0"/></Addresses>
            <FixtureID>12</FixtureID>
            <UnitNumber>abc</UnitNumber>
            <FixtureTypeId>0</FixtureTypeId>
            <Color>0.3,0.3</Color>
            <Mappings/>
            <Focus>00000000-0000-0000-0000-000000000000</Focus>
          </Fixture>
          <Fixture name="F2" uuid="AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE">
            <Addresses>
              <Address break="0">2.1</Address>
              <Network Interface="ethernet_1" IPv4="192.168.1.5" SubnetMask="255.255.255.0" DHCP="on"/>
            </Addresses>
            <Position>11223344-5566-4778-8899-AABBCCDDEEFF</Position>
          </Fixture>
          <Lamp name="unknown kind"/>
          <SceneObject name="S" uuid="12345678-1234-4234-8234-123456789abc">
            <Geometries><Symbol uuid="00000000-0000-0000-0000-000000000000" symdef="99999999-1234-4234-8234-123456789abc"/></Geometries>
          </SceneObject>
        </ChildList>
      </Layer>
    </Layers>
  </Scene>
</GeneralSceneDescription>
"#;

#[test]
fn messy_file_imports_with_warnings() {
    let bytes = zip_with(&[(SCENE_FILE, MESSY.as_bytes()), ("Vendor@Thing.gdtf", b"x")]);
    let imp = read_mvr(&bytes).unwrap();
    let w: Vec<String> = imp.warnings.iter().map(|w| w.to_string()).collect();
    let has = |s: &str| w.iter().any(|x| x.contains(s));
    for needle in [
        "malformed uuid \"not-a-uuid\"",
        "has the nil uuid",
        "<Layer> without uuid",
        "matrix has 5 values",
        "empty <Address>",
        "bad integer \"abc\"",
        "bad CIE colour",
        "unknown object <Lamp>",
        "unknown symdef",
        "position refers to unknown position",
        "missing.glb",
        "used 2 times",
    ] {
        assert!(has(needle), "no warning containing {needle:?} in {w:#?}");
    }
    assert!(!has("Vendor@Thing"), "GDTF without extension should resolve: {w:#?}");

    let s = &imp.mvr.scene;
    // The same bad text maps to the same UUID, so the classing still links.
    assert_eq!(s.objects_of(ObjectKind::Fixture).next().unwrap().classing, Some(s.aux.classes[0].uuid));
    let f2 = s.objects_of(ObjectKind::Fixture).nth(1).unwrap();
    assert_eq!(f2.addresses, vec![DmxAddress { dmx_break: 0, absolute: 513 }]);
    assert_eq!(f2.networks[0].geometry, "ethernet_1");
    assert_eq!(f2.networks[0].dhcp, Some(true));
    assert_eq!(f2.world_position(), [0.0, 0.0, 1000.0]);
    let f1 = s.objects_of(ObjectKind::Fixture).next().unwrap();
    assert_eq!(f1.focus, None, "nil reference means none");
    assert_eq!(f1.extra[0].name, "FixtureTypeId");
    assert_eq!(s.user_data.len(), 1);

    // And it round-trips, unknown nodes included.
    let again = read_mvr(&write_mvr(&imp.mvr).unwrap()).unwrap();
    assert_eq!(again.mvr.scene, imp.mvr.scene);
}

#[test]
fn fatal_errors() {
    assert!(matches!(read_mvr(b"definitely not a zip"), Err(MvrError::NotZip(_))));
    let no_scene = zip_with(&[("a.gdtf", b"x")]);
    assert!(matches!(read_mvr(&no_scene), Err(MvrError::NoSceneFile)));
    // Broken XML is not fatal.
    let broken = zip_with(&[(
        SCENE_FILE,
        b"<GeneralSceneDescription verMajor=\"1\" verMinor=\"6\"><Scene><Layers><Layer uuid=\"",
    )]);
    let imp = read_mvr(&broken).unwrap();
    assert!(!imp.warnings.is_empty());
}

#[test]
fn mutated_xml_never_panics() {
    // Deterministic pseudo-random mutations of a valid file: truncation, byte flips,
    // duplicated slices. The reader must always return.
    let base = write_scene_xml(&full_scene().scene).into_bytes();
    let mut seed: u64 = 0x5eed;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..400 {
        let mut b = base.clone();
        for _ in 0..(next() % 6 + 1) {
            let i = (next() as usize) % b.len();
            match next() % 4 {
                0 => b.truncate(i.max(1)),
                1 => b[i] = (next() % 256) as u8,
                2 => {
                    let j = (i + (next() as usize) % 64).min(b.len());
                    let slice = b[i..j].to_vec();
                    b.splice(i..i, slice);
                }
                _ => {
                    b.remove(i);
                }
            }
            if b.is_empty() {
                b.push(b'<');
            }
        }
        let text = String::from_utf8_lossy(&b);
        let (_, scene, _) = read_scene_xml(&text);
        let _ = write_scene_xml(&scene);
        let _ = scene.geometry_instances();
    }
}

#[test]
fn symbol_cycles_terminate() {
    let mut s = Scene::default();
    s.aux.symdefs.push(Symdef {
        uuid: id(1),
        name: "A".into(),
        geometries: vec![Geometry::Symbol { uuid: id(2), symdef: id(1), matrix: Matrix::IDENTITY }],
    });
    let mut l = Layer::new(id(3), "L");
    let mut o = Object::new(ObjectKind::SceneObject, id(4), "O");
    o.geometries.push(Geometry::Symbol { uuid: id(5), symdef: id(1), matrix: Matrix::IDENTITY });
    l.children.push(o);
    s.layers.push(l);
    assert!(s.geometry_instances().is_empty());
}
