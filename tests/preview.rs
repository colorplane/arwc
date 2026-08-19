mod common;

use common::{build_arw, build_jpeg, default_arw, JpegSpec, Spec};
use compress_arw::{encode_with_level, extract_preview, inspect, jpeg_end};

#[test]
fn preview_from_arw_is_largest_jpeg() {
    let mut spec = Spec::new(8, 4);
    spec.preview = JpegSpec::with_exif(b"BIG-PREVIEW");
    spec.thumb = Some(JpegSpec {
        app1: b"Exif\x00\x00SMOL".to_vec(),
        entropy: vec![0x01],
        padding: 0,
    });
    let arw = build_arw(spec);
    let prev = extract_preview(&arw).unwrap();
    assert!(prev.windows(11).any(|w| w == b"BIG-PREVIEW"));
    assert!(!prev.windows(4).any(|w| w == b"SMOL"));
}

#[test]
fn preview_from_container_does_not_need_trailer() {
    let arw = default_arw();
    let enc = encode_with_level(&arw, 3).unwrap();
    let eoi = jpeg_end(&enc).unwrap();
    let from_full = extract_preview(&enc).unwrap();
    let from_prefix = extract_preview(&enc[..eoi]).unwrap();
    assert_eq!(from_full, from_prefix);
    assert_eq!(from_full[0], 0xff);
    assert_eq!(*from_full.last().unwrap(), 0xd9);
    assert_eq!(inspect(&enc).unwrap().preview_bytes as usize, eoi);
}

#[test]
fn preview_bytes_is_enough_for_range_fetch() {
    let arw = default_arw();
    let enc = encode_with_level(&arw, 3).unwrap();
    let n = inspect(&enc).unwrap().preview_bytes as usize;
    assert!(n < enc.len(), "raw trailer must sit after the JPEG");
    let jpeg = extract_preview(&enc[..n]).unwrap();
    assert_eq!(jpeg.len(), n);
}

#[test]
fn generated_jpeg_scan_matches_builder() {
    let spec = JpegSpec::with_exif(b"HELLO");
    let j = build_jpeg(&spec);
    assert_eq!(jpeg_end(&j).unwrap(), j.len() - spec.padding);
}

#[test]
fn inspect_raw_arw_is_not_encoded() {
    let info = inspect(&default_arw()).unwrap();
    assert!(!info.encoded);
    assert_eq!(info.width, 16);
    assert_eq!(info.height, 8);
    assert_eq!(info.orientation, 1);
}

#[test]
fn inspect_reads_tiff_orientation_and_keeps_it_after_encode() {
    let mut spec = Spec::new(8, 4);
    spec.orientation = 8;
    let arw = build_arw(spec);
    assert_eq!(inspect(&arw).unwrap().orientation, 8);
    let enc = encode_with_level(&arw, 3).unwrap();
    assert_eq!(inspect(&enc).unwrap().orientation, 8);
}
