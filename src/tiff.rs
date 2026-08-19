use crate::error::{Error, Result};

pub const TAG_NEWSUBFILETYPE: u16 = 254;
pub const TAG_IMAGE_WIDTH: u16 = 256;
pub const TAG_IMAGE_LENGTH: u16 = 257;
pub const TAG_BITS_PER_SAMPLE: u16 = 258;
pub const TAG_COMPRESSION: u16 = 259;
pub const TAG_STRIP_OFFSETS: u16 = 273;
pub const TAG_ORIENTATION: u16 = 274;
pub const TAG_STRIP_BYTE_COUNTS: u16 = 279;
pub const TAG_SUB_IFDS: u16 = 330;
pub const TAG_JPEG_OFFSET: u16 = 513;
pub const TAG_JPEG_LENGTH: u16 = 514;

pub const TYPE_SHORT: u16 = 3;
pub const TYPE_LONG: u16 = 4;

#[derive(Clone, Debug)]
pub struct JpegRef {
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Debug)]
pub struct RawIfd {
    pub width: u16,
    pub height: u16,
    pub bits: u16,
    pub compression: u16,
    pub strip_offset: u32,
    pub strip_bytes: u32,
    pub compression_value_at: usize,
    pub strip_bytes_value_at: usize,
}

#[derive(Clone, Debug)]
pub struct ArwLayout {
    pub raw: RawIfd,
    pub jpegs: Vec<JpegRef>,
}

fn u16_at(data: &[u8], off: usize) -> Result<u16> {
    let s = data.get(off..off + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(data: &[u8], off: usize) -> Result<u32> {
    let s = data.get(off..off + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn endian_u16(data: &[u8], off: usize, le: bool) -> Option<u16> {
    let s = data.get(off..off + 2)?;
    Some(if le {
        u16::from_le_bytes([s[0], s[1]])
    } else {
        u16::from_be_bytes([s[0], s[1]])
    })
}

fn endian_u32(data: &[u8], off: usize, le: bool) -> Option<u32> {
    let s = data.get(off..off + 4)?;
    Some(if le {
        u32::from_le_bytes([s[0], s[1], s[2], s[3]])
    } else {
        u32::from_be_bytes([s[0], s[1], s[2], s[3]])
    })
}

/// EXIF/TIFF Orientation (tag 274) from IFD0. `1`–`8`, or `None` if absent.
pub fn ifd0_orientation(data: &[u8]) -> Option<u16> {
    if data.len() < 8 {
        return None;
    }
    let le = match &data[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if endian_u16(data, 2, le)? != 42 {
        return None;
    }
    let off = endian_u32(data, 4, le)? as usize;
    if off == 0 {
        return None;
    }
    let n = endian_u16(data, off, le)? as usize;
    for i in 0..n {
        let e = off + 2 + i * 12;
        if endian_u16(data, e, le)? != TAG_ORIENTATION {
            continue;
        }
        let typ = endian_u16(data, e + 2, le)?;
        let count = endian_u32(data, e + 4, le)?;
        let val = endian_u32(data, e + 8, le)?;
        let ori = if typ == TYPE_SHORT && count == 1 {
            if le {
                (val & 0xffff) as u16
            } else {
                (val >> 16) as u16
            }
        } else {
            val as u16
        };
        if (1..=8).contains(&ori) {
            return Some(ori);
        }
        return None;
    }
    None
}

struct IfdWalk {
    jpegs: Vec<JpegRef>,
    subifds: Vec<u32>,
    next: u32,
    raw: Option<RawIfd>,
}

fn walk_ifd(data: &[u8], off: u32) -> Result<IfdWalk> {
    let o = off as usize;
    let n = u16_at(data, o)? as usize;
    let mut jpeg_off: Option<u32> = None;
    let mut jpeg_len: Option<u32> = None;
    let mut subifds = Vec::new();
    let mut width = None;
    let mut height = None;
    let mut bits = None;
    let mut compression = None;
    let mut compression_at = None;
    let mut strip_offset = None;
    let mut strip_bytes = None;
    let mut strip_bytes_at = None;
    let mut newsub = None;

    for i in 0..n {
        let e = o + 2 + i * 12;
        let tag = u16_at(data, e)?;
        let typ = u16_at(data, e + 2)?;
        let count = u32_at(data, e + 4)?;
        let val_at = e + 8;
        let val = u32_at(data, val_at)?;

        let inline_short = || -> u16 {
            if typ == TYPE_SHORT && count == 1 {
                (val & 0xffff) as u16
            } else {
                (val & 0xffff) as u16
            }
        };

        match tag {
            TAG_NEWSUBFILETYPE if typ == TYPE_LONG && count == 1 => newsub = Some(val),
            TAG_IMAGE_WIDTH => width = Some(inline_short()),
            TAG_IMAGE_LENGTH => height = Some(inline_short()),
            TAG_BITS_PER_SAMPLE if count == 1 => bits = Some(inline_short()),
            TAG_COMPRESSION if count == 1 => {
                compression = Some(inline_short());
                compression_at = Some(val_at);
            }
            TAG_STRIP_OFFSETS if count == 1 => strip_offset = Some(val),
            TAG_STRIP_BYTE_COUNTS if count == 1 => {
                strip_bytes = Some(val);
                strip_bytes_at = Some(val_at);
            }
            TAG_SUB_IFDS => {
                let type_size = if typ == TYPE_LONG { 4 } else { 2 };
                let nbytes = count as usize * type_size;
                if nbytes <= 4 {
                    subifds.push(val);
                } else {
                    let base = val as usize;
                    for k in 0..count as usize {
                        subifds.push(u32_at(data, base + k * 4)?);
                    }
                }
            }
            TAG_JPEG_OFFSET if count == 1 => jpeg_off = Some(val),
            TAG_JPEG_LENGTH if count == 1 => jpeg_len = Some(val),
            _ => {}
        }
    }

    let mut jpegs = Vec::new();
    if let (Some(offset), Some(length)) = (jpeg_off, jpeg_len) {
        if length > 0 {
            jpegs.push(JpegRef { offset, length });
        }
    }

    let next = u32_at(data, o + 2 + n * 12)?;

    let raw = match (
        newsub,
        width,
        height,
        bits,
        compression,
        strip_offset,
        strip_bytes,
        compression_at,
        strip_bytes_at,
    ) {
        (
            Some(0),
            Some(w),
            Some(h),
            Some(b),
            Some(c),
            Some(so),
            Some(sb),
            Some(ca),
            Some(sba),
        ) if w > 0 && h > 0 && so > 0 && sb > 0 => Some(RawIfd {
            width: w,
            height: h,
            bits: b,
            compression: c,
            strip_offset: so,
            strip_bytes: sb,
            compression_value_at: ca,
            strip_bytes_value_at: sba,
        }),
        _ => None,
    };

    Ok(IfdWalk {
        jpegs,
        subifds,
        next,
        raw,
    })
}

/// Parse enough of a TIFF/ARW prefix to locate the raw strip and embedded JPEGs.
/// `data` may be a prefix of the file: IFDs live near the start. JPEG *payloads*
/// sit before the raw strip, so a prefix of `raw.strip_offset` bytes is enough
/// to copy every preview without the compressed raw.
pub fn parse_layout(data: &[u8]) -> Result<ArwLayout> {
    if data.len() < 8 {
        return Err(Error::Truncated);
    }
    if &data[0..2] != b"II" {
        return Err(Error::Unsupported("big-endian TIFF"));
    }
    if u16_at(data, 2)? != 42 {
        return Err(Error::Format("not TIFF"));
    }

    let mut jpegs = Vec::new();
    let mut raw = None;
    let mut queue = vec![u32_at(data, 4)?];
    let mut seen = 0usize;

    while let Some(off) = queue.pop() {
        if off == 0 {
            continue;
        }
        seen += 1;
        if seen > 32 {
            return Err(Error::Format("too many IFDs"));
        }
        // Need the IFD entry table in this prefix. If it's missing, the caller
        // has not fetched enough header bytes yet.
        if off as usize + 2 > data.len() {
            return Err(Error::Truncated);
        }
        let w = walk_ifd(data, off)?;
        jpegs.extend(w.jpegs);
        if raw.is_none() {
            raw = w.raw;
        }
        queue.extend(w.subifds);
        if w.next != 0 {
            queue.push(w.next);
        }
    }

    let raw = raw.ok_or(Error::Format("no uncompressed raw SubIFD"))?;
    Ok(ArwLayout { raw, jpegs })
}

pub fn jpeg_bytes<'a>(file: &'a [u8], jpeg: &JpegRef) -> Result<&'a [u8]> {
    let start = jpeg.offset as usize;
    let end = start
        .checked_add(jpeg.length as usize)
        .ok_or(Error::Format("jpeg overflow"))?;
    file.get(start..end).ok_or(Error::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_and_big_endian() {
        assert!(matches!(parse_layout(&[]), Err(Error::Truncated)));
        assert!(matches!(parse_layout(b"MMMMMMMM"), Err(Error::Unsupported(_))));
    }

    #[test]
    fn rejects_non_tiff_magic() {
        let mut d = b"II\x00\x00\x08\x00\x00\x00".to_vec();
        d.extend_from_slice(&[0; 32]);
        assert!(matches!(parse_layout(&d), Err(Error::Format(_))));
    }

    #[test]
    fn reads_ifd0_orientation_le_and_be() {
        let mut le = b"II*\x00\x08\x00\x00\x00".to_vec();
        le.extend_from_slice(&1u16.to_le_bytes());
        le.extend_from_slice(&274u16.to_le_bytes());
        le.extend_from_slice(&3u16.to_le_bytes());
        le.extend_from_slice(&1u32.to_le_bytes());
        le.extend_from_slice(&8u32.to_le_bytes());
        le.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(ifd0_orientation(&le), Some(8));

        let mut be = b"MM\x00*\x00\x00\x00\x08".to_vec();
        be.extend_from_slice(&1u16.to_be_bytes());
        be.extend_from_slice(&274u16.to_be_bytes());
        be.extend_from_slice(&3u16.to_be_bytes());
        be.extend_from_slice(&1u32.to_be_bytes());
        be.extend_from_slice(&0x0008_0000u32.to_be_bytes());
        be.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(ifd0_orientation(&be), Some(8));
        assert_eq!(ifd0_orientation(&[]), None);
    }
}
