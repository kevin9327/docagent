//! OpenDocument Text (ODF) codec. Reads and writes `content.xml` paragraphs
//! and tables. Lengths stay in the IR as HWPUNIT.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Color, Document, Hu, ImageData, InlineObject,
    NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
    Underline, WrapMode, HU_PER_INCH, HU_PER_POINT,
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
    let mut pictures: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        if name == "content.xml" {
            file.read_to_string(&mut xml)?;
        } else if is_package_picture(&name) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            pictures.insert(name, buf);
        }
    }
    parse_content_xml(&xml, &pictures)
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let (xml, pictures) = build_content(doc);
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored)?;
        zip.write_all(b"application/vnd.oasis.opendocument.text")?;
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("META-INF/manifest.xml", deflated)?;
        zip.write_all(manifest_xml(&pictures).as_bytes())?;
        zip.start_file("content.xml", deflated)?;
        zip.write_all(xml.as_bytes())?;
        for pic in &pictures {
            zip.start_file(&pic.path, stored)?;
            zip.write_all(&pic.bytes)?;
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn parse_content_xml(xml: &str, pictures: &HashMap<String, Vec<u8>>) -> Result<Document, Error> {
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
    let mut span_mark = false;
    let mut span_under = false;
    let mut span_super = false;
    let mut span_sub = false;
    let mut span_size: Option<i32> = None;
    let mut para_bold = false;
    let mut para_italic = false;
    let mut para_strike = false;
    let mut para_code = false;
    let mut para_mark = false;
    let mut para_under = false;
    let mut para_super = false;
    let mut para_sub = false;
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
    let mut in_header_rows = false;
    let mut in_frame = false;
    let mut in_frame_title = false;
    let mut frame_width: Option<Hu> = None;
    let mut frame_height: Option<Hu> = None;
    let mut frame_alt = String::new();
    let mut frame_as_char = true;
    let mut image_href: Option<String> = None;
    let mut image_mime: Option<String> = None;
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
                        if let Some(s) = attr(&e, "text-underline-style") {
                            style_bits.underline |= s != "none";
                        }
                        if let Some(s) = attr(&e, "text-underline-type") {
                            style_bits.underline |= s != "none";
                        }
                        if let Some(p) = attr(&e, "text-position") {
                            let p = p.trim().to_ascii_lowercase();
                            style_bits.superscript |= p.starts_with("super");
                            style_bits.subscript |= p.starts_with("sub");
                        }
                        if let Some(bg) = attr(&e, "background-color") {
                            let bg = bg.trim();
                            style_bits.code |= bg.eq_ignore_ascii_case("#ccfbf1")
                                || bg.eq_ignore_ascii_case("ccfbf1");
                            style_bits.highlight |= bg.eq_ignore_ascii_case("#fde68a")
                                || bg.eq_ignore_ascii_case("fde68a");
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
                        para_mark = st.highlight;
                        para_under = st.underline;
                        para_super = st.superscript;
                        para_sub = st.subscript;
                        para_size = st.size;
                        span_bold = para_bold;
                        span_italic = para_italic;
                        span_strike = para_strike;
                        span_code = para_code;
                        span_mark = para_mark;
                        span_under = para_under;
                        span_super = para_super;
                        span_sub = para_sub;
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
                                highlight: span_mark,
                                underline: span_under,
                                superscript: span_super,
                                subscript: span_sub,
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
                                highlight: span_mark,
                                underline: span_under,
                                superscript: span_super,
                                subscript: span_sub,
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
                            span_mark = st.highlight;
                            span_under = st.underline;
                            span_super = st.superscript;
                            span_sub = st.subscript;
                            span_size = st.size.or(para_size);
                        } else {
                            span_bold = para_bold;
                            span_italic = para_italic;
                            span_strike = para_strike;
                            span_code = para_code;
                            span_mark = para_mark;
                            span_under = para_under;
                            span_super = para_super;
                            span_sub = para_sub;
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
                        in_header_rows = false;
                    }
                    "table-header-rows" if in_table => in_header_rows = true,
                    "table-row" if in_table => cur_row.clear(),
                    "table-cell" if in_table => cell_blocks.clear(),
                    "frame" => {
                        if in_p {
                            flush_odt_run(
                                &mut para_runs,
                                &mut run_text,
                                StyleBits {
                                    bold: span_bold,
                                    italic: span_italic,
                                    strike: span_strike,
                                    code: span_code,
                                    highlight: span_mark,
                                    underline: span_under,
                                    superscript: span_super,
                                    subscript: span_sub,
                                    size: span_size,
                                },
                                None,
                            );
                        }
                        in_frame = true;
                        in_frame_title = false;
                        frame_width = attr(&e, "width").as_deref().and_then(parse_odf_length);
                        frame_height = attr(&e, "height").as_deref().and_then(parse_odf_length);
                        frame_alt.clear();
                        frame_as_char = matches!(
                            attr(&e, "anchor-type").as_deref(),
                            Some("as-char") | None
                        );
                        image_href = None;
                        image_mime = None;
                    }
                    "image" if in_frame => {
                        image_href = attr(&e, "href");
                        image_mime = attr(&e, "mime-type");
                    }
                    "title" | "desc" if in_frame => in_frame_title = true,
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
                                highlight: span_mark,
                                underline: span_under,
                                superscript: span_super,
                                subscript: span_sub,
                                size: span_size,
                            },
                            None,
                        );
                        span_bold = para_bold;
                        span_italic = para_italic;
                        span_strike = para_strike;
                        span_code = para_code;
                        span_mark = para_mark;
                        span_under = para_under;
                        span_super = para_super;
                        span_sub = para_sub;
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
                                highlight: span_mark,
                                underline: span_under,
                                superscript: span_super,
                                subscript: span_sub,
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
                                highlight: span_mark,
                                underline: span_under,
                                superscript: span_super,
                                subscript: span_sub,
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
                    "table-header-rows" if in_table => in_header_rows = false,
                    "table-row" if in_table => {
                        table_rows.push(TableRow {
                            cells: std::mem::take(&mut cur_row),
                            height: None,
                            header: in_header_rows,
                            cant_split: None,
                        });
                    }
                    "table" => {
                        in_table = false;
                        let rows = std::mem::take(&mut table_rows);
                        let header_row_count =
                            rows.iter().take_while(|r| r.header).count() as u8;
                        section.body.push(Block::Table(Table {
                            rows,
                            alignment: Alignment::Start,
                            header_row_count,
                            ..Table::from_cells(Vec::new())
                        }));
                    }
                    "title" | "desc" if in_frame => in_frame_title = false,
                    "frame" => {
                        if let Some(img) = take_frame_image(
                            pictures,
                            image_href.take(),
                            image_mime.take(),
                            (frame_width.take(), frame_height.take()),
                            std::mem::take(&mut frame_alt),
                            frame_as_char,
                        ) {
                            let run = Run {
                                style: CharStyle::default(),
                                content: RunContent::Inline(InlineObject::Image(img)),
                            };
                            if in_p {
                                para_runs.push(run);
                            } else {
                                let mut p = Paragraph::from_text("");
                                p.runs = vec![run];
                                if in_table {
                                    cell_blocks.push(Block::Paragraph(p));
                                } else {
                                    section.body.push(Block::Paragraph(p));
                                }
                            }
                        }
                        in_frame = false;
                        in_frame_title = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if in_frame_title {
                    let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                    frame_alt.push_str(&decoded);
                } else if in_p && !in_frame {
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
    highlight: bool,
    underline: bool,
    superscript: bool,
    subscript: bool,
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
    if bits.highlight {
        run.style.highlight = Some(Color::MARK);
    }
    if bits.underline {
        run.style.underline = Underline::Single;
    }
    run.style.superscript = bits.superscript;
    run.style.subscript = bits.subscript;
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

fn odt_para(p: &Paragraph, pics: &mut Vec<PicturePart>) -> String {
    let inner = odt_runs(p, pics);
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

fn odt_runs(p: &Paragraph, pics: &mut Vec<PicturePart>) -> String {
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
                s.push_str(r#"<text:a text:style-name="Tlink" xlink:href=""#);
                s.push_str(&xml_escape(target));
                s.push_str(r#"">"#);
                s.push_str(&xml_escape(display));
                s.push_str("</text:a>");
            }
            RunContent::Inline(InlineObject::Image(img)) => {
                push_odt_image(&mut s, img, pics);
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                let escaped = xml_escape(t);
                if let Some(style) = odt_span_style(&run.style) {
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

fn emit_list(body: &mut String, blocks: &[Block], i: &mut usize, pics: &mut Vec<PicturePart>) {
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
            emit_list(body, blocks, i, pics);
            continue;
        }
        if f != fmt {
            break;
        }
        let Block::Paragraph(p) = &blocks[*i] else {
            break;
        };
        body.push_str("<text:list-item>");
        body.push_str(&odt_para(p, pics));
        *i += 1;
        if *i < blocks.len() && list_level(&blocks[*i]).is_some_and(|nl| nl > base) {
            emit_list(body, blocks, i, pics);
        }
        body.push_str("</text:list-item>");
    }
    body.push_str("</text:list>");
}

fn push_table(body: &mut String, table: &Table, pics: &mut Vec<PicturePart>) {
    body.push_str("<table:table>");
    let mut i = 0usize;
    if table.rows.iter().any(|r| r.header) {
        body.push_str("<table:table-header-rows>");
        while i < table.rows.len() && table.rows[i].header {
            push_table_row(body, &table.rows[i], true, pics);
            i += 1;
        }
        body.push_str("</table:table-header-rows>");
    }
    while i < table.rows.len() {
        push_table_row(body, &table.rows[i], false, pics);
        i += 1;
    }
    body.push_str("</table:table>");
}

fn push_table_row(body: &mut String, row: &TableRow, header: bool, pics: &mut Vec<PicturePart>) {
    body.push_str("<table:table-row>");
    for cell in &row.cells {
        if header {
            body.push_str(r#"<table:table-cell table:style-name="THcell">"#);
        } else {
            body.push_str("<table:table-cell>");
        }
        let mut wrote = false;
        for b in &cell.blocks {
            if let Block::Paragraph(p) = b {
                body.push_str(&odt_para(p, pics));
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

#[cfg(test)]
fn content_xml(doc: &Document) -> String {
    build_content(doc).0
}

fn build_content(doc: &Document) -> (String, Vec<PicturePart>) {
    let mut pics = Vec::new();
    let mut body = String::new();
    for section in &doc.sections {
        let mut i = 0;
        while i < section.body.len() {
            if list_format(&section.body[i]).is_some() {
                emit_list(&mut body, &section.body, &mut i, &mut pics);
            } else {
                match &section.body[i] {
                    Block::Paragraph(p) => body.push_str(&odt_para(p, &mut pics)),
                    Block::Table(table) => push_table(&mut body, table, &mut pics),
                    Block::Break(BreakKind::Thematic) => {
                        body.push_str(r#"<text:p text:style-name="HorizontalLine"/>"#);
                    }
                    Block::Float(_) | Block::Break(_) => {}
                }
                i += 1;
            }
        }
    }
    let xml = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"><office:automatic-styles><style:style style:name="Heading1" style:family="paragraph"><style:text-properties fo:font-size="18pt" fo:font-weight="bold"/></style:style><style:style style:name="Heading2" style:family="paragraph"><style:text-properties fo:font-size="14pt" fo:font-weight="bold"/></style:style><style:style style:name="Heading3" style:family="paragraph"><style:text-properties fo:font-size="12pt" fo:font-weight="bold"/></style:style><style:style style:name="Quote" style:family="paragraph"><style:paragraph-properties fo:border-left="0.02in solid #134e4a" fo:padding-left="0.1in" fo:margin-left="0.1in"/></style:style><style:style style:name="HorizontalLine" style:family="paragraph"><style:paragraph-properties fo:border-bottom="0.02in solid #134e4a" fo:margin-top="0.15in" fo:margin-bottom="0.15in"/></style:style><style:style style:name="CodeBlock" style:family="paragraph"><style:paragraph-properties fo:background-color="#ccfbf1" fo:padding="0.1in"/><style:text-properties fo:font-family="Consolas"/></style:style><style:style style:name="Tbold" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style><style:style style:name="Titalic" style:family="text"><style:text-properties fo:font-style="italic"/></style:style><style:style style:name="Tbi" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic"/></style:style><style:style style:name="Tstrike" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style><style:style style:name="Tbstrike" style:family="text"><style:text-properties fo:font-weight="bold" style:text-line-through-style="solid"/></style:style><style:style style:name="Tistrike" style:family="text"><style:text-properties fo:font-style="italic" style:text-line-through-style="solid"/></style:style><style:style style:name="Tbistrike" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic" style:text-line-through-style="solid"/></style:style><style:style style:name="Tcode" style:family="text"><style:text-properties fo:background-color="#ccfbf1" fo:font-family="Consolas"/></style:style><style:style style:name="Tmark" style:family="text"><style:text-properties fo:background-color="#fde68a"/></style:style><style:style style:name="Tunder" style:family="text"><style:text-properties style:text-underline-style="solid"/></style:style><style:style style:name="Tsuper" style:family="text"><style:text-properties style:text-position="super 58%"/></style:style><style:style style:name="Tsub" style:family="text"><style:text-properties style:text-position="sub 58%"/></style:style><style:style style:name="THcell" style:family="table-cell"><style:table-cell-properties fo:background-color="#ccfbf1"/></style:style><style:style style:name="Tlink" style:family="text"><style:text-properties fo:color="#0d9488" style:text-underline-style="solid"/></style:style><text:list-style style:name="Lbullet"><text:list-level-style-bullet text:level="1" text:bullet-char="•"><style:list-level-properties text:space-before="0.25in" text:min-label-width="0.25in"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="2" text:bullet-char="•"><style:list-level-properties text:space-before="0.5in" text:min-label-width="0.25in"/></text:list-level-style-bullet><text:list-level-style-bullet text:level="3" text:bullet-char="•"><style:list-level-properties text:space-before="0.75in" text:min-label-width="0.25in"/></text:list-level-style-bullet></text:list-style><text:list-style style:name="Lnumber"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.25in" text:min-label-width="0.25in"/></text:list-level-style-number><text:list-level-style-number text:level="2" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.5in" text:min-label-width="0.25in"/></text:list-level-style-number><text:list-level-style-number text:level="3" style:num-format="1" style:num-suffix="."><style:list-level-properties text:space-before="0.75in" text:min-label-width="0.25in"/></text:list-level-style-number></text:list-style></office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"##
    );
    (xml, pics)
}

fn odt_span_style(style: &CharStyle) -> Option<&'static str> {
    if style.code {
        return Some("Tcode");
    }
    if style.highlight.is_some() {
        return Some("Tmark");
    }
    if style.underline != Underline::None {
        return Some("Tunder");
    }
    if style.superscript {
        return Some("Tsuper");
    }
    if style.subscript {
        return Some("Tsub");
    }
    match (style.bold, style.italic, style.strike) {
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
        .replace('"', "&quot;")
}

struct PicturePart {
    path: String,
    mime: String,
    bytes: Vec<u8>,
}

fn is_package_picture(name: &str) -> bool {
    let name = name.trim_start_matches("./");
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    name.starts_with("Pictures/")
        || matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "bmp" | "tif" | "tiff" | "webp"
        )
}

fn manifest_xml(pics: &[PicturePart]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:media-type="application/vnd.oasis.opendocument.text" manifest:full-path="/"/><manifest:file-entry manifest:media-type="text/xml" manifest:full-path="content.xml"/>"#,
    );
    for pic in pics {
        s.push_str(r#"<manifest:file-entry manifest:media-type=""#);
        s.push_str(&xml_escape(&pic.mime));
        s.push_str(r#"" manifest:full-path=""#);
        s.push_str(&xml_escape(&pic.path));
        s.push_str(r#""/>"#);
    }
    s.push_str("</manifest:manifest>");
    s
}

fn push_odt_image(s: &mut String, img: &ImageData, pics: &mut Vec<PicturePart>) {
    let mime = if img.mime.trim().is_empty() {
        sniff_image_mime(&img.bytes, "")
    } else {
        img.mime
            .split(';')
            .next()
            .unwrap_or(img.mime.as_str())
            .trim()
            .to_string()
    };
    let ext = image_ext(&mime);
    let n = pics.len() + 1;
    let path = format!("Pictures/image{n}.{ext}");
    pics.push(PicturePart {
        path: path.clone(),
        mime: mime.clone(),
        bytes: img.bytes.clone(),
    });
    let w = if img.width > 0 { img.width } else { HU_PER_INCH };
    let h = if img.height > 0 {
        img.height
    } else {
        HU_PER_INCH
    };
    let anchor = match img.wrap {
        WrapMode::Inline => "as-char",
        _ => "paragraph",
    };
    s.push_str(r#"<draw:frame draw:name="Image"#);
    s.push_str(&n.to_string());
    s.push_str(r#"" text:anchor-type=""#);
    s.push_str(anchor);
    s.push_str(r#"" svg:width=""#);
    s.push_str(&format_odf_length(w));
    s.push_str(r#"" svg:height=""#);
    s.push_str(&format_odf_length(h));
    s.push_str(r#"">"#);
    if let Some(alt) = img.alt_text.as_deref().filter(|a| !a.is_empty()) {
        s.push_str("<svg:title>");
        s.push_str(&xml_escape(alt));
        s.push_str("</svg:title>");
    }
    s.push_str(r#"<draw:image xlink:href=""#);
    s.push_str(&xml_escape(&path));
    s.push_str(r#"" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type=""#);
    s.push_str(&xml_escape(&mime));
    s.push_str(r#""/></draw:frame>"#);
}

fn take_frame_image(
    pictures: &HashMap<String, Vec<u8>>,
    href: Option<String>,
    mime: Option<String>,
    size: (Option<Hu>, Option<Hu>),
    alt: String,
    as_char: bool,
) -> Option<ImageData> {
    let href = href.filter(|h| !h.is_empty())?;
    let bytes = resolve_picture(pictures, &href)?.to_vec();
    let mime = mime
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| sniff_image_mime(&bytes, &href));
    let alt_text = {
        let t = alt.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    Some(ImageData {
        bytes,
        mime,
        width: size.0.filter(|w| *w > 0).unwrap_or(HU_PER_INCH),
        height: size.1.filter(|h| *h > 0).unwrap_or(HU_PER_INCH),
        alt_text,
        wrap: if as_char {
            WrapMode::Inline
        } else {
            WrapMode::Square
        },
    })
}

fn resolve_picture<'a>(pictures: &'a HashMap<String, Vec<u8>>, href: &str) -> Option<&'a [u8]> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let stripped = href
        .strip_prefix("./")
        .or_else(|| href.strip_prefix('/'))
        .unwrap_or(href)
        .replace('\\', "/");
    if let Some(b) = pictures.get(stripped.as_str()).or_else(|| pictures.get(href)) {
        return Some(b.as_slice());
    }
    let base = stripped.rsplit('/').next().unwrap_or(stripped.as_str());
    pictures.iter().find_map(|(k, v)| {
        k.replace('\\', "/")
            .rsplit('/')
            .next()
            .filter(|n| *n == base)
            .map(|_| v.as_slice())
    })
}

fn sniff_image_mime(bytes: &[u8], path: &str) -> String {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png".into();
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return "image/jpeg".into();
    }
    if bytes.starts_with(b"GIF8") {
        return "image/gif".into();
    }
    if bytes.starts_with(b"BM") {
        return "image/bmp".into();
    }
    mime_from_path(path).unwrap_or_else(|| "image/png".into())
}

fn mime_from_path(path: &str) -> Option<String> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    Some(
        match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "bmp" => "image/bmp",
            "tif" | "tiff" => "image/tiff",
            "webp" => "image/webp",
            _ => return None,
        }
        .into(),
    )
}

fn image_ext(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn format_odf_length(hu: Hu) -> String {
    if hu == 0 {
        return "0in".into();
    }
    let neg = hu < 0;
    let hu = hu.unsigned_abs();
    let inch = HU_PER_INCH as u32;
    let whole = hu / inch;
    let rem = hu % inch;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&whole.to_string());
    if rem == 0 {
        s.push_str("in");
        return s;
    }
    let frac = (u64::from(rem) * 10_000) / u64::from(inch);
    s.push('.');
    s.push_str(&format!("{frac:04}"));
    s.push_str("in");
    s
}

fn parse_odf_length(s: &str) -> Option<Hu> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let unit_at = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let (num, unit) = s.split_at(unit_at);
    let n = parse_fixed_4(num.trim())?;
    let unit = unit.trim().to_ascii_lowercase();
    let hu = match unit.as_str() {
        "in" | "inch" | "inches" => (i128::from(n) * i128::from(HU_PER_INCH)) / 10_000,
        "pt" => (i128::from(n) * i128::from(HU_PER_POINT)) / 10_000,
        "pc" => (i128::from(n) * i128::from(HU_PER_POINT) * 12) / 10_000,
        "cm" => (i128::from(n) * 36) / 127,
        "mm" => (i128::from(n) * 18) / 635,
        "px" => (i128::from(n) * 3) / 400,
        _ => return None,
    };
    Some(hu.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32)
}

fn parse_fixed_4(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let neg = s.starts_with('-');
    let s = s.strip_prefix(['-', '+']).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let mut v: i64 = if int.is_empty() {
        0
    } else {
        int.parse().ok()?
    };
    v = v.saturating_mul(10_000);
    if !frac.is_empty() {
        let mut place: i64 = 1_000;
        for ch in frac.chars() {
            if !ch.is_ascii_digit() {
                return None;
            }
            if place == 0 {
                continue;
            }
            v = v.saturating_add(i64::from(ch as u8 - b'0') * place);
            place /= 10;
        }
    }
    Some(if neg { -v } else { v })
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
    fn roundtrip_keeps_superscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("A");
        p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tsuper"), "{xml}");
        assert!(xml.contains("text-position=\"super"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
    }

    #[test]
    fn roundtrip_keeps_subscript() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("2");
        p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tsub"), "{xml}");
        assert!(xml.contains("text-position=\"sub"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
    }

    #[test]
    fn roundtrip_keeps_underline() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("09:00 local");
        p.runs[0].style.underline = Underline::Single;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tunder"), "{xml}");
        assert!(xml.contains("text-underline-style=\"solid\""), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.underline != Underline::None
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
    }

    #[test]
    fn roundtrip_keeps_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("amber");
        p.runs[0].style.highlight = Some(Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("Tmark"), "{xml}");
        assert!(xml.contains("#fde68a"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight.is_some() && matches!(&r.content, RunContent::Text(t) if t == "amber")
        }));
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
        assert!(xml.contains("Tlink"), "{xml}");
        assert!(xml.contains("fo:color=\"#0d9488\""), "{xml}");
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
    fn roundtrip_keeps_table_header() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into()],
            vec!["Input".into(), "letter.md".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains("table-header-rows"), "{xml}");
        assert!(xml.contains("THcell"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
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

    #[test]
    fn roundtrip_keeps_image_bytes() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "docs/assets/mark.png must be a PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(xml.contains("Pictures/image1.png"), "{xml}");
        assert!(xml.contains("xlink:href="), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/png\""), "{xml}");
        assert!(xml.contains("<svg:title>mark</svg:title>"), "{xml}");
        assert!(xml.contains("text:anchor-type=\"as-char\""), "{xml}");

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt.clone())).unwrap();
        let mut names = Vec::new();
        for i in 0..zip.len() {
            names.push(zip.by_index(i).unwrap().name().replace('\\', "/"));
        }
        assert!(
            names.iter().any(|n| n == "Pictures/image1.png"),
            "{names:?}"
        );
        {
            let mut f = zip.by_name("Pictures/image1.png").unwrap();
            let mut stored = Vec::new();
            f.read_to_end(&mut stored).unwrap();
            assert_eq!(stored.as_slice(), png);
        }
        {
            let mut f = zip.by_name("META-INF/manifest.xml").unwrap();
            let mut manifest = String::new();
            f.read_to_string(&mut manifest).unwrap();
            assert!(
                manifest.contains("Pictures/image1.png"),
                "{manifest}"
            );
            assert!(manifest.contains("image/png"), "{manifest}");
        }

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes.as_slice(), png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, HU_PER_INCH);
        assert_eq!(img.height, HU_PER_INCH);
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }
}
