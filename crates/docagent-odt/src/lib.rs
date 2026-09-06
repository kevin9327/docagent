//! OpenDocument Text (ODF) codec. Reads and writes `content.xml` paragraphs
//! and tables. Lengths stay in the IR as HWPUNIT.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, BreakKind, Document, InlineObject, NumberFormat, NumberingRef, Paragraph,
    Run, RunContent, Section, Table, TableCell, TableRow, HU_PER_POINT,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Error)]
pub enum Error {
    #[error("not an ODT package")]
    NotOdt,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml: {0}")]
    Xml(String),
}

pub fn sniff(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0..2] != [0x50, 0x4B] {
        return false;
    }
    ZipArchive::new(Cursor::new(bytes)).ok().is_some_and(|mut z| {
        z.by_name("content.xml").is_ok()
            && (z.by_name("mimetype").is_ok() || z.by_name("META-INF/manifest.xml").is_ok())
    })
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if !sniff(bytes) {
        return Err(Error::NotOdt);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec()))?;
    let mut xml = String::new();
    zip.by_name("content.xml")?
        .read_to_string(&mut xml)?;
    parse_content_xml(&xml)
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored)?;
        zip.write_all(b"application/vnd.oasis.opendocument.text")?;
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("META-INF/manifest.xml", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:media-type="application/vnd.oasis.opendocument.text" manifest:full-path="/"/><manifest:file-entry manifest:media-type="text/xml" manifest:full-path="content.xml"/></manifest:manifest>"#,
        )?;
        zip.start_file("content.xml", deflated)?;
        zip.write_all(content_xml(doc).as_bytes())?;
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn parse_content_xml(xml: &str) -> Result<Document, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut doc = Document::new();
    let mut section = Section::default();
    let mut in_p = false;
    let mut in_table = false;
    let mut in_style = false;
    let mut style_name = String::new();
    let mut style_bits = StyleBits::default();
    let mut styles: HashMap<String, StyleBits> = HashMap::new();
    let mut span_bold = false;
    let mut span_italic = false;
    let mut span_strike = false;
    let mut span_code = false;
    let mut span_size: Option<i32> = None;
    let mut para_bold = false;
    let mut para_italic = false;
    let mut para_strike = false;
    let mut para_code = false;
    let mut para_size: Option<i32> = None;
    let mut run_text = String::new();
    let mut para_runs: Vec<Run> = Vec::new();
    let mut para_outline: Option<u8> = None;
    let mut para_quote = false;
    let mut para_code_block = false;
    let mut para_hr = false;
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_blocks: Vec<Block> = Vec::new();
    let mut list_stack: Vec<ListCtx> = Vec::new();
    let mut a_href: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "style" => {
                        in_style = true;
                        style_name = attr(&e, "name").unwrap_or_default();
                        style_bits = StyleBits::default();
                    }
                    "text-properties" if in_style => {
                        if let Some(w) = attr(&e, "font-weight") {
                            style_bits.bold = w == "bold" || w == "700";
                        }
                        if let Some(s) = attr(&e, "font-style") {
                            style_bits.italic = s == "italic" || s == "oblique";
                        }
                        if let Some(s) = attr(&e, "text-line-through-style") {
                            style_bits.strike |= s != "none";
                        }
                        if let Some(s) = attr(&e, "text-line-through-type") {
                            style_bits.strike |= s != "none";
                        }
                        if let Some(bg) = attr(&e, "background-color") {
                            let bg = bg.trim();
                            style_bits.code |= bg.eq_ignore_ascii_case("#ccfbf1")
                                || bg.eq_ignore_ascii_case("ccfbf1");
                        }
                        if let Some(sz) = attr(&e, "font-size").and_then(|v| parse_fo_size(&v)) {
                            style_bits.size = Some(sz);
                        }
                    }
                    "p" | "h" => {
                        in_p = true;
                        run_text.clear();
                        para_runs.clear();
                        para_outline = if name == "h" {
                            attr(&e, "outline-level").and_then(|v| v.parse().ok())
                        } else {
                            None
                        };
                        let style_name = attr(&e, "style-name");
                        para_quote = style_name.as_deref() == Some("Quote");
                        para_code_block = style_name.as_deref() == Some("CodeBlock");
                        para_hr = style_name.as_deref() == Some("HorizontalLine");
                        let st = style_name
                            .as_ref()
                            .and_then(|k| styles.get(k).copied())
                            .unwrap_or_default();
                        para_bold = st.bold;
                        para_italic = st.italic;
                        para_strike = st.strike;
                        para_code = st.code;
                        para_size = st.size;
                        span_bold = para_bold;
                        span_italic = para_italic;
                        span_strike = para_strike;
                        span_code = para_code;
                        span_size = para_size;
                    }
                    "a" if in_p => {
                        flush_odt_run(
                            &mut para_runs,
                            &mut run_text,
                            StyleBits {
                                bold: span_bold,
                                italic: span_italic,
                                strike: span_strike,
                                code: span_code,
                                size: span_size,
                            },
                            None,
                        );
                        a_href = attr(&e, "href");
                    }
                    "span" if in_p => {
                        flush_odt_run(
                            &mut para_runs,
                            &mut run_text,
                            StyleBits {
                                bold: span_bold,
                                italic: span_italic,
                                strike: span_strike,
                                code: span_code,
                                size: span_size,
                            },
                            None,
                        );
                        let key = attr(&e, "style-name").unwrap_or_default();
                        if let Some(st) = styles.get(&key).copied() {
                            span_bold = st.bold;
                            span_italic = st.italic;
                            span_strike = st.strike;
                            span_code = st.code;
                            span_size = st.size.or(para_size);
                        } else {
                            span_bold = para_bold;
                            span_italic = para_italic;
                            span_strike = para_strike;
                            span_code = para_code;
                            span_size = para_size;
                        }
                    }
                    "list" if !in_table => {
                        let name = attr(&e, "style-name").unwrap_or_default();
                        let format = if name.contains("number") || name.contains("Number") {
                            NumberFormat::Decimal
                        } else {
                            NumberFormat::Bullet
                        };
                        list_stack.push(ListCtx {
                            format,
                            decimal_seq: 1,
                        });
                    }
                    "table" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "table-row" if in_table => cur_row.clear(),
                    "table-cell" if in_table => cell_blocks.clear(),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "style" => {
                        in_style = false;
                        if !style_name.is_empty() {
                            styles.insert(style_name.clone(), style_bits);
                        }
                    }
                    "span" if in_p => {
                        flush_odt_run(
                            &mut para_runs,
                            &mut run_text,
                            StyleBits {
                                bold: span_bold,
                                italic: span_italic,
                                strike: span_strike,
                                code: span_code,
                                size: span_size,
                            },
                            None,
                        );
                        span_bold = para_bold;
                        span_italic = para_italic;
                        span_strike = para_strike;
                        span_code = para_code;
                        span_size = para_size;
                    }
                    "a" if in_p => {
                        flush_odt_run(
                            &mut para_runs,
                            &mut run_text,
                            StyleBits {
                                bold: span_bold,
                                italic: span_italic,
                                strike: span_strike,
                                code: span_code,
                                size: span_size,
                            },
                            a_href.as_deref(),
                        );
                        a_href = None;
                    }
                    "p" | "h" => {
                        in_p = false;
                        flush_odt_run(
                            &mut para_runs,
                            &mut run_text,
                            StyleBits {
                                bold: span_bold,
                                italic: span_italic,
                                strike: span_strike,
                                code: span_code,
                                size: span_size,
                            },
                            a_href.take().as_deref(),
                        );
                        if para_hr && para_runs.is_empty() {
                            if !in_table {
                                section.body.push(Block::Break(BreakKind::Thematic));
                            }
                        } else if !para_runs.is_empty() {
                            let mut p = Paragraph::from_text("");
                            p.runs = std::mem::take(&mut para_runs);
                            p.outline_level = para_outline.take();
                            p.quote = para_quote;
                            p.code_block = para_code_block;
                            let level = list_stack.len().saturating_sub(1) as u8;
                            if !in_table
                                && let Some(ctx) = list_stack.last_mut()
                            {
                                let start = if ctx.format == NumberFormat::Decimal {
                                    let n = ctx.decimal_seq;
                                    ctx.decimal_seq = ctx.decimal_seq.saturating_add(1);
                                    Some(n)
                                } else {
                                    None
                                };
                                p.numbering = Some(NumberingRef {
                                    definition_id: if ctx.format == NumberFormat::Decimal {
                                        1
                                    } else {
                                        0
                                    },
                                    level,
                                    start,
                                    format: Some(ctx.format),
                                });
                            }
                            apply_task_marker(&mut p);
                            if in_table {
                                cell_blocks.push(Block::Paragraph(p));
                            } else {
                                section.body.push(Block::Paragraph(p));
                            }
                        }
                        run_text.clear();
                        para_outline = None;
                        para_quote = false;
                        para_code_block = false;
                        para_hr = false;
                    }
                    "list" if !in_table => {
                        list_stack.pop();
                    }
                    "table-cell" if in_table => {
                        cur_row.push(TableCell {
                            width: 10000,
                            blocks: std::mem::take(&mut cell_blocks),
                            ..TableCell::default()
                        });
                    }
                    "table-row" if in_table => {
                        table_rows.push(TableRow {
                            cells: std::mem::take(&mut cur_row),
                            height: None,
                            header: false,
                            cant_split: None,
                        });
                    }
                    "table" => {
                        in_table = false;
                        section.body.push(Block::Table(Table {
                            rows: std::mem::take(&mut table_rows),
                            alignment: Alignment::Start,
                            ..Table::from_cells(Vec::new())
                        }));
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if in_p {
                    let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                    run_text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    doc.sections.push(section);
    Ok(doc)
}

struct ListCtx {
    format: NumberFormat,
    decimal_seq: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct StyleBits {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    size: Option<i32>,
}

fn parse_fo_size(s: &str) -> Option<i32> {
    let s = s.trim();
    let num = s.strip_suffix("pt").or_else(|| s.strip_suffix("PT"))?;
    let pt: i32 = num.trim().parse().ok()?;
    (pt > 0).then_some(pt.saturating_mul(HU_PER_POINT))
}

fn heading_style_name(level: u8) -> Option<&'static str> {
    match level {
        1 => Some("Heading1"),
        2 => Some("Heading2"),
        3 => Some("Heading3"),
        _ => None,
    }
}

fn flush_odt_run(runs: &mut Vec<Run>, text: &mut String, bits: StyleBits, href: Option<&str>) {
    if text.is_empty() {
        return;
    }
    let mut run = if let Some(url) = href.filter(|u| !u.is_empty()) {
        Run::hyperlink(std::mem::take(text), url)
    } else {
        Run::text(std::mem::take(text))
    };
    run.style.bold = bits.bold;
    run.style.italic = bits.italic;
    run.style.strike = bits.strike;
    run.style.code = bits.code;
    if let Some(sz) = bits.size {
        run.style.size = sz;
    }
    runs.push(run);
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.local_name().as_ref() == key.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn apply_task_marker(p: &mut Paragraph) {
    let Some(run) = p.runs.first_mut() else {
        return;
    };
    let RunContent::Text(text) = &mut run.content else {
        return;
    };
    let checked = text.starts_with("[x] ") || text.starts_with("[X] ");
    let unchecked = text.starts_with("[ ] ");
    if !checked && !unchecked {
        return;
    }
    *text = text[4..].to_string();
    let level = p.numbering.as_ref().map(|n| n.level).unwrap_or(0);
    p.numbering = Some(NumberingRef {
        definition_id: 2,
        level,
        start: checked.then_some(1),
        format: Some(NumberFormat::Task),
    });
}

fn odt_para(p: &Paragraph) -> String {
    let inner = odt_runs(p);
    if p.code_block {
        return format!(r#"<text:p text:style-name="CodeBlock">{inner}</text:p>"#);
    }
    if p.quote {
        return format!(r#"<text:p text:style-name="Quote">{inner}</text:p>"#);
    }
    match p.outline_level {
        Some(level) if level > 0 => {
            if let Some(style) = heading_style_name(level) {
                format!(
                    r#"<text:h text:style-name="{style}" text:outline-level="{level}">{inner}</text:h>"#
                )
            } else {
                format!(r#"<text:h text:outline-level="{level}">{inner}</text:h>"#)
            }
        }
        _ => format!("<text:p>{inner}</text:p>"),
    }
}

fn odt_runs(p: &Paragraph) -> String {
    let mut s = String::new();
    if let Some(n) = p.numbering.as_ref()
        && n.format == Some(NumberFormat::Task)
    {
        if n.start == Some(1) {
            s.push_str("[x] ");
        } else {
            s.push_str("[ ] ");
        }
    }
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                s.push_str(r#"<text:a xlink:href=""#);
                s.push_str(&xml_escape(target));
                s.push_str(r#"">"#);
                s.push_str(&xml_escape(display));
                s.push_str("</text:a>");
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                let escaped = xml_escape(t);
                if let Some(style) = odt_span_style(
                    run.style.bold,
                    run.style.italic,
                    run.style.strike,
                    run.style.code,
                ) {
                    s.push_str(r#"<text:span text:style-name=""#);
                    s.push_str(style);
                    s.push_str(r#"">"#);
                    s.push_str(&escaped);
                    s.push_str("</text:span>");
                } else {
                    s.push_str(&escaped);
                }
            }
            RunContent::Inline(_) => {}
        }
    }
    s
}

fn list_format(block: &Block) -> Option<NumberFormat> {
    match block {
        Block::Paragraph(p) => p.numbering.as_ref().and_then(|n| n.format),
        _ => None,
    }
}

fn list_level(block: &Block) -> Option<u8> {
    match block {
        Block::Paragraph(p) => p.numbering.as_ref().map(|n| n.level),
        _ => None,
    }
}

fn emit_list(body: &mut String, blocks: &[Block], i: &mut usize) {
    let Some(fmt) = list_format(&blocks[*i]) else {
        return;
    };
    let base = list_level(&blocks[*i]).unwrap_or(0);
    let style = match fmt {
        NumberFormat::Decimal => "Lnumber",
        _ => "Lbullet",
    };
    body.push_str(r#"<text:list text:style-name=""#);
    body.push_str(style);
    body.push_str(r#"">"#);
    while *i < blocks.len() {
        let Some(f) = list_format(&blocks[*i]) else {
            break;
        };
        let lvl = list_level(&blocks[*i]).unwrap_or(0);
        if lvl < base {
            break;
        }
        if lvl > base {
            emit_list(body, blocks, i);
            continue;
        }
        if f != fmt {
            break;
        }
        let Block::Paragraph(p) = &blocks[*i] else {
            break;
        };
        body.push_str("<text:list-item>");
        body.push_str(&odt_para(p));
        *i += 1;
        if *i < blocks.len() && list_level(&blocks[*i]).is_some_and(|nl| nl > base) {
            emit_list(body, blocks, i);
        }
        body.push_str("</text:list-item>");
    }
    body.push_str("</text:list>");
}

fn push_table(body: &mut String, table: &Table) {
    body.push_str("<table:table>");
    for row in &table.rows {
        body.push_str("<table:table-row>");
        for cell in &row.cells {
            body.push_str("<table:table-cell>");
            let mut wrote = false;
            for b in &cell.blocks {
                if let Block::Paragraph(p) = b {
                    body.push_str(&odt_para(p));
                    wrote = true;
                }
            }
            if !wrote {
                body.push_str("<text:p/>");
            }
            body.push_str("</table:table-cell>");
        }
        body.push_str("</table:table-row>");
    }
    body.push_str("</table:table>");
}

fn content_xml(doc: &Document) -> String {
    let mut body = String::new();
    for section in &doc.sections {
        let mut i = 0;
        while i < section.body.len() {
            if list_format(&section.body[i]).is_some() {
                emit_list(&mut body, &section.body, &mut i);
            } else {
                match &section.body[i] {
                    Block::Paragraph(p) => body.push_str(&odt_para(p)),
                    Block::Table(table) => push_table(&mut body, table),
                    Block::Break(BreakKind::Thematic) => {
                        body.push_str(r#"<text:p text:style-name="HorizontalLine"/>"#);
                    }
                    Block::Float(_) | Block::Break(_) => {}
                }
                i += 1;
            }
        }
    }
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink"><office:automatic-styles><style:style style:name="Heading1" style:family="paragraph"><style:text-properties fo:font-size="18pt" fo:font-weight="bold"/></style:style><style:style style:name="Heading2" style:family="paragraph"><style:text-properties fo:font-size="14pt" fo:font-weight="bold"/></style:style><style:style style:name="Heading3" style:family="paragraph"><style:text-properties fo:font-size="12pt" fo:font-weight="bold"/></style:style><style:style style:name="Quote" style:family="paragraph"><style:paragraph-properties fo:border-left="0.02in solid #134e4a" fo:padding-left="0.1in" fo:margin-left="0.1in"/></style:style><style:style style:name="HorizontalLine" style:family="paragraph"><style:paragraph-properties fo:border-bottom="0.02in solid #134e4a" fo:margin-top="0.15in" fo:margin-bottom="0.15in"/></style:style><style:style style:name="CodeBlock" style:family="paragraph"><style:paragraph-properties fo:background-color="#ccfbf1" fo:padding="0.1in"/><style:text-properties fo:font-family="Consolas"/></style:style><style:style style:name="Tbold" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><style:style style:name="Titalic" style:family="text"><style:text-properties fo:font-style="italic"/></style:style><style:style style:name="Tbi" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style><style:style style:name="Tstrike" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style><style:style style:name="Tbstrike" style:family="text"><style:text-properties fo:font-weight="bold" style:text-line-through-style="solid"/></style:style><style:style style:name="Tistrike" style:family="text"><style:text-properties fo:font-style="italic" style:text-line-through-style="solid"/></style:style><style:style style:name="Tbistrike" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic" style:text-line-through-style="solid"/></style:style><style:style style:name="Tcode" style:family="text"><style:text-properties fo:background-color="#ccfbf1" fo:font-family="Consolas"/></style:style><text:list-style style:name="Lbullet"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:space-before="0.25in" text:min-label-width="0.25in"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="2" text:bullet-char="•"><style:list-level-properties text:space-before="0.5in" text:min-label-width="0.25in"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="3" text:bullet-char="•"><style:list-level-properties text:space-before="0.75in" text:min-label-width="0.25in"/></text:list-level-style-bullet></text:list-style><text:list-style style:name="Lnumber"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.25in" text:min-label-width="0.25in"/></text:list-level-style-number><text:list-level-style-number text:level="2" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.5in" text:min-label-width="0.25in"/></text:list-level-style-number><text:list-level-style-number text:level="3" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.75in" text:min-label-width="0.25in"/></text:list-level-style-number></text:list-style></office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"##
    )
}

fn odt_span_style(bold: bool, italic: bool, strike: bool, code: bool) -> Option<&'static str> {
    if code {
        return Some("Tcode");
    }
    match (bold, italic, strike) {
        (false, false, false) => None,
        (true, false, false) => Some("Tbold"),
        (false, true, false) => Some("Titalic"),
        (true, true, false) => Some("Tbi"),
        (false, false, true) => Some("Tstrike"),
        (true, false, true) => Some("Tbstrike"),
        (false, true, true) => Some("Tistrike"),
        (true, true, true) => Some("Tbistrike"),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_bold_and_italic() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        let mut italic = Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("fo:font-weight=\"bold\""));
        assert!(xml.contains("fo:font-style=\"italic\""));
        assert!(xml.contains("Tbold"));
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
        }));
    }

    #[test]
    fn roundtrip_keeps_task_item() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay the convert");
        p.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("[x] "), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert_eq!(
            p.numbering.as_ref().map(|n| (n.format, n.start)),
            Some((Some(NumberFormat::Task), Some(1)))
        );
        assert_eq!(p.plain_text(), "Replay the convert");
    }

    #[test]
    fn roundtrip_keeps_code_block() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("docagent prove examples/letter.md");
        p.code_block = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains(r#"text:style-name="CodeBlock""#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.code_block);
        assert!(p.plain_text().contains("docagent prove"));
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
        let xml = content_xml(&doc);
        assert!(xml.contains(r#"text:style-name="HorizontalLine""#), "{xml}");
        assert!(xml.contains("fo:border-bottom"), "{xml}");
        let back = roundtrip(&doc).unwrap();
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
    fn roundtrip_keeps_code_span() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Run ");
        let mut code = Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tcode"), "{xml}");
        assert!(xml.contains("fo:background-color=\"#ccfbf1\""), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.code && matches!(&r.content, RunContent::Text(t) if t == "prove")
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.code && matches!(&r.content, RunContent::Text(t) if t.contains("Run"))
        }));
    }

    #[test]
    fn roundtrip_keeps_quote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains(r#"text:style-name="Quote""#), "{xml}");
        assert!(xml.contains("fo:border-left"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.quote);
        assert!(p.plain_text().contains("Replay is evidence"));
    }

    #[test]
    fn roundtrip_keeps_strikethrough() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tstrike"), "{xml}");
        assert!(xml.contains("text-line-through-style=\"solid\""), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("guess"))
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("plain"))
        }));
    }

    #[test]
    fn roundtrip_keeps_heading_outline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.runs[0].style.size = 1800;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.runs[0].style.size = 1400;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains(r#"text:outline-level="1""#), "{xml}");
        assert!(xml.contains(r#"text:style-name="Heading1""#), "{xml}");
        assert!(xml.contains(r#"fo:font-size="18pt""#), "{xml}");
        assert!(xml.contains(r#"fo:font-size="14pt""#), "{xml}");
        assert!(xml.contains("<text:h"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("h1");
        };
        assert_eq!(p.outline_level, Some(1));
        assert_eq!(p.plain_text(), "Northwind Freight");
        assert_eq!(p.runs[0].style.size, 1800);
        assert!(p.runs[0].style.bold);
        let Block::Paragraph(p2) = &back.sections[0].body[1] else {
            panic!("h2");
        };
        assert_eq!(p2.outline_level, Some(2));
        assert_eq!(p2.runs[0].style.size, 1400);
    }

    #[test]
    fn roundtrip_keeps_list_numbering() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("Receiving dock is clear");
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("Bay 4, not bay 2");
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut c = Paragraph::from_text("Hash the input");
        c.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut d = Paragraph::from_text("Run Convert");
        d.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(2),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        section.body.push(Block::Paragraph(c));
        section.body.push(Block::Paragraph(d));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("<text:list"), "{xml}");
        assert!(xml.contains("Lbullet"), "{xml}");
        assert!(xml.contains("Lnumber"), "{xml}");
        assert!(xml.contains("<text:list-item>"), "{xml}");
        let back = roundtrip(&doc).unwrap();
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
                (Some(NumberFormat::Decimal), 0, Some(2)),
            ]
        );
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
        let xml = content_xml(&doc);
        assert!(xml.contains("xlink:href="), "{xml}");
        assert!(xml.contains("<text:a"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("para");
        };
        assert!(p.runs.iter().any(|r| matches!(
            &r.content,
            RunContent::Inline(InlineObject::Hyperlink { target, display })
                if target == "https://github.com/kevin9327/docagent" && display == "DocAgent"
        )));
    }

    #[test]
    fn roundtrip_keeps_cell_emphasis() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        let mut italic = Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs = vec![bold, italic];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tbold"), "{xml}");
        assert!(xml.contains("Titalic"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        let Block::Paragraph(cell) = &t.rows[0].cells[0].blocks[0] else {
            panic!("cell para");
        };
        assert!(cell.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(cell.runs.iter().any(|r| {
            r.style.italic && matches!(&r.content, RunContent::Text(t) if t.contains("byte-for-byte"))
        }));
    }

    #[test]
    fn roundtrip_paragraph_and_table() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("OpenDocument hello")));
        section.body.push(Block::Table(Table::from_cells(vec![
            vec!["x".into(), "y".into()],
        ])));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains("OpenDocument hello"));
        assert!(back.plain_text().contains('x'));
        assert!(sniff(&write(&doc).unwrap()));
    }
}
