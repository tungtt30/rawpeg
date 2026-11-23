//! Shared test helpers: builds a minimal uncompressed 4x4 RGGB DNG on disk.
//!
//! The file is a valid little-endian TIFF containing the tags the rawloader
//! DNG decoder needs: Make/Model in IFD0, a SubIFDs pointer to the raw IFD,
//! and the raw IFD with uncompressed 16-bit CFA data.

use std::io::Write;
use std::path::Path;

/// Size of the synthetic raw frame. Must be >= 10 on each side (imagepipe's
/// gofloat op computes `width - 10`), and an even multiple of the 2x2 RGGB
/// pattern.
const WIDTH: usize = 12;
const HEIGHT: usize = 12;

/// RGGB pattern with distinct per-channel values:
/// R=40000, G=20000, B=10000 (white level 65535).
fn raw_pixels() -> Vec<u16> {
    let mut pixels = Vec::with_capacity(WIDTH * HEIGHT);
    for row in 0..HEIGHT {
        for col in 0..WIDTH {
            let value = match (row % 2, col % 2) {
                (0, 0) => 40000, // R
                (1, 1) => 10000, // B
                _ => 20000,      // G
            };
            pixels.push(value);
        }
    }
    pixels
}

struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    value: u32,
    blob: Option<Vec<u8>>,
}

fn inline(tag: u16, typ: u16, count: u32, value: u32) -> Entry {
    Entry { tag, typ, count, value, blob: None }
}

/// `count` is the number of TIFF values (not bytes); data is the raw value bytes.
fn blob(tag: u16, typ: u16, count: u32, data: Vec<u8>) -> Entry {
    Entry { tag, typ, count, value: 0, blob: Some(data) }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Serializes an IFD (entries + blobs) into `buf`, returns the IFD base offset.
fn write_ifd(buf: &mut Vec<u8>, mut entries: Vec<Entry>) -> usize {
    let base = buf.len();
    let mut cursor = base + 2 + 12 * entries.len() + 4;
    for e in entries.iter_mut() {
        if let Some(b) = &e.blob {
            e.value = (base + cursor) as u32;
            cursor = align4(cursor + b.len());
        }
    }
    buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in &entries {
        buf.extend_from_slice(&e.tag.to_le_bytes());
        buf.extend_from_slice(&e.typ.to_le_bytes());
        buf.extend_from_slice(&e.count.to_le_bytes());
        buf.extend_from_slice(&e.value.to_le_bytes());
    }
    buf.extend_from_slice(&0u32.to_le_bytes()); // no chained IFD
    for e in &entries {
        if let Some(b) = &e.blob {
            while buf.len() < e.value as usize {
                buf.push(0);
            }
            buf.extend_from_slice(b);
        }
    }
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
    base
}

pub fn write_fixture_dng(path: &Path) -> std::io::Result<()> {
    // AsShotNeutral = three RATIONALs of (1, 1), each 8 bytes little-endian.
    let mut neutral = Vec::new();
    for _ in 0..3 {
        neutral.extend_from_slice(&1u32.to_le_bytes());
        neutral.extend_from_slice(&1u32.to_le_bytes());
    }

    // ColorMatrix1 = identity 3x3, SRATIONALs (num i32, den i32).
    // An identity camera -> XYZ mapping keeps the synthetic channels pure so
    // demosaic + color pipeline round trips stay channel-dominant.
    let mut color_matrix = Vec::new();
    for i in 0..9u8 {
        let (num, den): (i32, i32) = if i.is_multiple_of(4) { (1, 1) } else { (0, 1) };
        color_matrix.extend_from_slice(&num.to_le_bytes());
        color_matrix.extend_from_slice(&den.to_le_bytes());
    }

    // IFD0 (main) entries. SubIFDs is patched once the raw IFD offset is known.
    let main_entries = vec![
        blob(0x010f, 2, 5, b"TEST\0".to_vec()),       // Make
        blob(0x0110, 2, 11, b"FixtureCam\0".to_vec()), // Model
        inline(0xc612, 1, 4, 0x0000_0401),            // DNGVersion [1,4,0,0]
        blob(0xc621, 10, 9, color_matrix),            // ColorMatrix1 = identity 3x3
        blob(0xc628, 5, 3, neutral),                  // AsShotNeutral (1,1,1)
        inline(0x014a, 13, 1, 0),                  // SubIFDs -> raw IFD (patched)
    ];

    let strip_bytes = (WIDTH * HEIGHT * 2) as u32;

    // Raw IFD entries (strip offsets patched after layout is known).
    let raw_entries = vec![
        inline(0x0100, 4, 1, WIDTH as u32), // ImageWidth
        inline(0x0101, 4, 1, HEIGHT as u32), // ImageLength
        inline(0x0102, 3, 1, 16),            // BitsPerSample
        inline(0x0103, 3, 1, 1),             // Compression = none
        inline(0x0106, 3, 1, 32803),         // PhotometricInterpretation = CFA
        inline(0x0115, 3, 1, 1),             // SamplesPerPixel
        inline(0x0111, 4, 1, 0),             // StripOffsets (patched)
        inline(0x0117, 4, 1, strip_bytes),   // StripByteCounts
        inline(0xc61d, 4, 1, 65535),         // WhiteLevel
        inline(0x828e, 1, 4, 0x0201_0100),   // CFAPattern RGGB: [0,1,1,2]
    ];

    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8

    let ifd0 = write_ifd(&mut buf, main_entries);
    let ifd1 = write_ifd(&mut buf, raw_entries);
    let raw_off = buf.len();

    let mut strip: Vec<u8> = Vec::with_capacity(strip_bytes as usize);
    for v in raw_pixels() {
        strip.extend_from_slice(&v.to_le_bytes());
    }
    buf.extend_from_slice(&strip);

    // Patch SubIFDs (IFD0 entry 5) and StripOffsets (raw IFD entry 6).
    let subifd_value = (ifd1 as u32).to_le_bytes();
    buf[ifd0 + 2 + 12 * 5 + 8..ifd0 + 2 + 12 * 5 + 12].copy_from_slice(&subifd_value);
    let strip_value = (raw_off as u32).to_le_bytes();
    buf[ifd1 + 2 + 12 * 6 + 8..ifd1 + 2 + 12 * 6 + 12].copy_from_slice(&strip_value);

    let mut file = std::fs::File::create(path)?;
    file.write_all(&buf)
}
