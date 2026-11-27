//! The private venue acceptance test (`testdata/private/tests/venue_ui.rs`, git-ignored),
//! compiled only where that file exists (see `build.rs`).

#[cfg(stagecraft_private)]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/private/tests/venue_ui.rs"));
