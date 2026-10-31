#[test]
fn probe_debug() {
    let c = ocrx_ovisocr2::degenerate_tail_crop("循环片段循环片段循环片段循环");
    println!("deg={} text={:?}", c.degenerated, c.text);
}
