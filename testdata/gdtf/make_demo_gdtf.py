#!/usr/bin/env python3
"""Writes the synthetic demo GDTFs next to this script. Run: python3 -I make_demo_gdtf.py

Primitive geometry only (no mesh files), so the files stay a few kB. They are our
own drawings of generic fixtures, not copies of any manufacturer's file.
"""
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))


def m(x=0, y=0, z=0):
    return "{1,0,0,%g}{0,1,0,%g}{0,0,1,%g}{0,0,0,1}" % (x, y, z)


def ch(offset, geometry, attribute, brk="1"):
    return (f'<DMXChannel DMXBreak="{brk}" Offset="{offset}" Geometry="{geometry}">'
            f'<LogicalChannel Attribute="{attribute}"/></DMXChannel>')


def fixture(name, short, long, fid, weight, power, models, geometries, modes, desc):
    mode_xml = "".join(
        f'<DMXMode Name="{n}" Geometry="{g}"><DMXChannels>{"".join(c)}</DMXChannels></DMXMode>' for n, g, c in modes)
    model_xml = "".join(
        f'<Model Name="{n}" Length="{l}" Width="{w}" Height="{h}" PrimitiveType="{p}"{extra}/>'
        for n, l, w, h, p, extra in models)
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<GDTF DataVersion="1.2">
  <FixtureType Name="{name}" ShortName="{short}" LongName="{long}" Manufacturer="StageCraft Demo"
      Description="{desc}" FixtureTypeID="{fid}" CanHaveChildren="No">
    <AttributeDefinitions/>
    <PhysicalDescriptions><Properties><Weight Value="{weight}"/>
      <PowerConsumption Value="{power}" PowerFactor="0.95" Connector="Power In" VoltageLow="100" VoltageHigh="240"/>
    </Properties></PhysicalDescriptions>
    <Models>{model_xml}</Models>
    <Geometries>{geometries}</Geometries>
    <DMXModes>{mode_xml}</DMXModes>
    <Revisions><Revision Text="synthetic demo fixture" Date="2026-11-18T00:00:00" UserID="0"/></Revisions>
  </FixtureType>
</GDTF>
'''


def write(file_name, xml, extra=()):
    path = os.path.join(HERE, file_name)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        info = zipfile.ZipInfo("description.xml", date_time=(2026, 11, 18, 0, 0, 0))
        z.writestr(info, xml)
        for name, data in extra:
            z.writestr(zipfile.ZipInfo(name, date_time=(2026, 11, 18, 0, 0, 0)), data)
    print(path, os.path.getsize(path), "bytes")


# A profile spot: yoke, a long barrel hanging below the hanging point, lens at the bottom.
profile = fixture(
    "Profile 750", "PRF", "Demo Profile 750 W", "5c0a1f00-0000-4000-8000-000000000001", 7.5, 750,
    [("Yoke", 0.30, 0.05, 0.35, "Yoke", ""),
     ("Body", 0.20, 0.20, 0.60, "Conventional1_1", ""),
     ("Lens", 0.16, 0.16, 0.002, "Cylinder", "")],
    f'''<Geometry Name="Clamp" Position="{m()}">
          <Geometry Name="Yoke" Model="Yoke" Position="{m(z=-0.175)}"/>
          <Geometry Name="Body" Model="Body" Position="{m(z=-0.05)}">
            <Beam Name="Beam" Model="Lens" Position="{m(z=-0.6)}" LampType="Tungsten" PowerConsumption="750"
                LuminousFlux="16000" BeamAngle="26" FieldAngle="36" BeamType="Spot"/>
          </Geometry>
        </Geometry>''',
    [("Dimmer", "Clamp", [ch("1", "Beam", "Dimmer")]),
     ("Dimmer 16 bit", "Clamp", [ch("1,2", "Beam", "Dimmer")])],
    "Generic ellipsoidal profile spot, 26 degree lens")

# A moving head spot: base at the top, yoke (pan) and head (tilt) below.
spot_channels = [ch("1,2", "Yoke", "Pan"), ch("3,4", "Head", "Tilt"), ch("5", "Head", "Dimmer"),
                 ch("6", "Head", "Shutter1"), ch("7", "Head", "Color1"), ch("8", "Head", "Gobo1"),
                 ch("9", "Head", "Gobo1Pos"), ch("10", "Head", "Prism1"), ch("11", "Head", "Focus1"),
                 ch("12", "Head", "Zoom"), ch("13", "Head", "Iris"), ch("14", "Head", "ColorAdd_C"),
                 ch("15", "Head", "ColorAdd_M"), ch("16", "Head", "ColorAdd_Y")]
spot = fixture(
    "Spot 300", "SP3", "Demo Moving Head Spot 300", "5c0a1f00-0000-4000-8000-000000000002", 21.0, 480,
    [("Base", 0.40, 0.30, 0.12, "Base1_1", ""),
     ("Yoke", 0.36, 0.12, 0.34, "Yoke", ""),
     ("Head", 0.24, 0.30, 0.34, "Head", ""),
     ("Lens", 0.14, 0.14, 0.002, "Cylinder", "")],
    f'''<Geometry Name="Base" Model="Base" Position="{m()}">
          <Axis Name="Yoke" Model="Yoke" Position="{m(z=-0.29)}">
            <Axis Name="Head" Model="Head" Position="{m(z=-0.03)}">
              <Beam Name="Beam" Model="Lens" Position="{m(z=-0.17)}" LampType="LED" PowerConsumption="300"
                  LuminousFlux="12000" BeamAngle="16" FieldAngle="22" BeamType="Spot"/>
            </Axis>
          </Axis>
        </Geometry>''',
    [("Standard", "Base", spot_channels),
     ("Basic", "Base", [ch("1", "Yoke", "Pan"), ch("2", "Head", "Tilt"), ch("3", "Head", "Dimmer"),
                        ch("4", "Head", "Shutter1"), ch("5", "Head", "Color1"), ch("6", "Head", "Gobo1")])],
    "Generic moving head spot with gobos and CMY")

# An LED pixel bar: 8 RGB pixels; mode "Split" puts the pixels on a second DMX break.
refs = []
for i in range(8):
    x = -0.4375 + i * 0.125
    refs.append(f'<GeometryReference Name="Pixel {i + 1}" Geometry="Pixel" Position="{m(x=x, z=-0.091)}">'
                f'<Break DMXBreak="1" DMXOffset="{2 + 3 * i}"/><Break DMXBreak="2" DMXOffset="{1 + 3 * i}"/>'
                f'</GeometryReference>')
bar = fixture(
    "Pixel Bar 8", "PB8", "Demo LED Pixel Bar 1 m", "5c0a1f00-0000-4000-8000-000000000003", 4.2, 120,
    [("Bar", 1.0, 0.08, 0.09, "Cube", ' File="bar" SVGOffsetX="500" SVGOffsetY="40"'),
     ("Pixel", 0.11, 0.06, 0.002, "Cube", "")],
    f'''<Geometry Name="Bar" Model="Bar" Position="{m(z=-0.045)}">{"".join(refs)}</Geometry>
        <Beam Name="Pixel" Model="Pixel" LampType="LED" PowerConsumption="12" BeamAngle="20" FieldAngle="30"
            BeamType="Wash"/>''',
    [("8px RGB", "Bar", [ch("1", "Bar", "Dimmer"), ch("1", "Pixel", "ColorAdd_R"), ch("2", "Pixel", "ColorAdd_G"),
                         ch("3", "Pixel", "ColorAdd_B")]),
     ("Split", "Bar", [ch("1", "Bar", "Dimmer"), ch("2", "Bar", "Shutter1"), ch("1", "Pixel", "ColorAdd_R", "2"),
                       ch("2", "Pixel", "ColorAdd_G", "2"), ch("3", "Pixel", "ColorAdd_B", "2")])],
    "Generic LED batten with 8 RGB pixels")
pixels = "".join(f'<rect x="{12 + 125 * i}" y="12" width="101" height="56" fill="none"/>' for i in range(8))
bar_svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="1000mm" height="80mm" viewBox="0 0 1000 80">'
           '<rect x="0" y="0" width="1000" height="80" fill="#888" stroke="#000"/>' + pixels + '</svg>')

write("StageCraft Demo@Profile 750.gdtf", profile)
write("StageCraft Demo@Spot 300.gdtf", spot)
write("StageCraft Demo@Pixel Bar 8.gdtf", bar, [("models/svg/bar.svg", bar_svg)])
