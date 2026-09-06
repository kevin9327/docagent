//! HWP 3.x reader.
//!
//! Layout follows 한글문서파일구조 3.0: 30-byte signature, 128-byte document
//! info, 1008-byte summary, optional info block, then a (possibly deflated)
//! body of font lists, styles, and JOHAB paragraphs.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read};

use docagent_model::{
    Block, CharStyle, Diagnostic, Document, LayoutHint, Paragraph, Run, Section, Underline,
    A4_HEIGHT_HU, A4_WIDTH_HU,
};
use flate2::read::{DeflateDecoder, ZlibDecoder};
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
        inflated = inflate_hwp3(remaining)?;
        &inflated
    } else if let Ok(plain) = inflate_hwp3(remaining) {
        inflated = plain;
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
    let plain = section
        .body
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.plain_text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if !text_has_language(&plain) {
        diagnostics.push(Diagnostic::parse_loss(
            "HWP3 body did not decode to Hangul or Latin text",
        ));
        section.body.clear();
    }
    if section.body.is_empty() {
        diagnostics.push(Diagnostic::parse_loss("HWP3 contained no paragraphs"));
    }
    Ok(Document {
        sections: vec![section],
        diagnostics,
        ..Document::default()
    })
}

fn inflate_hwp3(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut raw = Vec::new();
    if DeflateDecoder::new(data).read_to_end(&mut raw).is_ok() && !raw.is_empty() {
        return Ok(raw);
    }
    let mut zlib = Vec::new();
    if ZlibDecoder::new(data).read_to_end(&mut zlib).is_ok() && !zlib.is_empty() {
        return Ok(zlib);
    }
    Err(Error::Truncated)
}

fn text_has_language(s: &str) -> bool {
    s.chars()
        .any(|c| ('가'..='힣').contains(&c) || c.is_ascii_alphabetic())
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
        let mut default_buf = [0u8; CHAR_SHAPE_LEN];
        if cur.read_exact(&mut default_buf).is_err() {
            break;
        }
        let default_style = char_shape_from_bytes(&default_buf);
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
        let mut per_char = Vec::new();
        if include_char_shape != 0 {
            for _ in 0..char_count {
                let flag = match read_u8(cur) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                if flag == 1 {
                    per_char.push(default_style.clone());
                } else {
                    let mut buf = [0u8; CHAR_SHAPE_LEN];
                    if cur.read_exact(&mut buf).is_err() {
                        break;
                    }
                    per_char.push(char_shape_from_bytes(&buf));
                }
            }
        }
        let (text, nested, char_styles) =
            read_para_chars(cur, char_count, diagnostics, &default_style, &per_char);
        if !text.trim().is_empty() {
            let mut para = Paragraph::from_text("");
            para.runs = coalesce_styled_runs(&text, &char_styles);
            para.layout_hints = hints;
            out.push(Block::Paragraph(para));
        } else if !hints.is_empty() {
            let mut para = Paragraph::from_text("");
            para.layout_hints = hints;
            out.push(Block::Paragraph(para));
        }
        out.extend(nested);
    }
    out
}

fn read_para_chars(
    cur: &mut Cursor<&[u8]>,
    char_count: u16,
    diagnostics: &mut Vec<Diagnostic>,
    default_style: &CharStyle,
    per_char: &[CharStyle],
) -> (String, Vec<Block>, Vec<CharStyle>) {
    let mut text = String::new();
    let mut nested = Vec::new();
    let mut styles = Vec::new();
    let mut i = 0u32;
    while i < u32::from(char_count) {
        let ch = match read_u16(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        let idx = i as usize;
        i += 1;
        if ch == 13 {
            continue;
        }
        if (1..32).contains(&ch) {
            match skip_control(cur, ch, diagnostics) {
                Ok((extra_hchars, blocks)) => {
                    i = i.saturating_add(extra_hchars);
                    nested.extend(blocks);
                }
                Err(_) => break,
            }
            continue;
        }
        if let Some(c) = decode_hchar(ch) {
            text.push(c);
            styles.push(
                per_char
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| default_style.clone()),
            );
        }
    }
    (text, nested, styles)
}

/// HWP 3.0 char shape is 31 bytes. Size is pt×25; HWPUNIT is pt×100.
fn char_shape_from_bytes(p: &[u8]) -> CharStyle {
    let mut style = CharStyle::default();
    if p.len() >= 2 {
        let size = u16::from_le_bytes(p[0..2].try_into().unwrap());
        if size > 0 {
            style.size = i32::from(size).saturating_mul(4).max(1);
        }
    }
    if p.len() > 26 {
        let attr = p[26];
        style.italic = attr & 0x01 != 0;
        style.bold = attr & 0x02 != 0;
        if attr & 0x04 != 0 {
            style.underline = Underline::Single;
        }
        style.superscript = attr & 0x20 != 0;
        style.subscript = attr & 0x40 != 0;
    }
    style
}

fn coalesce_styled_runs(text: &str, styles: &[CharStyle]) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut runs = Vec::new();
    let mut buf = String::new();
    let mut cur = styles.first().cloned().unwrap_or_default();
    for (i, ch) in chars.iter().enumerate() {
        let st = styles.get(i).cloned().unwrap_or_else(|| cur.clone());
        if st != cur && !buf.is_empty() {
            let mut run = Run::text(std::mem::take(&mut buf));
            run.style = cur;
            runs.push(run);
        }
        cur = st;
        buf.push(*ch);
    }
    if !buf.is_empty() {
        let mut run = Run::text(buf);
        run.style = cur;
        runs.push(run);
    }
    runs
}

/// Consume bytes that follow a control hchar. Returns extra hchars already
/// counted in `char_count` (not including the control code itself).
fn skip_control(
    cur: &mut Cursor<&[u8]>,
    ch: u16,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(u32, Vec<Block>), Error> {
    match ch {
        9 | 18..=21 => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new()))
        }
        1 | 2 | 3 | 4 | 12 | 27 => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new()))
        }
        22 => {
            skip_bytes(cur, 22)?;
            Ok((11, Vec::new()))
        }
        23 => {
            skip_bytes(cur, 8)?;
            Ok((4, Vec::new()))
        }
        24 | 25 => {
            skip_bytes(cur, 4)?;
            Ok((2, Vec::new()))
        }
        26 => {
            skip_bytes(cur, 244)?;
            Ok((122, Vec::new()))
        }
        28 => {
            skip_bytes(cur, 62)?;
            Ok((31, Vec::new()))
        }
        30 | 31 => {
            skip_bytes(cur, 2)?;
            Ok((1, Vec::new()))
        }
        5 | 6 | 7 | 8 | 10 | 11 | 14 | 15 | 16 | 17 | 29 => skip_object(cur, ch, diagnostics),
        _ => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new()))
        }
    }
}

fn skip_object(
    cur: &mut Cursor<&[u8]>,
    ch: u16,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(u32, Vec<Block>), Error> {
    let header_val1 = read_u32(cur)?;
    let _close = read_u16(cur)?;
    let extra_hchars = 3u32;
    let mut nested = Vec::new();
    match ch {
        5 => {
            if header_val1 > 0 && header_val1 < 1_000_000 {
                skip_bytes(cur, header_val1 as usize)?;
            }
        }
        6 => skip_bytes(cur, 34)?,
        7 => skip_bytes(cur, 76)?,
        8 => skip_bytes(cur, 88)?,
        10 => {
            let mut info = [0u8; 84];
            cur.read_exact(&mut info)?;
            let cell_count = u16::from_le_bytes(info[80..82].try_into().unwrap()).min(1024);
            skip_bytes(cur, usize::from(cell_count).saturating_mul(27))?;
            for _ in 0..cell_count {
                nested.extend(read_paragraphs(cur, diagnostics));
            }
            nested.extend(read_paragraphs(cur, diagnostics));
        }
        11 => {
            let mut info = [0u8; 348];
            cur.read_exact(&mut info)?;
            let n_ext = u32::from_le_bytes(info[0..4].try_into().unwrap());
            if n_ext > 0 && n_ext < 16 * 1024 * 1024 {
                skip_bytes(cur, n_ext as usize)?;
            }
            nested.extend(read_paragraphs(cur, diagnostics));
        }
        14 => skip_bytes(cur, 84)?,
        15 => {
            skip_bytes(cur, 8)?;
            nested.extend(read_paragraphs(cur, diagnostics));
        }
        16 => {
            skip_bytes(cur, 10)?;
            nested.extend(read_paragraphs(cur, diagnostics));
        }
        17 => {
            skip_bytes(cur, 14)?;
            nested.extend(read_paragraphs(cur, diagnostics));
        }
        29 => {
            if header_val1 > 0 && header_val1 < 1_000_000 {
                skip_bytes(cur, header_val1 as usize)?;
            }
        }
        _ => {}
    }
    Ok((extra_hchars, nested))
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
    decode_hchar(ch).unwrap_or('?')
}

fn decode_hchar(ch: u16) -> Option<char> {
    if ch < 0x80 {
        return Some(char::from(ch as u8));
    }
    if ch < 0x8000 {
        // Not ASCII and not JOHAB. Do not treat the raw code as Unicode CJK.
        return None;
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
            return Some(c);
        }
    }
    None
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

fn read_u32(cur: &mut Cursor<&[u8]>) -> Result<u32, Error> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
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

    fn write_classic_styled(text: &str, attr: u8, size_pt25: u16) -> Vec<u8> {
        let mut bytes = write_classic(text);
        let start = 30 + DOC_INFO_LEN + SUMMARY_LEN + 7 * 2 + 2 + 12;
        if start + CHAR_SHAPE_LEN <= bytes.len() {
            bytes[start..start + 2].copy_from_slice(&size_pt25.to_le_bytes());
            bytes[start + 26] = attr;
        }
        bytes
    }

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
                .any(|d| d.code == docagent_model::DiagnosticCode::ParseLoss)
        );
    }

    #[test]
    fn char_shape_attr_maps_marks() {
        let mut p = [0u8; CHAR_SHAPE_LEN];
        p[0..2].copy_from_slice(&250u16.to_le_bytes());
        p[26] = 0x02 | 0x01 | 0x04 | 0x20;
        let st = char_shape_from_bytes(&p);
        assert!(st.bold && st.italic && st.superscript);
        assert_eq!(st.underline, Underline::Single);
        assert_eq!(st.size, 1000);
        p[26] = 0x40;
        let st = char_shape_from_bytes(&p);
        assert!(st.subscript);
        assert!(!st.bold);
    }

    #[test]
    fn classic_hwp3_keeps_char_shape_marks() {
        let bytes = write_classic_styled("GPU-free", 0x02, 250);
        let back = read(&bytes).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert_eq!(p.runs[0].style.size, 1000);
    }

    #[test]
    fn shipped_read_parses_hangul_hwp3_sample_when_present() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir).join("samples").join("hwp3-sample.hwp");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let doc = read(&bytes).expect("parse HWP3");
        let text = doc.plain_text();
        assert!(
            text.contains("Creating Linux Virtual Servers"),
            "expected Hangul 3.0 title, got {:?}",
            text.chars().take(80).collect::<String>()
        );
        assert!(
            text.chars().any(|c| ('가'..='힣').contains(&c)),
            "expected JOHAB Korean syllables, got {:?}",
            text.chars().take(80).collect::<String>()
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
        assert!(
            !doc.diagnostics
                .iter()
                .any(|d| d.code == docagent_model::DiagnosticCode::ParseLoss),
            "parse loss: {:?}",
            doc.diagnostics
        );
    }

    #[test]
    fn rejects_hwp5_signature() {
        assert!(read(b"HWP Document File\0").is_err());
    }
}
