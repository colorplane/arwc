mod common;

use common::default_arw;
use compress_arw::{decode, encode, encode_with_level, extract_preview, inspect, Error};

#[test]
fn empty_and_truncated() {
    assert!(matches!(
        inspect(&[]),
        Err(Error::Truncated) | Err(Error::Format(_))
    ));
    assert!(encode(&[0xff, 0xd8]).is_err());
    assert!(decode(&[0xff, 0xd8, 0xff, 0xd9]).is_err());
}

#[test]
fn jpeg_without_trailer() {
    let j = [0xff, 0xd8, 0xff, 0xd9];
    assert!(matches!(inspect(&j), Err(Error::Format(_))));
    assert!(!compress_arw::is_encoded(&j));
}

#[test]
fn already_encoded_refused() {
    let enc = encode(&default_arw()).unwrap();
    assert!(matches!(encode(&enc), Err(Error::Format(_))));
}

#[test]
fn non_uncompressed_strip_refused() {
    let mut arw = default_arw();
    // SubIFD compression SHORT is inline at the entry for tag 259.
    // Safer: flip bits-per-sample by searching the 14-bit field is fragile.
    // Build with a post-patch: find uncompressed marker pattern in SubIFD.
    let layout = compress_arw::parse_layout(&arw).unwrap();
    let at = layout.raw.compression_value_at;
    arw[at] = 7;
    arw[at + 1] = 0;
    assert!(matches!(
        encode(&arw),
        Err(Error::Unsupported("raw strip is not uncompressed"))
    ));
}

#[test]
fn twelve_bit_refused() {
    let mut arw = default_arw();
    let off = find_bits_per_sample(&arw);
    arw[off] = 12;
    arw[off + 1] = 0;
    assert!(matches!(
        encode(&arw),
        Err(Error::Unsupported("expected 14-bit raw"))
    ));
}

fn find_bits_per_sample(arw: &[u8]) -> usize {
    // TIFF entry: tag 258 (0x02 0x01), type SHORT (0x03 0x00), count 1, value 14.
    for i in 0..arw.len().saturating_sub(12) {
        if arw[i] == 0x02
            && arw[i + 1] == 0x01
            && arw[i + 2] == 0x03
            && arw[i + 3] == 0x00
            && arw[i + 8] == 14
            && arw[i + 9] == 0
        {
            return i + 8;
        }
    }
    panic!("bits entry not found");
}

#[test]
fn strip_not_at_eof_refused() {
    let mut arw = default_arw();
    arw.push(0xff);
    assert!(matches!(encode(&arw), Err(Error::Unsupported(_))));
}

#[test]
fn truncated_container() {
    let enc = encode_with_level(&default_arw(), 3).unwrap();
    assert!(decode(&enc[..enc.len() / 2]).is_err());
    assert!(extract_preview(&enc[..3]).is_err());
}

#[test]
fn corrupt_zstd_payload() {
    let mut enc = encode_with_level(&default_arw(), 3).unwrap();
    let last = enc.len() - compress_arw::FOOTER_LEN - 1;
    enc[last] ^= 0xff;
    assert!(matches!(
        decode(&enc),
        Err(Error::Zstd(_)) | Err(Error::Format(_))
    ));
}

#[test]
fn tampered_sha1_is_rejected() {
    let mut enc = encode_with_level(&default_arw(), 3).unwrap();
    let sha1_byte = enc.len() - compress_arw::FOOTER_LEN + 8;
    enc[sha1_byte] ^= 0xff;
    assert!(matches!(
        decode(&enc),
        Err(Error::Integrity("SHA-1 mismatch"))
    ));
}

#[test]
fn tampered_orig_size_is_rejected() {
    let mut enc = encode_with_level(&default_arw(), 3).unwrap();
    let size_byte = enc.len() - compress_arw::FOOTER_LEN;
    enc[size_byte] ^= 0x01;
    assert!(matches!(
        decode(&enc),
        Err(Error::Integrity("uncompressed size mismatch"))
    ));
}

#[test]
fn big_endian_tiff_refused() {
    let mut t = b"MM\x00\x2a\x00\x00\x00\x08".to_vec();
    t.resize(64, 0);
    assert!(encode(&t).is_err());
}
