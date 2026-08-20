use crate::error::{Error, Result};

/// Return the byte offset *after* the first top-level JPEG EOI (`FF D9`).
/// Viewers ignore everything from this point on.
pub fn jpeg_end(data: &[u8]) -> Result<usize> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return Err(Error::Format("not a JPEG"));
    }
    let mut i = 2usize;
    loop {
        if i >= data.len() {
            return Err(Error::Truncated);
        }
        if data[i] != 0xff {
            return Err(Error::Format("invalid JPEG marker"));
        }
        while i < data.len() && data[i] == 0xff {
            i += 1;
        }
        if i >= data.len() {
            return Err(Error::Truncated);
        }
        let marker = data[i];
        i += 1;
        match marker {
            0xd9 => return Ok(i),
            0xd8 | 0x01 | 0xd0..=0xd7 => {}
            0xda => {
                if i + 2 > data.len() {
                    return Err(Error::Truncated);
                }
                let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                if len < 2 {
                    return Err(Error::Format("invalid SOS length"));
                }
                i = i
                    .checked_add(len)
                    .ok_or(Error::Format("JPEG SOS overflow"))?;
                loop {
                    let Some(b) = data.get(i) else {
                        return Err(Error::Truncated);
                    };
                    if *b != 0xff {
                        i += 1;
                        continue;
                    }
                    let Some(&n) = data.get(i + 1) else {
                        return Err(Error::Truncated);
                    };
                    match n {
                        0x00 | 0xd0..=0xd7 => i += 2,
                        0xff => i += 1,
                        0xd9 => return Ok(i + 2),
                        _ => i += 2,
                    }
                }
            }
            _ => {
                if i + 2 > data.len() {
                    return Err(Error::Truncated);
                }
                let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                if len < 2 {
                    return Err(Error::Format("invalid JPEG segment length"));
                }
                i = i
                    .checked_add(len)
                    .ok_or(Error::Format("JPEG segment overflow"))?;
            }
        }
    }
}

pub fn split_jpeg_prefix(data: &[u8]) -> Result<(&[u8], &[u8])> {
    let end = jpeg_end(data)?;
    Ok((&data[..end], &data[end..]))
}

/// Software tag written into the stripable view APP1 so decode can find it.
pub const VIEW_SOFTWARE: &[u8; 4] = b"ARWC";

fn mm_short_value(v: u16) -> [u8; 4] {
    (u32::from(v) << 16).to_be_bytes()
}

fn tiff_ifd0_software_is_arwc(tiff: &[u8]) -> bool {
    if tiff.len() < 8 || &tiff[0..2] != b"MM" {
        return false;
    }
    let Some(magic) = tiff.get(2..4) else {
        return false;
    };
    if magic != [0x00, 0x2a] {
        return false;
    }
    let Some(off_bytes) = tiff.get(4..8) else {
        return false;
    };
    let off = u32::from_be_bytes(off_bytes.try_into().unwrap()) as usize;
    let Some(count_bytes) = tiff.get(off..off + 2) else {
        return false;
    };
    let n = u16::from_be_bytes(count_bytes.try_into().unwrap()) as usize;
    for index in 0..n {
        let e = off + 2 + index * 12;
        let Some(entry) = tiff.get(e..e + 12) else {
            return false;
        };
        let tag = u16::from_be_bytes([entry[0], entry[1]]);
        if tag != 0x0131 {
            continue;
        }
        let typ = u16::from_be_bytes([entry[2], entry[3]]);
        let count = u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]);
        return typ == 2 && count == 4 && &entry[8..12] == VIEW_SOFTWARE;
    }
    false
}

fn is_arwc_view_app1(payload: &[u8]) -> bool {
    payload.starts_with(b"Exif\x00\x00")
        && payload.len() > 6
        && tiff_ifd0_software_is_arwc(&payload[6..])
}

fn jpeg_ifd1_thumbnail<'a>(
    tiff: &'a [u8],
    file: &'a [u8],
    tiff_in_file_offset: usize,
) -> Option<&'a [u8]> {
    if tiff.len() < 8 {
        return None;
    }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |off: usize| -> Option<u16> {
        let s = tiff.get(off..off + 2)?;
        Some(if le {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    };
    let read_u32 = |off: usize| -> Option<u32> {
        let s = tiff.get(off..off + 4)?;
        Some(if le {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };
    let first = read_u32(4)? as usize;
    let n0 = read_u16(first)? as usize;
    let next = read_u32(first + 2 + n0 * 12)?;
    if next == 0 {
        return None;
    }
    let ifd1 = next as usize;
    let n1 = read_u16(ifd1)? as usize;
    let mut jpeg_off = None;
    let mut jpeg_len = None;
    let mut compression = None;
    for index in 0..n1 {
        let e = ifd1 + 2 + index * 12;
        let tag = read_u16(e)?;
        let typ = read_u16(e + 2)?;
        let count = read_u32(e + 4)?;
        if count != 1 || (typ != 3 && typ != 4) {
            continue;
        }
        let val = if typ == 3 {
            if le {
                u32::from(read_u16(e + 8)?)
            } else {
                u32::from(read_u16(e + 8)?)
            }
        } else {
            read_u32(e + 8)?
        };
        match tag {
            0x0103 => compression = Some(val),
            0x0201 => jpeg_off = Some(val as usize),
            0x0202 => jpeg_len = Some(val as usize),
            _ => {}
        }
    }
    if compression.is_some() && compression != Some(6) {
        return None;
    }
    let off = jpeg_off?;
    let len = jpeg_len?;
    if len < 2 || len > 60_000 {
        return None;
    }
    let start = tiff_in_file_offset.checked_add(off)?;
    let end = start.checked_add(len)?;
    let thumb = file.get(start..end)?;
    if thumb[0] == 0xff && thumb[1] == 0xd8 {
        Some(thumb)
    } else {
        None
    }
}

/// IFD1 JPEG thumbnail from any APP1 in `jpeg`, if present.
pub fn jpeg_exif_thumbnail(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2usize;
    loop {
        if i >= data.len() || data[i] != 0xff {
            return None;
        }
        while i < data.len() && data[i] == 0xff {
            i += 1;
        }
        if i >= data.len() {
            return None;
        }
        let marker = data[i];
        i += 1;
        match marker {
            0xd8 | 0x01 | 0xd0..=0xd7 => {}
            0xd9 | 0xda => return None,
            _ => {
                if i + 2 > data.len() {
                    return None;
                }
                let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                if len < 2 || i + len > data.len() {
                    return None;
                }
                let payload = &data[i + 2..i + len];
                let segment_start = i + 2;
                i += len;
                if marker == 0xe1 && payload.starts_with(b"Exif\x00\x00") && payload.len() > 6 {
                    let tiff = &payload[6..];
                    let tiff_in_file = segment_start + 6;
                    if let Some(thumb) = jpeg_ifd1_thumbnail(tiff, data, tiff_in_file) {
                        return Some(thumb);
                    }
                }
            }
        }
    }
}

fn build_view_app1(orientation: u16, thumb: Option<&[u8]>) -> Result<Vec<u8>> {
    let ori = orientation.clamp(1, 8);
    let thumb = thumb.filter(|t| t.len() >= 2 && t[0] == 0xff && t[1] == 0xd8 && t.len() <= 60_000);

    let mut tiff = vec![0x4d, 0x4d, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x08];
    match thumb {
        Some(thumb) => {
            let ifd1_off = 8 + 2 + 2 * 12 + 4;
            let thumb_off = ifd1_off + 2 + 3 * 12 + 4;
            tiff.extend_from_slice(&2u16.to_be_bytes());
            tiff.extend_from_slice(&0x0112u16.to_be_bytes());
            tiff.extend_from_slice(&3u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&mm_short_value(ori));
            tiff.extend_from_slice(&0x0131u16.to_be_bytes());
            tiff.extend_from_slice(&2u16.to_be_bytes());
            tiff.extend_from_slice(&4u32.to_be_bytes());
            tiff.extend_from_slice(VIEW_SOFTWARE);
            tiff.extend_from_slice(&(ifd1_off as u32).to_be_bytes());
            tiff.extend_from_slice(&3u16.to_be_bytes());
            tiff.extend_from_slice(&0x0103u16.to_be_bytes());
            tiff.extend_from_slice(&3u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&mm_short_value(6));
            tiff.extend_from_slice(&0x0201u16.to_be_bytes());
            tiff.extend_from_slice(&4u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&(thumb_off as u32).to_be_bytes());
            tiff.extend_from_slice(&0x0202u16.to_be_bytes());
            tiff.extend_from_slice(&4u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&(thumb.len() as u32).to_be_bytes());
            tiff.extend_from_slice(&0u32.to_be_bytes());
            tiff.extend_from_slice(thumb);
        }
        None => {
            tiff.extend_from_slice(&2u16.to_be_bytes());
            tiff.extend_from_slice(&0x0112u16.to_be_bytes());
            tiff.extend_from_slice(&3u16.to_be_bytes());
            tiff.extend_from_slice(&1u32.to_be_bytes());
            tiff.extend_from_slice(&mm_short_value(ori));
            tiff.extend_from_slice(&0x0131u16.to_be_bytes());
            tiff.extend_from_slice(&2u16.to_be_bytes());
            tiff.extend_from_slice(&4u32.to_be_bytes());
            tiff.extend_from_slice(VIEW_SOFTWARE);
            tiff.extend_from_slice(&0u32.to_be_bytes());
        }
    }

    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);
    let len = payload.len() + 2;
    if len > 65535 {
        return build_view_app1(ori, None);
    }
    let mut app1 = vec![0xff, 0xe1];
    app1.extend_from_slice(&(len as u16).to_be_bytes());
    app1.extend_from_slice(&payload);
    Ok(app1)
}

/// Prepend a stripable EXIF APP1 so browsers and Gallery see Orientation
/// (and a copied IFD1 thumbnail when one exists). Decode removes it.
pub fn attach_view_exif(jpeg: &[u8], orientation: u16, thumb: Option<&[u8]>) -> Result<Vec<u8>> {
    if jpeg.len() < 2 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Err(Error::Format("not a JPEG"));
    }
    if strip_view_exif(jpeg)?.len() != jpeg.len() {
        return Ok(jpeg.to_vec());
    }
    let app1 = match build_view_app1(orientation, thumb) {
        Ok(app1) => app1,
        Err(_) if thumb.is_some() => build_view_app1(orientation, None)?,
        Err(error) => return Err(error),
    };
    let mut out = Vec::with_capacity(2 + app1.len() + jpeg.len() - 2);
    out.extend_from_slice(&[0xff, 0xd8]);
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    Ok(out)
}

/// Remove the ARWC view APP1, restoring the camera JPEG that belongs in the ARW.
pub fn strip_view_exif(jpeg: &[u8]) -> Result<Vec<u8>> {
    if jpeg.len() < 8 || jpeg[0] != 0xff || jpeg[1] != 0xd8 {
        return Ok(jpeg.to_vec());
    }
    let mut i = 2usize;
    while i < jpeg.len() && jpeg[i] == 0xff {
        i += 1;
    }
    if i >= jpeg.len() || jpeg[i] != 0xe1 {
        return Ok(jpeg.to_vec());
    }
    i += 1;
    if i + 2 > jpeg.len() {
        return Ok(jpeg.to_vec());
    }
    let len = u16::from_be_bytes([jpeg[i], jpeg[i + 1]]) as usize;
    if len < 2 || i + len > jpeg.len() {
        return Ok(jpeg.to_vec());
    }
    let payload = &jpeg[i + 2..i + len];
    if !is_arwc_view_app1(payload) {
        return Ok(jpeg.to_vec());
    }
    let mut out = Vec::with_capacity(jpeg.len() - (len + 2));
    out.extend_from_slice(&[0xff, 0xd8]);
    out.extend_from_slice(&jpeg[i + len..]);
    Ok(out)
}

/// EXIF Orientation from JPEG APP1, if present.
pub fn jpeg_exif_orientation(data: &[u8]) -> Option<u16> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2usize;
    loop {
        if i >= data.len() || data[i] != 0xff {
            return None;
        }
        while i < data.len() && data[i] == 0xff {
            i += 1;
        }
        if i >= data.len() {
            return None;
        }
        let marker = data[i];
        i += 1;
        match marker {
            0xd8 | 0x01 | 0xd0..=0xd7 => {}
            0xd9 | 0xda => return None,
            _ => {
                if i + 2 > data.len() {
                    return None;
                }
                let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
                if len < 2 || i + len > data.len() {
                    return None;
                }
                let payload = &data[i + 2..i + len];
                i += len;
                if marker == 0xe1 && payload.starts_with(b"Exif\x00\x00") && payload.len() > 6 {
                    if let Some(ori) = crate::tiff::ifd0_orientation(&payload[6..]) {
                        return Some(ori);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_eoi() {
        let j = [0xff, 0xd8, 0xff, 0xd9];
        assert_eq!(jpeg_end(&j).unwrap(), 4);
        let extra = [0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9, b'A', b'B'];
        assert_eq!(jpeg_end(&extra).unwrap(), 8);
        assert_eq!(jpeg_end(&[0xff, 0xd8, 0xff, 0xd9]).unwrap(), 4);
    }

    #[test]
    fn app1_then_sos() {
        let mut j = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x06, b'E', b'x', b'i', b'f'];
        j.extend_from_slice(&[0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]);
        assert_eq!(jpeg_end(&j).unwrap(), j.len());
    }

    #[test]
    fn stuffed_ff_in_entropy_is_not_eoi() {
        let j = [
            0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0x11, 0xff, 0x00, 0x22, 0xff, 0xd9,
        ];
        assert_eq!(jpeg_end(&j).unwrap(), j.len());
    }

    #[test]
    fn restart_markers_in_scan() {
        let j = [
            0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd0, 0xaa, 0xff, 0xd9,
        ];
        assert_eq!(jpeg_end(&j).unwrap(), j.len());
    }

    #[test]
    fn extra_after_eoi_ignored() {
        let j = [0xff, 0xd8, 0xff, 0xd9, b'A', b'R', b'W', b'Z'];
        assert_eq!(jpeg_end(&j).unwrap(), 4);
        let (pre, rest) = split_jpeg_prefix(&j).unwrap();
        assert_eq!(pre, &j[..4]);
        assert_eq!(rest, b"ARWZ");
    }

    #[test]
    fn truncated_and_not_jpeg() {
        assert!(jpeg_end(&[]).is_err());
        assert!(jpeg_end(&[0xff]).is_err());
        assert!(jpeg_end(&[0x00, 0xd8, 0xff, 0xd9]).is_err());
        assert!(jpeg_end(&[0xff, 0xd8, 0xff, 0xda, 0x00, 0x02]).is_err());
    }

    #[test]
    fn reads_app1_exif_orientation() {
        let mut tiff = b"II*\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&274u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&6u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let mut app1 = b"Exif\x00\x00".to_vec();
        app1.extend_from_slice(&tiff);
        let mut j = vec![0xff, 0xd8, 0xff, 0xe1];
        j.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        j.extend_from_slice(&app1);
        j.extend_from_slice(&[0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]);
        assert_eq!(jpeg_exif_orientation(&j), Some(6));
        assert_eq!(jpeg_exif_orientation(&[0xff, 0xd8, 0xff, 0xd9]), None);
    }

    #[test]
    fn view_exif_is_stripped_back_to_the_camera_jpeg() {
        let camera = [0xff, 0xd8, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9];
        let thumb = [0xff, 0xd8, 0xff, 0xd9];
        let view = attach_view_exif(&camera, 6, Some(&thumb)).unwrap();
        assert_eq!(jpeg_exif_orientation(&view), Some(6));
        assert_eq!(jpeg_exif_thumbnail(&view), Some(thumb.as_slice()));
        assert_eq!(strip_view_exif(&view).unwrap(), camera);
        assert_eq!(strip_view_exif(&camera).unwrap(), camera);
    }
}
