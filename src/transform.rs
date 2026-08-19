/// Bayer same-channel left delta, then byte-shuffle (low plane, high plane).
/// Inverse is prefix-sum per channel after unshuffle.

pub fn forward(pixels: &[u16], width: usize) -> Vec<u8> {
    let n = pixels.len();
    debug_assert!(width > 0 && n % width == 0);
    let mut tmp = vec![0u16; n];
    for (src_row, dst_row) in pixels.chunks_exact(width).zip(tmp.chunks_exact_mut(width)) {
        let mut p0 = 0u16;
        let mut p1 = 0u16;
        for (x, (&v, out)) in src_row.iter().zip(dst_row.iter_mut()).enumerate() {
            if x & 1 == 1 {
                *out = v.wrapping_sub(p1);
                p1 = v;
            } else {
                *out = v.wrapping_sub(p0);
                p0 = v;
            }
        }
    }
    let mut out = vec![0u8; n * 2];
    for (i, p) in tmp.iter().enumerate() {
        out[i] = *p as u8;
        out[n + i] = (*p >> 8) as u8;
    }
    out
}

pub fn inverse(shuffled: &[u8], width: usize) -> Vec<u16> {
    assert!(shuffled.len() % 2 == 0);
    let n = shuffled.len() / 2;
    debug_assert!(width > 0 && n % width == 0);
    let (lo, hi) = shuffled.split_at(n);
    let mut px = vec![0u16; n];
    for i in 0..n {
        px[i] = lo[i] as u16 | ((hi[i] as u16) << 8);
    }
    for row in px.chunks_exact_mut(width) {
        let mut p0 = 0u16;
        let mut p1 = 0u16;
        for (x, pix) in row.iter_mut().enumerate() {
            if x & 1 == 1 {
                p1 = p1.wrapping_add(*pix);
                *pix = p1;
            } else {
                p0 = p0.wrapping_add(*pix);
                *pix = p0;
            }
        }
    }
    px
}

pub fn pixels_from_le_bytes(strip: &[u8]) -> Vec<u16> {
    let n = strip.len() / 2;
    let mut px = vec![0u16; n];
    unsafe {
        std::ptr::copy_nonoverlapping(strip.as_ptr(), px.as_mut_ptr() as *mut u8, n * 2);
    }
    if cfg!(target_endian = "big") {
        for p in &mut px {
            *p = p.swap_bytes();
        }
    }
    px
}

pub fn pixels_to_le_bytes(px: &[u16]) -> Vec<u8> {
    let mut out = vec![0u8; px.len() * 2];
    if cfg!(target_endian = "little") {
        unsafe {
            std::ptr::copy_nonoverlapping(px.as_ptr() as *const u8, out.as_mut_ptr(), out.len());
        }
    } else {
        for (i, p) in px.iter().enumerate() {
            let b = p.to_le_bytes();
            out[i * 2] = b[0];
            out[i * 2 + 1] = b[1];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small() {
        let width = 8;
        let src: Vec<u16> = (0..32).map(|i| ((i * 17 + 3) & 0x3fff) as u16).collect();
        let enc = forward(&src, width);
        let dec = inverse(&enc, width);
        assert_eq!(src, dec);
    }

    fn roundtrip(width: usize, src: Vec<u16>) {
        assert_eq!(src.len() % width, 0);
        assert_eq!(inverse(&forward(&src, width), width), src);
    }

    #[test]
    fn constant_and_zero() {
        roundtrip(8, vec![0; 32]);
        roundtrip(8, vec![0x3fff; 32]);
        roundtrip(2, vec![7, 9]);
    }

    #[test]
    fn wrapping_deltas() {
        roundtrip(4, vec![0, 1, 0xffff, 0, 2, 0xfffe, 3, 4]);
    }

    #[test]
    fn odd_height_even_width() {
        roundtrip(6, (0..18).map(|i| (i * 999) as u16).collect());
    }

    #[test]
    fn high_bytes_split() {
        let src: Vec<u16> = (0..16).map(|i| 0x3e00 + i).collect();
        let enc = forward(&src, 8);
        let n = src.len();
        assert!(enc[n..].iter().all(|&b| b <= 0x3f), "14-bit high plane");
        assert_eq!(inverse(&enc, 8), src);
    }

    #[test]
    fn le_pixel_bytes() {
        let px = vec![0x1234u16, 0x3fff];
        let b = pixels_to_le_bytes(&px);
        assert_eq!(b, [0x34, 0x12, 0xff, 0x3f]);
        assert_eq!(pixels_from_le_bytes(&b), px);
    }
}
