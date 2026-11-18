//! `cargo run -p stagecraft-underlay --example convert -- in.svg out.png [page]`:
//! renders an underlay file as StageCraft would show it.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).expect("read input");
    let ext = a[1].rsplit('.').next().unwrap_or("");
    let f = stagecraft_underlay::Format::from_ext(ext).expect("png, jpg, pdf, svg or dxf");
    let page = a.get(3).and_then(|p| p.parse().ok()).unwrap_or(1);
    let c = stagecraft_underlay::convert(f, &bytes, page).expect("convert");
    println!("extent {:?}, unit {} mm, pages {}", c.extent, c.unit_mm, c.pages);
    if let Some(r) = c.raster {
        std::fs::write(&a[2], r.png).expect("write");
        println!("{} x {} px", r.width, r.height);
    }
}
