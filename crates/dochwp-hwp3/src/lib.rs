//! HWP 3.x reader.
//!
//! Layout follows 한글문서파일구조 3.0: 30-byte signature, 128-byte document
//! info, 1008-byte summary, optional info block, then a (possibly deflated)
//! body of font lists, styles, and JOHAB paragraphs.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read};

use dochwp_model::{Block, Diagnostic, Document, LayoutHint, Paragraph, Section, A4_HEIGHT_HU, A4_WIDTH_HU};
use flate2::read::DeflateDecoder;
use thiserror::Error;

/// 30-byte Hangul 3.0 signature: 24-byte ASCII including the trailing space.
pub const SIGNATURE: &[u8] = b"HWP Document File V3.00 \x1A\x01\x02\x03\x04\x05";
const DOC_INFO_LEN: usize = 128;
const SUMMARY_LEN: usize = 1008;
const CHAR_SHAPE_LEN: usize = 31;
const PARA_SHAPE_LEN: usize = 187;
const STYLE_LEN: usize = 20 + CHAR_SHAPE_LEN + PARA_SHAPE_LEN;
const LINE_INFO_LEN: usize = 14;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not HWP 3.x")]
    NotHwp3,
    #[error("truncated HWP3")]
    Truncated,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub fn sniff(bytes: &[u8]) -> bool {
    bytes.len() >= 23 && bytes.starts_with(b"HWP Document File V3.00")
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if !sniff(bytes) {
        return Err(Error::NotHwp3);
    }
    if bytes.len() < 30 + DOC_INFO_LEN {
        return Err(Error::Truncated);
    }
    let mut diagnostics = Vec::new();
    let info = &bytes[30..30 + DOC_INFO_LEN];
    let compressed = info.get(124).copied().unwrap_or(0) != 0;
    let encrypted = u16::from_le_bytes(info[96..98].try_into().unwrap());
    if encrypted != 0 {
        diagnostics.push(Diagnostic::parse_loss("HWP3 encrypted body is not opened"));
        return Ok(Document {
            sections: vec![Section::default()],
            diagnostics,
            ..Document::default()
        });
    }
    let paper_length = hwp3_unit_to_hu(u16::from_le_bytes(info[6..8].try_into().unwrap()));
    let paper_width = hwp3_unit_to_hu(u16::from_le_bytes(info[8..10].try_into().unwrap()));
    let top = hwp3_unit_to_hu(u16::from_le_bytes(info[10..12].try_into().unwrap()));
    let bottom = hwp3_unit_to_hu(u16::from_le_bytes(info[12..14].try_into().unwrap()));
    let left = hwp3_unit_to_hu(u16::from_le_bytes(info[14..16].try_into().unwrap()));
    let right = hwp3_unit_to_hu(u16::from_le_bytes(info[16..18].try_into().unwrap()));
    let info_block_length = u16::from_le_bytes(info[126..128].try_into().unwrap()) as usize;

    let body_start = 30 + DOC_INFO_LEN + SUMMARY_LEN + info_block_length;
    if bytes.len() < body_start {
        return Err(Error::Truncated);
    }
    let remaining = &bytes[body_start..];
    let inflated;
    let body: &[u8] = if compressed {
        inflated = inflate_raw(remaining)?;
        &inflated
    } else {
        remaining
    };

    let mut cur = Cursor::new(body);
    skip_font_lists(&mut cur)?;
    skip_styles(&mut cur)?;
    let paragraphs = read_paragraphs(&mut cur, &mut diagnostics);

    let mut section = Section::default();
    if paper_width > 0 {
        section.page.width = paper_width;
    } else {
        section.page.width = A4_WIDTH_HU;
    }
    if paper_length > 0 {
        section.page.height = paper_length;
    } else {
        section.page.height = A4_HEIGHT_HU;
    }
    section.page.margin_top = top;
    section.page.margin_bottom = bottom;
    section.page.margin_left = left;
    section.page.margin_right = right;
    section.body = paragraphs;
    if section.body.is_empty() {
        diagnostics.push(Diagnostic::parse_loss("HWP3 contained no paragraphs"));
    }
    Ok(Document {
        sections: vec![section],
        diagnostics,
        ..Document::default()
    })
}

fn inflate_raw(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    DeflateDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

fn skip_font_lists(cur: &mut Cursor<&[u8]>) -> Result<(), Error> {
    for _ in 0..7 {
        let n = read_u16(cur)?;
        let skip = (n as usize).saturating_mul(40);
        skip_bytes(cur, skip)?;
    }
    Ok(())
}

fn skip_styles(cur: &mut Cursor<&[u8]>) -> Result<(), Error> {
    let n = read_u16(cur)?;
    skip_bytes(cur, (n as usize).saturating_mul(STYLE_LEN))?;
    Ok(())
}

fn read_paragraphs(cur: &mut Cursor<&[u8]>, diagnostics: &mut Vec<Diagnostic>) -> Vec<Block> {
    let mut out = Vec::new();
    loop {
        let start = cur.position() as usize;
        let remaining = cur.get_ref().len().saturating_sub(start);
        if remaining < 3 {
            break;
        }
        let follow = match read_u8(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        let char_count = match read_u16(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        if char_count == 0 {
            let _ = skip_bytes(cur, 40);
            break;
        }
        let line_count = match read_u16(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        let include_char_shape = match read_u8(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        if read_u8(cur).is_err() {
            break;
        }
        if skip_bytes(cur, 4).is_err() {
            break;
        }
        if read_u8(cur).is_err() {
            break;
        }
        if skip_bytes(cur, CHAR_SHAPE_LEN).is_err() {
            break;
        }
        if follow == 0 && skip_bytes(cur, PARA_SHAPE_LEN).is_err() {
            diagnostics.push(Diagnostic::parse_loss("HWP3 paragraph shape truncated"));
            break;
        }
        let mut hints = Vec::new();
        for _ in 0..line_count {
            match read_line_hint(cur) {
                Ok(h) => hints.push(h),
                Err(_) => break,
            }
        }
        if include_char_shape != 0 {
            for _ in 0..char_count {
                let flag = match read_u8(cur) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                if flag != 1 && skip_bytes(cur, CHAR_SHAPE_LEN).is_err() {
                    break;
                }
            }
        }
        let mut text = String::new();
        let mut i = 0u16;
        while i < char_count {
            let ch = match read_u16(cur) {
                Ok(v) => v,
                Err(_) => break,
            };
            i += 1;
            if ch > 0 && ch < 32 && ch != 13 {
                let _ = skip_bytes(cur, 8);
                continue;
            }
            if ch == 13 {
                continue;
            }
            text.push(decode_johab(ch));
        }
        let mut para = Paragraph::from_text(text);
        para.layout_hints = hints;
        out.push(Block::Paragraph(para));
    }
    out
}

fn read_line_hint(cur: &mut Cursor<&[u8]>) -> Result<LayoutHint, Error> {
    let mut buf = [0u8; LINE_INFO_LEN];
    cur.read_exact(&mut buf)?;
    let start = u16::from_le_bytes(buf[0..2].try_into().unwrap());
    let line_height = u16::from_le_bytes(buf[4..6].try_into().unwrap());
    let pgy = u16::from_le_bytes(buf[6..8].try_into().unwrap());
    let sx = u16::from_le_bytes(buf[8..10].try_into().unwrap());
    let psx = u16::from_le_bytes(buf[10..12].try_into().unwrap());
    Ok(LayoutHint {
        text_start: u32::from(start),
        x: hwp3_unit_to_hu(sx),
        y: hwp3_unit_to_hu(pgy),
        width: hwp3_unit_to_hu(psx),
        line_height: hwp3_unit_to_hu(line_height),
        text_height: hwp3_unit_to_hu(line_height),
        baseline: 0,
        spacing: 0,
        flags: 0,
    })
}

/// Hangul 3.0 lengths are stored as HWPUNIT/4.
fn hwp3_unit_to_hu(v: u16) -> i32 {
    i32::from(v).saturating_mul(4)
}

fn hu_to_hwp3_unit(v: i32) -> u16 {
    u16::try_from((v / 4).clamp(0, i32::from(u16::MAX))).unwrap_or(u16::MAX)
}

/// KS X 1001 Johab (KSSM) → Unicode. Public 5+5+5 bit layout.
pub fn decode_johab(ch: u16) -> char {
    if ch < 0x80 {
        return char::from(ch as u8);
    }
    if ch < 0x8000 {
        return char::from_u32(u32::from(ch)).unwrap_or('?');
    }
    let cho_idx = ((ch >> 10) & 0x1F) as usize;
    let jung_idx = ((ch >> 5) & 0x1F) as usize;
    let jong_idx = (ch & 0x1F) as usize;
    const CHO: [i32; 32] = [
        -1, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, -1, -1, -1, -1,
        -1, -1, -1, -1, -1, -1, -1,
    ];
    const JUNG: [i32; 32] = [
        -1, -1, -1, 0, 1, 2, 3, 4, -1, -1, 5, 6, 7, 8, 9, 10, -1, -1, 11, 12, 13, 14, 15, 16, -1,
        -1, 17, 18, 19, 20, -1, -1,
    ];
    const JONG: [i32; 32] = [
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, -1, 17, 18, 19, 20, 21, 22,
        23, 24, 25, 26, 27, -1, -1,
    ];
    let cho = CHO[cho_idx];
    let jung = JUNG[jung_idx];
    let jong = JONG[jong_idx];
    if cho >= 0 && jung >= 0 && jong >= 0 {
        let uni = 0xAC00 + cho * 21 * 28 + jung * 28 + jong;
        if let Some(c) = char::from_u32(uni as u32) {
            return c;
        }
    }
    '?'
}

pub fn encode_johab(c: char) -> u16 {
    if (c as u32) < 0x80 {
        return c as u16;
    }
    let u = c as u32;
    if (0xAC00..=0xD7A3).contains(&u) {
        let s = (u - 0xAC00) as i32;
        let cho = s / (21 * 28);
        let jung = (s % (21 * 28)) / 28;
        let jong = s % 28;
        const CHO_INV: [u16; 19] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
        const JUNG_INV: [u16; 21] = [
            3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23, 26, 27, 28, 29,
        ];
        const JONG_INV: [u16; 28] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25,
            26, 27, 28, 29,
        ];
        return 0x8000
            | (CHO_INV[cho as usize] << 10)
            | (JUNG_INV[jung as usize] << 5)
            | JONG_INV[jong as usize];
    }
    c as u16
}

/// Spec-shaped HWP 3.0 bytes (JOHAB body). Not the custom `DOCHWP3` dialect.
pub fn write_classic(text: &str) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    let mut info = vec![0u8; DOC_INFO_LEN];
    info[6..8].copy_from_slice(&hu_to_hwp3_unit(A4_HEIGHT_HU).to_le_bytes());
    info[8..10].copy_from_slice(&hu_to_hwp3_unit(A4_WIDTH_HU).to_le_bytes());
    info[10..16].copy_from_slice(&[0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&info);
    out.extend_from_slice(&vec![0u8; SUMMARY_LEN]);
    for _ in 0..7 {
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    let units: Vec<u16> = text.chars().map(encode_johab).collect();
    out.push(1);
    out.extend_from_slice(&(units.len() as u16).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(0);
    out.push(0);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&[0u8; CHAR_SHAPE_LEN]);
    let mut line = [0u8; LINE_INFO_LEN];
    line[4..6].copy_from_slice(&275u16.to_le_bytes());
    line[10..12].copy_from_slice(&1000u16.to_le_bytes());
    out.extend_from_slice(&line);
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 40]);
    out
}

fn read_u8(cur: &mut Cursor<&[u8]>) -> Result<u8, Error> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u16(cur: &mut Cursor<&[u8]>) -> Result<u16, Error> {
    let mut b = [0u8; 2];
    cur.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn skip_bytes(cur: &mut Cursor<&[u8]>, n: usize) -> Result<(), Error> {
    let pos = cur.position() as usize;
    let end = pos.saturating_add(n);
    if end > cur.get_ref().len() {
        return Err(Error::Truncated);
    }
    cur.set_position(end as u64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn johab_roundtrip_syllable() {
        let ga = encode_johab('가');
        assert_eq!(decode_johab(ga), '가');
        let hih = encode_johab('힣');
        assert_eq!(decode_johab(hih), '힣');
    }

    #[test]
    fn classic_hwp3_bytes_are_not_custom_tag() {
        let bytes = write_classic("한글 HWP3");
        assert!(sniff(&bytes));
        assert!(!bytes.windows(8).any(|w| w == b"DOCHWP3\n"));
        let back = read(&bytes).unwrap();
        assert!(back.plain_text().contains("한글 HWP3"));
        assert!(
            !back
                .diagnostics
                .iter()
                .any(|d| d.code == dochwp_model::DiagnosticCode::ParseLoss)
        );
    }

    #[test]
    fn shipped_read_parses_hangul_hwp3_sample_when_present() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir).join("samples").join("hwp3-sample.hwp");
        let bytes = std::fs::read(&path).expect("sibling HWP3 sample");
        let doc = read(&bytes).expect("parse HWP3");
        assert!(
            !doc.plain_text().trim().is_empty(),
            "empty HWP3 body diagnostics={:?}",
            doc.diagnostics
        );
        let hints: usize = doc
            .sections
            .iter()
            .flat_map(|s| s.body.iter())
            .map(|b| match b {
                Block::Paragraph(p) => p.layout_hints.len(),
                _ => 0,
            })
            .sum();
        assert!(hints > 0, "HWP3 LineInfo must become LayoutHint");
        assert_eq!(bytes[0..30], SIGNATURE[..]);
    }

    #[test]
    fn rejects_hwp5_signature() {
        assert!(read(b"HWP Document File\0").is_err());
    }
}
