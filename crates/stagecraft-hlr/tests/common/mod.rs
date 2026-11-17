#![allow(dead_code)]

use std::path::PathBuf;

use stagecraft_hlr::svg::{SvgStyle, to_svg};
use stagecraft_hlr::{DVec2, Drawing, LineClass};

/// Where test views go: `$HLR_OUT`, else `<target>/hlr-views`.
pub fn out_dir() -> PathBuf {
    let dir = std::env::var_os("HLR_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target-hlr/hlr-views"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Write `name.svg`, and `name.png` when rsvg-convert is installed.
pub fn save(name: &str, d: &Drawing) {
    let dir = out_dir();
    let svg = dir.join(format!("{name}.svg"));
    std::fs::write(&svg, to_svg(d, &SvgStyle::default())).unwrap();
    let _ = std::process::Command::new("rsvg-convert")
        .args(["-d", "150", "-p", "150", "-b", "white", "-o"])
        .arg(dir.join(format!("{name}.png")))
        .arg(&svg)
        .status();
}

/// Straight segments of one class (a polyline with n points has n − 1, closed n).
pub fn segments(d: &Drawing, class: LineClass) -> Vec<(DVec2, DVec2)> {
    let mut out = Vec::new();
    for l in d.lines(class) {
        for w in l.points.windows(2) {
            out.push((w[0], w[1]));
        }
        if l.closed {
            out.push((*l.points.last().unwrap(), l.points[0]));
        }
    }
    out
}

pub fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[track_caller]
pub fn assert_near(a: f64, b: f64, tol: f64) {
    assert!(near(a, b, tol), "{a} is not within {tol} of {b}");
}
