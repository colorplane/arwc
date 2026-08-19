mod common;

use common::{build_arw, default_arw, JpegSpec, Spec};
use compress_arw::parse_layout;

#[test]
fn layout_finds_raw_and_preview() {
    let arw = default_arw();
    let layout = parse_layout(&arw).unwrap();
    assert_eq!(layout.raw.width, 16);
    assert_eq!(layout.raw.height, 8);
    assert_eq!(layout.raw.bits, 14);
    assert_eq!(layout.raw.compression, 1);
    assert_eq!(
        layout.raw.strip_bytes as usize,
        16 * 8 * 2
    );
    assert_eq!(
        layout.raw.strip_offset as usize + layout.raw.strip_bytes as usize,
        arw.len()
    );
    assert_eq!(layout.jpegs.len(), 1);
    let j = &layout.jpegs[0];
    assert_eq!(&arw[j.offset as usize..j.offset as usize + 2], &[0xff, 0xd8]);
}

#[test]
fn layout_finds_both_jpegs() {
    let mut spec = Spec::new(8, 4);
    spec.thumb = Some(JpegSpec::tiny());
    let arw = build_arw(spec);
    let layout = parse_layout(&arw).unwrap();
    assert_eq!(layout.jpegs.len(), 2);
    assert!(layout.jpegs[0].length != layout.jpegs[1].length);
}

#[test]
fn prefix_without_strip_still_parses_ifds() {
    let arw = default_arw();
    let layout = parse_layout(&arw).unwrap();
    let header = &arw[..layout.raw.strip_offset as usize];
    let from_header = parse_layout(header).unwrap();
    assert_eq!(from_header.raw.width, layout.raw.width);
    assert_eq!(from_header.jpegs.len(), layout.jpegs.len());
}
