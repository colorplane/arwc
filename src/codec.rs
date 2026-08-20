use crate::error::{Error, Result};
use crate::jpeg::{
    attach_view_exif, jpeg_end, jpeg_exif_orientation, jpeg_exif_thumbnail, split_jpeg_prefix,
    strip_view_exif,
};
use crate::tiff::{ifd0_orientation, jpeg_bytes, parse_layout};
use crate::transform;
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 4] = b"ARWZ";
pub const VERSION: u8 = 2;
pub const TRANSFORM_BAYER_HDELTA_SHUFFLE: u8 = 1;
pub const TRAILER_HEADER_LEN: usize = 40;
pub const DEFAULT_LEVEL: i32 = 19;

/// EOF footer: uncompressed size + SHA-1 of the original ARW.
/// `[orig_bytes u64 LE][sha1 20][magic ARWH]`
pub const FOOTER_MAGIC: &[u8; 4] = b"ARWH";
pub const FOOTER_LEN: usize = 32;

/// Canonical suffix for encoded files (`photo.ARW` → `photo.ARWC.JPG`).
pub const ENCODED_EXTENSION: &str = ".ARWC.JPG";

pub fn encoded_output_path(input: impl AsRef<Path>) -> PathBuf {
    input.as_ref().with_extension("ARWC.JPG")
}

/// `shot.ARWC.JPG` / `shot.arwc.jpg` → `shot.ARW`
pub fn decoded_output_path(input: impl AsRef<Path>) -> PathBuf {
    let input = input.as_ref();
    let name = input.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = name.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix(".arwc.jpg") {
        let stem = &name[..stripped.len()];
        input.with_file_name(format!("{stem}.ARW"))
    } else {
        input.with_extension("ARW")
    }
}

pub fn has_encoded_extension(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .as_os_str()
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".arwc.jpg")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `FF D8 … FF D9` then ARWZ trailer. Viewers see a normal JPEG.
    JpegContainer,
    /// Uncompressed Sony ARW (TIFF).
    SonyArw,
}

#[derive(Clone, Debug)]
pub struct FileInfo {
    pub kind: Kind,
    /// Bytes to fetch to get the viewable JPEG (and its EXIF). For a JPEG
    /// container this is the EOI offset — no trailer, no raw, no zstd.
    pub preview_bytes: u32,
    pub width: u16,
    pub height: u16,
    pub encoded: bool,
    /// EXIF Orientation 1–8. Encoded files put this in a stripable view APP1
    /// so browsers and Gallery do not need the ARW TIFF. Sony ARW still stores
    /// it on TIFF IFD0, not the camera JPEG.
    pub orientation: u16,
    /// Uncompressed ARW size in bytes (trailer and/or EOF footer).
    pub orig_bytes: Option<u64>,
    /// SHA-1 of the uncompressed ARW, when the EOF footer is present.
    pub orig_sha1: Option<[u8; 20]>,
}

impl FileInfo {
    pub fn orig_sha1_hex(&self) -> Option<String> {
        self.orig_sha1.map(hex_sha1)
    }
}

fn hex_sha1(h: [u8; 20]) -> String {
    let mut s = String::with_capacity(40);
    for b in h {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn sha1_20(data: &[u8]) -> [u8; 20] {
    let d = Sha1::digest(data);
    let mut out = [0u8; 20];
    out.copy_from_slice(&d);
    out
}

struct OrigFooter {
    orig_bytes: u64,
    sha1: [u8; 20],
}

fn parse_orig_footer(file: &[u8]) -> Option<OrigFooter> {
    if file.len() < FOOTER_LEN {
        return None;
    }
    let f = &file[file.len() - FOOTER_LEN..];
    if &f[28..32] != FOOTER_MAGIC {
        return None;
    }
    Some(OrigFooter {
        orig_bytes: u64::from_le_bytes(f[0..8].try_into().unwrap()),
        sha1: f[8..28].try_into().unwrap(),
    })
}

fn write_orig_footer(orig_bytes: u64, sha1: &[u8; 20]) -> [u8; FOOTER_LEN] {
    let mut f = [0u8; FOOTER_LEN];
    f[0..8].copy_from_slice(&orig_bytes.to_le_bytes());
    f[8..28].copy_from_slice(sha1);
    f[28..32].copy_from_slice(FOOTER_MAGIC);
    f
}

struct TrailerMeta {
    orig_compression: u16,
    width: u16,
    height: u16,
    jpeg_tiff_offset: u32,
    jpeg_len: u32,
    header_elided_len: u32,
    orig_strip_bytes: u32,
    orig_strip_offset: u32,
    jpeg_padding_len: u32,
}

fn parse_trailer_meta(t: &[u8]) -> Result<TrailerMeta> {
    if t.len() < TRAILER_HEADER_LEN {
        return Err(Error::Truncated);
    }
    if &t[0..4] != MAGIC {
        return Err(Error::Format("missing ARWZ trailer"));
    }
    if t[4] != VERSION {
        return Err(Error::Unsupported("unknown ARWZ version"));
    }
    if t[5] != TRANSFORM_BAYER_HDELTA_SHUFFLE {
        return Err(Error::Unsupported("unknown transform"));
    }
    Ok(TrailerMeta {
        orig_compression: u16::from_le_bytes([t[6], t[7]]),
        width: u16::from_le_bytes([t[8], t[9]]),
        height: u16::from_le_bytes([t[10], t[11]]),
        jpeg_tiff_offset: u32::from_le_bytes(t[12..16].try_into().unwrap()),
        jpeg_len: u32::from_le_bytes(t[16..20].try_into().unwrap()),
        header_elided_len: u32::from_le_bytes(t[20..24].try_into().unwrap()),
        orig_strip_bytes: u32::from_le_bytes(t[24..28].try_into().unwrap()),
        orig_strip_offset: u32::from_le_bytes(t[28..32].try_into().unwrap()),
        jpeg_padding_len: u32::from_le_bytes(t[32..36].try_into().unwrap()),
    })
}

fn write_trailer_meta(m: &TrailerMeta) -> [u8; TRAILER_HEADER_LEN] {
    let mut h = [0u8; TRAILER_HEADER_LEN];
    h[0..4].copy_from_slice(MAGIC);
    h[4] = VERSION;
    h[5] = TRANSFORM_BAYER_HDELTA_SHUFFLE;
    h[6..8].copy_from_slice(&m.orig_compression.to_le_bytes());
    h[8..10].copy_from_slice(&m.width.to_le_bytes());
    h[10..12].copy_from_slice(&m.height.to_le_bytes());
    h[12..16].copy_from_slice(&m.jpeg_tiff_offset.to_le_bytes());
    h[16..20].copy_from_slice(&m.jpeg_len.to_le_bytes());
    h[20..24].copy_from_slice(&m.header_elided_len.to_le_bytes());
    h[24..28].copy_from_slice(&m.orig_strip_bytes.to_le_bytes());
    h[28..32].copy_from_slice(&m.orig_strip_offset.to_le_bytes());
    h[32..36].copy_from_slice(&m.jpeg_padding_len.to_le_bytes());
    h
}

fn largest_jpeg_in_arw(arw: &[u8]) -> Result<(u32, u32)> {
    let layout = parse_layout(arw)?;
    let j = layout
        .jpegs
        .into_iter()
        .max_by_key(|j| j.length)
        .ok_or(Error::Format("no embedded JPEG"))?;
    Ok((j.offset, j.length))
}

/// Smallest extra JPEG in the ARW (typically TIFF IFD1), for the view APP1.
fn smallest_embedded_jpeg<'a>(
    arw: &'a [u8],
    jpegs: &[crate::tiff::JpegRef],
    skip_off: u32,
    skip_len: u32,
) -> Option<&'a [u8]> {
    jpegs
        .iter()
        .filter(|j| j.offset != skip_off || j.length != skip_len)
        .filter(|j| (2..=60_000).contains(&j.length))
        .filter_map(|j| jpeg_bytes(arw, j).ok())
        .filter(|t| t.len() >= 2 && t[0] == 0xff && t[1] == 0xd8)
        .min_by_key(|t| t.len())
}

fn preview_orientation(jpeg: &[u8], tiff: &[u8]) -> u16 {
    jpeg_exif_orientation(jpeg)
        .or_else(|| ifd0_orientation(tiff))
        .unwrap_or(1)
}

/// The viewable JPEG. Does not decompress the raw strip.
///
/// On a JPEG-container file this is a prefix of the file itself — scan to
/// `FF D9`. Encoded files prepend a stripable ARWC view APP1 (orientation and
/// a copied thumbnail) in front of the camera JPEG. On a Sony ARW it is
/// copied out of the TIFF.
pub fn extract_preview(data: &[u8]) -> Result<&[u8]> {
    if data.len() >= 2 && data[0] == 0xff && data[1] == 0xd8 {
        let end = jpeg_end(data)?;
        return Ok(&data[..end]);
    }
    let (off, len) = largest_jpeg_in_arw(data)?;
    jpeg_bytes(
        data,
        &crate::tiff::JpegRef {
            offset: off,
            length: len,
        },
    )
}

pub fn inspect(data: &[u8]) -> Result<FileInfo> {
    if data.len() >= 2 && data[0] == 0xff && data[1] == 0xd8 {
        let (jpeg, rest) = split_jpeg_prefix(data)?;
        if rest.len() >= 4 && &rest[..4] == MAGIC {
            let meta = parse_trailer_meta(rest)?;
            let elided_end = TRAILER_HEADER_LEN.saturating_add(meta.header_elided_len as usize);
            let elided = rest.get(TRAILER_HEADER_LEN..elided_end).unwrap_or(&[]);
            let footer = parse_orig_footer(data);
            let orig_bytes = footer.as_ref().map(|f| f.orig_bytes).unwrap_or_else(|| {
                u64::from(meta.orig_strip_offset) + u64::from(meta.orig_strip_bytes)
            });
            return Ok(FileInfo {
                kind: Kind::JpegContainer,
                preview_bytes: jpeg.len() as u32,
                width: meta.width,
                height: meta.height,
                encoded: true,
                orientation: preview_orientation(jpeg, elided),
                orig_bytes: Some(orig_bytes),
                orig_sha1: footer.map(|f| f.sha1),
            });
        }
        return Err(Error::Format("JPEG has no ARWZ trailer"));
    }
    let layout = parse_layout(data)?;
    let jpeg = largest_jpeg_in_arw(data)
        .ok()
        .and_then(|(off, len)| {
            jpeg_bytes(
                data,
                &crate::tiff::JpegRef {
                    offset: off,
                    length: len,
                },
            )
            .ok()
        })
        .unwrap_or(&[]);
    Ok(FileInfo {
        kind: Kind::SonyArw,
        preview_bytes: layout.raw.strip_offset,
        width: layout.raw.width,
        height: layout.raw.height,
        encoded: false,
        orientation: preview_orientation(jpeg, data),
        orig_bytes: Some(data.len() as u64),
        orig_sha1: None,
    })
}

pub fn is_encoded(data: &[u8]) -> bool {
    inspect(data).map(|i| i.encoded).unwrap_or(false)
}

fn elide_jpeg(header: &[u8], jpeg_off: usize, jpeg_len: usize) -> Result<Vec<u8>> {
    if jpeg_off
        .checked_add(jpeg_len)
        .map(|e| e > header.len())
        .unwrap_or(true)
    {
        return Err(Error::Format("JPEG outside TIFF header"));
    }
    let mut elided = Vec::with_capacity(header.len() - jpeg_len);
    elided.extend_from_slice(&header[..jpeg_off]);
    elided.extend_from_slice(&header[jpeg_off + jpeg_len..]);
    Ok(elided)
}

fn restore_header(elided: &[u8], jpeg: &[u8], jpeg_off: usize) -> Result<Vec<u8>> {
    if jpeg_off > elided.len() {
        return Err(Error::Format("JPEG insert offset past elided header"));
    }
    let mut header = Vec::with_capacity(elided.len() + jpeg.len());
    header.extend_from_slice(&elided[..jpeg_off]);
    header.extend_from_slice(jpeg);
    header.extend_from_slice(&elided[jpeg_off..]);
    Ok(header)
}

fn zstd_err(e: impl ToString) -> Error {
    Error::Zstd(e.to_string())
}

fn compress_shuffled(shuffled: &[u8], level: i32, progress: &mut dyn FnMut(u8)) -> Result<Vec<u8>> {
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), level).map_err(zstd_err)?;
    encoder
        .set_pledged_src_size(Some(shuffled.len() as u64))
        .map_err(zstd_err)?;
    const CHUNK: usize = 1 << 20;
    let total = shuffled.len().max(1);
    let mut done = 0usize;
    for chunk in shuffled.chunks(CHUNK) {
        encoder.write_all(chunk).map_err(zstd_err)?;
        encoder.flush().map_err(zstd_err)?;
        done += chunk.len();
        progress(15 + (done as u64 * 80 / total as u64) as u8);
    }
    encoder.finish().map_err(zstd_err)
}

/// Same as [`encode_with_level`], with `progress` called as percent 0–100 (non-decreasing).
pub fn encode_with_progress(
    arw: &[u8],
    level: i32,
    mut progress: impl FnMut(u8),
) -> Result<Vec<u8>> {
    let mut last = 255u8;
    let mut report = |pct: u8| {
        let pct = pct.min(100);
        if pct != last {
            last = pct;
            progress(pct);
        }
    };
    report(0);
    if is_encoded(arw) {
        return Err(Error::Format("already encoded"));
    }
    let orig_bytes = arw.len() as u64;
    let orig_sha1 = sha1_20(arw);
    let layout = parse_layout(arw)?;
    let raw = &layout.raw;
    if raw.compression != 1 {
        return Err(Error::Unsupported("raw strip is not uncompressed"));
    }
    if raw.bits != 14 {
        return Err(Error::Unsupported("expected 14-bit raw"));
    }
    let so = raw.strip_offset as usize;
    let sl = raw.strip_bytes as usize;
    if so + sl != arw.len() {
        return Err(Error::Unsupported(
            "raw strip is not at EOF (refusing to move trailing TIFF pointers)",
        ));
    }
    if sl != raw.width as usize * raw.height as usize * 2 {
        return Err(Error::Format("strip size != width*height*2"));
    }
    report(5);

    let (jpeg_off, jpeg_len) = largest_jpeg_in_arw(arw)?;
    let jpeg_full = jpeg_bytes(
        arw,
        &crate::tiff::JpegRef {
            offset: jpeg_off,
            length: jpeg_len,
        },
    )?;
    let eoi = jpeg_end(jpeg_full)?;
    let jpeg = &jpeg_full[..eoi];
    let padding = &jpeg_full[eoi..];
    let orientation = preview_orientation(jpeg, arw);
    let thumb = jpeg_exif_thumbnail(jpeg)
        .or_else(|| smallest_embedded_jpeg(arw, &layout.jpegs, jpeg_off, jpeg_len));
    let view = attach_view_exif(jpeg, orientation, thumb)?;

    let header = &arw[..so];
    let elided = elide_jpeg(header, jpeg_off as usize, jpeg_len as usize)?;
    report(10);

    let pixels = transform::pixels_from_le_bytes(&arw[so..so + sl]);
    let shuffled = transform::forward(&pixels, raw.width as usize);
    report(15);

    let z = compress_shuffled(&shuffled, level, &mut report)?;
    report(96);

    let meta = TrailerMeta {
        orig_compression: raw.compression,
        width: raw.width,
        height: raw.height,
        jpeg_tiff_offset: jpeg_off,
        jpeg_len,
        header_elided_len: elided.len() as u32,
        orig_strip_bytes: raw.strip_bytes,
        orig_strip_offset: raw.strip_offset,
        jpeg_padding_len: padding.len() as u32,
    };

    let mut out = Vec::with_capacity(
        view.len() + TRAILER_HEADER_LEN + elided.len() + padding.len() + z.len() + FOOTER_LEN,
    );
    out.extend_from_slice(&view);
    out.extend_from_slice(&write_trailer_meta(&meta));
    out.extend_from_slice(&elided);
    out.extend_from_slice(padding);
    out.extend_from_slice(&z);
    out.extend_from_slice(&write_orig_footer(orig_bytes, &orig_sha1));
    report(100);
    Ok(out)
}

pub fn encode_with_level(arw: &[u8], level: i32) -> Result<Vec<u8>> {
    encode_with_progress(arw, level, |_| {})
}

pub fn encode(arw: &[u8]) -> Result<Vec<u8>> {
    encode_with_level(arw, DEFAULT_LEVEL)
}

pub fn decode(file: &[u8]) -> Result<Vec<u8>> {
    let (view_jpeg, rest) = split_jpeg_prefix(file)?;
    let jpeg = strip_view_exif(view_jpeg)?;
    let meta = parse_trailer_meta(rest)?;
    if meta.jpeg_len as usize != jpeg.len() + meta.jpeg_padding_len as usize {
        return Err(Error::Format("JPEG length does not match trailer"));
    }
    let elided_end = TRAILER_HEADER_LEN + meta.header_elided_len as usize;
    let pad_end = elided_end + meta.jpeg_padding_len as usize;
    let elided = rest
        .get(TRAILER_HEADER_LEN..elided_end)
        .ok_or(Error::Truncated)?;
    let padding = rest.get(elided_end..pad_end).ok_or(Error::Truncated)?;
    let footer = parse_orig_footer(file);
    let zstd_end = if footer.is_some() {
        rest.len().checked_sub(FOOTER_LEN).ok_or(Error::Truncated)?
    } else {
        rest.len()
    };
    if zstd_end < pad_end {
        return Err(Error::Truncated);
    }
    let zstd_payload = rest.get(pad_end..zstd_end).ok_or(Error::Truncated)?;

    let mut jpeg_full = Vec::with_capacity(jpeg.len() + padding.len());
    jpeg_full.extend_from_slice(&jpeg);
    jpeg_full.extend_from_slice(padding);
    let header = restore_header(&elided, &jpeg_full, meta.jpeg_tiff_offset as usize)?;
    if header.len() != meta.orig_strip_offset as usize {
        return Err(Error::Format("reconstructed TIFF header has wrong size"));
    }

    let shuffled = zstd::bulk::decompress(zstd_payload, meta.orig_strip_bytes as usize)
        .map_err(|e| Error::Zstd(e.to_string()))?;
    if shuffled.len() != meta.orig_strip_bytes as usize {
        return Err(Error::Format("decompressed strip has wrong size"));
    }
    let pixels = transform::inverse(&shuffled, meta.width as usize);
    let raw_bytes = transform::pixels_to_le_bytes(&pixels);

    let mut out = Vec::with_capacity(header.len() + raw_bytes.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&raw_bytes);
    if let Some(info) = footer {
        if out.len() as u64 != info.orig_bytes {
            return Err(Error::Integrity("uncompressed size mismatch"));
        }
        if sha1_20(&out) != info.sha1 {
            return Err(Error::Integrity("SHA-1 mismatch"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn encoded_extension_from_arw() {
        assert_eq!(
            encoded_output_path("OCT00527.ARW"),
            Path::new("OCT00527.ARWC.JPG")
        );
        assert_eq!(
            encoded_output_path("/tmp/shot.arw"),
            Path::new("/tmp/shot.ARWC.JPG")
        );
        assert!(has_encoded_extension("shot.arwc.jpg"));
        assert!(has_encoded_extension("SHOT.ARWC.JPG"));
        assert!(!has_encoded_extension("shot.jpg"));
        assert!(!has_encoded_extension("shot.ARW"));
    }

    #[test]
    fn decoded_extension_from_arwc() {
        assert_eq!(
            decoded_output_path("OCT00527.arwc.jpg"),
            Path::new("OCT00527.ARW")
        );
        assert_eq!(
            decoded_output_path("/tmp/shot.ARWC.JPG"),
            Path::new("/tmp/shot.ARW")
        );
    }
}
