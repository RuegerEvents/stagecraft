//! Private acceptance tests: `testdata/private/tests/` is git-ignored (real venues and
//! client projects). Where it is present, `cfg(stagecraft_private)` compiles them into
//! `tests/private.rs`; elsewhere that test is empty.

const FILE: &str = "venue_sketch.rs";

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/tests");
    println!("cargo::rerun-if-changed={}", dir.display());
    println!("cargo::rustc-check-cfg=cfg(stagecraft_private)");
    if dir.join(FILE).is_file() {
        println!("cargo::rustc-cfg=stagecraft_private");
    }
}
