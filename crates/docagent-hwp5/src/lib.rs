//! HWP 5.x OLE/CFB codec.
//!
//! Record layout follows 한글 문서 파일 형식 5.0 revision 1.3 §4:
//! 10-bit tag, 10-bit level, 12-bit size (0xFFF → extra u32 size).
//! Tag values are HWPTAG_BEGIN (0x10) plus the spec offset.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};

use cfb::CompoundFile;
use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Color, Diagnostic, Document, InlineObject, LayoutHint,
    NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section, SplitPolicy, Table,
    TableBorders, TableCell, TableRow, Underline,
};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use thiserror::Error;

/// Spec §4.2: tag origin.
pub const HWPTAG_BEGIN: u16 = 0x10;
pub const HWPTAG_DOCUMENT_PROPERTIES: u16 = HWPTAG_BEGIN;
pub const HWPTAG_ID_MAPPINGS: u16 = HWPTAG_BEGIN + 1;
pub const HWPTAG_BIN_DATA: u16 = HWPTAG_BEGIN + 2;
pub const HWPTAG_FACE_NAME: u16 = HWPTAG_BEGIN + 3;
pub const HWPTAG_BORDER_FILL: u16 = HWPTAG_BEGIN + 4;
pub const HWPTAG_CHAR_SHAPE: u16 = HWPTAG_BEGIN + 5;
pub const HWPTAG_TAB_DEF: u16 = HWPTAG_BEGIN + 6;
pub const HWPTAG_NUMBERING: u16 = HWPTAG_BEGIN + 7;
pub const HWPTAG_BULLET: u16 = HWPTAG_BEGIN + 8;
pub const HWPTAG_PARA_SHAPE: u16 = HWPTAG_BEGIN + 9;
pub const HWPTAG_STYLE: u16 = HWPTAG_BEGIN + 10;
pub const HWPTAG_PARA_HEADER: u16 = HWPTAG_BEGIN + 50;
pub const HWPTAG_PARA_TEXT: u16 = HWPTAG_BEGIN + 51;
pub const HWPTAG_PARA_CHAR_SHAPE: u16 = HWPTAG_BEGIN + 52;
pub const HWPTAG_PARA_LINE_SEG: u16 = HWPTAG_BEGIN + 53;
pub const HWPTAG_PARA_RANGE_TAG: u16 = HWPTAG_BEGIN + 54;
pub const HWPTAG_CTRL_HEADER: u16 = HWPTAG_BEGIN + 55;
pub const HWPTAG_LIST_HEADER: u16 = HWPTAG_BEGIN + 56;
pub const HWPTAG_PAGE_DEF: u16 = HWPTAG_BEGIN + 57;
pub const HWPTAG_FOOTNOTE_SHAPE: u16 = HWPTAG_BEGIN + 58;
pub const HWPTAG_PAGE_BORDER_FILL: u16 = HWPTAG_BEGIN + 59;
pub const HWPTAG_SHAPE_COMPONENT: u16 = HWPTAG_BEGIN + 60;
pub const HWPTAG_TABLE: u16 = HWPTAG_BEGIN + 61;
pub const HWPTAG_CTRL_DATA: u16 = HWPTAG_BEGIN + 71;

const CTRL_TABLE: u32 = u32::from_be_bytes(*b"tbl ");
const CTRL_SECTION: u32 = u32::from_be_bytes(*b"secd");
const CTRL_HYPERLINK: u32 = u32::from_be_bytes(*b"%hlk");
const FILE_SIGNATURE: &[u8] = b"HWP Document File";

#[derive(Debug, Error)]
pub enum Error {
    #[error("not an HWP 5 compound file")]
    NotHwp5,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("record stream truncated")]
    Truncated,
    #[error("utf16 text")]
    Utf16,
}

#[derive(Clone, Debug)]
struct Rec {
    tag: u16,
    level: u16,
    payload: Vec<u8>,
}

pub fn sniff(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    bytes[0..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if !sniff(bytes) {
        return Err(Error::NotHwp5);
    }
    let mut comp = CompoundFile::open(Cursor::new(bytes.to_vec()))?;
    let header = read_named_stream(&mut comp, "FileHeader").unwrap_or_default();
    if header.len() < 32 || !header.starts_with(FILE_SIGNATURE) {
        return Err(Error::NotHwp5);
    }
    let flags = if header.len() >= 40 {
        u32::from_le_bytes(header[36..40].try_into().unwrap())
    } else {
        0
    };
    let compressed = flags & 1 != 0;
    let mut diagnostics = Vec::new();
    let docinfo = read_named_stream(&mut comp, "DocInfo").unwrap_or_default();
    let docinfo = inflate_hwp_stream(&docinfo, compressed);
    let info_recs = parse_records(&docinfo).unwrap_or_default();
    let catalog = StyleCatalog::from_records(&info_recs);

    let mut sections = Vec::new();
    let section_blobs = collect_body_sections(&mut comp);
    if section_blobs.is_empty() {
        diagnostics.push(Diagnostic::parse_loss("no BodyText section stream"));
    }
    for (idx, raw) in section_blobs.into_iter().enumerate() {
        let data = inflate_hwp_stream(&raw, compressed);
        let recs = match parse_records(&data) {
            Ok(recs) => recs,
            Err(_) => {
                diagnostics.push(Diagnostic::parse_loss(format!(
                    "section {idx} record stream unreadable"
                )));
                sections.push(Section::default());
                continue;
            }
        };
        sections.push(section_from_records(&recs, &catalog, &mut diagnostics));
    }
    if sections.is_empty() {
        sections.push(Section::default());
        diagnostics.push(Diagnostic::parse_loss("no BodyText section stream"));
    }
    Ok(Document {
        sections,
        diagnostics,
        ..Document::default()
    })
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let styles = collect_char_styles(doc);
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut comp = CompoundFile::create(&mut cursor)?;
        {
            let mut s = comp.create_stream("FileHeader")?;
            s.write_all(&file_header_bytes())?;
        }
        {
            let mut s = comp.create_stream("DocInfo")?;
            s.write_all(&write_docinfo(&styles))?;
        }
        if !comp.exists("BodyText") {
            comp.create_storage("BodyText")?;
        }
        for (i, section) in doc.sections.iter().enumerate() {
            let name = format!("BodyText/Section{i}");
            let bytes = write_section(section, &styles);
            let mut s = comp.create_stream(&name)?;
            s.write_all(&bytes)?;
        }
        if doc.sections.is_empty() {
            let mut s = comp.create_stream("BodyText/Section0")?;
            s.write_all(&write_section(&Section::default(), &styles))?;
        }
        {
            let mut s = comp.create_stream("PrvText")?;
            let preview: String = doc.plain_text().chars().take(256).collect();
            let utf16: Vec<u8> = preview
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            s.write_all(&utf16)?;
        }
    }
    Ok(cursor.into_inner())
}

fn read_named_stream(comp: &mut CompoundFile<Cursor<Vec<u8>>>, path: &str) -> Option<Vec<u8>> {
    for candidate in [
        path.to_string(),
        path.replace('/', "\\"),
        format!("/{path}"),
    ] {
        if !comp.exists(&candidate) {
            continue;
        }
        let mut s = comp.open_stream(&candidate).ok()?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }
    None
}

fn collect_body_sections(comp: &mut CompoundFile<Cursor<Vec<u8>>>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for idx in 0..256u32 {
        let name = format!("BodyText/Section{idx}");
        match read_named_stream(comp, &name) {
            Some(raw) => out.push(raw),
            None => break,
        }
    }
    out
}

/// Hangul 5.0 BodyText/DocInfo use raw deflate (wbits=-15); zlib wrapper is fallback.
fn inflate_hwp_stream(data: &[u8], compressed: bool) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    if !compressed {
        if looks_like_records(data) {
            return data.to_vec();
        }
        if let Some(plain) = try_inflate(data)
            && looks_like_records(&plain)
        {
            return plain;
        }
        return data.to_vec();
    }
    try_inflate(data).unwrap_or_else(|| data.to_vec())
}

fn try_inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut raw = Vec::new();
    if DeflateDecoder::new(data).read_to_end(&mut raw).is_ok() && !raw.is_empty() {
        return Some(raw);
    }
    let mut zlib = Vec::new();
    if ZlibDecoder::new(data).read_to_end(&mut zlib).is_ok() && !zlib.is_empty() {
        return Some(zlib);
    }
    None
}

fn looks_like_records(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    let header = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let tag = (header & 0x3FF) as u16;
    (HWPTAG_BEGIN..=HWPTAG_BEGIN + 90).contains(&tag)
}

fn file_header_bytes() -> Vec<u8> {
    let mut h = vec![0u8; 256];
    h[..FILE_SIGNATURE.len()].copy_from_slice(FILE_SIGNATURE);
    h[32] = 5;
    h[33] = 0;
    h[34] = 3;
    h[35] = 4;
    h
}

fn parse_records(data: &[u8]) -> Result<Vec<Rec>, Error> {
    let mut recs = Vec::new();
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let header = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
        i += 4;
        let tag = (header & 0x3FF) as u16;
        let level = ((header >> 10) & 0x3FF) as u16;
        let mut size = (header >> 20) as usize;
        if size == 0xFFF {
            if i + 4 > data.len() {
                return Err(Error::Truncated);
            }
            size = u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
        }
        if i + size > data.len() {
            return Err(Error::Truncated);
        }
        recs.push(Rec {
            tag,
            level,
            payload: data[i..i + size].to_vec(),
        });
        i += size;
    }
    Ok(recs)
}

fn write_record(out: &mut Vec<u8>, tag: u16, level: u16, payload: &[u8]) {
    let size = payload.len();
    let mut header = u32::from(tag) & 0x3FF;
    header |= (u32::from(level) & 0x3FF) << 10;
    if size >= 0xFFF {
        header |= 0xFFF << 20;
        out.extend_from_slice(&header.to_le_bytes());
        out.extend_from_slice(&(size as u32).to_le_bytes());
    } else {
        header |= (size as u32) << 20;
        out.extend_from_slice(&header.to_le_bytes());
    }
    out.extend_from_slice(payload);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StyleKind {
    Quote,
    CodeBlock,
    Thematic,
    Task,
}

fn style_kind(name: &str) -> Option<StyleKind> {
    let n = name.trim();
    if n.eq_ignore_ascii_case("Quote") || n == "인용" || n == "인용구" || n == "인용문" {
        Some(StyleKind::Quote)
    } else if n.eq_ignore_ascii_case("CodeBlock") || n.contains("코드블록") || n == "코드" {
        Some(StyleKind::CodeBlock)
    } else if n.eq_ignore_ascii_case("HorizontalLine")
        || n.contains("가로선")
        || n.contains("구분선")
    {
        Some(StyleKind::Thematic)
    } else if n.eq_ignore_ascii_case("Task") || n == "할 일" || n == "작업" {
        Some(StyleKind::Task)
    } else {
        None
    }
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

fn apply_task_paragraph(p: &mut Paragraph, from_style: bool) {
    let level = p.numbering.as_ref().map(|n| n.level).unwrap_or(0);
    let mut checked = false;
    let mut found = from_style;
    if let Some(run) = p.runs.first_mut()
        && let RunContent::Text(text) = &mut run.content
    {
        let is_checked = text.starts_with("[x] ") || text.starts_with("[X] ");
        let is_unchecked = text.starts_with("[ ] ");
        if is_checked || is_unchecked {
            *text = text[4..].to_string();
            checked = is_checked;
            found = true;
        }
    }
    if found {
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level,
            start: checked.then_some(1),
            format: Some(NumberFormat::Task),
        });
    }
}

#[derive(Clone, Debug, Default)]
struct StyleCatalog {
    chars: Vec<CharStyle>,
    para: Vec<ParsedParaShape>,
    styles: Vec<String>,
    fills: Vec<ParsedBorderFill>,
}

#[derive(Clone, Debug)]
struct ParsedParaShape {
    alignment: Alignment,
    indent_left: i32,
    indent_right: i32,
    indent_first: i32,
    line_spacing: docagent_model::LineSpacing,
    border_fill_id: u16,
}

impl Default for ParsedParaShape {
    fn default() -> Self {
        Self {
            alignment: Alignment::Start,
            indent_left: 0,
            indent_right: 0,
            indent_first: 0,
            line_spacing: docagent_model::LineSpacing::Percent(160),
            border_fill_id: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ParsedBorderFill {
    left: bool,
    bottom: bool,
    filled: bool,
}

impl StyleCatalog {
    fn from_records(recs: &[Rec]) -> Self {
        let mut cat = Self::default();
        for rec in recs {
            match rec.tag {
                HWPTAG_CHAR_SHAPE => cat.chars.push(char_shape_style(&rec.payload)),
                HWPTAG_PARA_SHAPE => cat.para.push(parse_para_shape(&rec.payload)),
                HWPTAG_STYLE => cat.styles.push(parse_style_name(&rec.payload)),
                HWPTAG_BORDER_FILL => cat.fills.push(parse_border_fill(&rec.payload)),
                _ => {}
            }
        }
        cat
    }

    fn kind_for(&self, header: &[u8], empty: bool) -> Option<StyleKind> {
        let style_id = if header.len() > 10 {
            header[10] as usize
        } else {
            0
        };
        if let Some(name) = self.styles.get(style_id)
            && let Some(kind) = style_kind(name)
        {
            return Some(kind);
        }
        let ps_id = u16_at(header, 8) as usize;
        let fill = self
            .para
            .get(ps_id)
            .and_then(|ps| self.fills.get(ps.border_fill_id as usize).copied());
        match fill {
            Some(f) if f.left && !f.bottom && !f.filled => Some(StyleKind::Quote),
            Some(f) if f.filled && !f.left && !f.bottom => Some(StyleKind::CodeBlock),
            Some(f) if f.bottom && !f.left && empty => Some(StyleKind::Thematic),
            _ => None,
        }
    }
}

/// 한글 5.0 r1.3 표 37: faces/ratio/spacing/rel/offset then i32 height, u32 attr,
/// then shadow xy, text/underline COLORREF, shade COLORREF at 60.
fn char_shape_style(p: &[u8]) -> CharStyle {
    let mut style = CharStyle::default();
    const OFF: usize = 7 * 2 + 7 * 4;
    if p.len() >= OFF + 4 {
        style.size = i32_at(p, OFF).max(1);
    }
    if p.len() >= OFF + 8 {
        let attr = u32::from_le_bytes(p[OFF + 4..OFF + 8].try_into().unwrap());
        style.italic = attr & 1 != 0;
        style.bold = attr & 2 != 0;
        if (attr >> 2) & 0x03 != 0 {
            style.underline = Underline::Single;
        }
        style.superscript = attr & (1 << 15) != 0;
        style.subscript = attr & (1 << 16) != 0;
        style.strike = (attr >> 18) & 0x07 != 0;
    }
    if p.len() >= 64 {
        let shade = u32::from_le_bytes(p[60..64].try_into().unwrap());
        style.highlight = highlight_from_colorref(shade);
    }
    style
}

fn encode_char_shape(style: &CharStyle) -> Vec<u8> {
    let mut p = vec![0u8; 72];
    for i in 0..7 {
        p[14 + i] = 100;
        p[28 + i] = 100;
    }
    put_i32(&mut p, 42, style.size.max(1));
    let mut attr = 0u32;
    if style.italic {
        attr |= 1;
    }
    if style.bold {
        attr |= 2;
    }
    if style.underline != Underline::None {
        attr |= 1 << 2;
    }
    if style.superscript {
        attr |= 1 << 15;
    }
    if style.subscript {
        attr |= 1 << 16;
    }
    if style.strike {
        attr |= 1 << 18;
    }
    p[46..50].copy_from_slice(&attr.to_le_bytes());
    let shade = match style.highlight {
        Some(c) => colorref(c),
        None => 0xFFFF_FFFF,
    };
    p[60..64].copy_from_slice(&shade.to_le_bytes());
    p
}

fn colorref(c: Color) -> u32 {
    u32::from(c.r) | (u32::from(c.g) << 8) | (u32::from(c.b) << 16)
}

fn highlight_from_colorref(v: u32) -> Option<Color> {
    if v == 0xFFFF_FFFF {
        return None;
    }
    let r = (v & 0xFF) as u8;
    let g = ((v >> 8) & 0xFF) as u8;
    let b = ((v >> 16) & 0xFF) as u8;
    if (r == 0 && g == 0 && b == 0) || (r == 255 && g == 255 && b == 255) {
        return None;
    }
    Some(Color::rgb(r, g, b))
}

fn collect_char_styles(doc: &Document) -> Vec<CharStyle> {
    let mut out = vec![CharStyle::default()];
    for section in &doc.sections {
        collect_block_styles(&section.body, &mut out);
    }
    out
}

fn collect_block_styles(blocks: &[Block], out: &mut Vec<CharStyle>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for run in &p.runs {
                    if !out.iter().any(|s| s == &run.style) {
                        out.push(run.style.clone());
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_block_styles(&cell.blocks, out);
                    }
                }
            }
            Block::Float(_) | Block::Break(_) => {}
        }
    }
}

fn style_id(styles: &[CharStyle], style: &CharStyle) -> u32 {
    styles.iter().position(|s| s == style).unwrap_or(0) as u32
}

fn parse_para_shape(p: &[u8]) -> ParsedParaShape {
    let mut out = ParsedParaShape::default();
    if p.len() < 28 {
        return out;
    }
    let attr1 = u32::from_le_bytes(p[0..4].try_into().unwrap());
    out.indent_left = i32_at(p, 4);
    out.indent_right = i32_at(p, 8);
    out.indent_first = i32_at(p, 12);
    let ls = i32_at(p, 24);
    out.line_spacing = match attr1 & 0x03 {
        1 => docagent_model::LineSpacing::Absolute(ls.max(1)),
        3 => docagent_model::LineSpacing::AtLeast(ls.max(1)),
        _ => docagent_model::LineSpacing::Percent(u16::try_from(ls.clamp(50, 500)).unwrap_or(160)),
    };
    out.alignment = match (attr1 >> 2) & 0x07 {
        2 => Alignment::End,
        3 => Alignment::Center,
        4 | 5 => Alignment::Distribute,
        0 => Alignment::Justify,
        _ => Alignment::Start,
    };
    if p.len() >= 34 {
        out.border_fill_id = u16_at(p, 32);
    }
    out
}

fn parse_style_name(p: &[u8]) -> String {
    let mut i = 0usize;
    let local = read_hwp_string(p, &mut i);
    let english = read_hwp_string(p, &mut i);
    if english.is_empty() { local } else { english }
}

fn read_hwp_string(p: &[u8], i: &mut usize) -> String {
    if *i + 2 > p.len() {
        return String::new();
    }
    let len = u16_at(p, *i) as usize;
    *i += 2;
    let mut units = Vec::with_capacity(len);
    for _ in 0..len {
        if *i + 2 > p.len() {
            break;
        }
        units.push(u16_at(p, *i));
        *i += 2;
    }
    String::from_utf16_lossy(&units)
}

fn parse_border_fill(p: &[u8]) -> ParsedBorderFill {
    let mut out = ParsedBorderFill::default();
    if p.len() < 32 {
        return out;
    }
    // Interleaved L/R/T/B: type + width + COLORREF.
    let left_ty = p[2];
    let right_ty = p.get(8).copied().unwrap_or(0);
    let top_ty = p.get(14).copied().unwrap_or(0);
    let bottom_ty = p.get(20).copied().unwrap_or(0);
    out.left = left_ty != 0 && right_ty == 0 && top_ty == 0;
    out.bottom = bottom_ty != 0 && left_ty == 0 && right_ty == 0 && top_ty == 0;
    if p.len() >= 36 {
        let fill_ty = u32::from_le_bytes(p[32..36].try_into().unwrap_or([0; 4]));
        out.filled = fill_ty & 1 != 0;
    }
    out
}

fn apply_catalog(para: &mut Paragraph, header: &[u8], children: &[Rec], cat: &StyleCatalog) {
    if header.len() >= 10 {
        let ps_id = u16_at(header, 8) as usize;
        if let Some(ps) = cat.para.get(ps_id) {
            para.alignment = ps.alignment;
            para.indent_left = ps.indent_left.max(0);
            para.indent_right = ps.indent_right.max(0);
            para.indent_first = ps.indent_first;
            para.line_spacing = ps.line_spacing;
        }
    }
    let ranges = para_char_shape_ranges(children);
    if ranges.is_empty() {
        return;
    }
    let text = para.plain_text();
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        if let Some((_, id)) = ranges.first()
            && let Some(st) = cat.chars.get(*id as usize)
        {
            for run in &mut para.runs {
                run.style = st.clone();
            }
        }
        return;
    }
    let mut runs = Vec::new();
    for (i, (start, id)) in ranges.iter().enumerate() {
        let s = (*start as usize).min(chars.len());
        let e = ranges
            .get(i + 1)
            .map(|(n, _)| *n as usize)
            .unwrap_or(chars.len())
            .min(chars.len());
        if s >= e {
            continue;
        }
        let slice: String = chars[s..e].iter().collect();
        let mut run = Run::text(slice);
        if let Some(st) = cat.chars.get(*id as usize) {
            run.style = st.clone();
        }
        runs.push(run);
    }
    if !runs.is_empty() {
        para.runs = runs;
    }
}

fn para_char_shape_ranges(children: &[Rec]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for rec in children {
        if rec.tag != HWPTAG_PARA_CHAR_SHAPE {
            continue;
        }
        let mut i = 0;
        while i + 8 <= rec.payload.len() {
            let start = u32::from_le_bytes(rec.payload[i..i + 4].try_into().unwrap());
            let id = u32::from_le_bytes(rec.payload[i + 4..i + 8].try_into().unwrap());
            out.push((start, id));
            i += 8;
        }
    }
    out
}

fn section_from_records(
    recs: &[Rec],
    catalog: &StyleCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Section {
    let mut section = Section::default();
    for rec in recs {
        if rec.tag == HWPTAG_PAGE_DEF {
            apply_page_def(&mut section, &rec.payload);
        }
    }
    let mut body = Vec::new();
    let mut i = 0usize;
    while i < recs.len() {
        match recs[i].tag {
            HWPTAG_PARA_HEADER => {
                if paragraph_is_section_def(recs, i) {
                    let end = skip_tree(recs, i);
                    body.extend(tables_in_paragraph(recs, i, catalog, diagnostics));
                    if let Block::Paragraph(p) = read_plain_paragraph(recs, i, end, catalog)
                        && !p.plain_text().trim().is_empty()
                    {
                        body.push(Block::Paragraph(p));
                    }
                    i = end;
                    continue;
                }
                let (block, next) = read_paragraph_or_control(recs, i, catalog, diagnostics);
                body.push(block);
                i = next;
            }
            HWPTAG_CTRL_HEADER => {
                if matches_ctrl(&recs[i].payload, CTRL_SECTION) {
                    i = skip_tree(recs, i);
                    continue;
                }
                let (block, next) = read_control(recs, i, catalog, diagnostics);
                if let Some(b) = block {
                    body.push(b);
                }
                i = next;
            }
            _ => {
                i += 1;
            }
        }
    }
    section.body = body;
    section
}

fn apply_page_def(section: &mut Section, p: &[u8]) {
    if p.len() < 36 {
        return;
    }
    section.page.width = i32_at(p, 0);
    section.page.height = i32_at(p, 4);
    section.page.margin_left = i32_at(p, 8);
    section.page.margin_right = i32_at(p, 12);
    section.page.margin_top = i32_at(p, 16);
    section.page.margin_bottom = i32_at(p, 20);
    section.page.header_distance = i32_at(p, 24);
    section.page.footer_distance = i32_at(p, 28);
    section.page.gutter = i32_at(p, 32);
}

fn skip_tree(recs: &[Rec], start: usize) -> usize {
    let level = recs[start].level;
    let mut end = start + 1;
    while end < recs.len() && recs[end].level > level {
        end += 1;
    }
    end
}

fn paragraph_is_section_def(recs: &[Rec], start: usize) -> bool {
    let level = recs[start].level;
    recs[start + 1..]
        .iter()
        .take_while(|r| r.level > level)
        .any(|r| r.tag == HWPTAG_CTRL_HEADER && matches_ctrl(&r.payload, CTRL_SECTION))
}

fn matches_ctrl(payload: &[u8], want: u32) -> bool {
    if payload.len() < 4 {
        return false;
    }
    let raw = u32::from_le_bytes(payload[0..4].try_into().unwrap());
    raw == want || raw.swap_bytes() == want
}

fn tables_in_paragraph(
    recs: &[Rec],
    start: usize,
    catalog: &StyleCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Block> {
    let level = recs[start].level;
    let mut out = Vec::new();
    let mut j = start + 1;
    while j < recs.len() && recs[j].level > level {
        if recs[j].tag == HWPTAG_CTRL_HEADER
            && recs[j].level == level + 1
            && matches_ctrl(&recs[j].payload, CTRL_TABLE)
        {
            let (block, next) = read_control(recs, j, catalog, diagnostics);
            if let Some(b) = block {
                out.push(b);
            }
            j = next;
            continue;
        }
        j += 1;
    }
    out
}

fn read_paragraph_or_control(
    recs: &[Rec],
    start: usize,
    catalog: &StyleCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Block, usize) {
    let level = recs[start].level;
    let mut end = start + 1;
    while end < recs.len() && recs[end].level > level {
        end += 1;
    }
    let children = &recs[start + 1..end];
    if let Some(ctrl_idx) = children
        .iter()
        .position(|r| r.tag == HWPTAG_CTRL_HEADER && matches_ctrl(&r.payload, CTRL_TABLE))
    {
        let table = read_table(children, ctrl_idx, catalog, diagnostics);
        return (Block::Table(table), end);
    }
    if children
        .iter()
        .any(|r| r.tag == HWPTAG_CTRL_HEADER && matches_ctrl(&r.payload, CTRL_SECTION))
    {
        return (read_plain_paragraph(recs, start, end, catalog), end);
    }
    (read_plain_paragraph(recs, start, end, catalog), end)
}

fn read_plain_paragraph(recs: &[Rec], start: usize, end: usize, catalog: &StyleCatalog) -> Block {
    let mut text = String::new();
    let mut hints = Vec::new();
    let mut text_payload: Option<&[u8]> = None;
    for rec in &recs[start..end] {
        match rec.tag {
            HWPTAG_PARA_TEXT => {
                text.push_str(&decode_para_text(&rec.payload));
                text_payload = Some(&rec.payload);
            }
            HWPTAG_PARA_LINE_SEG => {
                hints.extend(decode_lineseg(&rec.payload));
            }
            _ => {}
        }
    }
    let mut para = Paragraph::from_text(text);
    para.layout_hints = hints;
    apply_catalog(
        &mut para,
        &recs[start].payload,
        &recs[start + 1..end],
        catalog,
    );
    if let Some(payload) = text_payload {
        let urls = hyperlink_commands(&recs[start..end]);
        apply_hyperlinks(&mut para, payload, &urls);
    }
    match catalog.kind_for(&recs[start].payload, para.plain_text().trim().is_empty()) {
        Some(StyleKind::Thematic) => Block::Break(BreakKind::Thematic),
        kind => {
            para.quote = kind == Some(StyleKind::Quote);
            para.code_block = kind == Some(StyleKind::CodeBlock);
            apply_task_paragraph(&mut para, kind == Some(StyleKind::Task));
            Block::Paragraph(para)
        }
    }
}

fn read_control(
    recs: &[Rec],
    start: usize,
    catalog: &StyleCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Option<Block>, usize) {
    let level = recs[start].level;
    let mut end = start + 1;
    while end < recs.len() && recs[end].level > level {
        end += 1;
    }
    if matches_ctrl(&recs[start].payload, CTRL_TABLE) {
        let table = read_table(&recs[start..end], 0, catalog, diagnostics);
        return (Some(Block::Table(table)), end);
    }
    if ctrl_id(&recs[start].payload) == u32::from_be_bytes(*b"    ") {
        diagnostics.push(Diagnostic::unsupported("empty control id"));
    }
    (None, end)
}

fn read_table(
    recs: &[Rec],
    ctrl_at: usize,
    catalog: &StyleCatalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Table {
    let mut n_rows = 0u16;
    let mut n_cols = 0u16;
    let mut widths = Vec::new();
    for rec in recs {
        if rec.tag == HWPTAG_TABLE && rec.payload.len() >= 4 {
            let (rows, cols, cols_w) = table_geometry(&rec.payload);
            n_rows = rows;
            n_cols = cols;
            widths = cols_w;
        }
    }
    if n_rows == 0 || n_cols == 0 {
        diagnostics.push(Diagnostic::parse_loss("table missing row/col counts"));
        n_rows = n_rows.max(1);
        n_cols = n_cols.max(1);
    }
    let mut cell_heights = Vec::new();
    let mut cell_widths = Vec::new();
    for rec in recs {
        if rec.tag == HWPTAG_LIST_HEADER && rec.payload.len() >= 14 {
            let w = i32_at(&rec.payload, 6);
            let h = i32_at(&rec.payload, 10);
            if w > 0 {
                cell_widths.push(w);
            }
            if h > 0 && h < 20_000 {
                cell_heights.push(h);
            }
        }
    }
    if widths.is_empty() {
        widths = cell_widths;
    }
    let mut paras = Vec::new();
    let mut i = ctrl_at;
    while i < recs.len() {
        if recs[i].tag == HWPTAG_PARA_HEADER {
            let (block, next) = read_paragraph_or_control(recs, i, catalog, diagnostics);
            match block {
                Block::Paragraph(p) => paras.push(p),
                Block::Break(_) => paras.push(Paragraph::from_text("")),
                _ => {}
            }
            i = next;
        } else {
            i += 1;
        }
    }
    let total = (n_rows as usize).saturating_mul(n_cols as usize);
    while paras.len() < total {
        paras.push(Paragraph::from_text(""));
    }
    let mut rows = Vec::new();
    let mut iter = paras.into_iter();
    let mut hi = 0usize;
    for _ in 0..n_rows {
        let mut cells = Vec::new();
        let mut row_h = None;
        for c in 0..n_cols as usize {
            let para = iter.next().unwrap_or_else(|| Paragraph::from_text(""));
            if let Some(h) = cell_heights.get(hi).copied() {
                row_h = Some(row_h.unwrap_or(0).max(h));
            }
            hi += 1;
            cells.push(TableCell {
                width: *widths.get(c).unwrap_or(&10000),
                blocks: vec![Block::Paragraph(para)],
                ..TableCell::default()
            });
        }
        rows.push(TableRow {
            cells,
            height: row_h,
            header: false,
            cant_split: None,
        });
    }
    Table {
        rows,
        borders: TableBorders::default(),
        split_policy: SplitPolicy::Allow,
        width: None,
        alignment: Alignment::Start,
        cell_spacing: 0,
        indent: 0,
        header_row_count: 0,
        layout_hints: Vec::new(),
    }
}

fn decode_para_text(payload: &[u8]) -> String {
    if payload.len() < 2 {
        return String::new();
    }
    let mut units = Vec::new();
    let mut i = 0;
    while i + 1 < payload.len() {
        let u = u16::from_le_bytes([payload[i], payload[i + 1]]);
        i += 2;
        if u == 0x000B {
            i = i.saturating_add(14);
            continue;
        }
        if (1..32).contains(&u) && u != 0x0009 && u != 0x000A && u != 0x000D {
            if matches!(u, 0x0008 | 0x0002 | 0x0003 | 0x0004) {
                i = i.saturating_add(14);
            }
            continue;
        }
        if u == 0x000D {
            continue;
        }
        units.push(u);
    }
    String::from_utf16(&units).unwrap_or_else(|_| String::from_utf16_lossy(&units))
}

fn decode_lineseg(payload: &[u8]) -> Vec<LayoutHint> {
    const SIZE: usize = 36;
    let mut out = Vec::new();
    let mut i = 0;
    while i + SIZE <= payload.len() {
        let p = &payload[i..i + SIZE];
        out.push(LayoutHint {
            text_start: u32::from_le_bytes(p[0..4].try_into().unwrap()),
            y: i32_at(p, 4),
            line_height: i32_at(p, 8),
            text_height: i32_at(p, 12),
            baseline: i32_at(p, 16),
            spacing: i32_at(p, 20),
            x: i32_at(p, 24),
            width: i32_at(p, 28),
            flags: u32::from_le_bytes(p[32..36].try_into().unwrap()),
        });
        i += SIZE;
    }
    out
}

fn encode_lineseg(hints: &[LayoutHint]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hints.len() * 36);
    for h in hints {
        out.extend_from_slice(&h.text_start.to_le_bytes());
        out.extend_from_slice(&h.y.to_le_bytes());
        out.extend_from_slice(&h.line_height.to_le_bytes());
        out.extend_from_slice(&h.text_height.to_le_bytes());
        out.extend_from_slice(&h.baseline.to_le_bytes());
        out.extend_from_slice(&h.spacing.to_le_bytes());
        out.extend_from_slice(&h.x.to_le_bytes());
        out.extend_from_slice(&h.width.to_le_bytes());
        out.extend_from_slice(&h.flags.to_le_bytes());
    }
    out
}

/// Hangul 5.0 TABLE: UINT32 attr, UINT16 rows, UINT16 cols, then optional
/// cell-spacing / margins / row-size array. Self-authored files store
/// rows, cols, then i32 column widths.
fn table_geometry(p: &[u8]) -> (u16, u16, Vec<i32>) {
    let spec_rows = u16_at(p, 4);
    let spec_cols = u16_at(p, 6);
    if p.len() >= 18 && (1..128).contains(&spec_rows) && (1..64).contains(&spec_cols) {
        return (spec_rows, spec_cols, Vec::new());
    }
    let rows = u16_at(p, 0);
    let cols = u16_at(p, 2);
    let mut widths = Vec::new();
    let mut off = 4;
    while off + 4 <= p.len() && widths.len() < cols as usize {
        widths.push(i32_at(p, off));
        off += 4;
    }
    (rows, cols, widths)
}

fn ctrl_id(payload: &[u8]) -> u32 {
    if payload.len() < 4 {
        return 0;
    }
    u32::from_le_bytes(payload[0..4].try_into().unwrap()).swap_bytes()
}

fn push_extended_ctrl(units: &mut Vec<u16>, code: u16, id: u32) {
    units.push(code);
    let b = id.to_le_bytes();
    units.push(u16::from_le_bytes([b[0], b[1]]));
    units.push(u16::from_le_bytes([b[2], b[3]]));
    units.extend_from_slice(&[0, 0, 0, 0]);
    units.push(code);
}

fn write_hyperlink_ctrl(out: &mut Vec<u8>, level: u16, url: &str) {
    let cmd: Vec<u16> = url.encode_utf16().collect();
    let mut data = Vec::with_capacity(15 + cmd.len() * 2);
    data.extend_from_slice(&CTRL_HYPERLINK.swap_bytes().to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.push(0);
    data.extend_from_slice(&(cmd.len() as u16).to_le_bytes());
    for u in cmd {
        data.extend_from_slice(&u.to_le_bytes());
    }
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    write_record(out, HWPTAG_CTRL_HEADER, level, &data);
}

fn hyperlink_uri(command: &str) -> String {
    command
        .split(';')
        .next()
        .unwrap_or(command)
        .trim()
        .to_string()
}

fn parse_hyperlink_command(payload: &[u8]) -> Option<String> {
    if !matches_ctrl(payload, CTRL_HYPERLINK) || payload.len() < 11 {
        return None;
    }
    let cmd_len = u16::from_le_bytes(payload[9..11].try_into().ok()?) as usize;
    let start: usize = 11;
    let end = start.saturating_add(cmd_len.saturating_mul(2));
    if end > payload.len() {
        return None;
    }
    let mut chars = Vec::with_capacity(cmd_len);
    let mut i = start;
    while i + 1 < end {
        chars.push(u16::from_le_bytes([payload[i], payload[i + 1]]));
        i += 2;
    }
    let url = hyperlink_uri(&String::from_utf16_lossy(&chars));
    if url.is_empty() { None } else { Some(url) }
}

fn hyperlink_commands(recs: &[Rec]) -> Vec<String> {
    recs.iter()
        .filter(|r| r.tag == HWPTAG_CTRL_HEADER)
        .filter_map(|r| parse_hyperlink_command(&r.payload))
        .collect()
}

fn decode_field_spans(payload: &[u8]) -> Vec<(usize, usize)> {
    let mut units = Vec::new();
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i + 1 < payload.len() {
        let u = u16::from_le_bytes([payload[i], payload[i + 1]]);
        i += 2;
        if u == 0x000B {
            i = i.saturating_add(14);
            continue;
        }
        if (1..32).contains(&u) && u != 0x0009 && u != 0x000A && u != 0x000D {
            if u == 0x0003 {
                i = i.saturating_add(14);
                start = Some(units.len());
                continue;
            }
            if u == 0x0004 {
                i = i.saturating_add(14);
                if let Some(s) = start.take() {
                    let e = units.len();
                    if e > s {
                        spans.push((s, e));
                    }
                }
                continue;
            }
            if matches!(u, 0x0008 | 0x0002) {
                i = i.saturating_add(14);
            }
            continue;
        }
        if u == 0x000D {
            continue;
        }
        units.push(u);
    }
    spans
        .into_iter()
        .map(|(s, e)| {
            let cs = String::from_utf16_lossy(&units[..s.min(units.len())])
                .chars()
                .count();
            let ce = String::from_utf16_lossy(&units[..e.min(units.len())])
                .chars()
                .count();
            (cs, ce)
        })
        .collect()
}

fn apply_hyperlinks(para: &mut Paragraph, payload: &[u8], urls: &[String]) {
    let spans = decode_field_spans(payload);
    if spans.is_empty() || urls.is_empty() {
        return;
    }
    let nchars = para.plain_text().chars().count();
    let mut char_href = vec![None; nchars];
    for (i, (s, e)) in spans.iter().enumerate() {
        if urls.get(i).is_none_or(|u| u.is_empty()) {
            continue;
        }
        let end = (*e).min(char_href.len());
        let start = (*s).min(end);
        for slot in &mut char_href[start..end] {
            *slot = Some(i);
        }
    }
    if char_href.iter().all(Option::is_none) {
        return;
    }
    let mut pieces = Vec::new();
    for run in &para.runs {
        let style = run.style.clone();
        for ch in run.display_text().chars() {
            let href = char_href.get(pieces.len()).copied().flatten();
            pieces.push((ch, style.clone(), href));
        }
    }
    if pieces.is_empty() {
        return;
    }
    let mut runs = Vec::new();
    let mut i = 0;
    while i < pieces.len() {
        let href = pieces[i].2;
        let style = pieces[i].1.clone();
        let mut j = i + 1;
        while j < pieces.len() && pieces[j].2 == href && pieces[j].1 == style {
            j += 1;
        }
        let text: String = pieces[i..j].iter().map(|p| p.0).collect();
        let mut run = if let Some(idx) = href {
            Run::hyperlink(text, urls[idx].clone())
        } else {
            Run::text(text)
        };
        run.style = style;
        if href.is_some() {
            run.style.underline = Underline::Single;
        }
        runs.push(run);
        i = j;
    }
    para.runs = runs;
}

fn write_docinfo(styles: &[CharStyle]) -> Vec<u8> {
    let mut out = Vec::new();
    // 한글 5.0 DocInfo: 7×u16 시작번호. rhwp `parse_document_properties` 계약.
    let mut props = Vec::new();
    for _ in 0..7 {
        props.extend_from_slice(&1u16.to_le_bytes());
    }
    write_record(&mut out, HWPTAG_DOCUMENT_PROPERTIES, 0, &props);
    // Spec 표 16 INT32[18]: 8 border, 9 char, 13 para, 14 style. Index 3 kept for
    // this crate's earlier CHAR_SHAPE slot.
    let mut map = vec![0u8; 72];
    let n = styles.len() as u32;
    map[12..16].copy_from_slice(&n.to_le_bytes());
    map[32..36].copy_from_slice(&4u32.to_le_bytes());
    map[36..40].copy_from_slice(&n.to_le_bytes());
    map[52..56].copy_from_slice(&5u32.to_le_bytes());
    map[56..60].copy_from_slice(&5u32.to_le_bytes());
    write_record(&mut out, HWPTAG_ID_MAPPINGS, 0, &map);
    write_record(
        &mut out,
        HWPTAG_BORDER_FILL,
        0,
        &encode_border_fill(false, false, None),
    );
    write_record(
        &mut out,
        HWPTAG_BORDER_FILL,
        0,
        &encode_border_fill(true, false, None),
    );
    write_record(
        &mut out,
        HWPTAG_BORDER_FILL,
        0,
        &encode_border_fill(false, false, Some(0x00F1_FBCC)),
    );
    write_record(
        &mut out,
        HWPTAG_BORDER_FILL,
        0,
        &encode_border_fill(false, true, None),
    );
    for style in styles {
        write_record(&mut out, HWPTAG_CHAR_SHAPE, 0, &encode_char_shape(style));
    }
    write_record(
        &mut out,
        HWPTAG_PARA_SHAPE,
        0,
        &encode_para_shape(0, 0, 0, 0),
    );
    write_record(
        &mut out,
        HWPTAG_PARA_SHAPE,
        0,
        &encode_para_shape(1400, 1, 0, 0),
    );
    write_record(
        &mut out,
        HWPTAG_PARA_SHAPE,
        0,
        &encode_para_shape(0, 2, 0, 0),
    );
    write_record(
        &mut out,
        HWPTAG_PARA_SHAPE,
        0,
        &encode_para_shape(0, 3, 400, 400),
    );
    write_record(
        &mut out,
        HWPTAG_PARA_SHAPE,
        0,
        &encode_para_shape(0, 0, 0, 0),
    );
    for (i, name) in ["Normal", "Quote", "CodeBlock", "HorizontalLine", "Task"]
        .iter()
        .enumerate()
    {
        write_record(
            &mut out,
            HWPTAG_STYLE,
            0,
            &encode_style(name, name, i as u16),
        );
    }
    out
}

fn encode_hwp_string(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = Vec::with_capacity(2 + units.len() * 2);
    out.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

fn encode_style(local: &str, english: &str, para_shape_id: u16) -> Vec<u8> {
    let mut p = encode_hwp_string(local);
    p.extend(encode_hwp_string(english));
    p.push(0);
    p.push(0);
    p.extend_from_slice(&1042i16.to_le_bytes());
    p.extend_from_slice(&para_shape_id.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p
}

fn encode_para_shape(indent_left: i32, border_fill_id: u16, before: i32, after: i32) -> Vec<u8> {
    let mut p = vec![0u8; 54];
    // attr1 bits 2–4 = 1 → left/start alignment.
    p[0..4].copy_from_slice(&4u32.to_le_bytes());
    p[4..8].copy_from_slice(&indent_left.to_le_bytes());
    p[16..20].copy_from_slice(&before.to_le_bytes());
    p[20..24].copy_from_slice(&after.to_le_bytes());
    p[24..28].copy_from_slice(&160i32.to_le_bytes());
    p[32..34].copy_from_slice(&border_fill_id.to_le_bytes());
    p[50..54].copy_from_slice(&160u32.to_le_bytes());
    p
}

fn encode_border_fill(left: bool, bottom: bool, fill: Option<u32>) -> Vec<u8> {
    let mut p = vec![0u8; 2];
    let sides = [left, false, false, bottom];
    for on in sides {
        p.push(u8::from(on));
        p.push(if on { 9 } else { 0 });
        let color = if on { 0x004A_4E13u32 } else { 0 };
        p.extend_from_slice(&color.to_le_bytes());
    }
    p.push(0);
    p.push(0);
    p.extend_from_slice(&0u32.to_le_bytes());
    if let Some(rgb) = fill {
        p.extend_from_slice(&1u32.to_le_bytes());
        p.extend_from_slice(&rgb.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&0i32.to_le_bytes());
    } else {
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
    }
    p
}

fn write_section(section: &Section, styles: &[CharStyle]) -> Vec<u8> {
    let mut out = Vec::new();
    write_record(
        &mut out,
        HWPTAG_PAGE_DEF,
        0,
        &page_def_payload(&section.page),
    );
    if section.body.is_empty() {
        write_paragraph(&mut out, &Paragraph::from_text(""), 0, styles);
    } else {
        for block in &section.body {
            match block {
                Block::Paragraph(p) => write_paragraph(&mut out, p, 0, styles),
                Block::Table(t) => write_table(&mut out, t, 0, styles),
                Block::Break(BreakKind::Thematic) => {
                    write_paragraph_role(&mut out, &Paragraph::from_text(""), 0, styles, 3);
                }
                Block::Float(_) | Block::Break(_) => {
                    write_paragraph(&mut out, &Paragraph::from_text(""), 0, styles);
                }
            }
        }
    }
    out
}

fn page_def_payload(page: &docagent_model::PageSetup) -> Vec<u8> {
    let mut p = vec![0u8; 40];
    put_i32(&mut p, 0, page.width);
    put_i32(&mut p, 4, page.height);
    put_i32(&mut p, 8, page.margin_left);
    put_i32(&mut p, 12, page.margin_right);
    put_i32(&mut p, 16, page.margin_top);
    put_i32(&mut p, 20, page.margin_bottom);
    put_i32(&mut p, 24, page.header_distance);
    put_i32(&mut p, 28, page.footer_distance);
    put_i32(&mut p, 32, page.gutter);
    p
}

fn para_style_id(p: &Paragraph) -> u16 {
    if p.code_block {
        2
    } else if p.quote {
        1
    } else if task_prefix(p).is_some() {
        4
    } else {
        0
    }
}

fn write_paragraph(out: &mut Vec<u8>, para: &Paragraph, level: u16, styles: &[CharStyle]) {
    write_paragraph_role(out, para, level, styles, para_style_id(para));
}

fn write_paragraph_role(
    out: &mut Vec<u8>,
    para: &Paragraph,
    level: u16,
    styles: &[CharStyle],
    role: u16,
) {
    let mut utf16 = Vec::new();
    let mut ranges = Vec::new();
    let mut pos = 0u32;
    let mut links = Vec::new();
    let mut pending_marker = task_prefix(para);
    for run in &para.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                let n = display.encode_utf16().count() as u32;
                if n > 0 {
                    ranges.push((pos, style_id(styles, &run.style)));
                    pos = pos.saturating_add(n);
                }
                push_extended_ctrl(&mut utf16, 0x0003, CTRL_HYPERLINK);
                utf16.extend(display.encode_utf16());
                push_extended_ctrl(&mut utf16, 0x0004, CTRL_HYPERLINK);
                if !target.is_empty() {
                    links.push(target.clone());
                }
            }
            _ => {
                let displayed = run.display_text();
                let t = match pending_marker.take() {
                    Some(m) => format!("{m}{displayed}"),
                    None => displayed.to_string(),
                };
                let n = t.encode_utf16().count() as u32;
                if n == 0 {
                    continue;
                }
                ranges.push((pos, style_id(styles, &run.style)));
                pos = pos.saturating_add(n);
                utf16.extend(t.encode_utf16());
            }
        }
    }
    if let Some(m) = pending_marker {
        ranges.push((pos, 0));
        utf16.extend(m.encode_utf16());
    }
    let nchars = utf16.len() as u32;
    if ranges.is_empty() && nchars > 0 {
        ranges.push((0, 0));
    }
    let mut header = vec![0u8; 22];
    header[0..4].copy_from_slice(&nchars.to_le_bytes());
    header[8..10].copy_from_slice(&role.to_le_bytes());
    header[10] = role.min(255) as u8;
    let char_shapes = ranges.len() as u16;
    header[12..14].copy_from_slice(&char_shapes.to_le_bytes());
    let line_count = para.layout_hints.len() as u16;
    header[16..18].copy_from_slice(&line_count.to_le_bytes());
    write_record(out, HWPTAG_PARA_HEADER, level, &header);
    let mut bytes = Vec::with_capacity(utf16.len() * 2);
    for u in utf16 {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    write_record(out, HWPTAG_PARA_TEXT, level + 1, &bytes);
    if !ranges.is_empty() {
        let mut cs = Vec::with_capacity(ranges.len() * 8);
        for (start, id) in ranges {
            cs.extend_from_slice(&start.to_le_bytes());
            cs.extend_from_slice(&id.to_le_bytes());
        }
        write_record(out, HWPTAG_PARA_CHAR_SHAPE, level + 1, &cs);
    }
    if !para.layout_hints.is_empty() {
        write_record(
            out,
            HWPTAG_PARA_LINE_SEG,
            level + 1,
            &encode_lineseg(&para.layout_hints),
        );
    }
    for url in links {
        write_hyperlink_ctrl(out, level + 1, &url);
    }
}

fn write_table(out: &mut Vec<u8>, table: &Table, level: u16, styles: &[CharStyle]) {
    let n_rows = table.rows.len() as u16;
    let n_cols = table
        .rows
        .first()
        .map(|r| r.cells.len() as u16)
        .unwrap_or(0);
    let mut header = vec![0u8; 22];
    header[0..4].copy_from_slice(&8u32.to_le_bytes());
    header[4..8].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    write_record(out, HWPTAG_PARA_HEADER, level, &header);
    let mut text = Vec::new();
    text.extend_from_slice(&0x000Bu16.to_le_bytes());
    for b in b"tbl " {
        text.extend_from_slice(&u16::from(*b).to_le_bytes());
    }
    text.extend_from_slice(&0u16.to_le_bytes());
    text.extend_from_slice(&0u16.to_le_bytes());
    text.extend_from_slice(&0x000Bu16.to_le_bytes());
    write_record(out, HWPTAG_PARA_TEXT, level + 1, &text);

    let mut ctrl = vec![0u8; 4];
    ctrl.copy_from_slice(&CTRL_TABLE.swap_bytes().to_le_bytes());
    write_record(out, HWPTAG_CTRL_HEADER, level + 1, &ctrl);

    let mut tbl = Vec::new();
    tbl.extend_from_slice(&n_rows.to_le_bytes());
    tbl.extend_from_slice(&n_cols.to_le_bytes());
    if let Some(row) = table.rows.first() {
        for cell in &row.cells {
            tbl.extend_from_slice(&cell.width.to_le_bytes());
        }
    }
    write_record(out, HWPTAG_TABLE, level + 2, &tbl);

    for row in &table.rows {
        for cell in &row.cells {
            let mut list = vec![0u8; 14];
            list[0..2].copy_from_slice(&1u16.to_le_bytes());
            put_i32(&mut list, 6, cell.width);
            put_i32(&mut list, 10, row.height.unwrap_or(0));
            write_record(out, HWPTAG_LIST_HEADER, level + 2, &list);
            let text = cell_text(cell);
            write_paragraph(out, &Paragraph::from_text(text), level + 3, styles);
        }
    }
}

fn cell_text(cell: &TableCell) -> String {
    let mut s = String::new();
    for b in &cell.blocks {
        if let Block::Paragraph(p) = b {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(&p.plain_text());
        }
    }
    s
}

fn i32_at(p: &[u8], off: usize) -> i32 {
    if off + 4 > p.len() {
        return 0;
    }
    i32::from_le_bytes(p[off..off + 4].try_into().unwrap())
}

fn u16_at(p: &[u8], off: usize) -> u16 {
    if off + 2 > p.len() {
        return 0;
    }
    u16::from_le_bytes(p[off..off + 2].try_into().unwrap())
}

fn put_i32(p: &mut [u8], off: usize, v: i32) {
    p[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_values_match_hancom_spec() {
        assert_eq!(HWPTAG_PARA_HEADER, 66);
        assert_eq!(HWPTAG_PARA_TEXT, 67);
        assert_eq!(HWPTAG_PARA_LINE_SEG, 69);
        assert_eq!(HWPTAG_TABLE, 77);
    }

    #[test]
    fn write_read_preserves_paragraph_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Paragraph(Paragraph::from_text(
            "한글 HWP5 roundtrip",
        )));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        assert!(back.plain_text().contains("한글 HWP5 roundtrip"));
    }

    #[test]
    fn char_shape_style_reads_attr_bits() {
        let bold = CharStyle {
            bold: true,
            ..CharStyle::default()
        };
        let p = encode_char_shape(&bold);
        let back = char_shape_style(&p);
        assert!(back.bold);
        assert!(!back.italic);
        let strike = CharStyle {
            strike: true,
            italic: true,
            underline: Underline::Single,
            superscript: true,
            ..CharStyle::default()
        };
        let p = encode_char_shape(&strike);
        let back = char_shape_style(&p);
        assert!(back.strike && back.italic && back.superscript);
        assert_eq!(back.underline, Underline::Single);
        assert!(!back.bold);
        let sub = CharStyle {
            subscript: true,
            ..CharStyle::default()
        };
        let back = char_shape_style(&encode_char_shape(&sub));
        assert!(back.subscript);
        assert!(!back.superscript);
        let mark = CharStyle {
            highlight: Some(Color::MARK),
            ..CharStyle::default()
        };
        let p = encode_char_shape(&mark);
        let shade = u32::from_le_bytes(p[60..64].try_into().unwrap());
        assert_ne!(shade, 0);
        assert_ne!(shade, 0x00FF_FFFF);
        let back = char_shape_style(&p);
        assert_eq!(back.highlight, Some(Color::MARK));
        assert!(!back.bold);
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
    fn roundtrip_keeps_char_shape_marks() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        let mut strike = Run::text(" guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.strike
                && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("guess"))
        }));
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
        let back = roundtrip(&doc).expect("roundtrip");
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
    fn lineseg_is_preserved_as_hint() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut para = Paragraph::from_text("hinted");
        para.layout_hints.push(LayoutHint {
            text_start: 0,
            x: 0,
            y: 1000,
            width: 20000,
            line_height: 1600,
            text_height: 1000,
            baseline: 800,
            spacing: 600,
            flags: 0,
        });
        section.body.push(Block::Paragraph(para));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(p.layout_hints.len(), 1);
        assert_eq!(p.layout_hints[0].y, 1000);
        assert_eq!(p.layout_hints[0].width, 20000);
    }

    #[test]
    fn table_roundtrip_keeps_cell_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(Table::from_cells(vec![
            vec!["A".into(), "B".into()],
            vec!["C".into(), "D".into()],
        ])));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        let text = back.plain_text();
        assert!(text.contains('A'));
        assert!(text.contains('D'));
        assert_eq!(back.table_count(), 1);
    }

    #[test]
    fn garbage_bytes_are_rejected() {
        assert!(read(b"not a compound file").is_err());
    }

    #[test]
    fn parser_does_not_panic_on_arbitrary_bytes() {
        for n in 0..64 {
            let mut buf = vec![0u8; n];
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (i.wrapping_mul(37) % 256) as u8;
            }
            let _ = read(&buf);
        }
        let mut long = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        long.extend_from_slice(&[0u8; 2048]);
        let _ = read(&long);
    }

    #[test]
    fn hangul_office_file_yields_body_text_and_lineseg() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir)
            .join("samples")
            .join("basic")
            .join("pau-004.hwp");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let doc = read(&bytes).expect("parse Hangul HWP5");
        assert!(
            !doc.plain_text().trim().is_empty(),
            "body empty; diagnostics={:?}",
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
        assert!(hints > 0, "PARA_LINE_SEG must be preserved as LayoutHint");
        assert!(
            !doc.diagnostics
                .iter()
                .any(|d| d.code == docagent_model::DiagnosticCode::ParseLoss),
            "unexpected parse loss: {:?}",
            doc.diagnostics
        );
        let page = &doc.sections[0].page;
        assert!(
            page.margin_top >= 1000 && page.width >= 10_000,
            "PAGE_DEF must be applied from nested secd; page={page:?}"
        );
        let hint = doc.sections[0]
            .body
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) => p.layout_hints.first().cloned(),
                _ => None,
            })
            .expect("first LineSeg");
        assert_eq!(hint.y, 0, "Hangul PARA_LINE_SEG vertpos is body-relative");
        assert!(hint.line_height > 0);
    }

    #[test]
    fn hangul_english_file_yields_a_table() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir)
            .join("samples")
            .join("basic")
            .join("english.hwp");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let doc = read(&bytes).expect("parse english.hwp");
        assert!(
            doc.table_count() >= 1,
            "Hangul table next to secd must become IR Table, body={}",
            doc.sections[0].body.len()
        );
        assert!(!doc.plain_text().trim().is_empty());
    }

    #[test]
    fn table_row_height_roundtrips() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["A".into(), "B".into()]]);
        table.rows[0].height = Some(4000);
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("expected table");
        };
        assert_eq!(t.rows[0].height, Some(4000));
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
        let quote: Vec<u8> = "Quote"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(
            bytes.windows(quote.len()).any(|w| w == quote),
            "DocInfo STYLE Quote"
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
        let name: Vec<u8> = "CodeBlock"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(
            bytes.windows(name.len()).any(|w| w == name),
            "DocInfo STYLE CodeBlock"
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
        let name: Vec<u8> = "HorizontalLine"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(
            bytes.windows(name.len()).any(|w| w == name),
            "DocInfo STYLE HorizontalLine"
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
        let name: Vec<u8> = "Task"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert!(
            bytes.windows(name.len()).any(|w| w == name),
            "DocInfo STYLE Task"
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
}
