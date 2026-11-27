//! 3D images for `rendered` sheet viewports, rendered without a window.
//! Skips when the machine has no GPU adapter.

use serde_json::json;
use stagecraft_core::sheet::ViewKind;
use stagecraft_sheet::ImageData;
use stagecraft_ui::view3d::offscreen::Offscreen;

#[test]
fn rendered_viewports_get_images_and_are_cached() {
    let Some(mut off) = Offscreen::headless() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let mut e = stagecraft_ui::demo::engine();
    if let Ok(path) = std::env::var("STAGECRAFT_DEMO_FILE") {
        e.execute("doc.save", json!({ "path": path })).unwrap();
    }
    let doc = e.doc().clone();
    let vp = doc
        .sheets
        .values()
        .flat_map(|s| s.viewports.clone())
        .find(|v| matches!(v.view, ViewKind::Rendered { .. }))
        .unwrap();
    let img = off.render(&doc, &vp).expect("an image");
    let ImageData::Rgba { width, height, pixels } = &img else { panic!("rgba") };
    // 180 × 120 mm at 200 dpi, as cropped: 160 × 110 mm.
    assert_eq!((*width, *height), (1260, 866));
    // Not blank: the model is drawn on white.
    let dark = pixels.as_chunks::<4>().0.iter().filter(|p| p[0] < 200).count();
    assert!(dark > 10_000, "{dark} dark pixels");
    assert_eq!(off.renders, 1);
    off.render(&doc, &vp).unwrap();
    assert_eq!(off.renders, 1, "unchanged document: cached");
    // A sheet edit does not touch the model; moving an object does.
    let sheet = doc.sheets.values().next().unwrap().id;
    e.execute("sheet.update", json!({ "sheet": sheet, "name": "Renamed" })).unwrap();
    off.render(e.doc(), &vp).unwrap();
    assert_eq!(off.renders, 1);
    let any = e.doc().objects.values().find(|o| o.parent.is_none()).unwrap().id;
    e.execute("object.move", json!({ "ids": [any], "delta": [100, 0, 0] })).unwrap();
    off.render(e.doc(), &vp).unwrap();
    assert_eq!(off.renders, 2);
}
