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
                if marker == 0xe1
                    && payload.starts_with(b"Exif\x00\x00")
                    && payload.len() > 6
                {
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
}
