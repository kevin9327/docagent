//! HWP 3.x codec.
//!
//! Layout follows 한글문서파일구조 3.0: 30-byte signature, 128-byte document
//! info, 1008-byte summary, optional info block, then a (possibly deflated)
//! body of font lists, styles, and JOHAB paragraphs. Hyperlinks are control
//! char 10 with other-options `0x10` plus additional-info TagID 3 (617-byte
//! entries, kchar URL).

#![forbid(unsafe_code)]

use std::io::{Cursor, Read};

use docagent_model::{
    A4_HEIGHT_HU, A4_WIDTH_HU, Block, BreakKind, CharStyle, Color, Diagnostic, Document,
    InlineObject, LayoutHint, NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section,
    Underline,
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
const CTRL_TABLE: u16 = 10;
/// Spec §8 table-info other-options bit: hypertext object, not a grid.
const HYPERTEXT_OPTION: u16 = 0x10;
/// Additional-info TagID 3 = HyperLink (스펙 §8.3).
const HYPERLINK_INFO_ID: u32 = 3;
const HYPERLINK_ENTRY_LEN: usize = 617;
const HYPERLINK_URL_LEN: usize = 256;
const LIST_LEVEL_INDENT_HU: i32 = 1400;

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
    let styles = read_styles(&mut cur)?;
    let mut paragraphs = read_paragraphs(&mut cur, &mut diagnostics, &styles);
    let urls = read_additional_hyperlinks(&mut cur);
    apply_hwp3_hyperlinks(&mut paragraphs, &urls);

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

struct Hwp3NamedStyle {
    name: String,
    indent_left: i32,
}

fn read_styles(cur: &mut Cursor<&[u8]>) -> Result<Vec<Hwp3NamedStyle>, Error> {
    let n = read_u16(cur)?;
    let mut names = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut name_buf = [0u8; 20];
        cur.read_exact(&mut name_buf)?;
        let name = decode_hwp3_kchar(&name_buf);
        skip_bytes(cur, CHAR_SHAPE_LEN)?;
        let left = read_u16(cur)?;
        skip_bytes(cur, PARA_SHAPE_LEN.saturating_sub(2))?;
        names.push(Hwp3NamedStyle {
            name,
            indent_left: hwp3_unit_to_hu(left),
        });
    }
    Ok(names)
}

fn style_name_base(name: &str) -> &str {
    let n = name.trim();
    let digits = n.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits == n.len() {
        n
    } else {
        &n[..n.len() - digits]
    }
}

fn style_name_level(name: &str) -> u8 {
    let n = name.trim();
    let digits = n.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits == n.len() {
        0
    } else {
        n[n.len() - digits..].parse().unwrap_or(0)
    }
}

fn list_level_from_indent(indent: i32) -> u8 {
    if indent < 700 {
        0
    } else {
        let n = indent / LIST_LEVEL_INDENT_HU;
        u8::try_from(n.clamp(0, i32::from(u8::MAX))).unwrap_or(u8::MAX)
    }
}

fn recovered_list_level(style_name: Option<&str>, indent: i32) -> u8 {
    let from_name = style_name.map(style_name_level).unwrap_or(0);
    if from_name > 0 {
        from_name
    } else {
        list_level_from_indent(indent)
    }
}

fn style_kind(name: &str) -> Option<StyleKind> {
    let n = name.trim();
    let base = style_name_base(n);
    if base.eq_ignore_ascii_case("Quote") || n == "인용" || n == "인용구" || n == "인용문" {
        Some(StyleKind::Quote)
    } else if base.eq_ignore_ascii_case("CodeBlock") || n.contains("코드블록") || n == "코드" {
        Some(StyleKind::CodeBlock)
    } else if base.eq_ignore_ascii_case("HorizontalLine")
        || n.contains("가로선")
        || n.contains("구분선")
    {
        Some(StyleKind::Thematic)
    } else if base.eq_ignore_ascii_case("Task") || n == "할 일" || n == "작업" {
        Some(StyleKind::Task)
    } else if base.eq_ignore_ascii_case("Bullet") || n == "글머리" || n == "목록" {
        Some(StyleKind::Bullet)
    } else if base.eq_ignore_ascii_case("Number") || n == "번호" || n == "문단번호" {
        Some(StyleKind::Number)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StyleKind {
    Quote,
    CodeBlock,
    Thematic,
    Task,
    Bullet,
    Number,
}

fn task_prefix(p: &Paragraph) -> Option<&'static str> {
    let n = p.numbering.as_ref()?;
    if n.format != Some(NumberFormat::Task) {
        return None;
    }
    Some(if n.start == Some(1) {
        "[x] "
    } else {
        "[ ] "
    })
}

fn strip_decimal_prefix(text: &str) -> Option<(u32, String)> {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i + 2 > bytes.len() || &bytes[i..i + 2] != b". " {
        return None;
    }
    let n = text[..i].parse().ok()?;
    Some((n, text[i + 2..].to_string()))
}

fn apply_list_paragraph(p: &mut Paragraph, kind: Option<StyleKind>, level: u8) {
    let mut checked = false;
    let mut is_task = kind == Some(StyleKind::Task);
    if let Some(run) = p.runs.first_mut()
        && let RunContent::Text(text) = &mut run.content
    {
        let is_checked = text.starts_with("[x] ") || text.starts_with("[X] ");
        let is_unchecked = text.starts_with("[ ] ");
        if is_checked || is_unchecked {
            *text = text[4..].to_string();
            checked = is_checked;
            is_task = true;
        }
    }
    if is_task {
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level,
            start: checked.then_some(1),
            format: Some(NumberFormat::Task),
        });
        return;
    }
    if matches!(
        kind,
        Some(StyleKind::Quote | StyleKind::CodeBlock | StyleKind::Thematic)
    ) {
        return;
    }
    if let Some(run) = p.runs.first_mut()
        && let RunContent::Text(text) = &mut run.content
    {
        if let Some(rest) = text.strip_prefix("• ") {
            *text = rest.to_string();
            p.numbering = Some(NumberingRef {
                definition_id: 0,
                level,
                start: None,
                format: Some(NumberFormat::Bullet),
            });
            return;
        }
        if let Some((start, rest)) = strip_decimal_prefix(text) {
            *text = rest;
            p.numbering = Some(NumberingRef {
                definition_id: 1,
                level,
                start: Some(start),
                format: Some(NumberFormat::Decimal),
            });
            return;
        }
    }
    match kind {
        Some(StyleKind::Bullet) => {
            p.numbering = Some(NumberingRef {
                definition_id: 0,
                level,
                start: None,
                format: Some(NumberFormat::Bullet),
            });
        }
        Some(StyleKind::Number) => {
            p.numbering = Some(NumberingRef {
                definition_id: 1,
                level,
                start: Some(1),
                format: Some(NumberFormat::Decimal),
            });
        }
        _ => {}
    }
}

fn read_paragraphs(
    cur: &mut Cursor<&[u8]>,
    diagnostics: &mut Vec<Diagnostic>,
    styles: &[Hwp3NamedStyle],
) -> Vec<Block> {
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
        let style_index = match read_u8(cur) {
            Ok(v) => v as usize,
            Err(_) => break,
        };
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
        let (text, nested, char_styles, hrefs) = read_para_chars(
            cur,
            char_count,
            diagnostics,
            &default_style,
            &per_char,
            styles,
        );
        let style = styles.get(style_index);
        let kind = style.map(|s| s.name.as_str()).and_then(style_kind);
        let level = style
            .map(|s| recovered_list_level(Some(s.name.as_str()), s.indent_left))
            .unwrap_or(0);
        if kind == Some(StyleKind::Thematic) {
            out.push(Block::Break(BreakKind::Thematic));
        } else if !text.trim().is_empty() {
            let mut para = Paragraph::from_text("");
            para.runs = coalesce_styled_runs(&text, &char_styles, &hrefs);
            para.layout_hints = hints;
            para.quote = kind == Some(StyleKind::Quote);
            para.code_block = kind == Some(StyleKind::CodeBlock);
            apply_list_paragraph(&mut para, kind, level);
            out.push(Block::Paragraph(para));
        } else if !hints.is_empty() {
            let mut para = Paragraph::from_text("");
            para.layout_hints = hints;
            para.quote = kind == Some(StyleKind::Quote);
            para.code_block = kind == Some(StyleKind::CodeBlock);
            apply_list_paragraph(&mut para, kind, level);
            out.push(Block::Paragraph(para));
        } else if matches!(
            kind,
            Some(StyleKind::Task | StyleKind::Bullet | StyleKind::Number)
        ) {
            let mut para = Paragraph::from_text("");
            apply_list_paragraph(&mut para, kind, level);
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
    named_styles: &[Hwp3NamedStyle],
) -> (String, Vec<Block>, Vec<CharStyle>, Vec<Option<usize>>) {
    let mut text = String::new();
    let mut nested = Vec::new();
    let mut styles = Vec::new();
    let mut hrefs = Vec::new();
    let mut link_seq = 0usize;
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
            match skip_control(cur, ch, diagnostics, named_styles) {
                Ok((extra_hchars, blocks, link_display)) => {
                    i = i.saturating_add(extra_hchars);
                    nested.extend(blocks);
                    if let Some(display) = link_display {
                        let href = link_seq;
                        link_seq += 1;
                        let mut st = default_style.clone();
                        st.underline = Underline::Single;
                        for c in display.chars() {
                            text.push(c);
                            styles.push(st.clone());
                            hrefs.push(Some(href));
                        }
                    }
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
            hrefs.push(None);
        }
    }
    (text, nested, styles, hrefs)
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
    if p.len() > 25 && p[25] > 0 {
        // Offset 23 palette + 25 shade ratio (0–100). No RGB; map any shade to MARK.
        style.highlight = Some(Color::MARK);
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

fn coalesce_styled_runs(text: &str, styles: &[CharStyle], hrefs: &[Option<usize>]) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut runs = Vec::new();
    let mut buf = String::new();
    let mut cur = styles.first().cloned().unwrap_or_default();
    let mut cur_href = hrefs.first().copied().flatten();
    for (i, ch) in chars.iter().enumerate() {
        let st = styles.get(i).cloned().unwrap_or_else(|| cur.clone());
        let href = hrefs.get(i).copied().flatten();
        if (st != cur || href != cur_href) && !buf.is_empty() {
            runs.push(run_from_span(std::mem::take(&mut buf), cur, cur_href));
        }
        cur = st;
        cur_href = href;
        buf.push(*ch);
    }
    if !buf.is_empty() {
        runs.push(run_from_span(buf, cur, cur_href));
    }
    runs
}

fn run_from_span(text: String, mut style: CharStyle, href: Option<usize>) -> Run {
    if href.is_some() {
        let mut run = Run::hyperlink(text, "");
        style.underline = Underline::Single;
        run.style = style;
        run
    } else {
        let mut run = Run::text(text);
        run.style = style;
        run
    }
}

/// Consume bytes that follow a control hchar. Returns extra hchars already
/// counted in `char_count` (not including the control code itself), nested
/// blocks, and hypertext display text when the object is a hyperlink.
fn skip_control(
    cur: &mut Cursor<&[u8]>,
    ch: u16,
    diagnostics: &mut Vec<Diagnostic>,
    styles: &[Hwp3NamedStyle],
) -> Result<(u32, Vec<Block>, Option<String>), Error> {
    match ch {
        9 | 18..=21 => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new(), None))
        }
        1 | 2 | 3 | 4 | 12 | 27 => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new(), None))
        }
        22 => {
            skip_bytes(cur, 22)?;
            Ok((11, Vec::new(), None))
        }
        23 => {
            skip_bytes(cur, 8)?;
            Ok((4, Vec::new(), None))
        }
        24 | 25 => {
            skip_bytes(cur, 4)?;
            Ok((2, Vec::new(), None))
        }
        26 => {
            skip_bytes(cur, 244)?;
            Ok((122, Vec::new(), None))
        }
        28 => {
            skip_bytes(cur, 62)?;
            Ok((31, Vec::new(), None))
        }
        30 | 31 => {
            skip_bytes(cur, 2)?;
            Ok((1, Vec::new(), None))
        }
        5 | 6 | 7 | 8 | 10 | 11 | 14 | 15 | 16 | 17 | 29 => {
            skip_object(cur, ch, diagnostics, styles)
        }
        _ => {
            skip_bytes(cur, 6)?;
            Ok((3, Vec::new(), None))
        }
    }
}

fn skip_object(
    cur: &mut Cursor<&[u8]>,
    ch: u16,
    diagnostics: &mut Vec<Diagnostic>,
    styles: &[Hwp3NamedStyle],
) -> Result<(u32, Vec<Block>, Option<String>), Error> {
    let header_val1 = read_u32(cur)?;
    let _close = read_u16(cur)?;
    let extra_hchars = 3u32;
    let mut nested = Vec::new();
    let mut link_display = None;
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
            let other_options = u16::from_le_bytes(info[14..16].try_into().unwrap());
            let cell_count = u16::from_le_bytes(info[80..82].try_into().unwrap()).min(1024);
            skip_bytes(cur, usize::from(cell_count).saturating_mul(27))?;
            if other_options & HYPERTEXT_OPTION != 0 {
                let mut display = String::new();
                for n in 0..cell_count {
                    let cells = read_paragraphs(cur, diagnostics, styles);
                    if n != 0 {
                        continue;
                    }
                    for b in cells {
                        if let Block::Paragraph(p) = b {
                            display.push_str(&p.plain_text());
                        }
                    }
                }
                let _caption = read_paragraphs(cur, diagnostics, styles);
                if !display.is_empty() {
                    link_display = Some(display);
                }
            } else {
                for _ in 0..cell_count {
                    nested.extend(read_paragraphs(cur, diagnostics, styles));
                }
                nested.extend(read_paragraphs(cur, diagnostics, styles));
            }
        }
        11 => {
            let mut info = [0u8; 348];
            cur.read_exact(&mut info)?;
            let n_ext = u32::from_le_bytes(info[0..4].try_into().unwrap());
            if n_ext > 0 && n_ext < 16 * 1024 * 1024 {
                skip_bytes(cur, n_ext as usize)?;
            }
            nested.extend(read_paragraphs(cur, diagnostics, styles));
        }
        14 => skip_bytes(cur, 84)?,
        15 => {
            skip_bytes(cur, 8)?;
            nested.extend(read_paragraphs(cur, diagnostics, styles));
        }
        16 => {
            skip_bytes(cur, 10)?;
            nested.extend(read_paragraphs(cur, diagnostics, styles));
        }
        17 => {
            skip_bytes(cur, 14)?;
            nested.extend(read_paragraphs(cur, diagnostics, styles));
        }
        29 => {
            if header_val1 > 0 && header_val1 < 1_000_000 {
                skip_bytes(cur, header_val1 as usize)?;
            }
        }
        _ => {}
    }
    Ok((extra_hchars, nested, link_display))
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
        const CHO_INV: [u16; 19] = [
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ];
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

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    Ok(write_bytes(doc))
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

fn write_bytes(doc: &Document) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    let mut info = vec![0u8; DOC_INFO_LEN];
    let page = doc.sections.first().map(|s| &s.page);
    let (width, height, top, bottom, left, right) = match page {
        Some(p) => (
            p.width,
            p.height,
            p.margin_top,
            p.margin_bottom,
            p.margin_left,
            p.margin_right,
        ),
        None => (A4_WIDTH_HU, A4_HEIGHT_HU, 0, 0, 0, 0),
    };
    info[6..8].copy_from_slice(&hu_to_hwp3_unit(height).to_le_bytes());
    info[8..10].copy_from_slice(&hu_to_hwp3_unit(width).to_le_bytes());
    info[10..12].copy_from_slice(&hu_to_hwp3_unit(top).to_le_bytes());
    info[12..14].copy_from_slice(&hu_to_hwp3_unit(bottom).to_le_bytes());
    info[14..16].copy_from_slice(&hu_to_hwp3_unit(left).to_le_bytes());
    info[16..18].copy_from_slice(&hu_to_hwp3_unit(right).to_le_bytes());
    out.extend_from_slice(&info);
    out.extend_from_slice(&vec![0u8; SUMMARY_LEN]);
    for _ in 0..7 {
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.extend_from_slice(&10u16.to_le_bytes());
    out.extend_from_slice(&encode_hwp3_style("Normal", 0, 0, 0));
    out.extend_from_slice(&encode_hwp3_style("Quote", 350, 0, 1));
    out.extend_from_slice(&encode_hwp3_style("CodeBlock", 0, 40, 0));
    out.extend_from_slice(&encode_hwp3_style("HorizontalLine", 0, 0, 1));
    out.extend_from_slice(&encode_hwp3_style("Task", 0, 0, 0));
    out.extend_from_slice(&encode_hwp3_style("Bullet", 0, 0, 0));
    out.extend_from_slice(&encode_hwp3_style("Number", 0, 0, 0));
    let nested = hu_to_hwp3_unit(LIST_LEVEL_INDENT_HU);
    out.extend_from_slice(&encode_hwp3_style("Task1", nested, 0, 0));
    out.extend_from_slice(&encode_hwp3_style("Bullet1", nested, 0, 0));
    out.extend_from_slice(&encode_hwp3_style("Number1", nested, 0, 0));
    let mut urls = Vec::new();
    if let Some(section) = doc.sections.first() {
        for block in &section.body {
            match block {
                Block::Paragraph(p)
                    if !p.plain_text().trim().is_empty()
                        || p.numbering
                            .as_ref()
                            .and_then(|n| n.format)
                            .is_some() =>
                {
                    write_paragraph(&mut out, p, &mut urls, para_style_id(p));
                }
                Block::Break(BreakKind::Thematic) => {
                    write_paragraph(&mut out, &Paragraph::from_text(" "), &mut urls, 3);
                }
                _ => {}
            }
        }
    }
    write_para_list_end(&mut out);
    write_hyperlink_info_block(&mut out, &urls);
    out
}

fn para_style_id(p: &Paragraph) -> u8 {
    if p.code_block {
        2
    } else if p.quote {
        1
    } else {
        let nested = p.numbering.as_ref().is_some_and(|n| n.level > 0);
        match p.numbering.as_ref().and_then(|n| n.format) {
            Some(NumberFormat::Task) => {
                if nested {
                    7
                } else {
                    4
                }
            }
            Some(NumberFormat::Bullet) => {
                if nested {
                    8
                } else {
                    5
                }
            }
            Some(NumberFormat::Decimal) => {
                if nested {
                    9
                } else {
                    6
                }
            }
            _ => 0,
        }
    }
}

fn encode_hwp3_style(name: &str, left_margin: u16, shade: u8, border: u8) -> [u8; STYLE_LEN] {
    let mut buf = [0u8; STYLE_LEN];
    let bytes = name.as_bytes();
    let n = bytes.len().min(20);
    buf[..n].copy_from_slice(&bytes[..n]);
    const PS: usize = 20 + CHAR_SHAPE_LEN;
    buf[PS..PS + 2].copy_from_slice(&left_margin.to_le_bytes());
    buf[PS + 6..PS + 8].copy_from_slice(&160u16.to_le_bytes());
    buf[PS + 180] = shade;
    buf[PS + 181] = border;
    buf
}

fn write_paragraph(out: &mut Vec<u8>, para: &Paragraph, urls: &mut Vec<String>, style_index: u8) {
    let mut payload = Vec::new();
    let mut char_count = 0u16;
    if let Some(m) = task_prefix(para) {
        for c in m.chars() {
            char_count = char_count.saturating_add(1);
            payload.extend_from_slice(&encode_johab(c).to_le_bytes());
        }
    }
    for run in &para.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                let shown = if display.is_empty() {
                    target.as_str()
                } else {
                    display.as_str()
                };
                if shown.is_empty() {
                    continue;
                }
                char_count = char_count.saturating_add(4);
                write_hypertext_object(&mut payload, shown);
                if !target.is_empty() {
                    urls.push(target.clone());
                }
            }
            _ => {
                for c in run.display_text().chars() {
                    char_count = char_count.saturating_add(1);
                    payload.extend_from_slice(&encode_johab(c).to_le_bytes());
                }
            }
        }
    }
    out.push(1);
    out.extend_from_slice(&char_count.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.push(0);
    out.push(0);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(style_index);
    out.extend_from_slice(&encode_hwp3_char_shape(para));
    let mut line = [0u8; LINE_INFO_LEN];
    line[4..6].copy_from_slice(&275u16.to_le_bytes());
    line[10..12].copy_from_slice(&1000u16.to_le_bytes());
    out.extend_from_slice(&line);
    out.extend_from_slice(&payload);
}

fn encode_hwp3_char_shape(para: &Paragraph) -> [u8; CHAR_SHAPE_LEN] {
    let mut buf = [0u8; CHAR_SHAPE_LEN];
    if para.runs.iter().any(|r| r.style.highlight.is_some()) {
        buf[23] = 6; // yellow palette
        buf[25] = 100; // 100% shade
    }
    buf
}

fn write_hypertext_object(out: &mut Vec<u8>, display: &str) {
    out.extend_from_slice(&CTRL_TABLE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&CTRL_TABLE.to_le_bytes());
    let mut info = [0u8; 84];
    info[14..16].copy_from_slice(&HYPERTEXT_OPTION.to_le_bytes());
    info[80..82].copy_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&info);
    out.extend_from_slice(&[0u8; 27]);
    write_simple_paragraph(out, display);
    write_para_list_end(out);
}

fn write_simple_paragraph(out: &mut Vec<u8>, text: &str) {
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
    write_para_list_end(out);
}

fn write_para_list_end(out: &mut Vec<u8>) {
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 40]);
}

fn write_hyperlink_info_block(out: &mut Vec<u8>, urls: &[String]) {
    if urls.is_empty() {
        return;
    }
    let data_len = urls.len().saturating_mul(HYPERLINK_ENTRY_LEN);
    out.extend_from_slice(&HYPERLINK_INFO_ID.to_le_bytes());
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for url in urls {
        let mut entry = vec![0u8; HYPERLINK_ENTRY_LEN];
        let encoded = encode_hwp3_kchar(url, HYPERLINK_URL_LEN);
        entry[..HYPERLINK_URL_LEN].copy_from_slice(&encoded);
        entry[613] = 2;
        out.extend_from_slice(&entry);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
}

fn encode_hwp3_kchar(s: &str, max_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        if (c as u32) < 0x80 {
            if out.len() + 1 >= max_len {
                break;
            }
            out.push(c as u8);
        } else if out.len() + 2 >= max_len {
            break;
        } else {
            let j = encode_johab(c);
            out.push((j >> 8) as u8);
            out.push(j as u8);
        }
    }
    out.resize(max_len, 0);
    out
}

fn decode_hwp3_kchar(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b1 = bytes[i];
        if b1 == 0 {
            break;
        }
        if b1 < 0x80 {
            result.push(b1 as char);
            i += 1;
        } else if i + 1 < bytes.len() {
            let ch = u16::from_be_bytes([b1, bytes[i + 1]]);
            if let Some(c) = decode_hchar(ch) {
                result.push(c);
            }
            i += 2;
        } else {
            break;
        }
    }
    result
}

fn read_additional_hyperlinks(cur: &mut Cursor<&[u8]>) -> Vec<String> {
    let mut urls = Vec::new();
    loop {
        let start = cur.position() as usize;
        let remaining = cur.get_ref().len().saturating_sub(start);
        if remaining < 4 {
            break;
        }
        let id = match read_u32(cur) {
            Ok(v) => v,
            Err(_) => break,
        };
        if id == 0 {
            break;
        }
        let length = match read_u32(cur) {
            Ok(v) => v as usize,
            Err(_) => break,
        };
        if length == 0 || length > 16 * 1024 * 1024 {
            break;
        }
        let pos = cur.position() as usize;
        if pos.saturating_add(length) > cur.get_ref().len() {
            break;
        }
        let data = cur.get_ref()[pos..pos + length].to_vec();
        cur.set_position((pos + length) as u64);
        if id == HYPERLINK_INFO_ID {
            let n = data.len() / HYPERLINK_ENTRY_LEN;
            for i in 0..n {
                let off = i * HYPERLINK_ENTRY_LEN;
                let end = off + HYPERLINK_URL_LEN;
                if end <= data.len() {
                    let url = decode_hwp3_kchar(&data[off..end]);
                    if !url.is_empty() {
                        urls.push(url);
                    }
                }
            }
        }
    }
    urls
}

fn apply_hwp3_hyperlinks(blocks: &mut [Block], urls: &[String]) {
    let mut idx = 0usize;
    for block in blocks {
        let Block::Paragraph(p) = block else {
            continue;
        };
        for run in &mut p.runs {
            let RunContent::Inline(InlineObject::Hyperlink { target, .. }) = &mut run.content
            else {
                continue;
            };
            if idx < urls.len() {
                *target = urls[idx].clone();
                idx += 1;
            }
        }
    }
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
    fn roundtrip_keeps_hyperlink() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run::hyperlink(
            "DocAgent",
            "https://github.com/kevin9327/docagent",
        )];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(bytes.windows(4).any(|w| w == 3u32.to_le_bytes()), "TagID 3");
        assert!(
            bytes
                .windows(b"https://github.com/kevin9327/docagent".len())
                .any(|w| w == b"https://github.com/kevin9327/docagent"),
            "kchar URL"
        );
        let back = read(&bytes).expect("read");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
    }

    #[test]
    fn hyperlink_sits_between_plain_runs() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            Run::text("see "),
            Run::hyperlink("here", "https://example.com/hwp3"),
            Run::text(" now"),
        ];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        assert!(back.plain_text().contains("see here now"));
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://example.com/hwp3" && display == "here"
        )));
    }

    #[test]
    fn shipped_read_parses_hangul_hwp3_sample_when_present() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir)
            .join("samples")
            .join("hwp3-sample.hwp");
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

    #[test]
    fn roundtrip_keeps_quote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"Quote".len()).any(|w| w == b"Quote"),
            "style list Quote"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.quote);
        assert!(p.plain_text().contains("Replay is evidence"));
    }

    #[test]
    fn roundtrip_keeps_code_block() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("let n = 1;");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"CodeBlock".len()).any(|w| w == b"CodeBlock"),
            "style list CodeBlock"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.code_block);
        assert!(p.plain_text().contains("let n = 1;"));
    }

    #[test]
    fn roundtrip_keeps_thematic_break() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("before")));
        section.body.push(Block::Break(BreakKind::Thematic));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("after")));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes
                .windows(b"HorizontalLine".len())
                .any(|w| w == b"HorizontalLine"),
            "style list HorizontalLine"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        assert!(
            back.sections[0]
                .body
                .iter()
                .any(|b| matches!(b, Block::Break(BreakKind::Thematic))),
            "{:?}",
            back.sections[0].body
        );
        assert!(back.plain_text().contains("before"));
        assert!(back.plain_text().contains("after"));
    }

    #[test]
    fn roundtrip_keeps_task_item() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut checked = Paragraph::from_text("Replay the convert");
        checked.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut unchecked = Paragraph::from_text("Hope");
        unchecked.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: None,
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(checked));
        section.body.push(Block::Paragraph(unchecked));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"Task".len()).any(|w| w == b"Task"),
            "style list Task"
        );
        let marker: Vec<u8> = "[x] "
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(
            bytes.windows(marker.len()).any(|w| w == marker),
            "checked task marker"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("checked task");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.start)),
            Some((Some(NumberFormat::Task), Some(1)))
        );
        assert_eq!(p.plain_text(), "Replay the convert");
        let Block::Paragraph(p) = &back.sections[0].body[1] else {
            panic!("unchecked task");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.start)),
            Some((Some(NumberFormat::Task), None))
        );
        assert_eq!(p.plain_text(), "Hope");
    }

    #[test]
    fn roundtrip_keeps_bullet_list() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Receiving dock is clear");
        p.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"Bullet".len()).any(|w| w == b"Bullet"),
            "style list Bullet"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("bullet");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.level, n.start)),
            Some((Some(NumberFormat::Bullet), 0, None))
        );
        assert_eq!(p.plain_text(), "Receiving dock is clear");
    }

    #[test]
    fn roundtrip_keeps_decimal_list() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Hash the input");
        p.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"Number".len()).any(|w| w == b"Number"),
            "style list Number"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("decimal");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.level, n.start)),
            Some((Some(NumberFormat::Decimal), 0, Some(1)))
        );
        assert_eq!(p.plain_text(), "Hash the input");
    }

    #[test]
    fn roundtrip_keeps_nested_bullet_level() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut outer = Paragraph::from_text("Receiving dock is clear");
        outer.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut inner = Paragraph::from_text("Bay 4, not bay 2");
        inner.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut first = Paragraph::from_text("Hash the input");
        first.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut nested = Paragraph::from_text("Run Convert");
        nested.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 1,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(outer));
        section.body.push(Block::Paragraph(inner));
        section.body.push(Block::Paragraph(first));
        section.body.push(Block::Paragraph(nested));
        doc.sections.push(section);
        let bytes = write(&doc).expect("write");
        assert!(
            bytes.windows(b"Bullet1".len()).any(|w| w == b"Bullet1"),
            "style list Bullet1"
        );
        let back = roundtrip(&doc).expect("roundtrip");
        let nums: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| (n.format, n.level, n.start)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                (Some(NumberFormat::Bullet), 0, None),
                (Some(NumberFormat::Bullet), 1, None),
                (Some(NumberFormat::Decimal), 0, Some(1)),
                (Some(NumberFormat::Decimal), 1, Some(1)),
            ]
        );
    }

    #[test]
    fn roundtrip_keeps_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight.is_some()
                && matches!(&r.content, RunContent::Text(t) if t == "hashes")
        }));
    }

    #[test]
    fn char_shape_shade_ratio_maps_highlight() {
        let mut p = [0u8; CHAR_SHAPE_LEN];
        p[25] = 40;
        p[23] = 6;
        let st = char_shape_from_bytes(&p);
        assert_eq!(st.highlight, Some(Color::MARK));
        let blank = char_shape_from_bytes(&[0u8; CHAR_SHAPE_LEN]);
        assert!(blank.highlight.is_none());
    }

    #[test]
    fn hwp3_table_header_unsupported() {
        // HWP 3.0 table-info (84 bytes) has no repeating-header / nHeader field,
        // and this codec does not write Table IR (cells are dropped on write).
        // Do not fake a TableHeader style round-trip.
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = docagent_model::Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        assert_eq!(
            back.table_count(),
            0,
            "HWP3 has no table-header primitive and does not store Table IR"
        );
    }
}
