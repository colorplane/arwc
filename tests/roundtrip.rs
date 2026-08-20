mod common;

use common::{build_arw, default_arw, JpegSpec, Pixels, Spec};
use compress_arw::{
    decode, encode, encode_with_level, extract_preview, inspect, jpeg_end, Kind, FOOTER_LEN,
    FOOTER_MAGIC, MAGIC,
};

fn roundtrip_ok(arw: &[u8], level: i32) {
    let enc = encode_with_level(arw, level).unwrap();
    let dec = decode(&enc).unwrap();
    assert_eq!(dec, arw, "bit-identical at zstd level {level}");
}

#[test]
fn default_synthetic_is_bit_identical() {
    roundtrip_ok(&default_arw(), 3);
}

#[test]
fn many_sizes_and_pixel_patterns() {
    let sizes = [(2, 2), (8, 4), (10, 7), (32, 16), (64, 48)];
    let patterns = [
        Pixels::Constant(0),
        Pixels::Constant(0x3fff),
        Pixels::Gradient,
        Pixels::Wrap,
        Pixels::Lcg(1),
        Pixels::Lcg(0xC0FFEE),
    ];
    for (w, h) in sizes {
        for pixels in patterns {
            let mut spec = Spec::new(w, h);
            spec.pixels = pixels;
            let arw = build_arw(spec);
            roundtrip_ok(&arw, 1);
        }
    }
}

#[test]
fn zstd_levels_decode_the_same() {
    let arw = default_arw();
    let a = encode_with_level(&arw, 1).unwrap();
    let b = encode_with_level(&arw, 3).unwrap();
    let c = encode_with_level(&arw, 9).unwrap();
    assert_eq!(decode(&a).unwrap(), arw);
    assert_eq!(decode(&b).unwrap(), arw);
    assert_eq!(decode(&c).unwrap(), arw);
    assert_eq!(decode(&encode(&arw).unwrap()).unwrap(), arw);
}

#[test]
fn is_encoded_tracks_container() {
    let arw = default_arw();
    assert!(!compress_arw::is_encoded(&arw));
    let enc = encode_with_level(&arw, 3).unwrap();
    assert!(compress_arw::is_encoded(&enc));
}

#[test]
fn output_is_jpeg_with_trailer() {
    let enc = encode(&default_arw()).unwrap();
    assert_eq!(&enc[..2], &[0xff, 0xd8]);
    let eoi = jpeg_end(&enc).unwrap();
    assert!(eoi < enc.len());
    assert_eq!(&enc[eoi..eoi + 4], MAGIC);
    assert_eq!(&enc[enc.len() - 4..], FOOTER_MAGIC);
    let info = inspect(&enc).unwrap();
    assert_eq!(info.kind, Kind::JpegContainer);
    assert!(info.encoded);
    assert_eq!(info.preview_bytes, eoi as u32);
    assert_eq!(info.orig_bytes, Some(default_arw().len() as u64));
    assert!(info.orig_sha1.is_some());
}

#[test]
fn jpeg_padding_after_eoi_is_restored() {
    let mut spec = Spec::new(12, 6);
    spec.preview.padding = 17;
    let arw = build_arw(spec);
    roundtrip_ok(&arw, 3);
    let enc = encode_with_level(&arw, 3).unwrap();
    let preview = extract_preview(&enc).unwrap();
    assert_eq!(
        preview.last(),
        Some(&0xd9),
        "padding must not leak into the viewable JPEG"
    );
}

#[test]
fn thumbnail_ifd_survives_roundtrip() {
    let mut spec = Spec::new(8, 8);
    spec.thumb = Some(JpegSpec {
        app1: b"Exif\x00\x00THUMB".to_vec(),
        entropy: vec![0x33],
        padding: 0,
    });
    let arw = build_arw(spec);
    assert_eq!(compress_arw::parse_layout(&arw).unwrap().jpegs.len(), 2);
    roundtrip_ok(&arw, 3);
}

#[test]
fn make_ascii_and_exif_survive() {
    let arw = default_arw();
    assert!(arw.windows(4).any(|w| w == b"SONY"));
    let enc = encode_with_level(&arw, 3).unwrap();
    let prev = extract_preview(&enc).unwrap();
    assert!(prev.windows(4).any(|w| w == b"Exif"));
    assert!(prev.windows(14).any(|w| w == b"SONY-ILCE-TEST"));
    let dec = decode(&enc).unwrap();
    assert!(dec.windows(4).any(|w| w == b"SONY"));
}

#[test]
fn view_exif_carries_tiff_orientation_and_strips_on_decode() {
    let mut spec = Spec::new(8, 4);
    spec.orientation = 6;
    spec.thumb = Some(JpegSpec {
        app1: b"Exif\x00\x00THUMB".to_vec(),
        entropy: vec![0x33],
        padding: 0,
    });
    let arw = build_arw(spec);
    assert_eq!(inspect(&arw).unwrap().orientation, 6);
    let enc = encode_with_level(&arw, 3).unwrap();
    let prev = extract_preview(&enc).unwrap();
    assert_eq!(compress_arw::jpeg_exif_orientation(prev), Some(6));
    assert_eq!(
        compress_arw::jpeg_exif_orientation(&enc[..512]),
        Some(6),
        "orientation must sit in the first 512 bytes"
    );
    let thumb = compress_arw::jpeg_exif_thumbnail(prev).expect("copied TIFF IFD1 JPEG");
    assert_eq!(&thumb[..2], &[0xff, 0xd8]);
    assert!(thumb.windows(5).any(|w| w == b"THUMB"));
    assert_eq!(inspect(&enc).unwrap().orientation, 6);
    roundtrip_ok(&arw, 3);
}

#[test]
fn view_exif_orientation_fits_in_the_first_64_bytes_without_a_thumb() {
    let mut spec = Spec::new(8, 4);
    spec.orientation = 8;
    let arw = build_arw(spec);
    let enc = encode_with_level(&arw, 3).unwrap();
    assert_eq!(compress_arw::jpeg_exif_orientation(&enc[..64]), Some(8));
    roundtrip_ok(&arw, 3);
}

#[test]
fn encode_progress_reaches_100() {
    use compress_arw::encode_with_progress;
    let arw = default_arw();
    let mut seen = Vec::new();
    let enc = encode_with_progress(&arw, 3, |pct| seen.push(pct)).unwrap();
    assert_eq!(decode(&enc).unwrap(), arw);
    assert!(!seen.is_empty());
    assert_eq!(*seen.last().unwrap(), 100);
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn footer_stores_orig_size_and_sha1() {
    use sha1::{Digest, Sha1};
    let arw = default_arw();
    let enc = encode_with_level(&arw, 3).unwrap();
    assert_eq!(enc.len() >= FOOTER_LEN, true);
    let f = &enc[enc.len() - FOOTER_LEN..];
    assert_eq!(&f[28..32], FOOTER_MAGIC);
    assert_eq!(
        u64::from_le_bytes(f[0..8].try_into().unwrap()),
        arw.len() as u64
    );
    let mut want = [0u8; 20];
    want.copy_from_slice(&Sha1::digest(&arw));
    assert_eq!(&f[8..28], &want);
    let info = inspect(&enc).unwrap();
    assert_eq!(info.orig_bytes, Some(arw.len() as u64));
    assert_eq!(info.orig_sha1, Some(want));
    assert_eq!(info.orig_sha1_hex().unwrap().len(), 40);
}

#[test]
fn decode_still_works_without_footer() {
    let arw = default_arw();
    let enc = encode_with_level(&arw, 3).unwrap();
    let stripped = &enc[..enc.len() - FOOTER_LEN];
    assert_ne!(&stripped[stripped.len() - 4..], FOOTER_MAGIC);
    assert_eq!(decode(stripped).unwrap(), arw);
}

#[test]
fn decode_accepts_legacy_files_without_view_exif() {
    let arw = default_arw();
    let enc = encode_with_level(&arw, 3).unwrap();
    let eoi = jpeg_end(&enc).unwrap();
    let camera = compress_arw::strip_view_exif(&enc[..eoi]).unwrap();
    let mut legacy = camera;
    legacy.extend_from_slice(&enc[eoi..]);
    assert_eq!(
        compress_arw::jpeg_exif_orientation(&legacy[..eoi.min(256)]),
        None
    );
    assert_eq!(decode(&legacy).unwrap(), arw);
}
