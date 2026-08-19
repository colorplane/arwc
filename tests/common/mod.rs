//! Runtime Sony-like ARW generator. No real photos.

#![allow(dead_code)]

#[derive(Clone, Copy)]
pub enum Pixels {
    Constant(u16),
    Gradient,
    Wrap,
    Lcg(u32),
}

#[derive(Clone)]
pub struct JpegSpec {
    pub app1: Vec<u8>,
    pub entropy: Vec<u8>,
    pub padding: usize,
}

impl JpegSpec {
    pub fn tiny() -> Self {
        Self {
            app1: Vec::new(),
            entropy: vec![0x11, 0x22],
            padding: 0,
        }
    }

    pub fn with_exif(tag: &[u8]) -> Self {
        let mut app1 = b"Exif\x00\x00".to_vec();
        app1.extend_from_slice(tag);
        Self {
            app1,
            entropy: vec![0x10, 0xff, 0x20], // 0xff will be stuffed
            padding: 0,
        }
    }
}

pub struct Spec {
    pub width: u16,
    pub height: u16,
    pub pixels: Pixels,
    pub preview: JpegSpec,
    pub thumb: Option<JpegSpec>,
    pub make: Option<&'static str>,
    pub orientation: u16,
}

impl Spec {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            pixels: Pixels::Gradient,
            preview: JpegSpec::with_exif(b"SONY-ILCE-TEST"),
            thumb: None,
            make: Some("SONY"),
            orientation: 1,
        }
    }
}

pub fn build_jpeg(spec: &JpegSpec) -> Vec<u8> {
    let mut j = vec![0xff, 0xd8];
    if !spec.app1.is_empty() {
        let len = spec.app1.len() + 2;
        j.extend_from_slice(&[0xff, 0xe1]);
        j.extend_from_slice(&(len as u16).to_be_bytes());
        j.extend_from_slice(&spec.app1);
    }
    j.extend_from_slice(&[0xff, 0xda, 0x00, 0x02]);
    for &b in &spec.entropy {
        j.push(b);
        if b == 0xff {
            j.push(0x00);
        }
    }
    j.extend_from_slice(&[0xff, 0xd9]);
    j.extend(std::iter::repeat(0xaa).take(spec.padding));
    j
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn entry(buf: &mut Vec<u8>, tag: u16, typ: u16, count: u32, val: u32) {
    put_u16(buf, tag);
    put_u16(buf, typ);
    put_u32(buf, count);
    put_u32(buf, val);
}

fn fill_pixels(spec: &Spec) -> Vec<u16> {
    let n = spec.width as usize * spec.height as usize;
    (0..n)
        .map(|i| {
            let x = (i % spec.width as usize) as u16;
            let y = (i / spec.width as usize) as u16;
            match spec.pixels {
                Pixels::Constant(v) => v & 0x3fff,
                Pixels::Gradient => (x.wrapping_mul(13).wrapping_add(y.wrapping_mul(7))) & 0x3fff,
                Pixels::Wrap => {
                    if i % 3 == 0 {
                        0
                    } else if i % 3 == 1 {
                        0xffff
                    } else {
                        0x3fff
                    }
                }
                Pixels::Lcg(seed) => {
                    let mut s = seed.wrapping_add(i as u32).wrapping_mul(1664525).wrapping_add(1013904223);
                    s ^= s << 13;
                    (s as u16) & 0x3fff
                }
            }
        })
        .collect()
}

/// Uncompressed 14-bit CFA ARW: IFD0 JPEG preview, optional IFD1 thumb, SubIFD strip at EOF.
pub fn build_arw(spec: Spec) -> Vec<u8> {
    let preview = build_jpeg(&spec.preview);
    let thumb = spec.thumb.as_ref().map(build_jpeg);
    let pixels = fill_pixels(&spec);
    let strip_bytes = (pixels.len() * 2) as u32;

    let make = spec.make.map(|s| {
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        b
    });

    let ifd0_n = 6u16 + u16::from(make.is_some());
    let ifd0_size = 2 + ifd0_n as usize * 12 + 4;
    let ifd1_n = 4u16;
    let ifd1_size = if thumb.is_some() {
        2 + ifd1_n as usize * 12 + 4
    } else {
        0
    };
    let sub_n = 7u16;
    let sub_size = 2 + sub_n as usize * 12 + 4;

    let mut off = 8usize;
    let ifd0_off = off;
    off += ifd0_size;
    let ifd1_off = off;
    off += ifd1_size;
    let make_off = off;
    off += make.as_ref().map(|m| m.len()).unwrap_or(0);
    let jpeg_off = off;
    off += preview.len();
    let thumb_off = off;
    off += thumb.as_ref().map(|t| t.len()).unwrap_or(0);
    let sub_off = off;
    off += sub_size;
    let strip_off = off;

    let mut buf = Vec::with_capacity(strip_off + strip_bytes as usize);
    buf.extend_from_slice(b"II");
    put_u16(&mut buf, 42);
    put_u32(&mut buf, ifd0_off as u32);

    put_u16(&mut buf, ifd0_n);
    entry(&mut buf, 254, 4, 1, 1);
    entry(&mut buf, 259, 3, 1, 6);
    if let Some(m) = &make {
        entry(&mut buf, 271, 2, m.len() as u32, make_off as u32);
    }
    entry(&mut buf, 274, 3, 1, spec.orientation as u32);
    entry(&mut buf, 330, 4, 1, sub_off as u32);
    entry(&mut buf, 513, 4, 1, jpeg_off as u32);
    entry(&mut buf, 514, 4, 1, preview.len() as u32);
    put_u32(
        &mut buf,
        if thumb.is_some() { ifd1_off as u32 } else { 0 },
    );

    if let Some(t) = &thumb {
        put_u16(&mut buf, ifd1_n);
        entry(&mut buf, 254, 4, 1, 1);
        entry(&mut buf, 259, 3, 1, 6);
        entry(&mut buf, 513, 4, 1, thumb_off as u32);
        entry(&mut buf, 514, 4, 1, t.len() as u32);
        put_u32(&mut buf, 0);
    }

    if let Some(m) = &make {
        buf.extend_from_slice(m);
    }
    assert_eq!(buf.len(), jpeg_off);
    buf.extend_from_slice(&preview);
    if let Some(t) = &thumb {
        assert_eq!(buf.len(), thumb_off);
        buf.extend_from_slice(t);
    }
    assert_eq!(buf.len(), sub_off);
    put_u16(&mut buf, sub_n);
    entry(&mut buf, 254, 4, 1, 0);
    entry(&mut buf, 256, 3, 1, spec.width as u32);
    entry(&mut buf, 257, 3, 1, spec.height as u32);
    entry(&mut buf, 258, 3, 1, 14);
    entry(&mut buf, 259, 3, 1, 1);
    entry(&mut buf, 273, 4, 1, strip_off as u32);
    entry(&mut buf, 279, 4, 1, strip_bytes);
    put_u32(&mut buf, 0);

    assert_eq!(buf.len(), strip_off);
    for p in pixels {
        put_u16(&mut buf, p);
    }
    buf
}

pub fn default_arw() -> Vec<u8> {
    build_arw(Spec::new(16, 8))
}
