//! HWP 5.x OLE/CFB codec.
//!
//! Record layout follows 한글 문서 파일 형식 5.0 revision 1.3 §4:
//! 10-bit tag, 10-bit level, 12-bit size (0xFFF → extra u32 size).
//! Tag values are HWPTAG_BEGIN (0x10) plus the spec offset.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};

use cfb::CompoundFile;
use docagent_model::{
    Alignment, Block, Diagnostic, Document, LayoutHint, Paragraph, Section, SplitPolicy, Table,
    TableBorders, TableCell, TableRow,
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
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut comp = CompoundFile::create(&mut cursor)?;
        {
            let mut s = comp.create_stream("FileHeader")?;
            s.write_all(&file_header_bytes())?;
        }
        {
            let mut s = comp.create_stream("DocInfo")?;
            s.write_all(&write_docinfo())?;
        }
        if !comp.exists("BodyText") {
            comp.create_storage("BodyText")?;
        }
        for (i, section) in doc.sections.iter().enumerate() {
            let name = format!("BodyText/Section{i}");
            let bytes = write_section(section);
            let mut s = comp.create_stream(&name)?;
            s.write_all(&bytes)?;
        }
        if doc.sections.is_empty() {
            let mut s = comp.create_stream("BodyText/Section0")?;
            s.write_all(&write_section(&Section::default()))?;
        }
        {
            let mut s = comp.create_stream("PrvText")?;
            let preview: String = doc.plain_text().chars().take(256).collect();
            let utf16: Vec<u8> = preview.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
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

#[derive(Clone, Debug, Default)]
struct StyleCatalog {
    char_sizes: Vec<i32>,
    para: Vec<ParsedParaShape>,
}

#[derive(Clone, Debug)]
struct ParsedParaShape {
    alignment: Alignment,
    indent_left: i32,
    indent_right: i32,
    indent_first: i32,
    line_spacing: docagent_model::LineSpacing,
}

impl Default for ParsedParaShape {
    fn default() -> Self {
        Self {
            alignment: Alignment::Start,
            indent_left: 0,
            indent_right: 0,
            indent_first: 0,
            line_spacing: docagent_model::LineSpacing::Percent(160),
        }
    }
}

impl StyleCatalog {
    fn from_records(recs: &[Rec]) -> Self {
        let mut cat = Self::default();
        for rec in recs {
            match rec.tag {
                HWPTAG_CHAR_SHAPE => cat.char_sizes.push(char_shape_size(&rec.payload)),
                HWPTAG_PARA_SHAPE => cat.para.push(parse_para_shape(&rec.payload)),
                _ => {}
            }
        }
        cat
    }
}

fn char_shape_size(p: &[u8]) -> i32 {
    // 7×u16 faces + 7×u8 ratio/spacing/rel/offset, then i32 height (10pt = 1000).
    const OFF: usize = 7 * 2 + 7 * 4;
    if p.len() >= OFF + 4 {
        i32_at(p, OFF).max(1)
    } else {
        docagent_model::DEFAULT_FONT_SIZE_HU
    }
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
    let shape_id = children.iter().find_map(|r| {
        if r.tag == HWPTAG_PARA_CHAR_SHAPE && r.payload.len() >= 8 {
            Some(u32::from_le_bytes(r.payload[4..8].try_into().unwrap()) as usize)
        } else {
            None
        }
    });
    if let Some(id) = shape_id
        && let Some(size) = cat.char_sizes.get(id).copied()
    {
        for run in &mut para.runs {
            run.style.size = size;
        }
    }
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
    if let Some(ctrl_idx) = children.iter().position(|r| {
        r.tag == HWPTAG_CTRL_HEADER && matches_ctrl(&r.payload, CTRL_TABLE)
    }) {
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
    for rec in &recs[start..end] {
        match rec.tag {
            HWPTAG_PARA_TEXT => {
                text.push_str(&decode_para_text(&rec.payload));
            }
            HWPTAG_PARA_LINE_SEG => {
                hints.extend(decode_lineseg(&rec.payload));
            }
            _ => {}
        }
    }
    let mut para = Paragraph::from_text(text);
    para.layout_hints = hints;
    apply_catalog(&mut para, &recs[start].payload, &recs[start + 1..end], catalog);
    Block::Paragraph(para)
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
            if let Block::Paragraph(p) = block {
                paras.push(p);
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

fn write_docinfo() -> Vec<u8> {
    let mut out = Vec::new();
    // 한글 5.0 DocInfo: 7×u16 시작번호. rhwp `parse_document_properties` 계약.
    let mut props = Vec::new();
    for _ in 0..7 {
        props.extend_from_slice(&1u16.to_le_bytes());
    }
    write_record(&mut out, HWPTAG_DOCUMENT_PROPERTIES, 0, &props);
    // 15×u32 ID mappings. Counts stay 0 so no FACE_NAME/CHAR_SHAPE payloads follow.
    let map = vec![0u8; 60];
    write_record(&mut out, HWPTAG_ID_MAPPINGS, 0, &map);
    out
}

fn write_section(section: &Section) -> Vec<u8> {
    let mut out = Vec::new();
    write_record(&mut out, HWPTAG_PAGE_DEF, 0, &page_def_payload(&section.page));
    if section.body.is_empty() {
        write_paragraph(&mut out, &Paragraph::from_text(""), 0);
    } else {
        for block in &section.body {
            match block {
                Block::Paragraph(p) => write_paragraph(&mut out, p, 0),
                Block::Table(t) => write_table(&mut out, t, 0),
                Block::Float(_) | Block::Break(_) => {
                    write_paragraph(&mut out, &Paragraph::from_text(""), 0);
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

fn write_paragraph(out: &mut Vec<u8>, para: &Paragraph, level: u16) {
    let text = para.plain_text();
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let nchars = utf16.len() as u32;
    let mut header = vec![0u8; 22];
    header[0..4].copy_from_slice(&nchars.to_le_bytes());
    header[8..10].copy_from_slice(&0u16.to_le_bytes());
    let char_shapes = if nchars == 0 { 0u16 } else { 1u16 };
    header[12..14].copy_from_slice(&char_shapes.to_le_bytes());
    let line_count = para.layout_hints.len() as u16;
    header[16..18].copy_from_slice(&line_count.to_le_bytes());
    write_record(out, HWPTAG_PARA_HEADER, level, &header);
    let mut bytes = Vec::with_capacity(utf16.len() * 2);
    for u in utf16 {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    write_record(out, HWPTAG_PARA_TEXT, level + 1, &bytes);
    if char_shapes == 1 {
        let mut cs = vec![0u8; 8];
        cs[4..8].copy_from_slice(&0u32.to_le_bytes());
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
}

fn write_table(out: &mut Vec<u8>, table: &Table, level: u16) {
    let n_rows = table.rows.len() as u16;
    let n_cols = table.rows.first().map(|r| r.cells.len() as u16).unwrap_or(0);
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
            write_paragraph(out, &Paragraph::from_text(text), level + 3);
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
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("한글 HWP5 roundtrip")));
        doc.sections.push(section);
        let back = roundtrip(&doc).expect("roundtrip");
        assert!(back.plain_text().contains("한글 HWP5 roundtrip"));
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
        assert_eq!(
            hint.y, 0,
            "Hangul PARA_LINE_SEG vertpos is body-relative"
        );
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
}
