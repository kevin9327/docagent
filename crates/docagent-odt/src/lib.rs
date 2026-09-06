//! OpenDocument Text (ODF) codec. Reads and writes `content.xml` paragraphs
//! and tables. Lengths stay in the IR as HWPUNIT.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Color, Document, Hu, ImageData, InlineObject,
    NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
    Underline, WrapMode, A4_HEIGHT_HU, A4_WIDTH_HU, DEFAULT_MARGIN_HU, HU_PER_INCH, HU_PER_POINT,
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
        zip.start_file("styles.xml", deflated)?;
        zip.write_all(styles_xml().as_bytes())?;
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
    let mut graphic_styles: HashMap<String, WrapMode> = HashMap::new();
    let mut list_styles: HashMap<String, ListStyleBits> = HashMap::new();
    let mut in_list_style = false;
    let mut list_style_name = String::new();
    let mut list_style_bits = ListStyleBits::bullet();
    let mut list_item_checked: Vec<bool> = Vec::new();
    let mut style_family = String::new();
    let mut style_wrap = WrapMode::Square;
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
    let mut in_frame_desc = false;
    let mut frame_width: Option<Hu> = None;
    let mut frame_height: Option<Hu> = None;
    let mut frame_title = String::new();
    let mut frame_desc = String::new();
    let mut frame_as_char = true;
    let mut frame_style: Option<String> = None;
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
                        style_family = attr(&e, "family").unwrap_or_default();
                        style_bits = StyleBits::default();
                        style_wrap = WrapMode::Square;
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
                            let (sup, sub) = text_position_super_sub(&p);
                            style_bits.superscript |= sup;
                            style_bits.subscript |= sub;
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
                    "graphic-properties" if in_style => {
                        style_wrap = wrap_from_graphic(
                            attr(&e, "wrap").as_deref(),
                            attr(&e, "wrap-contour").as_deref() == Some("true"),
                            attr(&e, "run-through").as_deref(),
                        );
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
                    "list-style" => {
                        in_list_style = true;
                        list_style_name = attr(&e, "name").unwrap_or_default();
                        list_style_bits = format_from_list_style_name(&list_style_name)
                            .unwrap_or_else(ListStyleBits::bullet);
                    }
                    "list-level-style-number" if in_list_style => {
                        list_style_bits.format = NumberFormat::Decimal;
                    }
                    "list-level-style-bullet" if in_list_style => {
                        if let Some(ch) = attr(&e, "bullet-char")
                            && let Some(checked) = checkbox_char_state(&ch)
                        {
                            list_style_bits.format = NumberFormat::Task;
                            list_style_bits.checked = checked;
                        }
                    }
                    "list" if !in_table => {
                        let name = attr(&e, "style-name").unwrap_or_default();
                        let bits = list_styles
                            .get(&name)
                            .copied()
                            .or_else(|| format_from_list_style_name(&name))
                            .unwrap_or_else(ListStyleBits::bullet);
                        list_stack.push(ListCtx {
                            format: bits.format,
                            decimal_seq: 1,
                            checked: bits.checked,
                        });
                    }
                    "list-item" => {
                        let override_name = attr(&e, "style-override");
                        let checked = match list_stack.last() {
                            Some(ctx) if ctx.format == NumberFormat::Task => override_name
                                .as_ref()
                                .and_then(|n| {
                                    list_styles
                                        .get(n)
                                        .copied()
                                        .or_else(|| format_from_list_style_name(n))
                                })
                                .map(|b| b.checked)
                                .unwrap_or(ctx.checked),
                            _ => false,
                        };
                        list_item_checked.push(checked);
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
                        in_frame_desc = false;
                        frame_width = attr(&e, "width").as_deref().and_then(parse_odf_length);
                        frame_height = attr(&e, "height").as_deref().and_then(parse_odf_length);
                        frame_title.clear();
                        frame_desc.clear();
                        frame_as_char = attr(&e, "anchor-type").as_deref() == Some("as-char");
                        frame_style = attr(&e, "style-name");
                        image_href = None;
                        image_mime = None;
                    }
                    "image" if in_frame => {
                        image_href = attr(&e, "href");
                        image_mime = attr(&e, "mime-type");
                    }
                    "title" if in_frame => in_frame_title = true,
                    "desc" if in_frame => in_frame_desc = true,
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
                            if style_family == "graphic" {
                                graphic_styles.insert(style_name.clone(), style_wrap);
                            }
                        }
                    }
                    "list-style" => {
                        in_list_style = false;
                        if !list_style_name.is_empty() {
                            list_styles.insert(list_style_name.clone(), list_style_bits);
                        }
                        list_style_name.clear();
                    }
                    "list-item" => {
                        list_item_checked.pop();
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
                                let start = match ctx.format {
                                    NumberFormat::Decimal => {
                                        let n = ctx.decimal_seq;
                                        ctx.decimal_seq = ctx.decimal_seq.saturating_add(1);
                                        Some(n)
                                    }
                                    NumberFormat::Task => list_item_checked
                                        .last()
                                        .copied()
                                        .unwrap_or(ctx.checked)
                                        .then_some(1),
                                    _ => None,
                                };
                                p.numbering = Some(NumberingRef {
                                    definition_id: match ctx.format {
                                        NumberFormat::Decimal => 1,
                                        NumberFormat::Task => 2,
                                        _ => 0,
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
                    "title" if in_frame => in_frame_title = false,
                    "desc" if in_frame => in_frame_desc = false,
                    "frame" => {
                        let wrap = if frame_as_char {
                            WrapMode::Inline
                        } else {
                            frame_style
                                .as_ref()
                                .and_then(|n| graphic_styles.get(n).copied())
                                .unwrap_or(WrapMode::Square)
                        };
                        frame_style = None;
                        if let Some(img) = take_frame_image(
                            pictures,
                            image_href.take(),
                            image_mime.take(),
                            (frame_width.take(), frame_height.take()),
                            take_frame_alt(
                                std::mem::take(&mut frame_title),
                                std::mem::take(&mut frame_desc),
                            ),
                            wrap,
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
                        in_frame_desc = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if in_frame_title {
                    let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                    frame_title.push_str(&decoded);
                } else if in_frame_desc {
                    let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                    frame_desc.push_str(&decoded);
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
    checked: bool,
}

#[derive(Clone, Copy, Debug)]
struct ListStyleBits {
    format: NumberFormat,
    checked: bool,
}

impl ListStyleBits {
    fn bullet() -> Self {
        Self {
            format: NumberFormat::Bullet,
            checked: false,
        }
    }
}

fn format_from_list_style_name(name: &str) -> Option<ListStyleBits> {
    let n = name.to_ascii_lowercase();
    if n.contains("number") {
        Some(ListStyleBits {
            format: NumberFormat::Decimal,
            checked: false,
        })
    } else if n.contains("taskdone") || n.contains("task-done") || n.contains("checked") {
        Some(ListStyleBits {
            format: NumberFormat::Task,
            checked: true,
        })
    } else if n.contains("task") || n.contains("todo") || n.contains("checklist") {
        Some(ListStyleBits {
            format: NumberFormat::Task,
            checked: false,
        })
    } else if n.contains("bullet") {
        Some(ListStyleBits::bullet())
    } else {
        None
    }
}

fn checkbox_char_state(ch: &str) -> Option<bool> {
    let mut chars = ch.trim().chars();
    let first = chars.next()?;
    match first {
        '\u{2611}' | '\u{2612}' => Some(true),
        '\u{2610}' => Some(false),
        _ => None,
    }
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
    let (checked, rest) = if let Some(rest) = text
        .strip_prefix("[x] ")
        .or_else(|| text.strip_prefix("[X] "))
    {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix("[ ] ") {
        (false, rest)
    } else if let Some((checked, rest)) = strip_checkbox_prefix(text) {
        (checked, rest)
    } else {
        return;
    };
    *text = rest.to_string();
    let level = p.numbering.as_ref().map(|n| n.level).unwrap_or(0);
    p.numbering = Some(NumberingRef {
        definition_id: 2,
        level,
        start: checked.then_some(1),
        format: Some(NumberFormat::Task),
    });
}

fn strip_checkbox_prefix(text: &str) -> Option<(bool, &str)> {
    let mut chars = text.chars();
    let checked = match chars.next()? {
        '\u{2611}' | '\u{2612}' => true,
        '\u{2610}' => false,
        _ => return None,
    };
    Some((checked, chars.as_str().trim_start()))
}

fn paragraph_is_image_only(p: &Paragraph) -> bool {
    let mut saw_image = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Image(_)) => saw_image = true,
            _ => {
                if run.display_text().chars().any(|c| !c.is_whitespace()) {
                    return false;
                }
            }
        }
    }
    saw_image
}

fn odt_para(p: &Paragraph, pics: &mut Vec<PicturePart>, fallback: Option<&str>) -> String {
    let inner = odt_runs(p, pics);
    if p.code_block {
        return format!(r#"<text:p text:style-name="CodeBlock">{inner}</text:p>"#);
    }
    if p.quote {
        return format!(r#"<text:p text:style-name="Quote">{inner}</text:p>"#);
    }
    if paragraph_is_image_only(p) {
        return format!(r#"<text:p text:style-name="Pimage">{inner}</text:p>"#);
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
        _ => match fallback {
            Some(style) => format!(r#"<text:p text:style-name="{style}">{inner}</text:p>"#),
            None => format!("<text:p>{inner}</text:p>"),
        },
    }
}

fn odt_runs(p: &Paragraph, pics: &mut Vec<PicturePart>) -> String {
    let mut s = String::new();
    let heading = p.outline_level.is_some_and(|l| l > 0);
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                // ODF requires xlink:type="simple"; without it Writer drops the link.
                s.push_str(r#"<text:a text:style-name="Tlink" xlink:type="simple" xlink:href=""#);
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
                // Writer Heading 1/2 already paint bold; a Tbold span can reset 18pt.
                let span = if heading {
                    let mut style = run.style.clone();
                    style.bold = false;
                    odt_span_style(&style)
                } else {
                    odt_span_style(&run.style)
                };
                if let Some(style) = span {
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

fn emit_list(
    body: &mut String,
    blocks: &[Block],
    i: &mut usize,
    pics: &mut Vec<PicturePart>,
    last_chain: bool,
) {
    let Some(fmt) = list_format(&blocks[*i]) else {
        return;
    };
    let base = list_level(&blocks[*i]).unwrap_or(0);
    let style = match fmt {
        NumberFormat::Decimal => "Lnumber",
        NumberFormat::Task => "Ltask",
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
            emit_list(body, blocks, i, pics, last_chain);
            continue;
        }
        if f != fmt {
            break;
        }
        let Block::Paragraph(p) = &blocks[*i] else {
            break;
        };
        let last = list_item_is_last(blocks, *i, base, fmt);
        let nested = list_item_has_nested(blocks, *i, base);
        let terminal = last_chain && last;
        // Letter CSS `ul ul{margin:.25rem 0}` is the gap from parent text to the
        // nested list (inside the li). Consecutive `li{margin:0 0 .35rem}` is
        // the wrong nested strut — Writer would paint Bay 4 too far under dock.
        let para_style = if terminal && !nested {
            "PlistLast"
        } else if nested {
            "PlistNested"
        } else {
            "Plist"
        };
        let task_done = fmt == NumberFormat::Task
            && p.numbering.as_ref().is_some_and(|n| n.start == Some(1));
        if task_done {
            body.push_str(r#"<text:list-item text:style-override="LtaskDone">"#);
        } else {
            body.push_str("<text:list-item>");
        }
        body.push_str(&odt_para(p, pics, Some(para_style)));
        *i += 1;
        if *i < blocks.len() && list_level(&blocks[*i]).is_some_and(|nl| nl > base) {
            emit_list(body, blocks, i, pics, terminal);
        }
        body.push_str("</text:list-item>");
    }
    body.push_str("</text:list>");
}

fn list_item_is_last(blocks: &[Block], i: usize, base: u8, fmt: NumberFormat) -> bool {
    let mut j = i + 1;
    while j < blocks.len() {
        let Some(f) = list_format(&blocks[j]) else {
            return true;
        };
        let lvl = list_level(&blocks[j]).unwrap_or(0);
        if lvl < base {
            return true;
        }
        if lvl == base {
            return f != fmt;
        }
        j += 1;
    }
    true
}

fn list_item_has_nested(blocks: &[Block], i: usize, base: u8) -> bool {
    i + 1 < blocks.len() && list_level(&blocks[i + 1]).is_some_and(|nl| nl > base)
}

fn table_column_count(table: &Table) -> usize {
    table.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0)
}

fn table_column_widths(table: &Table) -> Vec<Hu> {
    let ncols = table_column_count(table);
    if ncols == 0 {
        return Vec::new();
    }
    let mut widths = vec![0; ncols];
    for row in &table.rows {
        for (i, cell) in row.cells.iter().enumerate() {
            if cell.width > widths[i] {
                widths[i] = cell.width;
            }
        }
    }
    for w in &mut widths {
        if *w <= 0 {
            *w = 10000;
        }
    }
    let total: i64 = widths.iter().map(|w| i64::from(*w)).sum();
    // Letter CSS `@page` 2.75rem gutters, not Hangul 30mm. Writer 1.18in
    // DEFAULT_MARGIN punches sparse bands beside the hop grid.
    let page = (A4_WIDTH_HU - 2 * PAGE_MARGIN_X_HU).max(1);
    let target = i64::from(table.width.unwrap_or(page).max(1));
    if total > 0 && total < target {
        widths = widths
            .iter()
            .map(|w| {
                let scaled = i64::from(*w) * target / total;
                i32::try_from(scaled.max(1)).unwrap_or(i32::MAX)
            })
            .collect();
    }
    widths
}

fn push_table(
    body: &mut String,
    styles: &mut String,
    table: &Table,
    pics: &mut Vec<PicturePart>,
    table_seq: &mut u32,
) {
    *table_seq += 1;
    let name = format!("Table{table_seq}");
    let widths = table_column_widths(table);
    let table_w = i32::try_from(
        widths
            .iter()
            .map(|w| i64::from(*w))
            .sum::<i64>()
            .max(1),
    )
    .unwrap_or(i32::MAX);
    styles.push_str(r#"<style:style style:name=""#);
    styles.push_str(&name);
    styles.push_str(
        r#"" style:family="table"><style:table-properties style:width=""#,
    );
    styles.push_str(&format_odf_length(table_w));
    // Letter CSS `table{border-collapse:collapse}`. Writer HTML-import /
    // WeasyPrint UA `border-collapse:separate;border-spacing:2px` punches
    // 2px gaps between hop cells. Pin collapsing like letter.html.
    styles.push_str(r#"" style:rel-width="100%" table:align="margins" table:border-model=""#);
    styles.push_str(TABLE_BORDER_MODEL);
    styles.push_str(r#"" fo:margin-top=""#);
    styles.push_str(&format_odf_length(TABLE_MARGIN_HU));
    styles.push_str(r#"" fo:margin-bottom=""#);
    styles.push_str(&format_odf_length(TABLE_MARGIN_HU));
    styles.push_str(r#""/></style:style>"#);
    body.push_str(r#"<table:table table:name=""#);
    body.push_str(&name);
    body.push_str(r#"" table:style-name=""#);
    body.push_str(&name);
    body.push_str(r#"">"#);
    for (i, w) in widths.iter().enumerate() {
        let col_name = format!("{name}.C{i}");
        styles.push_str(r#"<style:style style:name=""#);
        styles.push_str(&col_name);
        styles.push_str(
            r#"" style:family="table-column"><style:table-column-properties style:column-width=""#,
        );
        styles.push_str(&format_odf_length(*w));
        styles.push_str(r#""/></style:style>"#);
        body.push_str(r#"<table:table-column table:style-name=""#);
        body.push_str(&col_name);
        body.push_str(r#""/>"#);
    }
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
        let para_style = if header {
            body.push_str(r#"<table:table-cell table:style-name="THcell">"#);
            Some("Pth")
        } else {
            body.push_str(r#"<table:table-cell table:style-name="Tcell">"#);
            Some("Pcell")
        };
        let mut wrote = false;
        for b in &cell.blocks {
            if let Block::Paragraph(p) = b {
                body.push_str(&odt_para(p, pics, para_style));
                wrote = true;
            }
        }
        if !wrote {
            if header {
                body.push_str(r#"<text:p text:style-name="Pth"/>"#);
            } else {
                body.push_str(r#"<text:p text:style-name="Pcell"/>"#);
            }
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
    let mut extra_styles = String::new();
    let mut table_seq = 0u32;
    let mut body = String::new();
    for section in &doc.sections {
        let mut i = 0;
        while i < section.body.len() {
            if list_format(&section.body[i]).is_some() {
                emit_list(&mut body, &section.body, &mut i, &mut pics, true);
            } else {
                match &section.body[i] {
                    Block::Paragraph(p) => {
                        body.push_str(&odt_para(p, &mut pics, Some("Pbody")));
                    }
                    Block::Table(table) => {
                        push_table(
                            &mut body,
                            &mut extra_styles,
                            table,
                            &mut pics,
                            &mut table_seq,
                        );
                    }
                    Block::Break(BreakKind::Thematic) => {
                        body.push_str(r#"<text:p text:style-name="HorizontalLine"/>"#);
                    }
                    Block::Float(_) | Block::Break(_) => {}
                }
                i += 1;
            }
        }
    }
    let pad_x = format_odf_length(CELL_PAD_X_HU);
    let pad_y = format_odf_length(CELL_PAD_Y_HU);
    let image_before = format_odf_length(IMAGE_BEFORE_HU);
    let image_after = format_odf_length(IMAGE_AFTER_HU);
    let list_item_after = format_odf_length(LIST_ITEM_AFTER_HU);
    let list_nested = format_odf_length(LIST_NESTED_GAP_HU);
    let list_after = format_odf_length(LIST_AFTER_HU);
    let quote_before = format_odf_length(QUOTE_BEFORE_HU);
    let quote_after = format_odf_length(QUOTE_AFTER_HU);
    let quote_pad_x = format_odf_length(QUOTE_PAD_X_HU);
    let quote_pad_y = format_odf_length(QUOTE_PAD_Y_HU);
    let quote_bar = format_odf_length(QUOTE_BAR_W_HU);
    let code_before = format_odf_length(CODE_BEFORE_HU);
    let code_after = format_odf_length(CODE_AFTER_HU);
    let code_pad_x = format_odf_length(CODE_PAD_X_HU);
    let code_pad_y = format_odf_length(CODE_PAD_Y_HU);
    let hr_margin = format_odf_length(HR_MARGIN_HU);
    let hr_border = format_odf_length(HR_BORDER_HU);
    let code_span_pad_x = format_odf_length(CODE_SPAN_PAD_X_HU);
    let code_span_pad_y = format_odf_length(CODE_SPAN_PAD_Y_HU);
    let mark_span_pad_x = format_odf_length(MARK_SPAN_PAD_X_HU);
    let mark_span_pad_y = format_odf_length(MARK_SPAN_PAD_Y_HU);
    let th_rule = format_odf_length(TH_RULE_HU);
    let heading1_pad = format_odf_length(HEADING1_PAD_HU);
    let heading1_rule = format_odf_length(HEADING1_RULE_HU);
    let heading1_before = format_odf_length(HEADING1_BEFORE_HU);
    let heading1_after = format_odf_length(HEADING1_AFTER_HU);
    let heading2_pad = format_odf_length(HEADING2_PAD_HU);
    let heading2_rule = format_odf_length(HEADING2_RULE_HU);
    let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
    let heading2_after = format_odf_length(HEADING2_AFTER_HU);
    let body_after = format_odf_length(BODY_AFTER_HU);
    let list_styles = list_styles_xml();
    let xml = format!(
        r##"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" office:version="1.3"><office:font-face-decls><style:font-face style:name="{BODY_FONT}" svg:font-family="&apos;{BODY_FONT}&apos;" style:font-family-generic="swiss" style:font-pitch="variable"/><style:font-face style:name="{CODE_FONT}" svg:font-family="{CODE_FONT}" style:font-family-generic="modern" style:font-pitch="fixed"/></office:font-face-decls><office:automatic-styles><style:style style:name="Heading1" style:family="paragraph" style:parent-style-name="Heading_20_1"><style:paragraph-properties fo:margin-top="{heading1_before}" fo:margin-bottom="{heading1_after}" fo:padding-bottom="{heading1_pad}" fo:border-top="none" fo:border-left="none" fo:border-right="none" fo:border-bottom="{heading1_rule} solid #134e4a" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING1_SIZE_PT}pt" fo:font-weight="bold" fo:letter-spacing="{HEADING1_LETTER_SPACING}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading2" style:family="paragraph" style:parent-style-name="Heading_20_2"><style:paragraph-properties fo:margin-top="{heading2_before}" fo:margin-bottom="{heading2_after}" fo:padding-bottom="{heading2_pad}" fo:border-top="none" fo:border-left="none" fo:border-right="none" fo:border-bottom="{heading2_rule} solid #0d9488" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING2_SIZE_PT}pt" fo:font-weight="bold" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading3" style:family="paragraph" style:parent-style-name="Heading_20_3"><style:paragraph-properties fo:margin-top="{HEADING3_BEFORE}" fo:margin-bottom="{HEADING_AFTER}" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING3_SIZE_PT}pt" fo:font-weight="bold" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Quote" style:family="paragraph"><style:paragraph-properties fo:margin-top="{quote_before}" fo:margin-bottom="{quote_after}" fo:padding-top="{quote_pad_y}" fo:padding-bottom="{quote_pad_y}" fo:padding-left="{quote_pad_x}" fo:border-left="{quote_bar} solid {QUOTE_FG}" fo:border-right="none" fo:border-top="none" fo:border-bottom="none"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{QUOTE_FG}"/></style:style><style:style style:name="HorizontalLine" style:family="paragraph"><style:paragraph-properties fo:margin-top="{hr_margin}" fo:margin-bottom="{hr_margin}" fo:border-top="{hr_border} solid #134e4a" fo:border-bottom="none" fo:border-left="none" fo:border-right="none"/></style:style><style:style style:name="CodeBlock" style:family="paragraph"><style:paragraph-properties fo:margin-top="{code_before}" fo:margin-bottom="{code_after}" fo:padding-top="{code_pad_y}" fo:padding-bottom="{code_pad_y}" fo:padding-left="{code_pad_x}" fo:padding-right="{code_pad_x}" fo:background-color="#ccfbf1"/><style:text-properties style:font-name="{CODE_FONT}" fo:font-family="{CODE_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Tbold" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-weight="bold"/></style:style><style:style style:name="Titalic" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-style="italic"/></style:style><style:style style:name="Tbi" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-weight="bold" fo:font-style="italic"/></style:style><style:style style:name="Tstrike" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" style:text-line-through-style="solid" style:text-line-through-type="single" style:text-line-through-width="auto" style:text-line-through-color="font-color"/></style:style><style:style style:name="Tbstrike" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-weight="bold" style:text-line-through-style="solid" style:text-line-through-type="single" style:text-line-through-width="auto" style:text-line-through-color="font-color"/></style:style><style:style style:name="Tistrike" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-style="italic" style:text-line-through-style="solid" style:text-line-through-type="single" style:text-line-through-width="auto" style:text-line-through-color="font-color"/></style:style><style:style style:name="Tbistrike" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-weight="bold" fo:font-style="italic" style:text-line-through-style="solid" style:text-line-through-type="single" style:text-line-through-width="auto" style:text-line-through-color="font-color"/></style:style><style:style style:name="Tcode" style:family="text"><style:text-properties fo:background-color="#ccfbf1" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="#134e4a" style:font-name="{CODE_FONT}" fo:font-family="{CODE_FONT}" fo:font-size="{CODE_SPAN_SIZE}" fo:padding="{code_span_pad_y}" fo:padding-left="{code_span_pad_x}" fo:padding-right="{code_span_pad_x}"/></style:style><style:style style:name="Tmark" style:family="text" style:parent-style-name="Mark"><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:background-color="#fde68a" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{MARK_FG}" fo:padding="{mark_span_pad_y}" fo:padding-left="{mark_span_pad_x}" fo:padding-right="{mark_span_pad_x}"/></style:style><style:style style:name="Tunder" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style><style:style style:name="Tsuper" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" style:text-position="{SUPER_POSITION}"/></style:style><style:style style:name="Tsub" style:family="text"><style:text-properties style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" style:text-position="{SUB_POSITION}"/></style:style><style:style style:name="Tcell" style:family="table-cell"><style:table-cell-properties fo:padding="{pad_y}" fo:padding-left="{pad_x}" fo:padding-right="{pad_x}" fo:border="0.5pt solid {CELL_BORDER_COLOR}" style:vertical-align="{CELL_VALIGN}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-size="{CELL_SIZE_PT}"/></style:style><style:style style:name="THcell" style:family="table-cell"><style:table-cell-properties fo:background-color="#ccfbf1" fo:padding="{pad_y}" fo:padding-left="{pad_x}" fo:padding-right="{pad_x}" fo:border="0.5pt solid {CELL_BORDER_COLOR}" fo:border-bottom="{th_rule} solid #0d9488" style:vertical-align="{CELL_VALIGN}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="#134e4a" fo:font-weight="bold" fo:font-size="{CELL_SIZE_PT}"/></style:style><style:style style:name="Pbody" style:family="paragraph" style:parent-style-name="Text_20_body"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="{body_after}" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Pth" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="0in"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="#134e4a" fo:font-weight="bold" fo:font-size="{CELL_SIZE_PT}"/></style:style><style:style style:name="Pcell" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="0in"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}" fo:font-size="{CELL_SIZE_PT}"/></style:style><style:style style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}"/></style:style><style:style style:name="Plist" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="{list_item_after}" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="PlistNested" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="{list_nested}" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="PlistLast" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="{list_after}" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Tlink" style:family="text" style:parent-style-name="Internet_20_Link"><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{LINK_FG}" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style>{list_styles}<style:style style:name="Ginline" style:family="graphic" style:parent-style-name="Graphics"><style:graphic-properties draw:fill="none" fo:background-color="transparent" style:background-transparency="100%" fo:border="none" draw:color-mode="standard" draw:image-opacity="100%" style:vertical-pos="top" style:vertical-rel="baseline" style:horizontal-pos="center" style:horizontal-rel="paragraph"/></style:style><style:style style:name="Gsquare" style:family="graphic"><style:graphic-properties style:wrap="parallel"/></style:style><style:style style:name="Gtight" style:family="graphic"><style:graphic-properties style:wrap="parallel" style:wrap-contour="true"/></style:style><style:style style:name="Gthrough" style:family="graphic"><style:graphic-properties style:wrap="run-through"/></style:style><style:style style:name="Gtopbottom" style:family="graphic"><style:graphic-properties style:wrap="none"/></style:style><style:style style:name="Gbehind" style:family="graphic"><style:graphic-properties style:wrap="run-through" style:run-through="background"/></style:style><style:style style:name="Ginfront" style:family="graphic"><style:graphic-properties style:wrap="run-through" style:run-through="foreground"/></style:style>{extra_styles}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"##
    );
    (xml, pics)
}

fn styles_xml() -> String {
    let page_w = format_odf_length(A4_WIDTH_HU);
    let page_h = format_odf_length(A4_HEIGHT_HU);
    let margin_x = format_odf_length(PAGE_MARGIN_X_HU);
    let margin_y = format_odf_length(PAGE_MARGIN_Y_HU);
    let heading1_pad = format_odf_length(HEADING1_PAD_HU);
    let heading1_rule = format_odf_length(HEADING1_RULE_HU);
    let heading1_before = format_odf_length(HEADING1_BEFORE_HU);
    let heading1_after = format_odf_length(HEADING1_AFTER_HU);
    let heading2_pad = format_odf_length(HEADING2_PAD_HU);
    let heading2_rule = format_odf_length(HEADING2_RULE_HU);
    let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
    let heading2_after = format_odf_length(HEADING2_AFTER_HU);
    let body_after = format_odf_length(BODY_AFTER_HU);
    let mark_span_pad_x = format_odf_length(MARK_SPAN_PAD_X_HU);
    let mark_span_pad_y = format_odf_length(MARK_SPAN_PAD_Y_HU);
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?><office:document-styles xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" office:version="1.3"><office:font-face-decls><style:font-face style:name="{BODY_FONT}" svg:font-family="&apos;{BODY_FONT}&apos;" style:font-family-generic="swiss" style:font-pitch="variable"/><style:font-face style:name="Liberation Serif" svg:font-family="&apos;Liberation Serif&apos;" style:font-family-generic="roman" style:font-pitch="variable"/><style:font-face style:name="{CODE_FONT}" svg:font-family="{CODE_FONT}" style:font-family-generic="modern" style:font-pitch="fixed"/></office:font-face-decls><office:styles><style:default-style style:family="graphic"><style:graphic-properties draw:fill="none" fo:background-color="transparent" style:background-transparency="100%" fo:border="none" draw:color-mode="standard" draw:image-opacity="100%"/></style:default-style><style:default-style style:family="paragraph"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="0in" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="10pt" fo:language="en" fo:country="US" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:default-style><style:style style:name="Standard" style:family="paragraph" style:class="text"><style:paragraph-properties fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="10pt" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading" style:family="paragraph" style:parent-style-name="Standard" style:next-style-name="Text_20_body" style:class="text"><style:paragraph-properties fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-weight="bold" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Heading" style:next-style-name="Text_20_body" style:default-outline-level="1" style:class="text"><style:paragraph-properties fo:margin-top="{heading1_before}" fo:margin-bottom="{heading1_after}" fo:padding-bottom="{heading1_pad}" fo:border-top="none" fo:border-left="none" fo:border-right="none" fo:border-bottom="{heading1_rule} solid #134e4a" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING1_SIZE_PT}pt" fo:font-weight="bold" fo:letter-spacing="{HEADING1_LETTER_SPACING}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:parent-style-name="Heading" style:next-style-name="Text_20_body" style:default-outline-level="2" style:class="text"><style:paragraph-properties fo:margin-top="{heading2_before}" fo:margin-bottom="{heading2_after}" fo:padding-bottom="{heading2_pad}" fo:border-top="none" fo:border-left="none" fo:border-right="none" fo:border-bottom="{heading2_rule} solid #0d9488" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING2_SIZE_PT}pt" fo:font-weight="bold" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Heading_20_3" style:display-name="Heading 3" style:family="paragraph" style:parent-style-name="Heading" style:next-style-name="Text_20_body" style:default-outline-level="3" style:class="text"><style:paragraph-properties fo:margin-top="{HEADING3_BEFORE}" fo:margin-bottom="{HEADING_AFTER}" fo:line-height="{HEADING_LINE_HEIGHT}" fo:keep-with-next="always"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:font-size="{HEADING3_SIZE_PT}pt" fo:font-weight="bold" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Text_20_body" style:display-name="Text body" style:family="paragraph" style:parent-style-name="Standard" style:class="text"><style:paragraph-properties fo:margin-top="0in" fo:margin-bottom="{body_after}" fo:line-height="{BODY_LINE_HEIGHT}"/><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{BODY_FG}"/></style:style><style:style style:name="Internet_20_Link" style:display-name="Internet Link" style:family="text"><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{LINK_FG}" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style><style:style style:name="Visited_20_Internet_20_Link" style:display-name="Visited Internet Link" style:family="text"><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{LINK_FG}" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color"/></style:style><style:style style:name="Mark" style:display-name="Mark" style:family="text"><style:text-properties style:font-name="{BODY_FONT}" fo:font-family="{BODY_FONT}" fo:background-color="#fde68a" style:use-window-font-color="{USE_WINDOW_FONT_COLOR}" fo:color="{MARK_FG}" fo:padding="{mark_span_pad_y}" fo:padding-left="{mark_span_pad_x}" fo:padding-right="{mark_span_pad_x}"/></style:style><style:style style:name="Graphics" style:family="graphic"><style:graphic-properties draw:fill="none" fo:background-color="transparent" style:background-transparency="100%" fo:border="none" draw:color-mode="standard" draw:image-opacity="100%" text:anchor-type="as-char" style:vertical-pos="top" style:vertical-rel="baseline" style:horizontal-pos="center" style:horizontal-rel="paragraph"/></style:style></office:styles><office:automatic-styles><style:page-layout style:name="Mpm1"><style:page-layout-properties fo:page-width="{page_w}" fo:page-height="{page_h}" style:num-format="1" style:print-orientation="portrait" fo:margin-top="{margin_y}" fo:margin-bottom="{margin_y}" fo:margin-left="{margin_x}" fo:margin-right="{margin_x}"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name="Standard" style:page-layout-name="Mpm1"/></office:master-styles></office:document-styles>"##
    )
}

fn list_styles_xml() -> String {
    let mut s = String::new();
    s.push_str(
        r##"<style:style style:name="TtaskBullet" style:family="text"><style:text-properties style:use-window-font-color="false" fo:color="#134e4a" fo:font-size="12pt"/></style:style>"##,
    );
    s.push_str(r#"<text:list-style style:name="Lbullet">"#);
    for level in 1..=3 {
        s.push_str(r#"<text:list-level-style-bullet text:level=""#);
        s.push_str(&level.to_string());
        s.push_str(r#"" text:bullet-char="•">"#);
        s.push_str(&list_level_props(list_level_left(level)));
        s.push_str("</text:list-level-style-bullet>");
    }
    s.push_str("</text:list-style>");
    s.push_str(r#"<text:list-style style:name="Lnumber">"#);
    for level in 1..=3 {
        s.push_str(r#"<text:list-level-style-number text:level=""#);
        s.push_str(&level.to_string());
        s.push_str(r#"" style:num-format="1" style:num-suffix=".">"#);
        s.push_str(&list_level_props(list_level_left(level)));
        s.push_str("</text:list-level-style-number>");
    }
    s.push_str("</text:list-style>");
    s.push_str(&task_list_style("Ltask", TASK_BOX_EMPTY));
    s.push_str(&task_list_style("LtaskDone", TASK_BOX_CHECKED));
    s
}

fn task_list_style(name: &str, bullet: &str) -> String {
    let mut s = String::new();
    s.push_str(r#"<text:list-style style:name=""#);
    s.push_str(name);
    s.push_str("\">");
    for level in 1..=3 {
        s.push_str(r#"<text:list-level-style-bullet text:level=""#);
        s.push_str(&level.to_string());
        s.push_str(r#"" text:style-name="TtaskBullet" text:bullet-char=""#);
        s.push_str(bullet);
        s.push_str("\">");
        s.push_str(&list_level_props(list_level_left(level)));
        s.push_str("</text:list-level-style-bullet>");
    }
    s.push_str("</text:list-style>");
    s
}

fn list_level_left(level: i32) -> Hu {
    LIST_INDENT_HU.saturating_mul(level)
}

fn list_level_props(left: Hu) -> String {
    let left = format_odf_length(left);
    let hang = format_odf_length(-LIST_HANGING_HU);
    format!(
        r#"<style:list-level-properties text:list-level-position-and-space-mode="label-alignment"><style:list-level-label-alignment text:label-followed-by="listtab" text:list-tab-stop-position="{left}" fo:text-indent="{hang}" fo:margin-left="{left}"/></style:list-level-properties>"#
    )
}

/// Writer Font Effects Super/Sub. Keyword or signed % of font height.
/// `33% 58%` / `-33% 58%` is Writer with Automatic off; `super`/`sub` is on.
fn text_position_super_sub(raw: &str) -> (bool, bool) {
    let first = raw
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches('%');
    let lower = first.to_ascii_lowercase();
    if lower.starts_with("super") {
        return (true, false);
    }
    if lower.starts_with("sub") {
        return (false, true);
    }
    let neg = first.starts_with('-');
    let mag = first.trim_start_matches(['-', '+']);
    let int_part = mag.split('.').next().unwrap_or("");
    let n: i32 = int_part.parse().unwrap_or(0);
    if n == 0 {
        (false, false)
    } else if neg {
        (false, true)
    } else {
        (true, false)
    }
}

/// Writer Font Effects Single. Style-only Tstrike/Tunder is a missing line.
/// Style-only Tsuper/Tsub without text-position is a missing raise/lower.
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
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:media-type="application/vnd.oasis.opendocument.text" manifest:full-path="/"/><manifest:file-entry manifest:media-type="text/xml" manifest:full-path="content.xml"/><manifest:file-entry manifest:media-type="text/xml" manifest:full-path="styles.xml"/>"#,
    );
    if !pics.is_empty() {
        s.push_str(r#"<manifest:file-entry manifest:media-type="" manifest:full-path="Pictures/"/>"#);
    }
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
    let mime = odt_image_mime(&img.mime, &img.bytes, "");
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
    // 16px at 96dpi is 12pt; LibreOffice needs the 24pt on-page floor.
    let (w, h) = min_on_page_size(w, h);
    let (anchor, wrap_style) = odt_frame_wrap(img.wrap);
    s.push_str(r#"<draw:frame"#);
    if let Some(style) = wrap_style {
        s.push_str(r#" draw:style-name=""#);
        s.push_str(style);
        s.push('"');
    }
    s.push_str(r#" draw:name="Image"#);
    s.push_str(&n.to_string());
    s.push_str(r#"" text:anchor-type=""#);
    s.push_str(anchor);
    s.push_str(r#"" svg:width=""#);
    s.push_str(&format_odf_length(w));
    s.push_str(r#"" svg:height=""#);
    s.push_str(&format_odf_length(h));
    // Writer paints as-char frames with z-index; without it the PNG can vanish.
    s.push_str(r#"" draw:z-index="0">"#);
    s.push_str(r#"<draw:image xlink:href=""#);
    s.push_str(&xml_escape(&path));
    s.push_str(r#"" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad" draw:mime-type=""#);
    s.push_str(&xml_escape(&mime));
    s.push_str(r#""/>"#);
    if let Some(alt) = img.alt_text.as_deref().filter(|a| !a.is_empty()) {
        let escaped = xml_escape(alt);
        s.push_str("<svg:title>");
        s.push_str(&escaped);
        s.push_str("</svg:title>");
        s.push_str("<svg:desc>");
        s.push_str(&escaped);
        s.push_str("</svg:desc>");
    }
    s.push_str("</draw:frame>");
}

fn take_frame_alt(title: String, desc: String) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    desc.trim().to_string()
}

fn take_frame_image(
    pictures: &HashMap<String, Vec<u8>>,
    href: Option<String>,
    mime: Option<String>,
    size: (Option<Hu>, Option<Hu>),
    alt: String,
    wrap: WrapMode,
) -> Option<ImageData> {
    let href = href.filter(|h| !h.is_empty())?;
    let bytes = resolve_picture(pictures, &href)?.to_vec();
    let mime = odt_image_mime(mime.as_deref().unwrap_or(""), &bytes, &href);
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
        wrap,
    })
}

/// Default cell inset X when IR padding is 0. Letter CSS
/// `th,td{padding:.4rem .65rem}` at 16px rem / 96dpi: 0.65rem = 780 HU.
const CELL_PAD_X_HU: Hu = 780;
/// Default cell inset Y when IR padding is 0. 0.4rem = 480 HU.
const CELL_PAD_Y_HU: Hu = 480;
/// Letter CSS `th{border-bottom:2px solid #0d9488}` at 96dpi: 2px = 150 HU.
const TH_RULE_HU: Hu = 150;
/// Letter CSS `th,td{font-size:.95rem}` at 16px rem / 96dpi: 0.95rem = 11.4pt
/// (1140 HU). Body 10pt is the wrong table type next to WeasyPrint.
const CELL_SIZE_PT: &str = "11.4pt";
const CELL_SIZE_HU: Hu = 1140;
/// Letter CSS `th,td{border:1px solid #cbd5e1}`. Writer 0.5pt hairline stays;
/// LibreOffice `#000000` is a black TableGrid, not WeasyPrint letter input.
const CELL_BORDER_COLOR: &str = "#cbd5e1";
/// Letter CSS `th,td{vertical-align:top}`. Writer Table cell / HTML-import
/// `style:vertical-align="middle"` centers wrapped hop cells and punches
/// sparse bands vs WeasyPrint letter input.
const CELL_VALIGN: &str = "top";
/// Letter CSS `h1{padding-bottom:.4rem}` at 16px rem / 96dpi = 480 HU.
const HEADING1_PAD_HU: Hu = 480;
/// Letter CSS `h1{border-bottom:3px solid #134e4a}` at 96dpi = 225 HU.
const HEADING1_RULE_HU: Hu = 225;
/// Letter CSS `h2{padding-bottom:.2rem}` = 240 HU.
const HEADING2_PAD_HU: Hu = 240;
/// Letter CSS `h2{border-bottom:2px solid #0d9488}` at 96dpi = 150 HU.
const HEADING2_RULE_HU: Hu = 150;
/// Letter CSS `table{margin:1rem 0}` = 1200 HU. Writer default 0in jams the hop grid.
const TABLE_MARGIN_HU: Hu = 1200;
/// Letter CSS `table{border-collapse:collapse}`. Writer HTML-import /
/// WeasyPrint UA `border-collapse:separate;border-spacing:2px` punches
/// 2px gaps between hop cells (sparse grid). Pin collapsing like letter.html.
const TABLE_BORDER_MODEL: &str = "collapsing";
/// Gap before an image-only block. Letter CSS `img{margin:.6rem 0}` top
/// at 16px rem / 96dpi: 0.6rem = 720 HU. Writer `0in` jams the 24pt mark
/// under the H2 rule.
const IMAGE_BEFORE_HU: Hu = 720;
/// Gap after an image-only block. Letter CSS `p{margin:0 0 .7rem}` collapsed
/// with `img{margin:.6rem 0}` is 0.7rem = 840 HU.
const IMAGE_AFTER_HU: Hu = 840;
/// Writer Heading 1: 130% of 14pt parent = 18.2pt (Writer Guide 26.2).
const HEADING1_SIZE_PT: i32 = 18;
/// Letter CSS `h1{letter-spacing:-.02em}` at Heading 1 18pt = -0.36pt.
/// Writer Font Effects Spacing; 0pt / `normal` drops WeasyPrint tracking.
const HEADING1_LETTER_SPACING: &str = "-0.36pt";
/// Writer Heading 2: 115% of 14pt parent = 16.1pt.
const HEADING2_SIZE_PT: i32 = 16;
/// Writer Heading 3: 101% of 14pt parent ≈ 14pt.
const HEADING3_SIZE_PT: i32 = 14;
/// Letter CSS `h1{margin:0 0 .6rem}` top. Writer Heading 1 `0.1665in`
/// (~1200 HU) punches a sparse band under the 2.5rem page margin vs WeasyPrint.
const HEADING1_BEFORE_HU: Hu = 0;
/// Letter CSS `h1{margin:0 0 .6rem}` bottom at 16px rem / 96dpi:
/// 0.6rem = 720 HU. Writer Heading 1 `0.0835in` (~601 HU) is the wrong
/// strut after the 3px rule vs WeasyPrint. Same 0.6rem as [`IMAGE_BEFORE_HU`].
const HEADING1_AFTER_HU: Hu = 720;
/// Writer Heading `fo:margin-bottom` (0.212cm). Heading 1 uses
/// [`HEADING1_AFTER_HU`]; Heading 2 uses [`HEADING2_AFTER_HU`].
const HEADING_AFTER: &str = "0.0835in";
/// Letter CSS `h2{margin:1.1rem 0 .45rem}` top at 16px rem / 96dpi:
/// 1.1rem = 1320 HU. Writer Heading 2 `0.139in` (~1000 HU) jams H1→H2
/// vs WeasyPrint (same strut as [`HR_MARGIN_HU`]).
const HEADING2_BEFORE_HU: Hu = 1320;
/// Letter CSS `h2{margin:1.1rem 0 .45rem}` bottom at 16px rem / 96dpi:
/// 0.45rem = 540 HU. Writer Heading 2 `0.0835in` (~601 HU) is the wrong
/// strut after the 2px rule vs WeasyPrint. The 24pt mark still uses
/// [`IMAGE_BEFORE_HU`] (0.6rem), which wins CSS collapse.
const HEADING2_AFTER_HU: Hu = 540;
/// Writer Heading 3 `fo:margin-top` (0.247cm).
const HEADING3_BEFORE: &str = "0.0972in";
/// Letter CSS `p{margin:0 0 .7rem}` at 16px rem / 96dpi: 0.7rem = 840 HU.
/// Writer Text body `0.0972in` (~700 HU) is the wrong body strut vs WeasyPrint.
/// Same 0.7rem as [`IMAGE_AFTER_HU`] (collapsed p+img).
const BODY_AFTER_HU: Hu = 840;
/// Writer Text body 1.15 lines.
const BODY_LINE_HEIGHT: &str = "115%";
/// Letter CSS `h1,h2{line-height:1.2}`. Inherited Writer body 115% jams heading leading vs WeasyPrint.
const HEADING_LINE_HEIGHT: &str = "120%";
/// Letter CSS `ul,ol{padding-left:1.25rem}` at 16px rem / 96dpi: 1.25rem = 1500 HU.
/// Writer 0.5in (3600 HU) is a sparse gutter next to WeasyPrint.
const LIST_INDENT_HU: Hu = 1500;
/// Hanging label inside the 1.25rem gutter. Writer -0.25in (1800 HU) hangs past
/// letter 1.25rem (the bullet would sit in the page margin). 1rem (1200 HU)
/// keeps "1." / the disc in the gutter and tabs to the 1.25rem text start.
const LIST_HANGING_HU: Hu = 1200;
/// Writer To-Do unchecked (U+2610 BALLOT BOX). ASCII `[ ]` is a missing box.
const TASK_BOX_EMPTY: &str = "\u{2610}";
/// Writer To-Do checked (U+2611 BALLOT BOX WITH CHECK). ASCII `[x]` is a missing box.
const TASK_BOX_CHECKED: &str = "\u{2611}";
/// Letter CSS `li{margin:0 0 .35rem}` = 420 HU. Unstyled list items are jammed.
const LIST_ITEM_AFTER_HU: Hu = 420;
/// Letter CSS `ul ul,ol ul,ul ol,ol ol{margin:.25rem 0}` at 16px rem / 96dpi:
/// 0.25rem = 300 HU. Sibling [`LIST_ITEM_AFTER_HU`] 0.35rem is the wrong nested
/// gap (letter.md Bay 4 under the dock task, PDF/HTML under Convert).
const LIST_NESTED_GAP_HU: Hu = 300;
/// Letter CSS `ul,ol{margin:0 0 1rem}` after the last item = 1200 HU.
const LIST_AFTER_HU: Hu = 1200;
/// Letter CSS `blockquote{margin:.5rem 0 1rem}` top = 600 HU. IR 80 is jammed.
const QUOTE_BEFORE_HU: Hu = 600;
/// Letter CSS blockquote margin-bottom 1rem = 1200 HU. IR 200 is jammed.
const QUOTE_AFTER_HU: Hu = 1200;
/// Letter CSS `blockquote{padding:.15rem 0 .15rem 1rem}` left = 1200 HU.
const QUOTE_PAD_X_HU: Hu = 1200;
/// Letter CSS blockquote padding-top/bottom 0.15rem = 180 HU.
const QUOTE_PAD_Y_HU: Hu = 180;
/// Letter CSS `blockquote{border-left:4px solid #134e4a}` at 96dpi = 300 HU.
/// Writer 0.02in / HTML-import 0.0492in gray is a missing bar.
const QUOTE_BAR_W_HU: Hu = 300;
/// Letter CSS `blockquote{color:#134e4a}` (same teal as the 4px bar).
const QUOTE_FG: &str = "#134e4a";
/// Letter CSS `body,article,h1,h2{color:#134e4a}` — not Writer UA black / slate `#111827`.
/// Writer Heading parent is navy `#000080`; style-only Heading 1 is that blue, not letter teal.
const BODY_FG: &str = "#134e4a";
/// Letter CSS `article{font-family:"Segoe UI",Roboto,Helvetica,Arial,sans-serif}`.
/// Writer default Liberation Sans is not WeasyPrint letter input.
const BODY_FONT: &str = "Segoe UI";
/// Letter CSS `pre,code{font-family:ui-monospace,Consolas,monospace}`.
/// `fo:font-family` without `style:font-name` keeps the body face in Writer.
const CODE_FONT: &str = "Consolas";
/// Letter CSS `a,a:visited{color:#0d9488}`. Writer Internet Link is navy `#000080`;
/// Visited Internet Link is maroon `#800000` — style-only hyperlinks go blue/purple.
const LINK_FG: &str = "#0d9488";
/// Writer built-in Heading / Internet Link `fo:color` (Liberation navy).
const WRITER_HEADING_NAVY: &str = "#000080";
/// Writer built-in Visited Internet Link `fo:color` (Liberation maroon).
const WRITER_VISITED_MAROON: &str = "#800000";
/// Writer default-style sets this true, which ignores `fo:color` (Heading stays navy).
const USE_WINDOW_FONT_COLOR: &str = "false";
/// Letter CSS `pre{margin:.6rem 0 1rem}` top = 720 HU. IR 80 is jammed.
const CODE_BEFORE_HU: Hu = 720;
/// Letter CSS pre margin-bottom 1rem = 1200 HU. IR 200 is jammed.
const CODE_AFTER_HU: Hu = 1200;
/// Letter CSS `pre{padding:.75rem 1rem}` left = 1200 HU.
const CODE_PAD_X_HU: Hu = 1200;
/// Letter CSS pre padding-top/bottom 0.75rem = 900 HU.
const CODE_PAD_Y_HU: Hu = 900;
/// Letter CSS `code{padding:.1em .35em}` at 10pt: 0.35em = 350 HU.
const CODE_SPAN_PAD_X_HU: Hu = 350;
/// Letter CSS inline-code pad-y 0.1em at 10pt = 100 HU.
const CODE_SPAN_PAD_Y_HU: Hu = 100;
/// Letter CSS `code{font-size:.92em}`. Relative so Writer body size stays parent.
const CODE_SPAN_SIZE: &str = "92%";
/// Letter CSS `mark{padding:.05em .2em}` at 10pt: 0.2em = 200 HU.
const MARK_SPAN_PAD_X_HU: Hu = 200;
/// Letter CSS mark pad-y 0.05em at 10pt = 50 HU.
const MARK_SPAN_PAD_Y_HU: Hu = 50;
/// Letter CSS `mark{color:#134e4a}`. Amber fill without teal type is UA
/// Highlight black-on-yellow, not WeasyPrint letter input.
const MARK_FG: &str = BODY_FG;
/// Letter CSS `hr{border-top:3px}` at 96dpi = 225 HU. Writer 0.02in (144 HU) is missing.
const HR_BORDER_HU: Hu = 225;
/// Letter CSS `hr{margin:1.1rem 0}` = 1320 HU. Writer 0.15in (1080 HU) is sparse/jammed.
const HR_MARGIN_HU: Hu = 1320;
/// Letter CSS `@page{size:A4;margin:2.5rem 2.75rem}` left/right at 16px rem /
/// 96dpi: 2.75rem = 3300 HU. Hangul [`DEFAULT_MARGIN_HU`] 8504 (~30mm) and
/// WeasyPrint UA `@page{margin:75px}` (5625 HU) are a sparse Writer frame
/// (LibreOffice / Pandoc keep 1.18in).
const PAGE_MARGIN_X_HU: Hu = 3300;
/// Letter CSS `@page` top/bottom 2.5rem = 3000 HU.
const PAGE_MARGIN_Y_HU: Hu = 3000;
/// Writer Font Effects default relative size for Super/Sub (LibreOffice 58%).
const SUPER_REL_PCT: i32 = 58;
/// Writer Automatic superscript: keyword + 58% relative size.
const SUPER_POSITION: &str = "super 58%";
/// Writer Automatic subscript: keyword + 58% relative size.
const SUB_POSITION: &str = "sub 58%";
const _: () = assert!(HEADING1_SIZE_PT == 18);
const _: () = assert!(HEADING1_LETTER_SPACING.len() == 7);
const _: () = assert!(HEADING1_LETTER_SPACING.as_bytes()[0] == b'-');
const _: () = assert!(HEADING_LINE_HEIGHT.len() == 4);
const _: () = assert!(HEADING_LINE_HEIGHT.as_bytes()[0] == b'1' && HEADING_LINE_HEIGHT.as_bytes()[2] == b'0');
const _: () = assert!(HEADING2_SIZE_PT == 16);
const _: () = assert!(HEADING3_SIZE_PT == 14);
const _: () = assert!(LIST_INDENT_HU == 1500);
const _: () = assert!(LIST_HANGING_HU == 1200);
const _: () = assert!(LIST_HANGING_HU < LIST_INDENT_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU == 420);
const _: () = assert!(LIST_NESTED_GAP_HU == 300);
const _: () = assert!(LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU);
const _: () = assert!(LIST_AFTER_HU == 1200);
const _: () = assert!(LIST_ITEM_AFTER_HU < LIST_AFTER_HU);
const _: () = assert!(LIST_NESTED_GAP_HU < LIST_AFTER_HU);
const _: () = assert!(QUOTE_BEFORE_HU == 600);
const _: () = assert!(QUOTE_AFTER_HU == 1200);
const _: () = assert!(QUOTE_PAD_X_HU == 1200);
const _: () = assert!(QUOTE_PAD_Y_HU == 180);
const _: () = assert!(QUOTE_BAR_W_HU == 300);
const _: () = assert!(QUOTE_FG.len() == 7);
const _: () = assert!(BODY_FG.len() == 7);
const _: () = assert!(BODY_FG.as_bytes()[1] == b'1' && BODY_FG.as_bytes()[6] == b'a');
const _: () = assert!(BODY_FONT.len() == 8);
const _: () = assert!(BODY_FONT.as_bytes()[0] == b'S' && BODY_FONT.as_bytes()[6] == b'U');
const _: () = assert!(CODE_FONT.len() == 8);
const _: () = assert!(CODE_FONT.as_bytes()[0] == b'C');
const _: () = assert!(LINK_FG.len() == 7);
const _: () = assert!(LINK_FG.as_bytes()[1] == b'0' && LINK_FG.as_bytes()[6] == b'8');
const _: () = assert!(WRITER_HEADING_NAVY.len() == 7);
const _: () = assert!(WRITER_HEADING_NAVY.as_bytes()[1] == b'0' && WRITER_HEADING_NAVY.as_bytes()[6] == b'0');
const _: () = assert!(WRITER_VISITED_MAROON.len() == 7);
const _: () = assert!(WRITER_VISITED_MAROON.as_bytes()[1] == b'8' && WRITER_VISITED_MAROON.as_bytes()[6] == b'0');
const _: () = assert!(USE_WINDOW_FONT_COLOR.len() == 5);
const _: () = assert!(USE_WINDOW_FONT_COLOR.as_bytes()[0] == b'f');
const _: () = assert!(CODE_BEFORE_HU == 720);
const _: () = assert!(CODE_AFTER_HU == 1200);
const _: () = assert!(CODE_PAD_X_HU == 1200);
const _: () = assert!(CODE_PAD_Y_HU == 900);
const _: () = assert!(CODE_SPAN_PAD_X_HU == 350);
const _: () = assert!(CODE_SPAN_PAD_Y_HU == 100);
const _: () = assert!(MARK_SPAN_PAD_X_HU == 200);
const _: () = assert!(MARK_SPAN_PAD_Y_HU == 50);
const _: () = assert!(MARK_FG.len() == 7);
const _: () = assert!(MARK_FG.as_bytes()[1] == b'1' && MARK_FG.as_bytes()[6] == b'a');
const _: () = assert!(HR_BORDER_HU == 225);
const _: () = assert!(HR_MARGIN_HU == 1320);
const _: () = assert!(PAGE_MARGIN_X_HU == 3300);
const _: () = assert!(PAGE_MARGIN_Y_HU == 3000);
const _: () = assert!(PAGE_MARGIN_X_HU == 44 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU == 40 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_X_HU > PAGE_MARGIN_Y_HU);
const _: () = assert!(PAGE_MARGIN_X_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_Y_HU < DEFAULT_MARGIN_HU);
const _: () = assert!(PAGE_MARGIN_X_HU != 75 * (HU_PER_INCH / 96));
const _: () = assert!(PAGE_MARGIN_Y_HU != 75 * (HU_PER_INCH / 96));
const _: () = assert!(TH_RULE_HU == 150);
const _: () = assert!(CELL_SIZE_HU == 1140);
const _: () = assert!(CELL_SIZE_PT.len() == 6);
const _: () = assert!(CELL_SIZE_PT.as_bytes()[0] == b'1' && CELL_SIZE_PT.as_bytes()[3] == b'4');
const _: () = assert!(CELL_BORDER_COLOR.len() == 7);
const _: () = assert!(CELL_BORDER_COLOR.as_bytes()[1] == b'c' && CELL_BORDER_COLOR.as_bytes()[6] == b'1');
const _: () = assert!(CELL_VALIGN.len() == 3);
const _: () = assert!(CELL_VALIGN.as_bytes()[0] == b't' && CELL_VALIGN.as_bytes()[2] == b'p');
const _: () = assert!(HEADING1_PAD_HU == 480);
const _: () = assert!(HEADING1_RULE_HU == 225);
const _: () = assert!(HEADING2_PAD_HU == 240);
const _: () = assert!(HEADING2_RULE_HU == 150);
const _: () = assert!(HEADING2_BEFORE_HU == 1320);
const _: () = assert!(HEADING2_BEFORE_HU == HR_MARGIN_HU);
const _: () = assert!(HEADING2_AFTER_HU == 540);
const _: () = assert!(HEADING2_AFTER_HU < IMAGE_BEFORE_HU);
const _: () = assert!(HEADING2_AFTER_HU < HEADING2_BEFORE_HU);
const _: () = assert!(HEADING1_BEFORE_HU == 0);
const _: () = assert!(HEADING1_BEFORE_HU < HEADING1_AFTER_HU);
const _: () = assert!(HEADING1_BEFORE_HU < HEADING2_BEFORE_HU);
const _: () = assert!(HEADING1_AFTER_HU == 720);
const _: () = assert!(HEADING1_AFTER_HU == IMAGE_BEFORE_HU);
const _: () = assert!(HEADING1_AFTER_HU > HEADING2_AFTER_HU);
const _: () = assert!(TABLE_MARGIN_HU == 1200);
const _: () = assert!(IMAGE_BEFORE_HU == 720);
const _: () = assert!(IMAGE_AFTER_HU == 840);
const _: () = assert!(IMAGE_BEFORE_HU < IMAGE_AFTER_HU);
const _: () = assert!(BODY_AFTER_HU == 840);
const _: () = assert!(BODY_AFTER_HU == IMAGE_AFTER_HU);
const _: () = assert!(HEADING1_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(HEADING2_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_ITEM_AFTER_HU < BODY_AFTER_HU);
const _: () = assert!(LIST_AFTER_HU > BODY_AFTER_HU);
const _: () = assert!(TABLE_BORDER_MODEL.len() == 10);
const _: () = assert!(TABLE_BORDER_MODEL.as_bytes()[0] == b'c' && TABLE_BORDER_MODEL.as_bytes()[9] == b'g');
const _: () = assert!(SUPER_REL_PCT == 58);
const _: () = assert!(SUPER_POSITION.len() == 9);
const _: () = assert!(SUB_POSITION.len() == 7);
/// Smallest on-page image edge (24pt). 16px at 96dpi is 12pt.
const MIN_IMAGE_EDGE_HU: Hu = 24 * HU_PER_POINT;
const _: () = assert!(MIN_IMAGE_EDGE_HU >= HU_PER_INCH / 4);

fn min_on_page_size(width: Hu, height: Hu) -> (Hu, Hu) {
    let short = width.min(height);
    if short >= MIN_IMAGE_EDGE_HU || short <= 0 {
        return (width.max(0), height.max(0));
    }
    let w = (i64::from(width) * i64::from(MIN_IMAGE_EDGE_HU) / i64::from(short)).max(1);
    let h = (i64::from(height) * i64::from(MIN_IMAGE_EDGE_HU) / i64::from(short)).max(1);
    (
        i32::try_from(w).unwrap_or(i32::MAX),
        i32::try_from(h).unwrap_or(i32::MAX),
    )
}

fn odt_frame_wrap(wrap: WrapMode) -> (&'static str, Option<&'static str>) {
    match wrap {
        WrapMode::Inline => ("as-char", Some("Ginline")),
        WrapMode::Square => ("paragraph", Some("Gsquare")),
        WrapMode::Tight => ("paragraph", Some("Gtight")),
        WrapMode::Through => ("paragraph", Some("Gthrough")),
        WrapMode::TopAndBottom => ("paragraph", Some("Gtopbottom")),
        WrapMode::Behind => ("paragraph", Some("Gbehind")),
        WrapMode::InFront => ("paragraph", Some("Ginfront")),
    }
}

fn wrap_from_graphic(wrap: Option<&str>, contour: bool, run_through: Option<&str>) -> WrapMode {
    let wrap = wrap.unwrap_or("parallel").trim().to_ascii_lowercase();
    match wrap.as_str() {
        "none" => WrapMode::TopAndBottom,
        "run-through" => {
            let run_through = run_through.unwrap_or("").trim().to_ascii_lowercase();
            match run_through.as_str() {
                "background" => WrapMode::Behind,
                "foreground" => WrapMode::InFront,
                _ => WrapMode::Through,
            }
        }
        _ if contour => WrapMode::Tight,
        _ => WrapMode::Square,
    }
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

fn odt_image_mime(declared: &str, bytes: &[u8], path: &str) -> String {
    let declared = declared
        .split(';')
        .next()
        .unwrap_or(declared)
        .trim()
        .to_ascii_lowercase();
    match declared.as_str() {
        "image/jpeg" | "image/jpg" => "image/jpeg".into(),
        "" => sniff_image_mime(bytes, path),
        other => other.into(),
    }
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
        assert!(xml.contains(r#"text:style-name="Ltask""#), "{xml}");
        assert!(xml.contains(r#"text:style-override="LtaskDone""#), "{xml}");
        assert!(
            xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "checked To-Do must be Writer {TASK_BOX_CHECKED}, not missing [x]: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "unchecked To-Do must be Writer {TASK_BOX_EMPTY}, not missing [ ]: {xml}"
        );
        assert!(
            !xml.contains("[x] ") && !xml.contains("[ ] "),
            "task items must be Writer boxes, not ASCII [x]/[ ] text: {xml}"
        );
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
    fn write_task_list_checkboxes_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut done = Paragraph::from_text("Receiving dock is clear");
        done.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut nested = Paragraph::from_text("Bay 4");
        nested.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut open = Paragraph::from_text("Replay the convert instead of hoping");
        open.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: None,
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(done));
        section.body.push(Block::Paragraph(nested));
        section.body.push(Block::Paragraph(open));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#),
            "- [x]/[ ] must emit Writer Ltask, not a disc: {xml}"
        );
        let ltask = test_list_style_xml(&xml, "Ltask");
        let ldone = test_list_style_xml(&xml, "LtaskDone");
        assert!(
            ltask.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "unchecked To-Do must be Writer ballot box, not a missing [ ]: {ltask}"
        );
        assert!(
            ldone.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "checked To-Do must be Writer ballot-with-check, not a missing [x]: {ldone}"
        );
        assert!(
            ltask.contains(r#"text:style-name="TtaskBullet""#)
                && ltask.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
            "Ltask must keep Writer label-alignment + TtaskBullet: {ltask}"
        );
        assert!(
            xml.contains(concat!(
                r#"<text:list-item text:style-override="LtaskDone">"#,
                r#"<text:p text:style-name="PlistNested">Receiving dock is clear</text:p>"#,
            )),
            "checked - [x] must override to LtaskDone, not ASCII [x]: {xml}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistLast">Replay the convert instead of hoping</text:p>"#
            ) && !xml.contains("[ ] Replay"),
            "unchecked - [ ] must stay an Ltask item, not ASCII [ ]: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="Plist">Bay 4</text:p>"#)
                && xml.contains(r#"text:style-name="Lbullet""#),
            "nested Bay 4 under a task stays a bullet: {xml}"
        );
        assert!(
            !xml.contains("[x] ") && !xml.contains("[ ] "),
            "task lists must not keep missing-box ASCII markers: {xml}"
        );
        let tbullet = test_style_xml(&xml, "TtaskBullet", "text");
        assert!(
            tbullet.contains("fo:color=\"#134e4a\""),
            "task boxes must be letter teal: {tbullet}"
        );

        assert!(
            xml.contains(r#"style:name="Tcode""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Tmark""#)
                && xml.contains("fo:background-color=\"#fde68a\""),
            "task boxes must keep Tcode/Tmark chips: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="HorizontalLine""#)
                && xml.contains(&format!(
                    r#"fo:border-top="{} solid #134e4a""#,
                    format_odf_length(HR_BORDER_HU)
                ))
                && xml.contains(r#"style:name="Tlink""#)
                && xml.contains("fo:color=\"#0d9488\""),
            "task boxes must keep hr/link: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#)
                && xml.contains(r#"fo:font-size="18pt""#)
                && xml.contains(r#"style:name="Tcell""#)
                && xml.contains(r#"fo:border="0.5pt solid"#),
            "task boxes must keep heading/table styles: {xml}"
        );

        let back = roundtrip(&doc).unwrap();
        let nums: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p
                    .numbering
                    .as_ref()
                    .map(|n| (p.plain_text(), n.format, n.level, n.start)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                (
                    "Receiving dock is clear".into(),
                    Some(NumberFormat::Task),
                    0,
                    Some(1)
                ),
                ("Bay 4".into(), Some(NumberFormat::Bullet), 1, None),
                (
                    "Replay the convert instead of hoping".into(),
                    Some(NumberFormat::Task),
                    0,
                    None
                ),
            ]
        );
    }

    #[test]
    fn read_ascii_task_prefix_still_maps() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("[x] Replay the convert");
        p.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
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
        let hr = test_style_xml(&xml, "HorizontalLine", "paragraph");
        let hr_border = format_odf_length(HR_BORDER_HU);
        let hr_margin = format_odf_length(HR_MARGIN_HU);
        assert!(
            hr.contains(&format!(r#"fo:border-top="{hr_border} solid #134e4a""#)),
            "HorizontalLine must be letter 3px teal ({hr_border}), not Writer 0.02in: {hr}"
        );
        assert!(
            hr.contains(&format!(r#"fo:margin-top="{hr_margin}""#))
                && hr.contains(&format!(r#"fo:margin-bottom="{hr_margin}""#)),
            "HorizontalLine must use letter 1.1rem ({hr_margin}), not 0.15in: {hr}"
        );
        assert!(
            !hr.contains(r#"fo:border-bottom="0.02in solid"#)
                && !hr.contains(r#"fo:margin-top="0.15in""#),
            "must not keep the missing 0.02in/0.15in rule: {hr}"
        );
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
        assert!(
            xml.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#),
            "superscript run must emit Tsuper, not missing body text: {xml}"
        );
        let tsuper = test_style_xml(&xml, "Tsuper", "text");
        assert_writer_super(tsuper);
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
        assert!(
            xml.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
            "subscript run must emit Tsub, not missing body text: {xml}"
        );
        let tsub = test_style_xml(&xml, "Tsub", "text");
        assert_writer_sub(tsub);
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
    }

    #[test]
    fn read_writer_percent_text_position() {
        let xml = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0">"#,
            r#"<office:automatic-styles>"#,
            r#"<style:style style:name="T1" style:family="text"><style:text-properties style:text-position="33% 58%"/></style:style>"#,
            r#"<style:style style:name="T2" style:family="text"><style:text-properties style:text-position="-33% 58%"/></style:style>"#,
            r#"</office:automatic-styles><office:body><office:text>"#,
            r#"<text:p>PDF/<text:span text:style-name="T1">A</text:span> H<text:span text:style-name="T2">2</text:span>O</text:p>"#,
            r#"</office:text></office:body></office:document-content>"#,
        );
        let doc = parse_content_xml(xml, &HashMap::new()).unwrap();
        let Block::Paragraph(p) = &doc.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(
            p.runs.iter().any(|r| {
                r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
            }),
            "Writer 33% 58% must map to superscript, not missing: {:?}",
            p.runs
        );
        assert!(
            p.runs.iter().any(|r| {
                r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
            }),
            "Writer -33% 58% must map to subscript, not missing: {:?}",
            p.runs
        );
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
        assert!(
            xml.contains(r#"<text:span text:style-name="Tunder">09:00 local</text:span>"#),
            "underline run must emit Tunder, not missing body text: {xml}"
        );
        let tunder = test_style_xml(&xml, "Tunder", "text");
        assert_writer_underline(tunder);
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
        let tmark = test_style_xml(&xml, "Tmark", "text");
        let mark_pad_x = format_odf_length(MARK_SPAN_PAD_X_HU);
        let mark_pad_y = format_odf_length(MARK_SPAN_PAD_Y_HU);
        assert_writer_mark(tmark);
        assert!(
            tmark.contains(&format!(r#"fo:padding="{mark_pad_y}""#))
                && tmark.contains(&format!(r#"fo:padding-left="{mark_pad_x}""#)),
            "Tmark must keep letter .05em/.2em padding, not a tight glyph wash: {tmark}"
        );
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
        let tcode = test_style_xml(&xml, "Tcode", "text");
        let code_pad_x = format_odf_length(CODE_SPAN_PAD_X_HU);
        let code_pad_y = format_odf_length(CODE_SPAN_PAD_Y_HU);
        assert!(
            tcode.contains("fo:background-color=\"#ccfbf1\"")
                && tcode.contains("fo:color=\"#134e4a\"")
                && tcode.contains(&format!(r#"style:font-name="{CODE_FONT}""#))
                && tcode.contains(&format!(r#"fo:font-family="{CODE_FONT}""#))
                && tcode.contains(&format!(r#"fo:font-size="{CODE_SPAN_SIZE}""#)),
            "Tcode must be letter teal Consolas chip, not a bare mono span: {tcode}"
        );
        assert!(
            tcode.contains(&format!(r#"fo:padding="{code_pad_y}""#))
                && tcode.contains(&format!(r#"fo:padding-left="{code_pad_x}""#)),
            "Tcode must keep letter .1em/.35em padding: {tcode}"
        );
        assert!(
            !tcode.contains(r#"fo:padding="0in""#) && !tcode.contains(r#"fo:background-color="transparent"#),
            "Tcode must not collapse to a reset chip: {tcode}"
        );
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
        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);
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
        assert!(
            xml.contains(r#"<text:span text:style-name="Tstrike">guess</text:span>"#),
            "strike run must emit Tstrike, not missing body text: {xml}"
        );
        let tstrike = test_style_xml(&xml, "Tstrike", "text");
        assert_writer_strike(tstrike);
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
        h2.runs[0].style.size = HEADING2_SIZE_PT * HU_PER_POINT;
        section.body.push(Block::Paragraph(h2));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        assert!(xml.contains(r#"text:outline-level="1""#), "{xml}");
        assert!(xml.contains(r#"text:style-name="Heading1""#), "{xml}");
        assert!(
            xml.contains(&format!(r#"fo:font-size="{HEADING1_SIZE_PT}pt""#)),
            "{xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:font-size="{HEADING2_SIZE_PT}pt""#)),
            "{xml}"
        );
        assert!(xml.contains("<text:h"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("h1");
        };
        assert_eq!(p.outline_level, Some(1));
        assert_eq!(p.plain_text(), "Northwind Freight");
        assert_eq!(p.runs[0].style.size, HEADING1_SIZE_PT * HU_PER_POINT);
        assert!(p.runs[0].style.bold);
        let Block::Paragraph(p2) = &back.sections[0].body[1] else {
            panic!("h2");
        };
        assert_eq!(p2.outline_level, Some(2));
        assert_eq!(p2.runs[0].style.size, HEADING2_SIZE_PT * HU_PER_POINT);
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
    fn roundtrip_nests_list_inside_list_item() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut a = Paragraph::from_text("dock");
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut b = Paragraph::from_text("bay 4");
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut c = Paragraph::from_text("hash");
        c.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut d = Paragraph::from_text("convert");
        d.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 1,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        section.body.push(Block::Paragraph(c));
        section.body.push(Block::Paragraph(d));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let bullet_nested = concat!(
            r#"<text:list-item><text:p text:style-name="PlistNested">dock</text:p>"#,
            r#"<text:list text:style-name="Lbullet">"#,
            r#"<text:list-item><text:p text:style-name="PlistLast">bay 4</text:p></text:list-item>"#,
            "</text:list></text:list-item>",
        );
        let decimal_nested = concat!(
            r#"<text:list-item><text:p text:style-name="PlistNested">hash</text:p>"#,
            r#"<text:list text:style-name="Lnumber">"#,
            r#"<text:list-item><text:p text:style-name="PlistLast">convert</text:p></text:list-item>"#,
            "</text:list></text:list-item>",
        );
        assert!(xml.contains(bullet_nested), "bullet nest flattened: {xml}");
        assert!(
            xml.contains(decimal_nested),
            "decimal nest flattened: {xml}"
        );
        assert!(
            !xml.contains("</text:p></text:list-item><text:list-item><text:p>bay 4</text:p>"),
            "bullet list flattened to one level: {xml}"
        );
        assert!(
            !xml.contains("</text:p></text:list-item><text:list-item><text:p>convert</text:p>"),
            "decimal list flattened to one level: {xml}"
        );

        let back = roundtrip(&doc).unwrap();
        let nums: Vec<_> = back.sections[0]
            .body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p
                    .numbering
                    .as_ref()
                    .map(|n| (p.plain_text(), n.level, n.format, n.start)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                ("dock".into(), 0, Some(NumberFormat::Bullet), None),
                ("bay 4".into(), 1, Some(NumberFormat::Bullet), None),
                ("hash".into(), 0, Some(NumberFormat::Decimal), Some(1)),
                ("convert".into(), 1, Some(NumberFormat::Decimal), Some(1)),
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
        assert!(
            xml.contains(r#"<text:a text:style-name="Tlink" xlink:type="simple" xlink:href="https://github.com/kevin9327/docagent">DocAgent</text:a>"#),
            "Writer must emit a styled text:a, not missing body text: {xml}"
        );
        assert!(xml.contains("Tlink"), "{xml}");
        let tlink = test_style_xml(&xml, "Tlink", "text");
        assert_writer_internet_link(tlink);
        assert!(
            tlink.contains(r#"style:parent-style-name="Internet_20_Link""#),
            "Tlink must parent Writer Internet Link so the sidebar is not navy: {tlink}"
        );
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
        assert!(xml.contains(r#"table:style-name="THcell""#), "{xml}");
        assert!(xml.contains(r#"table:style-name="Table1""#), "{xml}");
        let table1 = test_style_xml(&xml, "Table1", "table");
        assert_writer_table_collapse(table1);
        assert!(
            xml.contains(&format!(
                r#"fo:margin-top="{}""#,
                format_odf_length(TABLE_MARGIN_HU)
            )),
            "table must keep letter 1rem before so the hop grid is not jammed: {xml}"
        );
        assert!(xml.contains("<table:table-column"), "{xml}");
        assert!(xml.contains("style:column-width"), "{xml}");
        assert!(
            xml.contains(&format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#))
                && !xml.contains(r#"fo:border="0.5pt solid #000000""#),
            "hop grid must be 0.5pt #cbd5e1, not LibreOffice black: {xml}"
        );
        assert!(xml.contains(r#"table:style-name="Tcell""#), "{xml}");
        let pad_x = format_odf_length(CELL_PAD_X_HU);
        let pad_y = format_odf_length(CELL_PAD_Y_HU);
        assert!(
            xml.contains(&format!(r#"fo:padding="{pad_y}""#)),
            "cells must keep fo:padding so text is not jammed on the 0.5pt grid: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:padding-left="{pad_x}""#)),
            "cell pad x must be letter CSS 0.65rem ({pad_x}): {xml}"
        );
        assert!(
            !xml.contains(r#"fo:padding="0.0382in""#),
            "LibreOffice default 0.097cm pad still jams text on a 0.5pt grid: {xml}"
        );
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        let th_rule = format_odf_length(TH_RULE_HU);
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\""),
            "THcell must keep letter #ccfbf1 fill, not a sparse grid: {thcell}"
        );
        assert!(
            thcell.contains(&format!(r#"fo:border-bottom="{th_rule} solid #0d9488""#)),
            "THcell must keep letter 2px #0d9488 header rule, not a sparse 0.5pt grid: {thcell}"
        );
        assert!(
            thcell.contains("fo:color=\"#134e4a\"") && thcell.contains(r#"fo:font-weight="bold""#),
            "THcell text must stay letter teal bold: {thcell}"
        );
        assert!(
            !thcell.contains("transparent")
                && !thcell.contains(r#"fo:background-color="none"#)
                && !thcell.contains("fo:background-color=\"#fff\"")
                && !thcell.contains("fo:background-color=\"#ffffff\"")
                && !thcell.contains(r#"fo:border="none"#),
            "UA/reset THcell is a sparse grid, not WeasyPrint letter input: {thcell}"
        );
        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        assert!(
            !tcell.contains("fo:background-color=\"#ccfbf1\""),
            "body Tcell must not keep th fill: {tcell}"
        );
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(&thcell);
        let pth = test_style_xml(&xml, "Pth", "paragraph");
        let pcell = test_style_xml(&xml, "Pcell", "paragraph");
        assert_writer_cell_type(pth);
        assert_writer_cell_type(pcell);
        assert!(
            xml.contains(r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Hop</text:p>"#),
            "header Hop must use THcell so Writer paints the teal fill: {xml}"
        );
        assert!(
            xml.contains(r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">Input</text:p>"#),
            "body Input must stay Tcell + Pcell 0.95rem, not THcell/body 10pt: {xml}"
        );
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
    }

    #[test]
    fn write_table_emits_cell_padding() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        let pad_x = format_odf_length(CELL_PAD_X_HU);
        let pad_y = format_odf_length(CELL_PAD_Y_HU);
        assert!(
            xml.contains(r#"style:name="Tcell" style:family="table-cell""#),
            "body cells must use a table-cell style: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:padding="{pad_y}""#)),
            "fo:padding must be letter 0.4rem ({pad_y}), not jammed on the 0.5pt grid: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:padding-left="{pad_x}""#)),
            "fo:padding-left must be letter 0.65rem ({pad_x}): {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:padding-right="{pad_x}""#)),
            "fo:padding-right must be letter 0.65rem ({pad_x}): {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#))
                && !xml.contains(r#"fo:border="0.5pt solid #000000""#),
            "0.5pt #cbd5e1 borders must stay on the padded cells: {xml}"
        );
        assert!(
            !xml.contains(r#"fo:padding="0in""#) && !xml.contains(r#"fo:padding="0pt""#),
            "cell padding must not collapse to 0: {xml}"
        );
        assert!(
            !xml.contains(r#"fo:padding="0.0382in""#),
            "must not keep LibreOffice 0.097cm default pad: {xml}"
        );
    }

    #[test]
    fn write_table_header_fill_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains("<table:table-header-rows>"),
            "LibreOffice header row must be table-header-rows, not a body row: {xml}"
        );
        assert!(
            xml.contains(r#"table:style-name="THcell""#),
            "header cells must use THcell so Writer paints the fill: {xml}"
        );
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        let pad_x = format_odf_length(CELL_PAD_X_HU);
        let pad_y = format_odf_length(CELL_PAD_Y_HU);
        let th_rule = format_odf_length(TH_RULE_HU);
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\""),
            "THcell must keep WeasyPrint teal fill #ccfbf1, not a sparse grid: {thcell}"
        );
        assert!(
            thcell.contains(&format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#))
                && thcell.contains(&format!(r#"fo:border-bottom="{th_rule} solid #0d9488""#))
                && !thcell.contains(r#"fo:border="0.5pt solid #000000""#),
            "THcell must keep 0.5pt #cbd5e1 grid + letter 2px #0d9488 header rule: {thcell}"
        );
        assert!(
            thcell.contains(&format!(r#"fo:padding="{pad_y}""#))
                && thcell.contains(&format!(r#"fo:padding-left="{pad_x}""#))
                && thcell.contains(&format!(r#"fo:padding-right="{pad_x}""#)),
            "THcell pad must stay letter 0.4rem/0.65rem, not jammed on the grid: {thcell}"
        );
        assert!(
            thcell.contains("fo:color=\"#134e4a\"") && thcell.contains(r#"fo:font-weight="bold""#),
            "THcell must keep letter teal bold header text: {thcell}"
        );
        assert!(
            !thcell.contains("transparent")
                && !thcell.contains(r#"fo:background-color="none"#)
                && !thcell.contains("fo:background-color=\"#fff\"")
                && !thcell.contains("fo:background-color=\"#ffffff\"")
                && !thcell.contains(r#"fo:background-color="white"#)
                && !thcell.contains(r#"fo:border="none"#)
                && !thcell.contains(r#"fo:font-weight="400"#),
            "UA/reset THcell is a sparse grid, not WeasyPrint letter input: {thcell}"
        );
        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        assert!(
            !tcell.contains("fo:background-color=\"#ccfbf1\""),
            "body Tcell must not keep th fill: {tcell}"
        );
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(&thcell);
        let pth = test_style_xml(&xml, "Pth", "paragraph");
        let pcell = test_style_xml(&xml, "Pcell", "paragraph");
        assert_writer_cell_type(pth);
        assert_writer_cell_type(pcell);
        assert!(
            !pcell.contains(r#"fo:font-weight="bold""#),
            "Pcell must stay body weight, not th bold: {pcell}"
        );
        assert!(
            xml.contains(r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Hop</text:p>"#)
                && xml.contains(r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">File</text:p>"#)
                && xml.contains(r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Proof</text:p>"#),
            "letter header cells must use THcell: {xml}"
        );
        assert!(
            xml.contains(r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">Input</text:p>"#)
                && xml.contains(
                    r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">letter.md</text:p>"#
                ),
            "letter body cells must stay Tcell + Pcell 0.95rem: {xml}"
        );

        assert!(
            xml.contains(r#"style:name="Tcode""#)
                && xml.contains(r#"style:name="Tmark""#)
                && xml.contains("fo:background-color=\"#fde68a\""),
            "th fill must keep Tcode/Tmark chips: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "th fill must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "th fill must keep Heading1 18pt: {xml}"
        );
        let tcode = test_style_xml(&xml, "Tcode", "text");
        assert!(
            tcode.contains("fo:background-color=\"#ccfbf1\""),
            "th fill must keep Tcode #ccfbf1 chip: {tcode}"
        );
    }

    #[test]
    fn write_table_type_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert_eq!(CELL_SIZE_HU, 1140);
        assert_eq!(CELL_SIZE_PT, "11.4pt");
        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        let pth = test_style_xml(&xml, "Pth", "paragraph");
        let pcell = test_style_xml(&xml, "Pcell", "paragraph");
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(thcell);
        assert_writer_cell_type(pth);
        assert_writer_cell_type(pcell);
        assert!(
            pcell.contains(&format!(r#"fo:color="{BODY_FG}""#))
                && pcell.contains(r#"style:use-window-font-color="false""#)
                && !pcell.contains(r#"fo:font-weight="bold""#),
            "Pcell must stay letter teal body type, not th bold: {pcell}"
        );
        assert!(
            xml.contains(
                r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">Input</text:p>"#
            ),
            "body Input must use Pcell so Writer paints 0.95rem, not missing Tcell text-properties: {xml}"
        );
        assert!(
            xml.contains(
                r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Hop</text:p>"#
            ),
            "header Hop must keep Pth 0.95rem: {xml}"
        );
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains("solid #0d9488")
                && thcell.contains(r#"fo:font-weight="bold""#),
            "0.95rem table type must keep THcell fill: {thcell}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "0.95rem table type must keep Writer task boxes: {xml}"
        );
    }

    #[test]
    fn write_table_border_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(thcell);
        let grid = format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#);
        let th_rule = format_odf_length(TH_RULE_HU);
        assert!(
            tcell.contains(&grid) && !tcell.contains(r#"fo:border="0.5pt solid #000000""#),
            "Tcell must be letter 0.5pt #cbd5e1, not LibreOffice TableGrid black: {tcell}"
        );
        assert!(
            thcell.contains(&grid)
                && thcell.contains(&format!(r#"fo:border-bottom="{th_rule} solid #0d9488""#))
                && !thcell.contains(r#"fo:border="0.5pt solid #000000""#),
            "THcell must keep 0.5pt #cbd5e1 grid + 2px #0d9488 rule, not black: {thcell}"
        );
        assert!(
            !xml.contains(r#"fo:border="0.5pt solid #000000""#)
                && !xml.contains(r#"fo:border="0.5pt solid #000""#),
            "hop grid must not keep LibreOffice black hairline: {xml}"
        );
        let pad_x = format_odf_length(CELL_PAD_X_HU);
        let pad_y = format_odf_length(CELL_PAD_Y_HU);
        assert!(
            tcell.contains(&format!(r#"fo:padding="{pad_y}""#))
                && tcell.contains(&format!(r#"fo:padding-left="{pad_x}""#))
                && thcell.contains("fo:background-color=\"#ccfbf1\""),
            "slate grid must keep letter cell pad + THcell fill: {tcell} {thcell}"
        );
        assert!(
            tcell.contains(&format!(r#"fo:color="{BODY_FG}""#))
                && xml.contains(r#"fo:line-height="115%""#),
            "slate grid must keep body #134e4a and Writer 1.15: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Pth""#)
                && xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "slate grid must keep Pth + Writer task boxes: {xml}"
        );
        let table1 = test_style_xml(&xml, "Table1", "table");
        assert_writer_table_collapse(table1);
    }

    #[test]
    fn write_table_collapse_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert_eq!(TABLE_BORDER_MODEL, "collapsing");
        let table1 = test_style_xml(&xml, "Table1", "table");
        assert_writer_table_collapse(table1);
        assert!(
            table1.contains(r#"style:rel-width="100%""#)
                && table1.contains(&format!(
                    r#"fo:margin-top="{}""#,
                    format_odf_length(TABLE_MARGIN_HU)
                ))
                && table1.contains(&format!(
                    r#"fo:margin-bottom="{}""#,
                    format_odf_length(TABLE_MARGIN_HU)
                )),
            "collapsed hop grid must keep letter 1rem + 100% measure: {table1}"
        );
        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(thcell);
        assert_writer_cell_valign(tcell);
        assert_writer_cell_valign(thcell);
        let grid = format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#);
        assert!(
            tcell.contains(&grid)
                && thcell.contains(&grid)
                && thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains("solid #0d9488"),
            "collapsed hop grid must keep 0.5pt #cbd5e1 + THcell fill: {tcell} {thcell}"
        );
        assert!(
            xml.contains(
                r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">Input</text:p>"#
            ) && xml.contains(
                r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Hop</text:p>"#
            ),
            "collapsed hop grid must keep Pcell/Pth: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "collapsed hop grid must keep Writer task boxes: {xml}"
        );
    }

    #[test]
    fn write_cell_valign_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert_writer_cell_type(tcell);
        assert_writer_cell_type(thcell);
        assert_writer_cell_valign(tcell);
        assert_writer_cell_valign(thcell);
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains("solid #0d9488")
                && thcell.contains(r#"fo:font-weight="bold""#),
            "cell valign must keep THcell fill: {thcell}"
        );
        assert!(
            tcell.contains(&format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#))
                && !tcell.contains(r#"fo:border="0.5pt solid #000000""#),
            "cell valign must keep 0.5pt #cbd5e1 grid: {tcell}"
        );
        assert!(
            xml.contains(
                r#"<table:table-cell table:style-name="THcell"><text:p text:style-name="Pth">Hop</text:p>"#
            ) && xml.contains(
                r#"<table:table-cell table:style-name="Tcell"><text:p text:style-name="Pcell">Input</text:p>"#
            ),
            "cell valign must keep THcell/Pth + Tcell/Pcell: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#)),
            "cell valign must keep Writer task boxes: {xml}"
        );
    }

    #[test]
    fn write_image_paragraph_spaces_after_mark() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        img_p.space_before = 80;
        img_p.space_after = 200;
        section.body.push(Block::Paragraph(img_p));
        let mut body = Paragraph::from_text("6 September 2026");
        body.space_after = 200;
        section.body.push(Block::Paragraph(body));
        doc.sections.push(section);
        let xml = content_xml(&doc);
        let before = format_odf_length(IMAGE_BEFORE_HU);
        let after = format_odf_length(IMAGE_AFTER_HU);
        let ir_before = format_odf_length(80);
        let body_after = format_odf_length(200);
        let img_at = xml.find("<draw:image").expect("draw:image");
        let p_start = xml[..img_at].rfind("<text:p").expect("image paragraph");
        let p_end = xml[img_at..].find("</text:p>").expect("image para end") + img_at;
        let p = &xml[p_start..p_end + 9];
        assert_eq!(IMAGE_BEFORE_HU, 720, "letter img margin-top 0.6rem must stay 720 HU");
        assert_eq!(IMAGE_AFTER_HU, 840, "letter p+img after 0.7rem must stay 840 HU");
        assert!(
            IMAGE_BEFORE_HU < IMAGE_AFTER_HU,
            "img 0.6rem before must stay tighter than collapsed 0.7rem after"
        );
        assert!(
            p.contains(r#"text:style-name="Pimage""#),
            "Harbor mark paragraph must use Pimage so the page is not sparse under the image: {p}"
        );
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{before}" fo:margin-bottom="{after}""#
            )),
            "Pimage must space before {before} (0.6rem) and after {after} (0.7rem), not IR 80/200 HU: {xml}"
        );
        assert!(
            !xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="0in""#
            )),
            "Pimage before must not stay Writer 0in under the H2 rule: {xml}"
        );
        assert!(
            !xml.contains(&format!(r#"fo:margin-bottom="{body_after}""#)),
            "image-only after must not keep body space_after 200 HU: {xml}"
        );
        assert!(
            !xml.contains(&format!(r#"fo:margin-top="{ir_before}""#)),
            "image-only before must not keep IR space_before 80 HU: {xml}"
        );
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "Harbor draw:frame must keep the 24pt floor: {xml}"
        );
    }

    #[test]
    fn write_heading_and_body_type_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.runs[0].style.size = 1800;
        h1.space_before = 200;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.runs[0].style.size = 1400;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk, Duisburg terminal.");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        assert!(
            h1.contains(&format!(r#"fo:font-size="{HEADING1_SIZE_PT}pt""#)),
            "Heading1 must be Writer 18pt, not IR 18pt without a style: {h1}"
        );
        let heading1_before = format_odf_length(HEADING1_BEFORE_HU);
        assert_eq!(HEADING1_BEFORE_HU, 0, "letter h1 margin-top 0 must stay 0 HU");
        assert!(
            h1.contains(&format!(r#"fo:margin-top="{heading1_before}""#)),
            "Heading1 before must be letter 0 ({heading1_before}), not Writer 0.1665in / IR 200 HU: {h1}"
        );
        assert!(
            !h1.contains(r#"fo:margin-top="0.1665in""#),
            "Heading1 must not keep Writer 0.1665in before: {h1}"
        );
        let heading1_after = format_odf_length(HEADING1_AFTER_HU);
        assert_eq!(HEADING1_AFTER_HU, 720, "letter h1 margin-bottom 0.6rem must stay 720 HU");
        assert!(
            h1.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#)),
            "Heading1 after must be letter 0.6rem ({heading1_after}), not Writer 0.0835in: {h1}"
        );
        assert!(
            !h1.contains(r#"fo:margin-bottom="0.0835in""#),
            "Heading1 must not keep Writer 0.0835in after: {h1}"
        );
        assert!(
            h1.contains(r#"fo:keep-with-next="always""#),
            "Writer headings keep-with-next: {h1}"
        );
        assert!(
            h1.contains(r#"style:parent-style-name="Heading_20_1""#),
            "Heading1 must parent Writer Heading 1: {h1}"
        );
        assert!(
            h1.contains(&format!(
                r#"fo:border-bottom="{} solid #134e4a""#,
                format_odf_length(HEADING1_RULE_HU)
            )),
            "Heading1 must keep letter 3px #134e4a rule: {h1}"
        );
        assert_writer_heading_color(h1);
        assert_writer_heading_tracking(h1);
        assert_writer_heading_leading(h1);
        assert_writer_body_font(h1);

        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert!(
            h2.contains(&format!(r#"fo:font-size="{HEADING2_SIZE_PT}pt""#)),
            "Heading2 must be Writer 16pt (115% of 14pt), not 14pt: {h2}"
        );
        assert!(
            !h2.contains(r#"fo:font-size="14pt""#),
            "Heading2 must not keep the 14pt parent size: {h2}"
        );
        let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
        let heading2_after = format_odf_length(HEADING2_AFTER_HU);
        assert_eq!(HEADING2_BEFORE_HU, 1320, "letter h2 margin-top 1.1rem must stay 1320 HU");
        assert_eq!(HEADING2_AFTER_HU, 540, "letter h2 margin-bottom 0.45rem must stay 540 HU");
        assert!(
            h2.contains(&format!(r#"fo:margin-top="{heading2_before}""#)),
            "Heading2 before must be letter 1.1rem ({heading2_before}), not Writer 0.139in: {h2}"
        );
        assert!(
            !h2.contains(r#"fo:margin-top="0.139in""#),
            "Heading2 must not keep Writer 0.139in before: {h2}"
        );
        assert!(
            h2.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#)),
            "Heading2 after must be letter 0.45rem ({heading2_after}), not Writer 0.0835in: {h2}"
        );
        assert!(
            !h2.contains(r#"fo:margin-bottom="0.0835in""#),
            "Heading2 must not keep Writer 0.0835in after: {h2}"
        );
        assert!(
            h2.contains(r#"style:parent-style-name="Heading_20_2""#)
                && h2.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(HEADING2_RULE_HU)
                )),
            "Heading2 must keep Writer Heading 2 + letter teal rule: {h2}"
        );
        assert_writer_heading_color(h2);
        assert_writer_heading_leading(h2);
        assert_writer_body_font(h2);

        let h3 = test_style_xml(&xml, "Heading3", "paragraph");
        assert!(
            h3.contains(&format!(r#"fo:font-size="{HEADING3_SIZE_PT}pt""#)),
            "Heading3 must be Writer 14pt, not 12pt: {h3}"
        );
        assert_writer_heading_color(h3);
        assert_writer_heading_leading(h3);

        assert!(
            xml.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#),
            "body copy must use Pbody, not a jammed unstyled text:p: {xml}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        let body_after = format_odf_length(BODY_AFTER_HU);
        assert_eq!(BODY_AFTER_HU, 840, "letter p margin-bottom 0.7rem must stay 840 HU");
        assert_eq!(BODY_AFTER_HU, IMAGE_AFTER_HU);
        assert!(
            pbody.contains(&format!(r#"fo:margin-bottom="{body_after}""#)),
            "Pbody after must be letter 0.7rem ({body_after}), not Writer 0.0972in / IR 200 HU: {pbody}"
        );
        assert!(
            !pbody.contains(r#"fo:margin-bottom="0.0972in""#),
            "Pbody must not keep Writer Text body 0.0972in after: {pbody}"
        );
        assert!(
            pbody.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#)),
            "Pbody must use Writer 1.15 lines so the page is not jammed: {pbody}"
        );
        assert!(
            !pbody.contains(r#"fo:margin-bottom="0.0277in""#)
                && !xml.contains(r#"fo:margin-bottom="0.0277in""#),
            "body after must not keep IR space_after 200 HU: {xml}"
        );
        assert!(
            !pbody.contains(r#"fo:margin-bottom="0in""#),
            "Pbody must not collapse to 0 (jammed page): {pbody}"
        );
        assert_writer_body_color(pbody);
        assert_writer_body_font(pbody);

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading = test_office_style_xml(&styles, "Heading");
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        let heading3_named = test_office_style_xml(&styles, "Heading_20_3");
        assert_writer_heading_color(heading);
        assert_writer_heading_color(heading1_named);
        assert_writer_heading_color(heading2_named);
        assert_writer_heading_color(heading3_named);
        assert_writer_heading_tracking(heading1_named);
        assert_writer_body_font(test_default_style_xml(&styles, "paragraph"));
        assert_writer_body_font(test_office_style_xml(&styles, "Standard"));
        assert_writer_body_font(heading);
        assert_writer_body_font(heading1_named);
        assert!(
            !styles.contains(&format!(r#"fo:color="{WRITER_HEADING_NAVY}""#)),
            "Writer package Heading must not keep Liberation navy {WRITER_HEADING_NAVY}: {styles}"
        );
        assert!(
            !styles.contains(r#"style:font-name="Liberation Sans""#)
                && !styles.contains(r#"fo:font-family="Liberation Sans""#),
            "Writer package must not keep Liberation Sans body type: {styles}"
        );
        assert!(
            heading2_named.contains(&format!(
                r#"fo:margin-top="{}""#,
                format_odf_length(HEADING2_BEFORE_HU)
            )),
            "Writer Heading 2 must keep letter 1.1rem before, not 0.139in: {heading2_named}"
        );
        assert!(
            heading2_named.contains(&format!(
                r#"fo:margin-bottom="{}""#,
                format_odf_length(HEADING2_AFTER_HU)
            )),
            "Writer Heading 2 must keep letter 0.45rem after, not 0.0835in: {heading2_named}"
        );
        assert!(
            heading1_named.contains(&format!(r#"fo:margin-top="{heading1_before}""#))
                && !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
            "Writer Heading 1 must keep letter h1 margin-top 0, not 0.1665in: {heading1_named}"
        );
        assert!(
            heading1_named.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#)),
            "Writer Heading 1 must keep letter 0.6rem after, not 0.0835in: {heading1_named}"
        );
    }

    #[test]
    fn write_heading2_before_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.space_before = 200;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let before = format_odf_length(HEADING2_BEFORE_HU);
        let after = format_odf_length(HEADING2_AFTER_HU);
        let image_before = format_odf_length(IMAGE_BEFORE_HU);
        let image_after = format_odf_length(IMAGE_AFTER_HU);
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert_eq!(HEADING2_BEFORE_HU, 1320, "letter h2 margin-top 1.1rem must stay 1320 HU");
        assert_eq!(HEADING2_AFTER_HU, 540, "letter h2 margin-bottom 0.45rem must stay 540 HU");
        assert_eq!(HEADING1_AFTER_HU, 720, "letter h1 margin-bottom 0.6rem must stay 720 HU");
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert!(
            HEADING2_BEFORE_HU > IMAGE_BEFORE_HU && HEADING2_BEFORE_HU == HR_MARGIN_HU,
            "h2 1.1rem must stay the same strut as hr, looser than img 0.6rem"
        );
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        assert!(
            h1.contains(&format!(
                r#"fo:margin-bottom="{}""#,
                format_odf_length(HEADING1_AFTER_HU)
            )) && !h1.contains(r#"fo:margin-bottom="0.0835in""#),
            "h2 1.1rem before must keep letter h1 0.6rem after, not Writer 0.0835in: {h1}"
        );
        assert!(
            h2.contains(&format!(r#"fo:margin-bottom="{after}""#))
                && !h2.contains(r#"fo:margin-bottom="0.0835in""#),
            "h2 1.1rem before must keep letter 0.45rem after, not Writer 0.0835in: {h2}"
        );
        assert!(
            h2.contains(&format!(r#"fo:margin-top="{before}""#)),
            "Heading2 before must be letter 1.1rem ({before}), not Writer 0.139in / IR 360 HU: {h2}"
        );
        assert!(
            !h2.contains(r#"fo:margin-top="0.139in""#)
                && !h2.contains(r#"fo:margin-top="0.05in""#),
            "Writer 0.139in / IR 360 HU H2 before is jammed vs WeasyPrint 1.1rem: {h2}"
        );
        assert_writer_heading_leading(h2);
        assert_writer_heading_color(h2);
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}""#
            )),
            "h2 1.1rem must keep Pimage 0.6rem/0.7rem: {xml}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "h2 1.1rem must keep the 24pt Harbor floor: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-top="{before}""#)),
            "Writer Heading 2 sidebar must keep letter 1.1rem before: {heading2_named}"
        );
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-bottom="{after}""#)),
            "Writer Heading 2 sidebar must keep letter 0.45rem after: {heading2_named}"
        );
        assert_writer_heading_leading(heading2_named);
    }

    #[test]
    fn write_heading2_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.space_before = 200;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let before = format_odf_length(HEADING2_BEFORE_HU);
        let after = format_odf_length(HEADING2_AFTER_HU);
        let image_before = format_odf_length(IMAGE_BEFORE_HU);
        let image_after = format_odf_length(IMAGE_AFTER_HU);
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert_eq!(HEADING2_BEFORE_HU, 1320, "letter h2 margin-top 1.1rem must stay 1320 HU");
        assert_eq!(HEADING2_AFTER_HU, 540, "letter h2 margin-bottom 0.45rem must stay 540 HU");
        assert_eq!(HEADING1_AFTER_HU, 720, "letter h1 margin-bottom 0.6rem must stay 720 HU");
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert!(
            HEADING2_AFTER_HU < IMAGE_BEFORE_HU && HEADING2_AFTER_HU < HEADING2_BEFORE_HU,
            "h2 0.45rem after must stay tighter than img 0.6rem and h2 1.1rem before"
        );
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        assert!(
            h1.contains(&format!(
                r#"fo:margin-bottom="{}""#,
                format_odf_length(HEADING1_AFTER_HU)
            )) && !h1.contains(r#"fo:margin-bottom="0.0835in""#),
            "h2 0.45rem after must keep letter h1 0.6rem after, not Writer 0.0835in: {h1}"
        );
        assert!(
            h2.contains(&format!(r#"fo:margin-bottom="{after}""#)),
            "Heading2 after must be letter 0.45rem ({after}), not Writer 0.0835in / IR 240 HU: {h2}"
        );
        assert!(
            !h2.contains(r#"fo:margin-bottom="0.0835in""#)
                && !h2.contains(r#"fo:margin-bottom="0.0333in""#),
            "Writer 0.0835in / IR 240 HU H2 after is the wrong strut vs WeasyPrint 0.45rem: {h2}"
        );
        assert!(
            h2.contains(&format!(r#"fo:margin-top="{before}""#)),
            "h2 0.45rem after must keep letter 1.1rem before: {h2}"
        );
        assert_writer_heading_leading(h2);
        assert_writer_heading_color(h2);
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}""#
            )),
            "h2 0.45rem after must keep Pimage 0.6rem/0.7rem: {xml}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "h2 0.45rem after must keep the 24pt Harbor floor: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-bottom="{after}""#)),
            "Writer Heading 2 sidebar must keep letter 0.45rem after: {heading2_named}"
        );
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-top="{before}""#)),
            "Writer Heading 2 sidebar must keep letter 1.1rem before: {heading2_named}"
        );
        assert_writer_heading_leading(heading2_named);
    }

    #[test]
    fn write_body_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        let mut date = Paragraph::from_text("6 September 2026");
        date.space_after = 200;
        section.body.push(Block::Paragraph(date));
        let mut dest = Paragraph::from_text("To: operations desk, Duisburg terminal.");
        dest.space_after = 200;
        section.body.push(Block::Paragraph(dest));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let body_after = format_odf_length(BODY_AFTER_HU);
        let heading2_after = format_odf_length(HEADING2_AFTER_HU);
        let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
        let image_before = format_odf_length(IMAGE_BEFORE_HU);
        let image_after = format_odf_length(IMAGE_AFTER_HU);
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_eq!(BODY_AFTER_HU, 840, "letter p margin-bottom 0.7rem must stay 840 HU");
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(
            BODY_AFTER_HU, IMAGE_AFTER_HU,
            "body 0.7rem after must stay the same strut as collapsed p+img"
        );
        assert!(
            HEADING2_AFTER_HU < BODY_AFTER_HU && HEADING1_AFTER_HU < BODY_AFTER_HU,
            "h2 0.45rem / h1 0.6rem after must stay tighter than body 0.7rem"
        );
        assert!(
            pbody.contains(&format!(r#"fo:margin-bottom="{body_after}""#)),
            "Pbody after must be letter 0.7rem ({body_after}), not Writer 0.0972in / IR 200 HU: {pbody}"
        );
        assert!(
            !pbody.contains(r#"fo:margin-bottom="0.0972in""#)
                && !pbody.contains(r#"fo:margin-bottom="0.0277in""#)
                && !pbody.contains(r#"fo:margin-bottom="0in""#),
            "Writer 0.0972in / IR 200 HU / 0in Pbody after is the wrong strut vs WeasyPrint 0.7rem: {pbody}"
        );
        assert_writer_line_height(pbody);
        assert_writer_body_color(pbody);
        assert!(
            xml.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#)
                && xml.contains(r#"<text:p text:style-name="Pbody">To: operations desk, Duisburg terminal.</text:p>"#),
            "letter body copy must use Pbody so Writer paints 0.7rem, not jammed IR 200: {xml}"
        );
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        assert!(
            h1.contains(&format!(
                r#"fo:margin-top="{}""#,
                format_odf_length(HEADING1_BEFORE_HU)
            )) && !h1.contains(r#"fo:margin-top="0.1665in""#),
            "body 0.7rem after must keep letter h1 margin-top 0, not Writer 0.1665in: {h1}"
        );
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert!(
            h2.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#))
                && h2.contains(&format!(r#"fo:margin-top="{heading2_before}""#))
                && !h2.contains(r#"fo:margin-bottom="0.0835in""#),
            "body 0.7rem after must keep h2 1.1rem/0.45rem: {h2}"
        );
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}""#
            )),
            "body 0.7rem after must keep Pimage 0.6rem/0.7rem: {xml}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "body 0.7rem after must keep the 24pt Harbor floor: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let text_body = test_office_style_xml(&styles, "Text_20_body");
        assert!(
            text_body.contains(&format!(r#"fo:margin-bottom="{body_after}""#)),
            "Writer Text body sidebar must keep letter 0.7rem after, not 0.0972in: {text_body}"
        );
        assert!(
            !text_body.contains(r#"fo:margin-bottom="0.0972in""#),
            "Writer Text body must not keep Writer 0.0972in after: {text_body}"
        );
        assert_writer_line_height(text_body);
        assert_writer_body_color(text_body);
    }

    #[test]
    fn write_heading1_before_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.space_before = 200;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let heading1_before = format_odf_length(HEADING1_BEFORE_HU);
        let heading1_after = format_odf_length(HEADING1_AFTER_HU);
        let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
        let heading2_after = format_odf_length(HEADING2_AFTER_HU);
        let image_before = format_odf_length(IMAGE_BEFORE_HU);
        let image_after = format_odf_length(IMAGE_AFTER_HU);
        let body_after = format_odf_length(BODY_AFTER_HU);
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert_eq!(HEADING1_BEFORE_HU, 0, "letter h1 margin-top 0 must stay 0 HU");
        assert_eq!(HEADING1_AFTER_HU, 720, "letter h1 margin-bottom 0.6rem must stay 720 HU");
        assert_eq!(HEADING2_BEFORE_HU, 1320, "letter h2 margin-top 1.1rem must stay 1320 HU");
        assert_eq!(HEADING2_AFTER_HU, 540, "letter h2 margin-bottom 0.45rem must stay 540 HU");
        assert_eq!(BODY_AFTER_HU, 840, "letter p margin-bottom 0.7rem must stay 840 HU");
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert!(
            HEADING1_BEFORE_HU < HEADING1_AFTER_HU && HEADING1_BEFORE_HU < HEADING2_BEFORE_HU,
            "h1 margin-top 0 must stay tighter than h1 0.6rem after and h2 1.1rem before"
        );
        assert!(
            h1.contains(&format!(r#"fo:margin-top="{heading1_before}""#)),
            "Heading1 before must be letter 0 ({heading1_before}), not Writer 0.1665in / IR 200 HU: {h1}"
        );
        assert!(
            !h1.contains(r#"fo:margin-top="0.1665in""#)
                && !h1.contains(r#"fo:margin-top="0.0277in""#),
            "Writer 0.1665in / IR 200 HU H1 before is sparse vs WeasyPrint margin-top 0: {h1}"
        );
        assert!(
            h1.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#))
                && !h1.contains(r#"fo:margin-bottom="0.0835in""#),
            "h1 margin-top 0 must keep letter 0.6rem after, not Writer 0.0835in: {h1}"
        );
        assert_writer_heading_leading(h1);
        assert_writer_heading_tracking(h1);
        assert_writer_heading_color(h1);
        assert!(
            h2.contains(&format!(r#"fo:margin-top="{heading2_before}""#))
                && h2.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#))
                && !h2.contains(r#"fo:margin-bottom="0.0835in""#),
            "h1 margin-top 0 must keep h2 1.1rem/0.45rem: {h2}"
        );
        assert_writer_heading_leading(h2);
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}""#
            )),
            "h1 margin-top 0 must keep Pimage 0.6rem/0.7rem: {xml}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert!(
            pbody.contains(&format!(r#"fo:margin-bottom="{body_after}""#))
                && !pbody.contains(r#"fo:margin-bottom="0.0972in""#),
            "h1 margin-top 0 must keep letter p 0.7rem after: {pbody}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "h1 margin-top 0 must keep the 24pt Harbor floor: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        assert!(
            heading1_named.contains(&format!(r#"fo:margin-top="{heading1_before}""#)),
            "Writer Heading 1 sidebar must keep letter h1 margin-top 0: {heading1_named}"
        );
        assert!(
            !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
            "Writer Heading 1 sidebar must not keep Writer 0.1665in before: {heading1_named}"
        );
        assert!(
            heading1_named.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#))
                && !heading1_named.contains(r#"fo:margin-bottom="0.0835in""#),
            "h1 margin-top 0 must keep letter 0.6rem after on Heading 1: {heading1_named}"
        );
        assert_writer_heading_tracking(heading1_named);
        assert_writer_heading_leading(heading1_named);
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-top="{heading2_before}""#))
                && heading2_named.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#)),
            "h1 margin-top 0 must keep Writer Heading 2 1.1rem/0.45rem: {heading2_named}"
        );
        assert_writer_heading_leading(heading2_named);
    }

    #[test]
    fn write_heading1_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        h1.space_before = 200;
        h1.space_after = 400;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        h2.space_before = 360;
        h2.space_after = 240;
        section.body.push(Block::Paragraph(h2));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 1200,
                height: 1200,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let heading1_after = format_odf_length(HEADING1_AFTER_HU);
        let heading2_before = format_odf_length(HEADING2_BEFORE_HU);
        let heading2_after = format_odf_length(HEADING2_AFTER_HU);
        let image_before = format_odf_length(IMAGE_BEFORE_HU);
        let image_after = format_odf_length(IMAGE_AFTER_HU);
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert_eq!(HEADING1_AFTER_HU, 720, "letter h1 margin-bottom 0.6rem must stay 720 HU");
        assert_eq!(HEADING2_BEFORE_HU, 1320, "letter h2 margin-top 1.1rem must stay 1320 HU");
        assert_eq!(HEADING2_AFTER_HU, 540, "letter h2 margin-bottom 0.45rem must stay 540 HU");
        assert_eq!(IMAGE_BEFORE_HU, 720);
        assert_eq!(IMAGE_AFTER_HU, 840);
        assert_eq!(
            HEADING1_AFTER_HU, IMAGE_BEFORE_HU,
            "h1 0.6rem after must stay the same strut as img 0.6rem"
        );
        assert!(
            h1.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#)),
            "Heading1 after must be letter 0.6rem ({heading1_after}), not Writer 0.0835in / IR 400 HU: {h1}"
        );
        assert!(
            !h1.contains(r#"fo:margin-bottom="0.0835in""#)
                && !h1.contains(r#"fo:margin-bottom="0.0555in""#),
            "Writer 0.0835in / IR 400 HU H1 after is the wrong strut vs WeasyPrint 0.6rem: {h1}"
        );
        assert!(
            h1.contains(&format!(
                r#"fo:margin-top="{}""#,
                format_odf_length(HEADING1_BEFORE_HU)
            )) && !h1.contains(r#"fo:margin-top="0.1665in""#),
            "h1 0.6rem after must keep letter h1 margin-top 0, not Writer 0.1665in: {h1}"
        );
        assert_writer_heading_leading(h1);
        assert_writer_heading_tracking(h1);
        assert_writer_heading_color(h1);
        assert!(
            h2.contains(&format!(r#"fo:margin-top="{heading2_before}""#))
                && h2.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#))
                && !h2.contains(r#"fo:margin-bottom="0.0835in""#),
            "h1 0.6rem after must keep h2 1.1rem/0.45rem: {h2}"
        );
        assert_writer_heading_leading(h2);
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{image_before}" fo:margin-bottom="{image_after}""#
            )),
            "h1 0.6rem after must keep Pimage 0.6rem/0.7rem: {xml}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\"") && xml.contains("svg:height=\"0.3333in\""),
            "h1 0.6rem after must keep the 24pt Harbor floor: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        assert!(
            heading1_named.contains(&format!(
                r#"fo:margin-top="{}""#,
                format_odf_length(HEADING1_BEFORE_HU)
            )) && !heading1_named.contains(r#"fo:margin-top="0.1665in""#),
            "h1 0.6rem after must keep letter h1 margin-top 0, not Writer 0.1665in: {heading1_named}"
        );
        assert!(
            heading1_named.contains(&format!(r#"fo:margin-bottom="{heading1_after}""#)),
            "Writer Heading 1 sidebar must keep letter 0.6rem after: {heading1_named}"
        );
        assert!(
            !heading1_named.contains(r#"fo:margin-bottom="0.0835in""#),
            "Writer Heading 1 sidebar must not keep Writer 0.0835in after: {heading1_named}"
        );
        assert_writer_heading_tracking(heading1_named);
        assert_writer_heading_leading(heading1_named);
        assert!(
            heading2_named.contains(&format!(r#"fo:margin-top="{heading2_before}""#))
                && heading2_named.contains(&format!(r#"fo:margin-bottom="{heading2_after}""#)),
            "h1 0.6rem after must keep Writer Heading 2 1.1rem/0.45rem: {heading2_named}"
        );
        assert_writer_heading_leading(heading2_named);
    }

    #[test]
    fn write_body_font_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        section.body.push(Block::Paragraph(code));
        let mut chip = Paragraph::from_text("prove");
        chip.runs[0].style.code = true;
        section.body.push(Block::Paragraph(chip));
        doc.sections.push(section);

        assert_eq!(BODY_FONT, "Segoe UI");
        assert_eq!(CODE_FONT, "Consolas");
        assert_ne!(BODY_FONT, "Liberation Sans");

        let xml = content_xml(&doc);
        assert_writer_body_font(test_style_xml(&xml, "Heading1", "paragraph"));
        assert_writer_body_font(test_style_xml(&xml, "Heading2", "paragraph"));
        assert_writer_body_font(test_style_xml(&xml, "Pbody", "paragraph"));
        assert_writer_body_font(test_style_xml(&xml, "Quote", "paragraph"));
        assert_writer_body_font(test_style_xml(&xml, "Plist", "paragraph"));
        assert_writer_body_font(test_style_xml(&xml, "PlistNested", "paragraph"));
        assert_writer_code_font(test_style_xml(&xml, "Tcode", "text"));
        assert_writer_code_font(test_style_xml(&xml, "CodeBlock", "paragraph"));
        assert!(
            xml.contains(&format!(r#"style:name="{BODY_FONT}""#))
                && xml.contains(&format!(r#"style:name="{CODE_FONT}""#)),
            "content.xml must declare letter {BODY_FONT} + {CODE_FONT} faces: {xml}"
        );
        assert!(
            !xml.contains(r#"style:font-name="Liberation Sans""#)
                && !xml.contains(r#"fo:font-family="Liberation Sans""#),
            "automatic styles must not keep Writer Liberation Sans: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        assert_writer_body_font(test_default_style_xml(&styles, "paragraph"));
        assert_writer_body_font(test_office_style_xml(&styles, "Standard"));
        assert_writer_body_font(test_office_style_xml(&styles, "Heading"));
        assert_writer_body_font(test_office_style_xml(&styles, "Heading_20_1"));
        assert_writer_body_font(test_office_style_xml(&styles, "Heading_20_2"));
        assert_writer_body_font(test_office_style_xml(&styles, "Text_20_body"));
        assert_writer_body_font(test_office_style_xml(&styles, "Internet_20_Link"));
        assert!(
            styles.contains(&format!(r#"style:name="{BODY_FONT}""#))
                && styles.contains(&format!(r#"style:name="{CODE_FONT}""#)),
            "styles.xml must declare letter {BODY_FONT} + {CODE_FONT} faces: {styles}"
        );
        assert!(
            !styles.contains(r#"style:font-name="Liberation Sans""#)
                && !styles.contains(r#"fo:font-family="Liberation Sans""#),
            "Writer package must not keep Liberation Sans body type: {styles}"
        );
    }

    #[test]
    fn write_heading_color_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        assert_ne!(BODY_FG, WRITER_HEADING_NAVY);
        assert_eq!(BODY_FG, "#134e4a");
        assert_eq!(WRITER_HEADING_NAVY, "#000080");
        assert_eq!(USE_WINDOW_FONT_COLOR, "false");

        let xml = content_xml(&doc);
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        let h3 = test_style_xml(&xml, "Heading3", "paragraph");
        assert_writer_heading_color(h1);
        assert_writer_heading_tracking(h1);
        assert_writer_heading_leading(h1);
        assert_writer_heading_color(h2);
        assert_writer_heading_leading(h2);
        assert_writer_heading_color(h3);
        assert_writer_heading_leading(h3);
        assert!(
            xml.contains(r#"text:style-name="Heading1""#)
                && xml.contains("Northwind Freight"),
            "letter H1 must stay Heading1: {xml}"
        );
        assert!(
            !xml.contains(&format!(r#"fo:color="{WRITER_HEADING_NAVY}""#)),
            "automatic Heading 1/2/3 must not keep Writer navy {WRITER_HEADING_NAVY}: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading = test_office_style_xml(&styles, "Heading");
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        let heading3_named = test_office_style_xml(&styles, "Heading_20_3");
        assert_writer_heading_color(heading);
        assert_writer_heading_color(heading1_named);
        assert_writer_heading_tracking(heading1_named);
        assert_writer_heading_leading(heading1_named);
        assert_writer_heading_color(heading2_named);
        assert_writer_heading_leading(heading2_named);
        assert_writer_heading_color(heading3_named);
        assert_writer_heading_leading(heading3_named);
        assert!(
            heading.contains(r#"style:parent-style-name="Standard""#)
                && heading1_named.contains(r#"style:display-name="Heading 1""#)
                && heading2_named.contains(r#"style:display-name="Heading 2""#),
            "named Heading 1/2 must stay the Writer sidebar styles: {styles}"
        );
        assert!(
            !styles.contains(&format!(r#"fo:color="{WRITER_HEADING_NAVY}""#))
                && !styles.contains("fo:color=\"#0000ff\"")
                && !styles.contains("fo:color=\"#2F5496\""),
            "Writer package Heading must not keep navy/Word blue: {styles}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_body_color(pbody);
        assert!(
            xml.contains(r#"style:name="THcell""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Quote""#)
                && xml.contains(&format!(r#"fo:border-left="{} solid {QUOTE_FG}""#, format_odf_length(QUOTE_BAR_W_HU))),
            "heading teal must keep THcell fill and quote bar: {xml}"
        );
    }

    #[test]
    fn write_heading_tracking_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        assert_eq!(HEADING1_LETTER_SPACING, "-0.36pt");
        assert_eq!(HEADING1_SIZE_PT, 18);

        let xml = content_xml(&doc);
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        assert_writer_heading_tracking(h1);
        assert_writer_heading_color(h1);
        assert_writer_heading_leading(h1);
        assert_writer_heading_leading(h2);
        assert!(
            !h2.contains("fo:letter-spacing="),
            "letter.css letter-spacing is h1-only, not Heading 2: {h2}"
        );
        assert!(
            xml.contains(r#"text:style-name="Heading1""#) && xml.contains("Northwind Freight"),
            "letter H1 must stay Heading1: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        assert_writer_heading_tracking(heading1_named);
        assert_writer_heading_color(heading1_named);
        assert_writer_heading_leading(heading1_named);
        assert_writer_heading_leading(heading2_named);
        assert!(
            !heading2_named.contains("fo:letter-spacing="),
            "Writer Heading 2 must not pick up h1 -.02em tracking: {heading2_named}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_body_color(pbody);
        assert_writer_line_height(pbody);
        assert!(
            xml.contains(r#"style:name="THcell""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Quote""#)
                && xml.contains(&format!(
                    r#"fo:border-left="{} solid {QUOTE_FG}""#,
                    format_odf_length(QUOTE_BAR_W_HU)
                ))
                && xml.contains(r#"style:name="Ltask""#),
            "h1 tracking must keep THcell fill, quote bar, and tasks: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:font-size="{CELL_SIZE_PT}""#)),
            "h1 tracking must keep letter 0.95rem table type: {xml}"
        );
    }

    #[test]
    fn write_heading_leading_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut h1 = Paragraph::from_text("Northwind Freight");
        h1.outline_level = Some(1);
        h1.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h1));
        let mut h2 = Paragraph::from_text("Delivery confirmation — order 48291");
        h2.outline_level = Some(2);
        h2.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h2));
        let mut h3 = Paragraph::from_text("Seals");
        h3.outline_level = Some(3);
        h3.runs[0].style.bold = true;
        section.body.push(Block::Paragraph(h3));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        doc.sections.push(section);

        assert_eq!(HEADING_LINE_HEIGHT, "120%");
        assert_eq!(BODY_LINE_HEIGHT, "115%");
        assert_eq!(HEADING1_LETTER_SPACING, "-0.36pt");

        let xml = content_xml(&doc);
        let h1 = test_style_xml(&xml, "Heading1", "paragraph");
        let h2 = test_style_xml(&xml, "Heading2", "paragraph");
        let h3 = test_style_xml(&xml, "Heading3", "paragraph");
        assert_writer_heading_leading(h1);
        assert_writer_heading_leading(h2);
        assert_writer_heading_leading(h3);
        assert_writer_heading_tracking(h1);
        assert_writer_heading_color(h1);
        assert_writer_heading_color(h2);
        assert!(
            xml.contains(r#"text:style-name="Heading1""#) && xml.contains("Northwind Freight"),
            "letter H1 must stay Heading1: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let heading1_named = test_office_style_xml(&styles, "Heading_20_1");
        let heading2_named = test_office_style_xml(&styles, "Heading_20_2");
        let heading3_named = test_office_style_xml(&styles, "Heading_20_3");
        assert_writer_heading_leading(heading1_named);
        assert_writer_heading_leading(heading2_named);
        assert_writer_heading_leading(heading3_named);
        assert_writer_heading_tracking(heading1_named);
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_body_color(pbody);
        assert_writer_line_height(pbody);
        assert!(
            !pbody.contains(&format!(r#"fo:line-height="{HEADING_LINE_HEIGHT}""#)),
            "body 1.15 must not pick up heading 1.2 leading: {pbody}"
        );
        assert!(
            xml.contains(r#"style:name="THcell""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Quote""#)
                && xml.contains(&format!(
                    r#"fo:border-left="{} solid {QUOTE_FG}""#,
                    format_odf_length(QUOTE_BAR_W_HU)
                ))
                && xml.contains(r#"style:name="Ltask""#),
            "h1 leading must keep THcell fill, quote bar, and tasks: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:font-size="{CELL_SIZE_PT}""#)),
            "h1 leading must keep letter 0.95rem table type: {xml}"
        );
    }

    #[test]
    fn write_body_color_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut super_p = Paragraph::from_text("A");
        super_p.runs[0].style.superscript = true;
        section.body.push(Block::Paragraph(super_p));
        let mut sub_p = Paragraph::from_text("2");
        sub_p.runs[0].style.subscript = true;
        section.body.push(Block::Paragraph(sub_p));
        let mut table = Table::from_cells(vec![vec!["Hop".into()], vec!["Input".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut done = Paragraph::from_text("Receiving dock is clear");
        done.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(done));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 2400,
                height: 2400,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        assert_eq!(BODY_FG, QUOTE_FG);
        assert_eq!(BODY_FG, "#134e4a");

        let xml = content_xml(&doc);
        assert!(
            xml.contains(r#"<text:p text:style-name="Pbody">6 September 2026</text:p>"#),
            "body copy must use Pbody so Writer paints letter teal, not UA black: {xml}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_body_color(pbody);
        let plist = test_style_xml(&xml, "Plist", "paragraph");
        assert_writer_body_color(plist);
        let plist_nested = test_style_xml(&xml, "PlistNested", "paragraph");
        assert_writer_body_color(plist_nested);
        let plist_last = test_style_xml(&xml, "PlistLast", "paragraph");
        assert_writer_body_color(plist_last);
        let tcell = test_style_xml(&xml, "Tcell", "table-cell");
        assert_writer_body_color(tcell);
        assert!(
            !tcell.contains(r#"fo:font-weight="bold""#)
                && !tcell.contains("fo:background-color=\"#ccfbf1\""),
            "body Tcell must stay letter teal, not th fill/bold: {tcell}"
        );

        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);
        let tsuper = test_style_xml(&xml, "Tsuper", "text");
        assert_writer_super(tsuper);
        let tsub = test_style_xml(&xml, "Tsub", "text");
        assert_writer_sub(tsub);
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                ))
                && thcell.contains(r#"fo:font-weight="bold""#),
            "body color must keep THcell teal fill: {thcell}"
        );
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "body color must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains("<draw:image") && !xml.contains(r#"svg:width="0.1666in""#),
            "body color must keep 24pt draw:image: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let default_p = test_default_style_xml(&styles, "paragraph");
        assert_writer_body_color(default_p);
        let standard = test_office_style_xml(&styles, "Standard");
        assert_writer_body_color(standard);
        let text_body = test_office_style_xml(&styles, "Text_20_body");
        assert_writer_body_color(text_body);
        assert!(
            styles.contains(r#"style:display-name="Heading 1""#)
                && styles.contains(r#"style:display-name="Text body""#)
                && styles.contains(r#"style:master-page style:name="Standard""#),
            "body color must keep Writer package styles.xml: {styles}"
        );
        assert_writer_line_height(pbody);
        assert_writer_line_height(default_p);
        assert_writer_line_height(standard);
        assert_writer_line_height(text_body);
    }

    #[test]
    fn write_text_body_line_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut table = Table::from_cells(vec![vec!["Hop".into()], vec!["Input".into()]]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut done = Paragraph::from_text("Receiving dock is clear");
        done.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(done));
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: 2400,
                height: 2400,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(img_p));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_line_height(pbody);
        assert_writer_body_color(pbody);
        let plist = test_style_xml(&xml, "Plist", "paragraph");
        let plist_nested = test_style_xml(&xml, "PlistNested", "paragraph");
        let plist_last = test_style_xml(&xml, "PlistLast", "paragraph");
        assert_writer_line_height(plist);
        assert_writer_line_height(plist_nested);
        assert_writer_line_height(plist_last);
        assert_writer_body_color(plist);
        assert_writer_body_color(plist_nested);
        assert_writer_body_color(plist_last);

        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                ))
                && thcell.contains(r#"fo:font-weight="bold""#),
            "Writer 1.15 must keep THcell teal fill: {thcell}"
        );
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "Writer 1.15 must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains("<draw:image") && !xml.contains(r#"svg:width="0.1666in""#),
            "Writer 1.15 must keep 24pt draw:image: {xml}"
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let default_p = test_default_style_xml(&styles, "paragraph");
        let standard = test_office_style_xml(&styles, "Standard");
        let text_body = test_office_style_xml(&styles, "Text_20_body");
        assert_writer_line_height(default_p);
        assert_writer_line_height(standard);
        assert_writer_line_height(text_body);
        assert_writer_body_color(default_p);
        assert_writer_body_color(standard);
        assert_writer_body_color(text_body);
        assert!(
            styles.contains(r#"style:display-name="Heading 1""#)
                && styles.contains(r#"style:display-name="Text body""#)
                && styles.contains(r#"style:master-page style:name="Standard""#),
            "Writer 1.15 must keep Writer package styles.xml: {styles}"
        );
        assert!(
            !standard.contains(r#"fo:line-height="100%""#)
                && !standard.contains(r#"fo:line-height="single""#)
                && !text_body.contains(r#"fo:line-height="100%""#),
            "style-only Standard/Text body must not reset to single: {standard} {text_body}"
        );
    }

    #[test]
    fn write_page_margin_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        assert_eq!(section.page.margin_left, DEFAULT_MARGIN_HU);
        assert_eq!(section.page.margin_top, DEFAULT_MARGIN_HU);
        let mut h = Paragraph::from_text("Northwind Freight");
        h.outline_level = Some(1);
        section.body.push(Block::Paragraph(h));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("6 September 2026")));
        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        let mut done = Paragraph::from_text("Receiving dock is clear");
        done.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(done));
        doc.sections.push(section);

        let letter_x = format_odf_length(PAGE_MARGIN_X_HU);
        let letter_y = format_odf_length(PAGE_MARGIN_Y_HU);
        let hangul = format_odf_length(DEFAULT_MARGIN_HU);
        let ua_75px = format_odf_length(75 * (HU_PER_INCH / 96));
        let letter_measure = A4_WIDTH_HU - 2 * PAGE_MARGIN_X_HU;
        let hangul_measure = A4_WIDTH_HU - 2 * DEFAULT_MARGIN_HU;
        assert!(letter_measure > hangul_measure);

        let xml = content_xml(&doc);
        let table1 = test_style_xml(&xml, "Table1", "table");
        let hop_w = parse_odf_length(test_style_attr(table1, "style:width")).unwrap();
        assert!(
            (letter_measure - 8..letter_measure + 8).contains(&hop_w),
            "hop table must span letter 2.75rem measure ({letter_measure}), got {hop_w}: {table1}"
        );
        assert!(
            hop_w > hangul_measure
                && !table1.contains(&format!(
                    r#"style:width="{}""#,
                    format_odf_length(hangul_measure)
                )),
            "Hangul 30mm gutters punch sparse bands beside the hop grid: {table1}"
        );
        let pbody = test_style_xml(&xml, "Pbody", "paragraph");
        assert_writer_body_color(pbody);
        assert_writer_line_height(pbody);
        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                ))
                && thcell.contains(r#"fo:font-weight="bold""#),
            "@page must keep THcell teal fill: {thcell}"
        );
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "@page must keep Writer task boxes: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let page = test_page_layout_xml(&styles);
        assert!(
            page.contains(&format!(r#"fo:margin-left="{letter_x}""#))
                && page.contains(&format!(r#"fo:margin-right="{letter_x}""#))
                && page.contains(&format!(r#"fo:margin-top="{letter_y}""#))
                && page.contains(&format!(r#"fo:margin-bottom="{letter_y}""#)),
            "Writer page-layout must be letter @page 2.5rem/2.75rem, not Hangul 30mm: {page}"
        );
        assert!(
            !page.contains(&format!(r#"fo:margin-left="{hangul}""#))
                && !page.contains(&format!(r#"fo:margin-top="{hangul}""#))
                && !page.contains(&format!(r#"fo:margin-left="{ua_75px}""#))
                && !page.contains(r#"fo:margin-left="0.7874in""#)
                && !page.contains(r#"fo:margin-left="1in""#),
            "Hangul 30mm / Writer 2cm / Word 1in / WeasyPrint UA 75px is a sparse Writer frame: {page}"
        );
        let standard = test_office_style_xml(&styles, "Standard");
        assert_writer_body_color(standard);
        assert_writer_line_height(standard);
        assert!(
            styles.contains(r#"style:display-name="Heading 1""#)
                && styles.contains(r#"style:master-page style:name="Standard""#),
            "@page must keep Writer package styles.xml: {styles}"
        );
    }

    #[test]
    fn write_list_quote_code_spacing_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        quote.space_before = 80;
        quote.space_after = 200;
        section.body.push(Block::Paragraph(quote));
        let mut code = Paragraph::from_text("docagent prove examples/letter.md");
        code.code_block = true;
        code.space_before = 80;
        code.space_after = 200;
        section.body.push(Block::Paragraph(code));
        let mut a = Paragraph::from_text("Receiving dock is clear");
        a.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        a.space_after = 80;
        let mut b = Paragraph::from_text("Bay 4, not bay 2");
        b.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        b.space_after = 80;
        let mut c = Paragraph::from_text("Hash the input");
        c.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        c.space_after = 80;
        section.body.push(Block::Paragraph(a));
        section.body.push(Block::Paragraph(b));
        section.body.push(Block::Paragraph(c));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        let quote = test_style_xml(&xml, "Quote", "paragraph");
        let quote_before = format_odf_length(QUOTE_BEFORE_HU);
        let quote_after = format_odf_length(QUOTE_AFTER_HU);
        let quote_pad_x = format_odf_length(QUOTE_PAD_X_HU);
        let quote_pad_y = format_odf_length(QUOTE_PAD_Y_HU);
        let quote_bar = format_odf_length(QUOTE_BAR_W_HU);
        assert!(
            quote.contains(&format!(r#"fo:margin-top="{quote_before}""#)),
            "Quote before must be letter 0.5rem ({quote_before}), not IR 80 HU: {quote}"
        );
        assert!(
            quote.contains(&format!(r#"fo:margin-bottom="{quote_after}""#)),
            "Quote after must be letter 1rem ({quote_after}), not IR 200 HU: {quote}"
        );
        assert!(
            quote.contains(&format!(r#"fo:padding-left="{quote_pad_x}""#)),
            "Quote pad-x must be letter 1rem ({quote_pad_x}): {quote}"
        );
        assert!(
            quote.contains(&format!(r#"fo:padding-top="{quote_pad_y}""#)),
            "Quote pad-y must be letter 0.15rem ({quote_pad_y}): {quote}"
        );
        assert!(
            quote.contains(&format!(r#"fo:border-left="{quote_bar} solid {QUOTE_FG}""#)),
            "Quote bar must be letter.html 4px {QUOTE_FG} ({quote_bar}), not 0.02in/gray: {quote}"
        );
        assert_writer_quote_bar(quote);
        assert!(
            !quote.contains(r#"fo:margin-left="0.1in""#),
            "Quote must not keep the jammed 0.1in left stub: {quote}"
        );

        let code = test_style_xml(&xml, "CodeBlock", "paragraph");
        let code_before = format_odf_length(CODE_BEFORE_HU);
        let code_after = format_odf_length(CODE_AFTER_HU);
        let code_pad_x = format_odf_length(CODE_PAD_X_HU);
        let code_pad_y = format_odf_length(CODE_PAD_Y_HU);
        assert!(
            code.contains(&format!(r#"fo:margin-top="{code_before}""#)),
            "CodeBlock before must be letter 0.6rem ({code_before}), not jammed: {code}"
        );
        assert!(
            code.contains(&format!(r#"fo:margin-bottom="{code_after}""#)),
            "CodeBlock after must be letter 1rem ({code_after}): {code}"
        );
        assert!(
            code.contains(&format!(r#"fo:padding-left="{code_pad_x}""#)),
            "CodeBlock pad-x must be letter 1rem ({code_pad_x}): {code}"
        );
        assert!(
            code.contains(&format!(r#"fo:padding-top="{code_pad_y}""#)),
            "CodeBlock pad-y must be letter 0.75rem ({code_pad_y}): {code}"
        );
        assert!(
            !code.contains(r#"fo:padding="0.1in""#),
            "CodeBlock must not keep the jammed 0.1in pad: {code}"
        );
        assert_writer_code_font(code);

        let plist = test_style_xml(&xml, "Plist", "paragraph");
        let plist_nested = test_style_xml(&xml, "PlistNested", "paragraph");
        let plist_last = test_style_xml(&xml, "PlistLast", "paragraph");
        let item_after = format_odf_length(LIST_ITEM_AFTER_HU);
        let nested_after = format_odf_length(LIST_NESTED_GAP_HU);
        let list_after = format_odf_length(LIST_AFTER_HU);
        assert!(
            plist.contains(&format!(r#"fo:margin-bottom="{item_after}""#)),
            "Plist after must be letter 0.35rem ({item_after}), not jammed 0: {plist}"
        );
        assert!(
            plist.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#)),
            "Plist must keep Writer 1.15 lines: {plist}"
        );
        assert!(
            plist_nested.contains(&format!(r#"fo:margin-bottom="{nested_after}""#))
                && plist_nested.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#)),
            "PlistNested after must be letter 0.25rem ({nested_after}), not consecutive 0.35rem: {plist_nested}"
        );
        assert!(
            plist_last.contains(&format!(r#"fo:margin-bottom="{list_after}""#)),
            "PlistLast after must be letter 1rem ({list_after}): {plist_last}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistNested">Receiving dock is clear</text:p>"#
            ),
            "parent of nested Bay 4 must use PlistNested 0.25rem, not Plist 0.35rem: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="PlistLast">Bay 4, not bay 2</text:p>"#),
            "last nested list item must use PlistLast: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="PlistLast">Hash the input</text:p>"#),
            "last decimal item must use PlistLast: {xml}"
        );
        assert!(
            !xml.contains(r#"fo:margin-bottom="0.0111in""#),
            "list/quote/code must not keep IR space_after 80 HU: {xml}"
        );

        let lbullet = test_list_style_xml(&xml, "Lbullet");
        let lnumber = test_list_style_xml(&xml, "Lnumber");
        let ltask = test_list_style_xml(&xml, "Ltask");
        for (name, style) in [("Lbullet", lbullet), ("Lnumber", lnumber), ("Ltask", ltask)] {
            assert!(
                style.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
                "{name} must use Writer label-alignment, not space-before: {style}"
            );
            let left1 = format_odf_length(LIST_INDENT_HU);
            let left2 = format_odf_length(list_level_left(2));
            let hang = format_odf_length(-LIST_HANGING_HU);
            assert!(
                style.contains(&format!(r#"fo:margin-left="{left1}""#)),
                "{name} level 1 must be letter 1.25rem ({left1}), not Writer 0.5in: {style}"
            );
            assert!(
                style.contains(&format!(r#"fo:text-indent="{hang}""#)),
                "{name} hanging must stay inside the 1.25rem gutter ({hang}), not Writer -0.25in: {style}"
            );
            assert!(
                style.contains(&format!(r#"fo:margin-left="{left2}""#)),
                "{name} level 2 must be letter 2.5rem ({left2}), not Writer 0.75in: {style}"
            );
            assert!(
                !style.contains(r#"fo:margin-left="0.5in""#)
                    && !style.contains(r#"fo:margin-left="0.75in""#)
                    && !style.contains(r#"fo:text-indent="-0.25in""#),
                "{name} must not keep Writer 0.5in/0.75in/-0.25in: {style}"
            );
            assert!(
                !style.contains("text:space-before"),
                "{name} must not keep the old space-before indent: {style}"
            );
        }

        assert!(
            xml.contains(r#"fo:font-size="18pt""#) && xml.contains(r#"style:name="Heading1""#),
            "list/quote spacing must not drop Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"fo:padding-left="{}""#, format_odf_length(CELL_PAD_X_HU)))
                || xml.contains(r#"style:name="Tcell""#),
            "list/quote spacing must keep the Tcell style: {xml}"
        );
    }

    #[test]
    fn write_list_indent_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut dock = Paragraph::from_text("Receiving dock is clear");
        dock.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut bay = Paragraph::from_text("Bay 4, not bay 2");
        bay.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut hash = Paragraph::from_text("Hash the input");
        hash.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(dock));
        section.body.push(Block::Paragraph(bay));
        section.body.push(Block::Paragraph(hash));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        let left1 = format_odf_length(LIST_INDENT_HU);
        let left2 = format_odf_length(list_level_left(2));
        let left3 = format_odf_length(list_level_left(3));
        let hang = format_odf_length(-LIST_HANGING_HU);
        assert_eq!(LIST_INDENT_HU, 1500, "1.25rem at 16px/96dpi must stay 1500 HU");
        assert_eq!(LIST_HANGING_HU, 1200, "1rem hang must stay inside 1.25rem");
        assert!(
            LIST_HANGING_HU < LIST_INDENT_HU,
            "Writer -0.25in hang would sit in the page margin"
        );
        for name in ["Lbullet", "Lnumber", "Ltask", "LtaskDone"] {
            let style = test_list_style_xml(&xml, name);
            assert!(
                style.contains(&format!(r#"fo:margin-left="{left1}""#)),
                "{name} level 1 must be letter 1.25rem ({left1}), not Writer 0.5in: {style}"
            );
            assert!(
                style.contains(&format!(r#"fo:margin-left="{left2}""#)),
                "{name} level 2 must be letter 2.5rem ({left2}), not Writer 0.75in: {style}"
            );
            assert!(
                style.contains(&format!(r#"fo:margin-left="{left3}""#)),
                "{name} level 3 must be letter 3.75rem ({left3}), not Writer 1in: {style}"
            );
            assert!(
                style.contains(&format!(r#"fo:text-indent="{hang}""#)),
                "{name} hanging must stay inside the 1.25rem gutter ({hang}): {style}"
            );
            assert!(
                !style.contains(r#"fo:margin-left="0.5in""#)
                    && !style.contains(r#"fo:margin-left="0.75in""#)
                    && !style.contains(r#"fo:margin-left="1in""#)
                    && !style.contains(r#"fo:text-indent="-0.25in""#),
                "{name} must not keep Writer 0.5in/0.75in/1in/-0.25in: {style}"
            );
            assert!(
                style.contains(r#"text:list-level-position-and-space-mode="label-alignment""#)
                    && !style.contains("text:space-before"),
                "{name} must keep Writer label-alignment: {style}"
            );
        }
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(r#"<text:list text:style-name="Lbullet">"#)
                && xml.contains(r#"<text:list text:style-name="Lnumber">"#),
            "letter lists must emit Ltask/Lbullet/Lnumber: {xml}"
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);
    }

    #[test]
    fn write_nested_list_gap_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut dock = Paragraph::from_text("Receiving dock is clear");
        dock.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut bay = Paragraph::from_text("Bay 4, H2O seals intact");
        bay.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut capsule = Paragraph::from_text("Capsule hashes travel with the files");
        capsule.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut replay = Paragraph::from_text("Replay the convert instead of hoping");
        replay.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: None,
            format: Some(NumberFormat::Task),
        });
        let mut convert = Paragraph::from_text("Run Convert");
        convert.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(2),
            format: Some(NumberFormat::Decimal),
        });
        let mut pdf = Paragraph::from_text("PDF/A");
        pdf.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 1,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(dock));
        section.body.push(Block::Paragraph(bay));
        section.body.push(Block::Paragraph(capsule));
        section.body.push(Block::Paragraph(replay));
        section.body.push(Block::Paragraph(convert));
        section.body.push(Block::Paragraph(pdf));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert_eq!(LIST_NESTED_GAP_HU, 300);
        assert!(LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU);
        let plist = test_style_xml(&xml, "Plist", "paragraph");
        let plist_nested = test_style_xml(&xml, "PlistNested", "paragraph");
        let plist_last = test_style_xml(&xml, "PlistLast", "paragraph");
        let item_after = format_odf_length(LIST_ITEM_AFTER_HU);
        let nested_after = format_odf_length(LIST_NESTED_GAP_HU);
        let list_after = format_odf_length(LIST_AFTER_HU);
        assert!(
            plist.contains(&format!(r#"fo:margin-bottom="{item_after}""#)),
            "Plist after must stay letter 0.35rem ({item_after}): {plist}"
        );
        assert!(
            plist_nested.contains(&format!(r#"fo:margin-bottom="{nested_after}""#))
                && !plist_nested.contains(&format!(r#"fo:margin-bottom="{item_after}""#))
                && !plist_nested.contains(&format!(r#"fo:margin-bottom="{list_after}""#)),
            "PlistNested after must be letter 0.25rem ({nested_after}), not consecutive 0.35rem: {plist_nested}"
        );
        assert!(
            plist_nested.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#))
                && plist_nested.contains(&format!(r#"fo:color="{BODY_FG}""#)),
            "PlistNested must keep Writer 1.15 + letter teal: {plist_nested}"
        );
        assert!(
            plist_last.contains(&format!(r#"fo:margin-bottom="{list_after}""#)),
            "nested gap must keep PlistLast 1rem ({list_after}): {plist_last}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistNested">Receiving dock is clear</text:p>"#
            ),
            "dock parent of Bay 4 must use PlistNested 0.25rem, not Plist 0.35rem: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="Plist">Bay 4, H2O seals intact</text:p>"#),
            "nested Bay 4 stays Plist: {xml}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="Plist">Capsule hashes travel with the files</text:p>"#
            ),
            "consecutive task after nested must stay Plist 0.35rem: {xml}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistLast">Replay the convert instead of hoping</text:p>"#
            ),
            "last task must stay PlistLast: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="PlistNested">Run Convert</text:p>"#),
            "Convert parent of nested PDF/A must use PlistNested: {xml}"
        );
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "nested gap must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "nested gap must keep Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tcell""#) && xml.contains(r#"fo:border="0.5pt solid"#),
            "nested gap must keep table pad/borders: {xml}"
        );
    }

    #[test]
    fn write_list_block_after_like_letter() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut dock = Paragraph::from_text("Receiving dock is clear");
        dock.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut bay = Paragraph::from_text("Bay 4, H2O seals intact");
        bay.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut capsule = Paragraph::from_text("Capsule hashes travel with the files");
        capsule.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Task),
        });
        let mut replay = Paragraph::from_text("Replay the convert instead of hoping");
        replay.numbering = Some(NumberingRef {
            definition_id: 2,
            level: 0,
            start: None,
            format: Some(NumberFormat::Task),
        });
        section.body.push(Block::Paragraph(dock));
        section.body.push(Block::Paragraph(bay));
        section.body.push(Block::Paragraph(capsule));
        section.body.push(Block::Paragraph(replay));
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("Agent replay:")));
        let mut hash = Paragraph::from_text("Hash the input");
        hash.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut convert = Paragraph::from_text("Run Convert");
        convert.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(2),
            format: Some(NumberFormat::Decimal),
        });
        let mut pdf = Paragraph::from_text("PDF/A");
        pdf.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 1,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        let mut html = Paragraph::from_text("HTML");
        html.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 1,
            start: Some(2),
            format: Some(NumberFormat::Decimal),
        });
        let mut compare = Paragraph::from_text("Compare the capsule");
        compare.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(3),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(hash));
        section.body.push(Block::Paragraph(convert));
        section.body.push(Block::Paragraph(pdf));
        section.body.push(Block::Paragraph(html));
        section.body.push(Block::Paragraph(compare));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert_eq!(LIST_AFTER_HU, 1200, "letter ul,ol 1rem must stay 1200 HU");
        assert_eq!(LIST_ITEM_AFTER_HU, 420, "letter li 0.35rem must stay 420 HU");
        assert_eq!(LIST_NESTED_GAP_HU, 300, "letter ul ul 0.25rem must stay 300 HU");
        assert!(
            LIST_ITEM_AFTER_HU < LIST_AFTER_HU,
            "last-item 1rem must stay larger than consecutive 0.35rem"
        );
        assert!(
            LIST_NESTED_GAP_HU < LIST_ITEM_AFTER_HU,
            "nested 0.25rem must stay tighter than consecutive 0.35rem"
        );

        let plist = test_style_xml(&xml, "Plist", "paragraph");
        let plist_nested = test_style_xml(&xml, "PlistNested", "paragraph");
        let plist_last = test_style_xml(&xml, "PlistLast", "paragraph");
        let item_after = format_odf_length(LIST_ITEM_AFTER_HU);
        let nested_after = format_odf_length(LIST_NESTED_GAP_HU);
        let list_after = format_odf_length(LIST_AFTER_HU);
        assert!(
            plist.contains(&format!(r#"fo:margin-bottom="{item_after}""#))
                && !plist.contains(&format!(r#"fo:margin-bottom="{list_after}""#)),
            "Plist after must stay letter 0.35rem ({item_after}), not last-item 1rem: {plist}"
        );
        assert!(
            plist_nested.contains(&format!(r#"fo:margin-bottom="{nested_after}""#))
                && !plist_nested.contains(&format!(r#"fo:margin-bottom="{list_after}""#)),
            "PlistNested after must stay letter 0.25rem ({nested_after}): {plist_nested}"
        );
        assert!(
            plist_last.contains(&format!(r#"fo:margin-bottom="{list_after}""#))
                && !plist_last.contains(&format!(r#"fo:margin-bottom="{item_after}""#))
                && plist_last.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#))
                && plist_last.contains(&format!(r#"fo:color="{BODY_FG}""#)),
            "PlistLast after must be letter ul,ol 1rem ({list_after}), not consecutive 0.35rem: {plist_last}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistLast">Replay the convert instead of hoping</text:p>"#
            ),
            "last task must use PlistLast 1rem so the list is not jammed into the next block: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="PlistLast">Compare the capsule</text:p>"#),
            "last decimal must use PlistLast 1rem: {xml}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="Plist">Capsule hashes travel with the files</text:p>"#
            ),
            "consecutive task must stay Plist 0.35rem: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="Plist">Hash the input</text:p>"#),
            "consecutive decimal must stay Plist 0.35rem, not PlistLast 1rem: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="Plist">HTML</text:p>"#),
            "last nested HTML under Convert must stay Plist 0.35rem, not outer 1rem: {xml}"
        );
        assert!(
            xml.contains(
                r#"<text:p text:style-name="PlistNested">Receiving dock is clear</text:p>"#
            ) && xml.contains(r#"<text:p text:style-name="PlistNested">Run Convert</text:p>"#),
            "parents of nested children must keep PlistNested 0.25rem: {xml}"
        );
        assert!(
            xml.contains(r#"<text:p text:style-name="Pbody">Agent replay:</text:p>"#),
            "list 1rem after must not leak onto the next body: {xml}"
        );
        assert!(
            xml.contains(r#"<text:list text:style-name="Ltask">"#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "list after must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "list after must keep Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tcell""#) && xml.contains(r#"fo:border="0.5pt solid"#),
            "list after must keep table pad/borders: {xml}"
        );
    }

    #[test]
    fn write_quote_bar_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:p text:style-name="Quote">Replay is evidence. Same fonts, same bytes.</text:p>"#),
            "letter blockquote must use Quote so Writer paints the left bar: {xml}"
        );
        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);
        let quote_bar = format_odf_length(QUOTE_BAR_W_HU);
        assert_eq!(QUOTE_BAR_W_HU, 300, "4px at 96dpi must stay 300 HU");
        assert_eq!(QUOTE_FG, "#134e4a", "quote bar and text share letter teal");
        assert!(
            quote.contains(&format!(r#"fo:border-left="{quote_bar} solid {QUOTE_FG}""#)),
            "Quote fo:border-left must match letter.html 4px {QUOTE_FG} ({quote_bar}): {quote}"
        );
        assert!(
            quote.contains(r#"fo:border-right="none""#)
                && quote.contains(r#"fo:border-top="none""#)
                && quote.contains(r#"fo:border-bottom="none""#),
            "Quote must keep other sides none so Writer paints a left bar, not a box: {quote}"
        );
        assert!(
            quote.contains(&format!(r#"fo:color="{QUOTE_FG}""#)),
            "Quote text must stay letter.html {QUOTE_FG}: {quote}"
        );

        let tsuper = test_style_xml(&xml, "Tsuper", "text");
        assert_writer_super(tsuper);
        let tsub = test_style_xml(&xml, "Tsub", "text");
        assert_writer_sub(tsub);
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                ))
                && thcell.contains(r#"fo:font-weight="bold""#),
            "quote bar must keep THcell teal fill + 2px rule: {thcell}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "quote bar must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{}" fo:margin-bottom="{}""#,
                format_odf_length(IMAGE_BEFORE_HU),
                format_odf_length(IMAGE_AFTER_HU)
            )),
            "quote bar must keep Pimage 0.6rem/0.7rem 24pt-mark gap: {xml}"
        );
        assert_eq!(MIN_IMAGE_EDGE_HU, 24 * HU_PER_POINT);
        assert!(
            xml.contains(r#"style:name="Tcode""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Tmark""#),
            "quote bar must keep Tcode/Tmark chips: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "quote bar must keep Heading1 18pt: {xml}"
        );
    }

    #[test]
    fn write_hr_and_hyperlink_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("before")));
        section.body.push(Block::Break(BreakKind::Thematic));
        let mut p = Paragraph::from_text("");
        p.runs = vec![
            Run::hyperlink("DocAgent", "https://github.com/kevin9327/docagent"),
            Run::text(" runtime"),
        ];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:p text:style-name="HorizontalLine"/>"#),
            "thematic break must emit Writer HorizontalLine: {xml}"
        );
        let hr = test_style_xml(&xml, "HorizontalLine", "paragraph");
        let hr_border = format_odf_length(HR_BORDER_HU);
        let hr_margin = format_odf_length(HR_MARGIN_HU);
        assert!(
            hr.contains(&format!(r#"fo:border-top="{hr_border} solid #134e4a""#)),
            "hr must be letter 3px ({hr_border}), not missing/0.02in: {hr}"
        );
        assert!(
            hr.contains(&format!(r#"fo:margin-top="{hr_margin}""#))
                && hr.contains(&format!(r#"fo:margin-bottom="{hr_margin}""#)),
            "hr must keep letter 1.1rem ({hr_margin}): {hr}"
        );
        assert!(
            !hr.contains(r#"0.02in"#) && !hr.contains(r#"0.15in"#),
            "hr must not keep the missing 0.02in/0.15in rule: {hr}"
        );

        assert!(
            xml.contains(r#"<text:a text:style-name="Tlink" xlink:type="simple" xlink:href="https://github.com/kevin9327/docagent">DocAgent</text:a>"#),
            "letter link must be a visible text:a, not dropped: {xml}"
        );
        let tlink = test_style_xml(&xml, "Tlink", "text");
        assert_writer_internet_link(tlink);
        assert!(
            tlink.contains(r#"style:parent-style-name="Internet_20_Link""#),
            "Tlink must parent Writer Internet Link so the sidebar is not navy: {tlink}"
        );

        let odt = write(&doc).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        let internet = test_office_style_xml(&styles, "Internet_20_Link");
        let visited = test_office_style_xml(&styles, "Visited_20_Internet_20_Link");
        assert_writer_internet_link(internet);
        assert_writer_internet_link(visited);
        assert!(
            styles.contains(r#"style:display-name="Internet Link""#)
                && styles.contains(r#"style:display-name="Visited Internet Link""#),
            "styles.xml must include Writer Internet Link / Visited Internet Link: {styles}"
        );
        assert!(
            !styles.contains(&format!(r#"fo:color="{WRITER_VISITED_MAROON}""#)),
            "Visited Internet Link must not keep Writer maroon: {visited}"
        );

        assert!(
            xml.contains(r#"fo:font-size="18pt""#) && xml.contains(r#"style:name="Heading1""#),
            "hr/link styles must not drop Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tcell""#)
                && xml.contains(r#"fo:border="0.5pt solid"#),
            "hr/link styles must keep table pad/borders: {xml}"
        );
        assert!(
            xml.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
            "hr/link styles must keep label-alignment lists: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Quote""#) && xml.contains(r#"style:name="CodeBlock""#),
            "hr/link styles must keep Quote/CodeBlock: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Pbody""#) && xml.contains(r#"fo:line-height="115%""#),
            "hr/link styles must keep Pbody spacing: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "hr/link styles must keep Writer task boxes: {xml}"
        );
    }

    #[test]
    fn write_inline_code_and_mark_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("one convert, ");
        let mut mark = Run::text("three hashes");
        mark.style.highlight = Some(Color::MARK);
        p.runs.push(mark);
        p.runs.push(Run::text(", "));
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs.push(bold);
        p.runs.push(Run::text(". Run "));
        let mut code = Run::text("prove");
        code.style.code = true;
        p.runs.push(code);
        p.runs.push(Run::text(" twice."));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:span text:style-name="Tcode">prove</text:span>"#),
            "inline code must emit Tcode, not plain text: {xml}"
        );
        assert!(
            xml.contains(r#"<text:span text:style-name="Tmark">three hashes</text:span>"#),
            "highlight must emit Tmark so Writer shows the amber wash: {xml}"
        );

        let tcode = test_style_xml(&xml, "Tcode", "text");
        let code_pad_x = format_odf_length(CODE_SPAN_PAD_X_HU);
        let code_pad_y = format_odf_length(CODE_SPAN_PAD_Y_HU);
        assert!(
            tcode.contains("fo:background-color=\"#ccfbf1\""),
            "Tcode background must be letter #ccfbf1: {tcode}"
        );
        assert!(
            tcode.contains("fo:color=\"#134e4a\"")
                && tcode.contains(&format!(r#"style:font-name="{CODE_FONT}""#))
                && tcode.contains(&format!(r#"fo:font-family="{CODE_FONT}""#))
                && tcode.contains(&format!(r#"fo:font-size="{CODE_SPAN_SIZE}""#)),
            "Tcode must keep letter teal Consolas .92em: {tcode}"
        );
        assert!(
            tcode.contains(&format!(r#"fo:padding="{code_pad_y}""#))
                && tcode.contains(&format!(r#"fo:padding-left="{code_pad_x}""#))
                && tcode.contains(&format!(r#"fo:padding-right="{code_pad_x}""#)),
            "Tcode pad must be letter .1em/.35em ({code_pad_y}/{code_pad_x}): {tcode}"
        );
        assert!(
            !tcode.contains(r#"fo:padding="0in""#)
                && !tcode.contains(r#"fo:background-color="none"#)
                && !tcode.contains("transparent"),
            "Tcode must not reset to a bare mono span: {tcode}"
        );

        let tmark = test_style_xml(&xml, "Tmark", "text");
        let mark_pad_x = format_odf_length(MARK_SPAN_PAD_X_HU);
        let mark_pad_y = format_odf_length(MARK_SPAN_PAD_Y_HU);
        assert_writer_mark(tmark);
        assert!(
            tmark.contains(r#"style:parent-style-name="Mark""#),
            "Tmark must parent Writer Mark so UA Highlight is not black-on-amber: {tmark}"
        );
        assert!(
            tmark.contains(&format!(r#"fo:padding="{mark_pad_y}""#))
                && tmark.contains(&format!(r#"fo:padding-left="{mark_pad_x}""#))
                && tmark.contains(&format!(r#"fo:padding-right="{mark_pad_x}""#)),
            "Tmark pad must be letter .05em/.2em ({mark_pad_y}/{mark_pad_x}): {tmark}"
        );

        let odt = write(&doc).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut styles_bytes = Vec::new();
        zip.by_name("styles.xml")
            .unwrap()
            .read_to_end(&mut styles_bytes)
            .unwrap();
        let styles = String::from_utf8(styles_bytes).unwrap();
        assert!(
            styles.contains(r#"style:display-name="Mark""#),
            "styles.xml must include Writer Mark: {styles}"
        );
        let mark_named = test_office_style_xml(&styles, "Mark");
        assert_writer_mark(mark_named);

        assert!(
            xml.contains(r#"style:name="HorizontalLine""#)
                && xml.contains(&format!(r#"fo:border-top="{} solid #134e4a""#, format_odf_length(HR_BORDER_HU))),
            "inline code/mark must keep hr 3px #134e4a: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tlink""#) && xml.contains("fo:color=\"#0d9488\""),
            "inline code/mark must keep Tlink teal underline: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Quote""#) && xml.contains(r#"style:name="CodeBlock""#),
            "inline code/mark must keep Quote/CodeBlock: {xml}"
        );
        assert!(
            xml.contains(r#"text:list-level-position-and-space-mode="label-alignment""#),
            "inline code/mark must keep label-alignment lists: {xml}"
        );
        assert!(
            xml.contains(r#"fo:font-size="18pt""#) && xml.contains(r#"style:name="Heading1""#),
            "inline code/mark must keep Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tcell""#) && xml.contains(r#"fo:border="0.5pt solid"#),
            "inline code/mark must keep table pad/borders: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Pbody""#) && xml.contains(r#"fo:line-height="115%""#),
            "inline code/mark must keep Pbody spacing: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "inline code/mark must keep Writer task boxes: {xml}"
        );
    }

    #[test]
    fn write_strike_and_underline_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut strike_p = Paragraph::from_text("Agents ");
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        strike_p.runs.push(strike);
        strike_p.runs.push(Run::text("."));
        section.body.push(Block::Paragraph(strike_p));
        let mut under_p = Paragraph::from_text("Please confirm before ");
        let mut under = Run::text("09:00 local");
        under.style.underline = Underline::Single;
        under_p.runs.push(under);
        under_p.runs.push(Run::text(":"));
        section.body.push(Block::Paragraph(under_p));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:span text:style-name="Tstrike">guess</text:span>"#),
            "~~guess~~ must emit Tstrike so Writer paints the line: {xml}"
        );
        assert!(
            xml.contains(r#"<text:span text:style-name="Tunder">09:00 local</text:span>"#),
            "__09:00 local__ must emit Tunder so Writer paints the rule: {xml}"
        );

        let tstrike = test_style_xml(&xml, "Tstrike", "text");
        assert_writer_strike(tstrike);
        let tbstrike = test_style_xml(&xml, "Tbstrike", "text");
        assert_writer_strike(tbstrike);
        assert!(
            tbstrike.contains(r#"fo:font-weight="bold""#),
            "Tbstrike must keep bold + Writer strike: {tbstrike}"
        );
        let tunder = test_style_xml(&xml, "Tunder", "text");
        assert_writer_underline(tunder);
        let tsuper = test_style_xml(&xml, "Tsuper", "text");
        assert_writer_super(tsuper);
        let tsub = test_style_xml(&xml, "Tsub", "text");
        assert_writer_sub(tsub);

        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                )),
            "strike/underline must keep THcell teal fill + 2px rule: {thcell}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "strike/underline must keep Writer task boxes: {xml}"
        );
        let tcode = test_style_xml(&xml, "Tcode", "text");
        let tmark = test_style_xml(&xml, "Tmark", "text");
        assert!(
            tcode.contains("fo:background-color=\"#ccfbf1\"")
                && tcode.contains("fo:color=\"#134e4a\""),
            "strike/underline must keep Tcode chip: {tcode}"
        );
        assert_writer_mark(tmark);
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "strike/underline must keep Heading1 18pt: {xml}"
        );
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{}" fo:margin-bottom="{}""#,
                format_odf_length(IMAGE_BEFORE_HU),
                format_odf_length(IMAGE_AFTER_HU)
            )),
            "strike/underline must keep Pimage 0.6rem/0.7rem 24pt-mark gap: {xml}"
        );

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("strike para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t == "guess")
        }));
        let Block::Paragraph(p) = &back.sections[0].body[1] else {
            panic!("underline para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.underline == Underline::Single
                && matches!(&r.content, RunContent::Text(t) if t == "09:00 local")
        }));
    }

    #[test]
    fn write_super_and_sub_like_writer() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("the PDF/");
        let mut sup = Run::text("A");
        sup.style.superscript = true;
        p.runs.push(sup);
        p.runs.push(Run::text(" bytes match. Wet cargo is H"));
        let mut sub = Run::text("2");
        sub.style.subscript = true;
        p.runs.push(sub);
        p.runs.push(Run::text("O only."));
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = content_xml(&doc);

        assert!(
            xml.contains(r#"<text:span text:style-name="Tsuper">A</text:span>"#),
            "PDF/^A^ must emit Tsuper so Writer raises the A, not missing: {xml}"
        );
        assert!(
            xml.contains(r#"<text:span text:style-name="Tsub">2</text:span>"#),
            "H~2~O must emit Tsub so Writer lowers the 2, not missing: {xml}"
        );

        let tsuper = test_style_xml(&xml, "Tsuper", "text");
        assert_writer_super(tsuper);
        let tsub = test_style_xml(&xml, "Tsub", "text");
        assert_writer_sub(tsub);

        let tstrike = test_style_xml(&xml, "Tstrike", "text");
        assert_writer_strike(tstrike);
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\"")
                && thcell.contains(&format!(
                    r#"fo:border-bottom="{} solid #0d9488""#,
                    format_odf_length(TH_RULE_HU)
                )),
            "super/sub must keep THcell teal fill + 2px rule: {thcell}"
        );
        assert!(
            xml.contains(r#"style:name="Ltask""#)
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_EMPTY}""#))
                && xml.contains(&format!(r#"text:bullet-char="{TASK_BOX_CHECKED}""#)),
            "super/sub must keep Writer task boxes: {xml}"
        );
        assert!(
            xml.contains(&format!(
                r#"style:name="Pimage" style:family="paragraph"><style:paragraph-properties fo:margin-top="{}" fo:margin-bottom="{}""#,
                format_odf_length(IMAGE_BEFORE_HU),
                format_odf_length(IMAGE_AFTER_HU)
            )),
            "super/sub must keep Pimage 0.6rem/0.7rem 24pt-mark gap: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Tcode""#)
                && xml.contains("fo:background-color=\"#ccfbf1\"")
                && xml.contains(r#"style:name="Tmark""#)
                && xml.contains("fo:background-color=\"#fde68a\""),
            "super/sub must keep Tcode/Tmark chips: {xml}"
        );
        assert!(
            xml.contains(r#"style:name="Heading1""#) && xml.contains(r#"fo:font-size="18pt""#),
            "super/sub must keep Heading1 18pt: {xml}"
        );
        let quote = test_style_xml(&xml, "Quote", "paragraph");
        assert_writer_quote_bar(quote);

        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("super/sub para");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.superscript && matches!(&r.content, RunContent::Text(t) if t == "A")
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.subscript && matches!(&r.content, RunContent::Text(t) if t == "2")
        }));
        assert!(p.runs.iter().any(|r| {
            !r.style.superscript
                && !r.style.subscript
                && matches!(&r.content, RunContent::Text(t) if t.contains("PDF/"))
        }));
    }

    fn assert_writer_cell_type(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:font-size="{CELL_SIZE_PT}""#)),
            "table type must be letter CSS 0.95rem ({CELL_SIZE_PT}), not body 10pt: {style}"
        );
        assert!(
            !style.contains(r#"fo:font-size="10pt""#)
                && !style.contains(r#"fo:font-size="12pt""#)
                && !style.contains(r#"fo:font-size="11pt""#),
            "body 10pt / Writer 11pt / 12pt is the wrong hop-table type: {style}"
        );
    }

    fn assert_writer_cell_valign(style: &str) {
        assert!(
            style.contains(&format!(r#"style:vertical-align="{CELL_VALIGN}""#)),
            "hop cell must stay letter.html th,td vertical-align top, not Writer middle: {style}"
        );
        assert!(
            !style.contains(r#"style:vertical-align="middle""#)
                && !style.contains(r#"style:vertical-align="automatic""#)
                && !style.contains(r#"style:vertical-align="bottom""#)
                && !style.contains(r#"style:vertical-align="center""#),
            "Writer/HTML-import middle hop cells punch sparse bands vs WeasyPrint: {style}"
        );
    }

    fn assert_writer_table_collapse(style: &str) {
        assert!(
            style.contains(&format!(r#"table:border-model="{TABLE_BORDER_MODEL}""#)),
            "hop table must stay letter.html table{{border-collapse:collapse}}, not Writer separating: {style}"
        );
        assert!(
            !style.contains(r#"table:border-model="separating""#)
                && !style.contains("fo:border-spacing=")
                && !style.contains(r#"table:border-model="separate""#),
            "Writer/HTML-import separating + 2px spacing punches sparse hop gaps vs WeasyPrint: {style}"
        );
    }

    fn assert_writer_body_font(style: &str) {
        assert!(
            style.contains(&format!(r#"style:font-name="{BODY_FONT}""#))
                && style.contains(&format!(r#"fo:font-family="{BODY_FONT}""#)),
            "body type must stay letter.html {BODY_FONT}, not Writer Liberation Sans: {style}"
        );
        assert!(
            !style.contains(r#"style:font-name="Liberation Sans""#)
                && !style.contains(r#"fo:font-family="Liberation Sans""#)
                && !style.contains(r#"style:font-name="Calibri""#)
                && !style.contains(r#"fo:font-family="Calibri""#),
            "Writer Liberation Sans / Word Calibri is not WeasyPrint letter {BODY_FONT}: {style}"
        );
    }

    fn assert_writer_code_font(style: &str) {
        assert!(
            style.contains(&format!(r#"style:font-name="{CODE_FONT}""#))
                && style.contains(&format!(r#"fo:font-family="{CODE_FONT}""#)),
            "code must keep letter {CODE_FONT}; Writer ignores fo:font-family without style:font-name: {style}"
        );
        assert!(
            !style.contains(&format!(r#"style:font-name="{BODY_FONT}""#))
                && !style.contains(r#"style:font-name="Liberation Sans""#),
            "code must not inherit body {BODY_FONT} / Liberation Sans: {style}"
        );
    }

    fn assert_writer_mark(style: &str) {
        assert!(
            style.contains("fo:background-color=\"#fde68a\""),
            "Tmark must stay letter amber so the highlight is visible: {style}"
        );
        assert!(
            style.contains(&format!(r#"fo:color="{MARK_FG}""#))
                && style.contains(r#"style:use-window-font-color="false""#)
                && !style.contains(r#"style:use-window-font-color="true""#),
            "Tmark type must stay letter.html {MARK_FG}, not UA black on yellow: {style}"
        );
        assert!(
            style.contains(&format!(r#"style:font-name="{BODY_FONT}""#))
                && style.contains(&format!(r#"fo:font-family="{BODY_FONT}""#)),
            "Tmark must keep letter {BODY_FONT} so Writer Highlight is not UA black-on-amber: {style}"
        );
        assert!(
            !style.contains("transparent")
                && !style.contains("#ffff00")
                && !style.contains("fo:color=\"#000\"")
                && !style.contains("fo:color=\"#000000\"")
                && !style.contains("fo:color=\"#111827\"")
                && !style.contains(r#"fo:padding="0in""#),
            "UA yellow/black mark punches the letter teal chip: {style}"
        );
    }

    fn assert_writer_body_color(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:color="{BODY_FG}""#)),
            "body type must stay letter.html {BODY_FG}, not UA black: {style}"
        );
        assert!(
            style.contains(r#"style:use-window-font-color="false""#)
                && !style.contains(r#"style:use-window-font-color="true""#),
            "Writer must not ignore fo:color {BODY_FG} via window font color: {style}"
        );
        assert!(
            !style.contains("fo:color=\"#000\"")
                && !style.contains("fo:color=\"#000000\"")
                && !style.contains("fo:color=\"#111827\"")
                && !style.contains("fo:color=\"black\""),
            "UA black / slate #111827 body type is not letter.html: {style}"
        );
    }

    fn assert_writer_internet_link(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:color="{LINK_FG}""#)),
            "Internet Link must stay letter.html {LINK_FG}, not Writer navy/maroon: {style}"
        );
        assert!(
            style.contains(r#"style:use-window-font-color="false""#)
                && !style.contains(r#"style:use-window-font-color="true""#),
            "Writer must not ignore Internet Link {LINK_FG} via window font color: {style}"
        );
        assert!(
            style.contains(r#"style:text-underline-style="solid""#)
                && style.contains(r#"style:text-underline-width="auto""#)
                && style.contains(r#"style:text-underline-color="font-color""#),
            "Internet Link must underline like Writer, not a missing rule: {style}"
        );
        assert!(
            !style.contains(&format!(r#"fo:color="{WRITER_HEADING_NAVY}""#))
                && !style.contains(&format!(r#"fo:color="{WRITER_VISITED_MAROON}""#))
                && !style.contains("fo:color=\"#0000ff\"")
                && !style.contains("fo:color=\"#0000FF\"")
                && !style.contains("fo:color=\"#800080\""),
            "Writer navy / visited maroon / UA blue is not letter.html {LINK_FG}: {style}"
        );
        assert!(
            !style.contains(r#"style:text-underline-style="none""#),
            "Internet Link must not reset to a missing rule: {style}"
        );
    }

    fn assert_writer_heading_tracking(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:letter-spacing="{HEADING1_LETTER_SPACING}""#)),
            "Heading 1 must keep letter.css -.02em tracking ({HEADING1_LETTER_SPACING}), not 0pt: {style}"
        );
        assert!(
            !style.contains(r#"fo:letter-spacing="0pt""#)
                && !style.contains(r#"fo:letter-spacing="0in""#)
                && !style.contains(r#"fo:letter-spacing="normal""#),
            "0pt / normal Heading 1 tracking is WeasyPrint letter-spacing dropped: {style}"
        );
    }

    fn assert_writer_heading_leading(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:line-height="{HEADING_LINE_HEIGHT}""#)),
            "heading must keep letter.css line-height:1.2 ({HEADING_LINE_HEIGHT}), not body 115%: {style}"
        );
        assert!(
            !style.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#))
                && !style.contains(r#"fo:line-height="100%""#)
                && !style.contains(r#"fo:line-height="single""#)
                && !style.contains(r#"fo:line-height="115%""#),
            "inherited Writer body 115% / single is jammed heading leading vs WeasyPrint: {style}"
        );
    }

    fn assert_writer_heading_color(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:color="{BODY_FG}""#)),
            "heading type must stay letter.html {BODY_FG}, not Writer navy: {style}"
        );
        assert!(
            style.contains(r#"style:use-window-font-color="false""#)
                && !style.contains(r#"style:use-window-font-color="true""#),
            "Writer use-window-font-color must stay false so Heading is {BODY_FG}, not navy {WRITER_HEADING_NAVY}: {style}"
        );
        assert!(
            !style.contains(&format!(r#"fo:color="{WRITER_HEADING_NAVY}""#))
                && !style.contains("fo:color=\"#0000ff\"")
                && !style.contains("fo:color=\"#0000FF\"")
                && !style.contains("fo:color=\"#2F5496\"")
                && !style.contains("fo:color=\"#2f5496\"")
                && !style.contains("fo:color=\"#365f91\"")
                && !style.contains("fo:color=\"#365F91\""),
            "Writer navy / Word heading blue is not letter.html teal: {style}"
        );
    }

    fn assert_writer_line_height(style: &str) {
        assert!(
            style.contains(&format!(r#"fo:line-height="{BODY_LINE_HEIGHT}""#)),
            "must keep Writer 1.15 lines, not single/jammed: {style}"
        );
        assert!(
            !style.contains(r#"fo:line-height="100%""#)
                && !style.contains(r#"fo:line-height="single""#)
                && !style.contains(r#"fo:line-height="240""#),
            "must not reset to single spacing: {style}"
        );
    }

    fn assert_writer_quote_bar(style: &str) {
        let quote_bar = format_odf_length(QUOTE_BAR_W_HU);
        assert!(
            style.contains(&format!(r#"fo:border-left="{quote_bar} solid {QUOTE_FG}""#)),
            "Quote bar must be letter.html 4px {QUOTE_FG} ({quote_bar}), not 0.02in/gray: {style}"
        );
        assert!(
            style.contains(r#"fo:border-right="none""#)
                && style.contains(r#"fo:border-top="none""#)
                && style.contains(r#"fo:border-bottom="none""#),
            "Quote must keep other sides none so Writer paints a left bar, not a box: {style}"
        );
        assert!(
            style.contains(&format!(r#"fo:color="{QUOTE_FG}""#)),
            "Quote text must stay letter.html {QUOTE_FG}: {style}"
        );
        assert!(
            !style.contains(r#"fo:border-left="none"#)
                && !style.contains(r#"fo:border-left="0in"#)
                && !style.contains(r#"fo:border-left="0.02in"#)
                && !style.contains(r#"fo:border-left="0.0492in"#)
                && !style.contains("solid #ccc")
                && !style.contains("solid #999")
                && !style.contains("solid #808080")
                && !style.contains("solid #000"),
            "UA/reset/0.02in/gray quote bar is not WeasyPrint letter input: {style}"
        );
    }

    fn assert_writer_strike(style: &str) {
        assert!(
            style.contains(r#"style:text-line-through-style="solid""#)
                && style.contains(r#"style:text-line-through-type="single""#)
                && style.contains(r#"style:text-line-through-width="auto""#)
                && style.contains(r#"style:text-line-through-color="font-color""#),
            "strike must be Writer Single line-through, not a missing rule: {style}"
        );
        assert_writer_body_color(style);
        assert!(
            !style.contains(r#"style:text-line-through-style="none""#)
                && !style.contains(r#"style:text-line-through-type="none""#),
            "strike must not reset to a missing line: {style}"
        );
    }

    fn assert_writer_underline(style: &str) {
        assert!(
            style.contains(r#"style:text-underline-style="solid""#)
                && style.contains(r#"style:text-underline-width="auto""#)
                && style.contains(r#"style:text-underline-color="font-color""#),
            "underline must be Writer Single, not a missing rule: {style}"
        );
        assert_writer_body_color(style);
        assert!(
            !style.contains(r#"style:text-underline-style="none""#),
            "underline must not reset to a missing rule: {style}"
        );
    }

    fn assert_writer_super(style: &str) {
        assert!(
            style.contains(&format!(r#"style:text-position="{SUPER_POSITION}""#)),
            "Tsuper must be Writer Automatic super {SUPER_REL_PCT}%, not missing: {style}"
        );
        assert_writer_body_color(style);
        assert!(
            !style.contains(r#"style:text-position="none""#)
                && !style.contains(r#"style:text-position="0%"#)
                && !style.contains("100%")
                && !style.contains(r#"style:text-position="sub"#),
            "Tsuper must not reset to baseline or subscript: {style}"
        );
    }

    fn assert_writer_sub(style: &str) {
        assert!(
            style.contains(&format!(r#"style:text-position="{SUB_POSITION}""#)),
            "Tsub must be Writer Automatic sub {SUPER_REL_PCT}%, not missing: {style}"
        );
        assert_writer_body_color(style);
        assert!(
            !style.contains(r#"style:text-position="none""#)
                && !style.contains(r#"style:text-position="0%"#)
                && !style.contains("100%")
                && !style.contains(r#"style:text-position="super"#),
            "Tsub must not reset to baseline or superscript: {style}"
        );
    }

    fn test_style_xml<'a>(xml: &'a str, name: &str, family: &str) -> &'a str {
        let needle = format!(r#"style:name="{name}" style:family="{family}""#);
        let start = xml
            .find(&needle)
            .unwrap_or_else(|| panic!("{name} {family} style"));
        let rest = &xml[start..];
        let end = rest.find("</style:style>").expect("style close");
        &rest[..end]
    }

    fn test_office_style_xml<'a>(xml: &'a str, name: &str) -> &'a str {
        let needle = format!(r#"style:style style:name="{name}""#);
        let start = xml.find(&needle).unwrap_or_else(|| panic!("{name} style"));
        let rest = &xml[start..];
        let end = rest.find("</style:style>").expect("style close");
        &rest[..end]
    }

    fn test_default_style_xml<'a>(xml: &'a str, family: &str) -> &'a str {
        let needle = format!(r#"style:default-style style:family="{family}""#);
        let start = xml
            .find(&needle)
            .unwrap_or_else(|| panic!("default {family} style"));
        let rest = &xml[start..];
        let end = rest
            .find("</style:default-style>")
            .expect("default style close");
        &rest[..end]
    }

    fn test_page_layout_xml(xml: &str) -> &str {
        let start = xml
            .find("<style:page-layout-properties")
            .expect("page-layout-properties");
        let rest = &xml[start..];
        let end = rest.find('>').expect("page-layout-properties close");
        &rest[..=end]
    }

    fn test_style_attr<'a>(style: &'a str, key: &str) -> &'a str {
        let needle = format!("{key}=\"");
        let start = style
            .find(&needle)
            .unwrap_or_else(|| panic!("{key} on {style}"))
            + needle.len();
        let end = style[start..]
            .find('"')
            .map(|i| start + i)
            .unwrap_or_else(|| panic!("{key} close on {style}"));
        &style[start..end]
    }

    fn test_list_style_xml<'a>(xml: &'a str, name: &str) -> &'a str {
        let needle = format!(r#"text:list-style style:name="{name}""#);
        let start = xml
            .find(&needle)
            .unwrap_or_else(|| panic!("{name} list style"));
        let rest = &xml[start..];
        let end = rest.find("</text:list-style>").expect("list style close");
        &rest[..end]
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
        assert!(
            xml.contains("draw:style-name=\"Ginline\"") && xml.contains("draw:z-index=\"0\""),
            "as-char Harbor mark must use Writer Ginline so draw:image is visible: {xml}"
        );
        let ginline = test_style_xml(&xml, "Ginline", "graphic");
        assert!(
            ginline.contains(r#"draw:fill="none""#)
                && ginline.contains(r#"style:background-transparency="100%""#)
                && ginline.contains(r#"fo:border="none""#)
                && ginline.contains(r#"draw:color-mode="standard""#)
                && ginline.contains(r#"draw:image-opacity="100%""#),
            "Ginline must not fill/stroke over the PNG: {ginline}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt.clone())).unwrap();
        let mut names = Vec::new();
        for i in 0..zip.len() {
            names.push(zip.by_index(i).unwrap().name().replace('\\', "/"));
        }
        assert!(
            names.iter().any(|n| n == "styles.xml"),
            "Writer package must include styles.xml: {names:?}"
        );
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
            assert!(
                manifest.contains(r#"manifest:full-path="styles.xml""#),
                "manifest must list styles.xml: {manifest}"
            );
            assert!(
                manifest.contains(r#"manifest:full-path="Pictures/""#),
                "manifest must list the Pictures/ directory: {manifest}"
            );
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

    #[test]
    fn write_floors_sub_24pt_png_to_visible_size() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "docs/assets/mark.png must be a PNG"
        );
        // 16px at 96dpi = 12pt. Same size markdown assigns to mark.png.
        let px = HU_PER_INCH / 96;
        let tiny = 16 * px;
        assert_eq!(tiny, 1200);
        assert!(tiny < MIN_IMAGE_EDGE_HU);

        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: tiny,
                height: tiny,
                alt_text: Some("Harbor mark".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(xml.contains("Pictures/image1.png"), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/png\""), "{xml}");
        assert!(xml.contains("<svg:title>Harbor mark</svg:title>"), "{xml}");
        assert!(
            !xml.contains("svg:width=\"0.1666in\""),
            "must not write the 12pt 16px box: {xml}"
        );
        assert!(
            !xml.contains("svg:height=\"0.1666in\""),
            "must not write the 12pt 16px box: {xml}"
        );
        assert!(
            xml.contains("svg:width=\"0.3333in\""),
            "24pt floor must be visible in LibreOffice: {xml}"
        );
        assert!(
            xml.contains("svg:height=\"0.3333in\""),
            "24pt floor must be visible in LibreOffice: {xml}"
        );

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt)).unwrap();
        let mut stored = Vec::new();
        zip.by_name("Pictures/image1.png")
            .unwrap()
            .read_to_end(&mut stored)
            .unwrap();
        assert_eq!(stored.as_slice(), png);
    }

    #[test]
    fn letter_shape_roundtrip_keeps_png_bytes() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "docs/assets/mark.png must be a PNG"
        );

        let mut doc = Document::new();
        let mut section = Section::default();
        let mut heading = Paragraph::from_text("Northwind Freight");
        heading.outline_level = Some(1);
        heading.runs[0].style.bold = true;
        heading.runs[0].style.size = 1800;
        section.body.push(Block::Paragraph(heading));

        let mut quote = Paragraph::from_text("Replay is evidence. Same fonts, same bytes.");
        quote.quote = true;
        section.body.push(Block::Paragraph(quote));

        let mut img_p = Paragraph::from_text("");
        img_p.runs.push(Run {
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
        section.body.push(Block::Paragraph(img_p));

        let mut table = Table::from_cells(vec![
            vec!["Hop".into(), "File".into(), "Proof".into()],
            vec!["Input".into(), "letter.md".into(), "input SHA-256".into()],
        ]);
        table.rows[0].header = true;
        table.header_row_count = 1;
        section.body.push(Block::Table(table));

        let mut bullet = Paragraph::from_text("Receiving dock is clear");
        bullet.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 0,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut nested = Paragraph::from_text("Bay 4, H2O seals intact");
        nested.numbering = Some(NumberingRef {
            definition_id: 0,
            level: 1,
            start: None,
            format: Some(NumberFormat::Bullet),
        });
        let mut numbered = Paragraph::from_text("Hash the input");
        numbered.numbering = Some(NumberingRef {
            definition_id: 1,
            level: 0,
            start: Some(1),
            format: Some(NumberFormat::Decimal),
        });
        section.body.push(Block::Paragraph(bullet));
        section.body.push(Block::Paragraph(nested));
        section.body.push(Block::Paragraph(numbered));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        assert!(xml.contains("<text:h"), "{xml}");
        assert!(xml.contains(r#"text:style-name="Heading1""#), "{xml}");
        assert!(xml.contains(r#"text:style-name="Quote""#), "{xml}");
        assert!(xml.contains("<table:table"), "{xml}");
        assert!(xml.contains("<table:table-column"), "{xml}");
        assert!(xml.contains("style:column-width"), "{xml}");
        assert!(
            xml.contains(&format!(r#"fo:border="0.5pt solid {CELL_BORDER_COLOR}""#))
                && !xml.contains(r#"fo:border="0.5pt solid #000000""#),
            "letter hop grid must be 0.5pt #cbd5e1, not LibreOffice black: {xml}"
        );
        assert!(xml.contains("table-header-rows"), "{xml}");
        let thcell = test_style_xml(&xml, "THcell", "table-cell");
        assert!(
            thcell.contains("fo:background-color=\"#ccfbf1\""),
            "letter table header must keep #ccfbf1 fill, not a sparse grid: {thcell}"
        );
        assert!(
            thcell.contains(&format!(
                r#"fo:border-bottom="{} solid #0d9488""#,
                format_odf_length(TH_RULE_HU)
            )),
            "letter table header must keep the 2px #0d9488 rule: {thcell}"
        );
        assert!(xml.contains("<text:list"), "{xml}");
        assert!(xml.contains("Lbullet"), "{xml}");
        assert!(xml.contains("Lnumber"), "{xml}");
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(xml.contains("Pictures/image1.png"), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/png\""), "{xml}");
        assert!(xml.contains("text:anchor-type=\"as-char\""), "{xml}");

        let odt = write(&doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt.clone())).unwrap();
        {
            let mut f = zip.by_name("Pictures/image1.png").unwrap();
            let mut stored = Vec::new();
            f.read_to_end(&mut stored).unwrap();
            assert_eq!(stored.as_slice(), png);
        }

        let back = roundtrip(&doc).unwrap();
        let body = &back.sections[0].body;
        let Block::Paragraph(h) = &body[0] else {
            panic!("heading {:?}", body);
        };
        assert_eq!(h.outline_level, Some(1));
        assert_eq!(h.plain_text(), "Northwind Freight");

        let Block::Paragraph(q) = &body[1] else {
            panic!("quote {:?}", body);
        };
        assert!(q.quote);
        assert!(q.plain_text().contains("Replay is evidence"));

        assert!(body.iter().any(|b| matches!(b, Block::Table(_))));
        let nums: Vec<_> = body
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => p.numbering.as_ref().map(|n| (n.format, n.level)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            [
                (Some(NumberFormat::Bullet), 0),
                (Some(NumberFormat::Bullet), 1),
                (Some(NumberFormat::Decimal), 0),
            ]
        );

        let img = first_image(&back);
        assert_eq!(img.bytes.as_slice(), png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, HU_PER_INCH);
        assert_eq!(img.height, HU_PER_INCH);
        assert_eq!(img.alt_text.as_deref(), Some("mark"));
    }

    /// 1×1 grayscale JPEG (Pillow quality=1). Magic is `FF D8 FF`.
    const JPEG_1X1: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00,
        0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00,
        0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x03, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08, 0x01,
        0x01, 0x00, 0x00, 0x3F, 0x00, 0x37, 0xFF, 0xD9,
    ];

    fn jpeg_image_doc(mime: &str) -> Document {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs.push(Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: JPEG_1X1.to_vec(),
                mime: mime.into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH / 2,
                alt_text: Some("jpeg-pixel".into()),
                wrap: WrapMode::Inline,
            })),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        doc
    }

    fn assert_odt_jpeg_package(doc: &Document) {
        let xml = content_xml(doc);
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(
            xml.contains("Pictures/image1.jpg") || xml.contains("Pictures/image1.jpeg"),
            "{xml}"
        );
        assert!(xml.contains("xlink:href="), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/jpeg\""), "{xml}");
        assert!(xml.contains("<svg:title>jpeg-pixel</svg:title>"), "{xml}");
        assert!(xml.contains("svg:width=\"1in\""), "{xml}");
        assert!(xml.contains("svg:height=\"0.5000in\""), "{xml}");

        let odt = write(doc).unwrap();
        assert!(sniff(&odt));
        let mut zip = ZipArchive::new(Cursor::new(odt.clone())).unwrap();
        let mut names = Vec::new();
        for i in 0..zip.len() {
            names.push(zip.by_index(i).unwrap().name().replace('\\', "/"));
        }
        let pic = names
            .iter()
            .find(|n| {
                let n = n.as_str();
                n.starts_with("Pictures/") && (n.ends_with(".jpg") || n.ends_with(".jpeg"))
            })
            .cloned()
            .unwrap_or_else(|| panic!("missing JPEG under Pictures/: {names:?}"));
        {
            let mut f = zip.by_name(&pic).unwrap();
            let mut stored = Vec::new();
            f.read_to_end(&mut stored).unwrap();
            assert_eq!(stored.as_slice(), JPEG_1X1);
        }
        {
            let mut f = zip.by_name("META-INF/manifest.xml").unwrap();
            let mut manifest = String::new();
            f.read_to_string(&mut manifest).unwrap();
            assert!(manifest.contains(&pic), "{manifest}");
            assert!(manifest.contains("image/jpeg"), "{manifest}");
        }

        let back = roundtrip(doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph {:?}", back.sections[0].body);
        };
        let Some(img) = p.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing image run: {:?}", p.runs);
        };
        assert_eq!(img.bytes.as_slice(), JPEG_1X1);
        assert_eq!(img.mime, "image/jpeg");
        assert_eq!(img.width, HU_PER_INCH);
        assert_eq!(img.height, HU_PER_INCH / 2);
        assert_eq!(img.alt_text.as_deref(), Some("jpeg-pixel"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn roundtrip_keeps_jpeg_image_bytes() {
        assert!(
            JPEG_1X1.starts_with(&[0xFF, 0xD8, 0xFF]),
            "JPEG_1X1 must start with FF D8 FF"
        );
        assert_odt_jpeg_package(&jpeg_image_doc("image/jpeg"));
        // Sniffed magic (empty mime) must still land as image/jpeg under Pictures/*.jpg.
        assert_odt_jpeg_package(&jpeg_image_doc(""));
    }

    #[test]
    fn roundtrip_keeps_non_inline_image_wrap() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let cases = [
            (WrapMode::Square, "Gsquare", "parallel"),
            (WrapMode::Tight, "Gtight", "parallel"),
            (WrapMode::Through, "Gthrough", "run-through"),
            (WrapMode::TopAndBottom, "Gtopbottom", "none"),
            (WrapMode::Behind, "Gbehind", "run-through"),
            (WrapMode::InFront, "Ginfront", "run-through"),
        ];
        for (wrap, style, wrap_attr) in cases {
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
                    alt_text: Some("wrapped".into()),
                    wrap,
                })),
            });
            section.body.push(Block::Paragraph(p));
            doc.sections.push(section);

            let xml = content_xml(&doc);
            assert!(
                xml.contains("text:anchor-type=\"paragraph\""),
                "{wrap:?}: {xml}"
            );
            assert!(
                !xml.contains("text:anchor-type=\"as-char\""),
                "{wrap:?}: {xml}"
            );
            assert!(
                xml.contains(&format!("draw:style-name=\"{style}\"")),
                "{wrap:?}: {xml}"
            );
            assert!(
                xml.contains(&format!("style:name=\"{style}\" style:family=\"graphic\"")),
                "{wrap:?}: {xml}"
            );
            assert!(
                xml.contains(&format!("style:wrap=\"{wrap_attr}\"")),
                "{wrap:?}: {xml}"
            );
            if wrap == WrapMode::Tight {
                assert!(xml.contains("style:wrap-contour=\"true\""), "{xml}");
            }
            if wrap == WrapMode::Behind {
                assert!(xml.contains("style:run-through=\"background\""), "{xml}");
            }
            if wrap == WrapMode::InFront {
                assert!(xml.contains("style:run-through=\"foreground\""), "{xml}");
            }
            assert!(xml.contains("<svg:title>wrapped</svg:title>"), "{xml}");

            let back = roundtrip(&doc).unwrap();
            let Block::Paragraph(p) = &back.sections[0].body[0] else {
                panic!("{wrap:?} paragraph {:?}", back.sections[0].body);
            };
            let Some(img) = p.runs.iter().find_map(|r| match &r.content {
                RunContent::Inline(InlineObject::Image(img)) => Some(img),
                _ => None,
            }) else {
                panic!("{wrap:?} missing image run: {:?}", p.runs);
            };
            assert_eq!(img.wrap, wrap);
            assert_eq!(img.bytes.as_slice(), png);
            assert_eq!(img.mime, "image/png");
            assert_eq!(img.width, HU_PER_INCH);
            assert_eq!(img.height, HU_PER_INCH);
            assert_eq!(img.alt_text.as_deref(), Some("wrapped"));
        }
    }

    #[test]
    fn roundtrip_keeps_tight_wrap() {
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
                height: HU_PER_INCH / 2,
                alt_text: Some("tight wrap".into()),
                wrap: WrapMode::Tight,
            })),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        assert!(xml.contains("text:anchor-type=\"paragraph\""), "{xml}");
        assert!(!xml.contains("text:anchor-type=\"as-char\""), "{xml}");
        assert!(xml.contains("draw:style-name=\"Gtight\""), "{xml}");
        assert!(
            xml.contains(
                "style:name=\"Gtight\" style:family=\"graphic\"><style:graphic-properties style:wrap=\"parallel\" style:wrap-contour=\"true\"/>"
            ),
            "{xml}"
        );
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(xml.contains("Pictures/image1.png"), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/png\""), "{xml}");
        assert!(xml.contains("<svg:title>tight wrap</svg:title>"), "{xml}");
        assert!(xml.contains("svg:width=\"1in\""), "{xml}");
        assert!(xml.contains("svg:height=\"0.5000in\""), "{xml}");

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
            assert!(manifest.contains("Pictures/image1.png"), "{manifest}");
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
        assert_eq!(img.height, HU_PER_INCH / 2);
        assert_eq!(img.alt_text.as_deref(), Some("tight wrap"));
        assert_eq!(img.wrap, WrapMode::Tight);
    }

    #[test]
    fn roundtrip_keeps_behind_wrap() {
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
                height: HU_PER_INCH / 2,
                alt_text: Some("behind wrap".into()),
                wrap: WrapMode::Behind,
            })),
        });
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        assert!(xml.contains("text:anchor-type=\"paragraph\""), "{xml}");
        assert!(!xml.contains("text:anchor-type=\"as-char\""), "{xml}");
        assert!(xml.contains("draw:style-name=\"Gbehind\""), "{xml}");
        assert!(
            xml.contains(
                "style:name=\"Gbehind\" style:family=\"graphic\"><style:graphic-properties style:wrap=\"run-through\" style:run-through=\"background\"/>"
            ),
            "{xml}"
        );
        assert!(xml.contains("<draw:image"), "{xml}");
        assert!(xml.contains("Pictures/image1.png"), "{xml}");
        assert!(xml.contains("draw:mime-type=\"image/png\""), "{xml}");
        assert!(xml.contains("<svg:title>behind wrap</svg:title>"), "{xml}");
        assert!(xml.contains("svg:width=\"1in\""), "{xml}");
        assert!(xml.contains("svg:height=\"0.5000in\""), "{xml}");

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
            assert!(manifest.contains("Pictures/image1.png"), "{manifest}");
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
        assert_eq!(img.height, HU_PER_INCH / 2);
        assert_eq!(img.alt_text.as_deref(), Some("behind wrap"));
        assert_eq!(img.wrap, WrapMode::Behind);
    }

    #[test]
    fn roundtrip_keeps_cell_image() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        assert!(
            png.starts_with(&[0x89, b'P', b'N', b'G']),
            "docs/assets/mark.png must be a PNG"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("cell mark".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let cell_start = xml
            .find("<table:table-cell")
            .expect("table cell in content.xml");
        let cell_xml = &xml[cell_start..];
        let cell_end = cell_xml
            .find("</table:table-cell>")
            .expect("table cell close");
        let cell_xml = &cell_xml[..cell_end];
        assert!(cell_xml.contains("<draw:frame"), "{cell_xml}");
        assert!(cell_xml.contains("<draw:image"), "{cell_xml}");
        assert!(cell_xml.contains("Pictures/image1.png"), "{cell_xml}");
        assert!(cell_xml.contains("xlink:href="), "{cell_xml}");
        assert!(
            cell_xml.contains("draw:mime-type=\"image/png\""),
            "{cell_xml}"
        );
        assert!(
            cell_xml.contains("<svg:title>cell mark</svg:title>"),
            "{cell_xml}"
        );
        assert!(
            cell_xml.contains("<svg:desc>cell mark</svg:desc>"),
            "{cell_xml}"
        );
        assert!(
            cell_xml.contains("text:anchor-type=\"as-char\""),
            "{cell_xml}"
        );

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
            assert!(manifest.contains("Pictures/image1.png"), "{manifest}");
            assert!(manifest.contains("image/png"), "{manifest}");
        }

        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table {:?}", back.sections[0].body);
        };
        let Block::Paragraph(cell) = &t.rows[0].cells[0].blocks[0] else {
            panic!("cell para {:?}", t.rows[0].cells[0].blocks);
        };
        let Some(img) = cell.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing cell image run: {:?}", cell.runs);
        };
        assert_eq!(img.bytes.as_slice(), png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.width, HU_PER_INCH);
        assert_eq!(img.height, HU_PER_INCH);
        assert_eq!(img.alt_text.as_deref(), Some("cell mark"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    #[test]
    fn roundtrip_keeps_cell_jpeg_image() {
        assert!(
            JPEG_1X1.starts_with(&[0xFF, 0xD8, 0xFF]),
            "JPEG_1X1 must start with FF D8 FF"
        );
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: JPEG_1X1.to_vec(),
                mime: "image/jpeg".into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH / 2,
                alt_text: Some("cell jpeg".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let xml = content_xml(&doc);
        let cell_start = xml
            .find("<table:table-cell")
            .expect("table cell in content.xml");
        let cell_xml = &xml[cell_start..];
        let cell_end = cell_xml
            .find("</table:table-cell>")
            .expect("table cell close");
        let cell_xml = &cell_xml[..cell_end];
        assert!(cell_xml.contains("<draw:frame"), "{cell_xml}");
        assert!(cell_xml.contains("<draw:image"), "{cell_xml}");
        assert!(
            cell_xml.contains("Pictures/image1.jpg") || cell_xml.contains("Pictures/image1.jpeg"),
            "{cell_xml}"
        );
        assert!(
            cell_xml.contains("draw:mime-type=\"image/jpeg\""),
            "{cell_xml}"
        );
        assert!(
            cell_xml.contains("<svg:title>cell jpeg</svg:title>"),
            "{cell_xml}"
        );

        let odt = write(&doc).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(odt.clone())).unwrap();
        let mut names = Vec::new();
        for i in 0..zip.len() {
            names.push(zip.by_index(i).unwrap().name().replace('\\', "/"));
        }
        let pic = names
            .iter()
            .find(|n| {
                let n = n.as_str();
                n.starts_with("Pictures/") && (n.ends_with(".jpg") || n.ends_with(".jpeg"))
            })
            .cloned()
            .unwrap_or_else(|| panic!("missing JPEG under Pictures/: {names:?}"));
        {
            let mut f = zip.by_name(&pic).unwrap();
            let mut stored = Vec::new();
            f.read_to_end(&mut stored).unwrap();
            assert_eq!(stored.as_slice(), JPEG_1X1);
        }

        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table {:?}", back.sections[0].body);
        };
        let Block::Paragraph(cell) = &t.rows[0].cells[0].blocks[0] else {
            panic!("cell para {:?}", t.rows[0].cells[0].blocks);
        };
        let Some(img) = cell.runs.iter().find_map(|r| match &r.content {
            RunContent::Inline(InlineObject::Image(img)) => Some(img),
            _ => None,
        }) else {
            panic!("missing cell jpeg run: {:?}", cell.runs);
        };
        assert_eq!(img.bytes.as_slice(), JPEG_1X1);
        assert_eq!(img.mime, "image/jpeg");
        assert_eq!(img.alt_text.as_deref(), Some("cell jpeg"));
        assert_eq!(img.wrap, WrapMode::Inline);
    }

    fn rewrite_content_xml(odt: &[u8], rewrite: impl FnOnce(&str) -> String) -> Vec<u8> {
        let mut zip = ZipArchive::new(Cursor::new(odt.to_vec())).unwrap();
        let mut files = Vec::new();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().to_string();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).unwrap();
            files.push((name, buf));
        }
        drop(zip);
        let content_i = files
            .iter()
            .position(|(name, _)| name.replace('\\', "/") == "content.xml")
            .expect("content.xml");
        let xml = String::from_utf8(files[content_i].1.clone()).unwrap();
        files[content_i].1 = rewrite(&xml).into_bytes();
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, buf) in &files {
                zip.start_file(name, stored).unwrap();
                zip.write_all(buf).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn first_image(doc: &Document) -> &ImageData {
        for block in &doc.sections[0].body {
            let runs = match block {
                Block::Paragraph(p) => &p.runs,
                Block::Table(t) => {
                    let Block::Paragraph(p) = &t.rows[0].cells[0].blocks[0] else {
                        panic!("cell para");
                    };
                    &p.runs
                }
                _ => continue,
            };
            if let Some(img) = runs.iter().find_map(|r| match &r.content {
                RunContent::Inline(InlineObject::Image(img)) => Some(img),
                _ => None,
            }) {
                return img;
            }
        }
        panic!("missing image in {:?}", doc.sections[0].body);
    }

    #[test]
    fn reads_alt_from_svg_desc_when_title_absent() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("cell mark".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);

        let patched = rewrite_content_xml(&write(&doc).unwrap(), |xml| {
            assert!(xml.contains("<svg:title>cell mark</svg:title>"), "{xml}");
            assert!(xml.contains("<svg:desc>cell mark</svg:desc>"), "{xml}");
            xml.replace("<svg:title>cell mark</svg:title>", "")
                .replace("<svg:desc>cell mark</svg:desc>", "<svg:desc>from desc</svg:desc>")
        });
        let back = read(&patched).unwrap();
        let img = first_image(&back);
        assert_eq!(img.bytes.as_slice(), png);
        assert_eq!(img.mime, "image/png");
        assert_eq!(img.alt_text.as_deref(), Some("from desc"));
    }

    #[test]
    fn prefers_svg_title_over_svg_desc() {
        let png: &[u8] = include_bytes!("../../../docs/assets/mark.png");
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("");
        p.runs = vec![Run {
            style: CharStyle::default(),
            content: RunContent::Inline(InlineObject::Image(ImageData {
                bytes: png.to_vec(),
                mime: "image/png".into(),
                width: HU_PER_INCH,
                height: HU_PER_INCH,
                alt_text: Some("from title".into()),
                wrap: WrapMode::Inline,
            })),
        }];
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);

        let patched = rewrite_content_xml(&write(&doc).unwrap(), |xml| {
            xml.replace(
                "<svg:desc>from title</svg:desc>",
                "<svg:desc>from desc</svg:desc>",
            )
        });
        let back = read(&patched).unwrap();
        let img = first_image(&back);
        assert_eq!(img.alt_text.as_deref(), Some("from title"));
        assert_eq!(img.bytes.as_slice(), png);
    }
}
