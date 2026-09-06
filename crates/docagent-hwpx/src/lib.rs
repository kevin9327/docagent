//! HWPX (OWPML) ZIP+XML codec.
//!
//! Element names follow Hancom OWPML 2011 (`hp:p`, `hp:run`, `hp:t`, `hs:sec`,
//! `hp:tbl`). Writer emits a deterministic subset; reader accepts prefixed and
//! unprefixed names.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Color, Document, InlineObject, LayoutHint,
    NumberFormat, NumberingRef, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
    Underline,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const NS_SECTION: &str = "http://www.hancom.co.kr/hwpml/2011/section";
const NS_PARA: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";

#[derive(Debug, Error)]
pub enum Error {
    #[error("not an HWPX package")]
    NotHwpx,
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
    ZipArchive::new(Cursor::new(bytes))
        .ok()
        .map(|mut z| {
            z.by_name("Contents/content.hpf").is_ok()
                || z.by_name("version.xml").is_ok()
                || (0..z.len()).any(|i| {
                    z.by_index(i)
                        .map(|f| f.name().starts_with("Contents/section"))
                        .unwrap_or(false)
                })
        })
        .unwrap_or(false)
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if bytes.len() < 4 || bytes[0..2] != [0x50, 0x4B] {
        return Err(Error::NotHwpx);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec()))?;
    let (chars, named_styles) = if let Ok(mut f) = zip.by_name("Contents/header.xml") {
        let mut xml = String::new();
        f.read_to_string(&mut xml).ok();
        parse_header_meta(&xml)
    } else {
        (HashMap::new(), HashMap::new())
    };
    let mut sections = Vec::new();
    let names: Vec<String> = {
        let mut n = Vec::new();
        for i in 0..zip.len() {
            if let Ok(f) = zip.by_index(i) {
                n.push(f.name().to_string());
            }
        }
        n.sort();
        n
    };
    for name in names {
        if name.starts_with("Contents/section") && name.ends_with(".xml") {
            let mut f = zip.by_name(&name)?;
            let mut xml = String::new();
            f.read_to_string(&mut xml)?;
            sections.push(parse_section_xml(&xml, &chars, &named_styles)?);
        }
    }
    if sections.is_empty() {
        return Err(Error::NotHwpx);
    }
    Ok(Document {
        sections,
        ..Document::default()
    })
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let styles = collect_char_styles(doc);
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("mimetype", stored)?;
        zip.write_all(b"application/hwp+zip")?;
        zip.start_file("version.xml", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><ha:HCFVersion xmlns:ha="http://www.hancom.co.kr/hwpml/2011/application" targetApplication="DocAgent" major="1" minor="0" micro="0"/>"#,
        )?;
        zip.start_file("Contents/content.hpf", deflated)?;
        zip.write_all(content_hpf(doc.sections.len()).as_bytes())?;
        zip.start_file("Contents/header.xml", deflated)?;
        zip.write_all(header_xml(&styles).as_bytes())?;
        if doc.sections.is_empty() {
            zip.start_file("Contents/section0.xml", deflated)?;
            zip.write_all(section_xml(&Section::default(), &styles).as_bytes())?;
        } else {
            for (i, section) in doc.sections.iter().enumerate() {
                zip.start_file(format!("Contents/section{i}.xml"), deflated)?;
                zip.write_all(section_xml(section, &styles).as_bytes())?;
            }
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn content_hpf(n: usize) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><opf:package xmlns:opf="http://www.hancom.co.kr/hwpml/2011/opf"><opf:manifest>"#,
    );
    s.push_str(
        r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#,
    );
    let count = n.max(1);
    for i in 0..count {
        s.push_str(&format!(
            r#"<opf:item id="section{i}" href="Contents/section{i}.xml" media-type="application/xml"/>"#
        ));
    }
    s.push_str("</opf:manifest></opf:package>");
    s
}

fn header_xml(styles: &[CharStyle]) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"><hh:charProperties itemCnt="{}">"#,
        styles.len()
    );
    for (i, st) in styles.iter().enumerate() {
        s.push_str(&format!(
            r#"<hh:charPr id="{i}" height="{}"{}"#,
            st.size.max(1),
            shade_color_attr(st.highlight)
        ));
        s.push('>');
        if st.bold {
            s.push_str("<hh:bold/>");
        }
        if st.italic {
            s.push_str("<hh:italic/>");
        }
        if st.underline != Underline::None {
            s.push_str(r#"<hh:underline type="BOTTOM"/>"#);
        }
        if st.strike {
            s.push_str(r#"<hh:strikeout shape="SOLID"/>"#);
        }
        if st.superscript {
            s.push_str("<hh:supscript/>");
        }
        if st.subscript {
            s.push_str("<hh:subscript/>");
        }
        if let Some(c) = st.highlight {
            s.push_str(&format!(
                r##"<hh:shade color="#{:02X}{:02X}{:02X}"/>"##,
                c.r, c.g, c.b
            ));
        }
        s.push_str("</hh:charPr>");
    }
    s.push_str("</hh:charProperties>");
    s.push_str(
        r#"<hh:paraProperties itemCnt="5"><hh:paraPr id="0"><hh:border borderFillIDRef="0"/></hh:paraPr><hh:paraPr id="1"><hh:margin left="1400"/><hh:border borderFillIDRef="1" offsetLeft="400"/></hh:paraPr><hh:paraPr id="2"><hh:border borderFillIDRef="2"/></hh:paraPr><hh:paraPr id="3"><hh:border borderFillIDRef="3" offsetBottom="200"/></hh:paraPr><hh:paraPr id="4"><hh:border borderFillIDRef="0"/></hh:paraPr></hh:paraProperties>"#,
    );
    s.push_str(
        r#"<hh:styles itemCnt="5"><hh:style id="0" type="PARA" name="Normal" engName="Normal" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/><hh:style id="1" type="PARA" name="Quote" engName="Quote" paraPrIDRef="1" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/><hh:style id="2" type="PARA" name="CodeBlock" engName="CodeBlock" paraPrIDRef="2" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/><hh:style id="3" type="PARA" name="HorizontalLine" engName="HorizontalLine" paraPrIDRef="3" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/><hh:style id="4" type="PARA" name="Task" engName="Task" paraPrIDRef="4" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/></hh:styles></hh:head>"#,
    );
    s
}

fn shade_color_attr(highlight: Option<Color>) -> String {
    match highlight {
        Some(c) => format!(r##" shadeColor="#{:02X}{:02X}{:02X}""##, c.r, c.g, c.b),
        None => String::new(),
    }
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        return None;
    }
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    let r = ((n >> 16) & 0xFF) as u8;
    let g = ((n >> 8) & 0xFF) as u8;
    let b = (n & 0xFF) as u8;
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

fn section_xml(section: &Section, styles: &[CharStyle]) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><hs:sec xmlns:hs="{NS_SECTION}" xmlns:hp="{NS_PARA}" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">"#
    );
    s.push_str(&format!(
        r#"<hp:pagePr width="{}" height="{}" landscape="{}"><hp:margin left="{}" right="{}" top="{}" bottom="{}" header="{}" footer="{}" gutter="{}"/></hp:pagePr>"#,
        section.page.width,
        section.page.height,
        if section.page.landscape { 1 } else { 0 },
        section.page.margin_left,
        section.page.margin_right,
        section.page.margin_top,
        section.page.margin_bottom,
        section.page.header_distance,
        section.page.footer_distance,
        section.page.gutter
    ));
    if section.body.is_empty() {
        s.push_str(
            r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0"><hp:run><hp:t></hp:t></hp:run></hp:p>"#,
        );
    } else {
        for (i, block) in section.body.iter().enumerate() {
            match block {
                Block::Paragraph(p) => {
                    s.push_str(&para_xml(i, p, styles));
                }
                Block::Table(t) => s.push_str(&table_xml(t)),
                Block::Break(BreakKind::Thematic) => {
                    s.push_str(&format!(
                        r#"<hp:p id="{i}" paraPrIDRef="3" styleIDRef="3"><hp:run><hp:t></hp:t></hp:run></hp:p>"#
                    ));
                }
                Block::Float(_) | Block::Break(_) => {
                    s.push_str(&format!(
                        r#"<hp:p id="{i}" paraPrIDRef="0" styleIDRef="0"><hp:run><hp:t></hp:t></hp:run></hp:p>"#
                    ));
                }
            }
        }
    }
    s.push_str("</hs:sec>");
    s
}

fn para_style_id(p: &Paragraph) -> u32 {
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

fn para_xml(id: usize, p: &Paragraph, styles: &[CharStyle]) -> String {
    let sid = para_style_id(p);
    let mut s = format!(r#"<hp:p id="{id}" paraPrIDRef="{sid}" styleIDRef="{sid}">"#);
    let mut pending_marker = task_prefix(p);
    if p.runs.is_empty() {
        let inner = pending_marker.take().unwrap_or("");
        s.push_str(&format!(
            r#"<hp:run charPrIDRef="0"><hp:t>{}</hp:t></hp:run>"#,
            xml_escape(inner)
        ));
    } else {
        for (i, run) in p.runs.iter().enumerate() {
            let sid = style_id(styles, &run.style);
            match &run.content {
                RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                    s.push_str(&format!(
                        r#"<hp:run charPrIDRef="{sid}"><hp:fieldBegin id="{i}" type="HYPERLINK"><hp:parameters><hp:stringParam name="Command">{}</hp:stringParam></hp:parameters></hp:fieldBegin></hp:run><hp:run charPrIDRef="{sid}"><hp:t>{}</hp:t></hp:run><hp:run charPrIDRef="{sid}"><hp:fieldEnd/></hp:run>"#,
                        xml_escape(target),
                        xml_escape(display)
                    ));
                }
                RunContent::Text(t) => {
                    let body = match pending_marker.take() {
                        Some(m) => format!("{m}{t}"),
                        None => t.clone(),
                    };
                    s.push_str(&format!(
                        r#"<hp:run charPrIDRef="{sid}"><hp:t>{}</hp:t></hp:run>"#,
                        xml_escape(&body)
                    ));
                }
                RunContent::Inline(_) => {}
            }
        }
        if let Some(m) = pending_marker {
            s.push_str(&format!(
                r#"<hp:run charPrIDRef="0"><hp:t>{}</hp:t></hp:run>"#,
                xml_escape(m)
            ));
        }
    }
    for h in &p.layout_hints {
        s.push_str(&format!(
            r#"<hp:linesegarray><hp:lineseg textpos="{}" vertpos="{}" vertsize="{}" textheight="{}" baseline="{}" spacing="{}" horzpos="{}" horzsize="{}" flags="{}"/></hp:linesegarray>"#,
            h.text_start, h.y, h.line_height, h.text_height, h.baseline, h.spacing, h.x, h.width, h.flags
        ));
    }
    s.push_str("</hp:p>");
    s
}

fn table_header_row_count(table: &Table) -> u8 {
    table
        .header_row_count
        .max(table.rows.iter().take_while(|r| r.header).count() as u8)
}

fn table_xml(table: &Table) -> String {
    let rows = table.rows.len();
    let cols = table.rows.first().map(|r| r.cells.len()).unwrap_or(0);
    let header_n = table_header_row_count(table);
    let repeat = if header_n > 0 {
        r#" repeatHeader="1""#
    } else {
        ""
    };
    let mut s = format!(r#"<hp:tbl rowCnt="{rows}" colCnt="{cols}"{repeat}><hp:tbody>"#);
    for (ri, row) in table.rows.iter().enumerate() {
        let is_header = row.header || ri < header_n as usize;
        if is_header {
            s.push_str(r#"<hp:tr header="1">"#);
        } else {
            s.push_str("<hp:tr>");
        }
        for cell in &row.cells {
            let text = xml_escape(&cell_text(cell));
            let header_attr = if is_header { r#" header="1""# } else { "" };
            s.push_str(&format!(
                r#"<hp:tc{header_attr} width="{}"><hp:p><hp:run><hp:t>{text}</hp:t></hp:run></hp:p></hp:tc>"#,
                cell.width
            ));
        }
        s.push_str("</hp:tr>");
    }
    s.push_str("</hp:tbody></hp:tbl>");
    s
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn parse_header_meta(xml: &str) -> (HashMap<u32, CharStyle>, HashMap<u32, String>) {
    let chars = parse_header_chars(xml);
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut named = HashMap::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(&e) == "style"
                    && let Some(id) = attr_u32(&e, "id")
                {
                    let eng = attr_string(&e, "engName").unwrap_or_default();
                    let name = attr_string(&e, "name").unwrap_or_default();
                    named.insert(id, if eng.is_empty() { name } else { eng });
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    (chars, named)
}

fn parse_header_chars(xml: &str) -> HashMap<u32, CharStyle> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut out = HashMap::new();
    let mut cur_id: Option<u32> = None;
    let mut cur = CharStyle::default();
    let mut in_char_pr = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "charPr" => {
                        in_char_pr = true;
                        cur_id = attr_u32(&e, "id");
                        cur = CharStyle::default();
                        if let Some(h) = attr_i32(&e, "height").filter(|h| *h > 0) {
                            cur.size = h;
                        }
                        if let Some(c) = attr_string(&e, "shadeColor").and_then(|s| parse_hex_color(&s))
                        {
                            cur.highlight = Some(c);
                        }
                    }
                    "bold" if in_char_pr => cur.bold = true,
                    "italic" if in_char_pr => cur.italic = true,
                    "underline" if in_char_pr => {
                        let ty = attr_string(&e, "type").unwrap_or_default();
                        if !ty.is_empty() && !ty.eq_ignore_ascii_case("NONE") {
                            cur.underline = Underline::Single;
                        }
                    }
                    "strikeout" if in_char_pr => {
                        let shape = attr_string(&e, "shape").unwrap_or_default();
                        if !shape.eq_ignore_ascii_case("NONE") {
                            cur.strike = true;
                        }
                    }
                    "supscript" if in_char_pr => cur.superscript = true,
                    "subscript" if in_char_pr => cur.subscript = true,
                    "shade" if in_char_pr => {
                        if let Some(c) = attr_string(&e, "color").and_then(|s| parse_hex_color(&s)) {
                            cur.highlight = Some(c);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                if name == "charPr" {
                    if let Some(id) = cur_id.take() {
                        out.insert(id, cur.clone());
                    }
                    in_char_pr = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

fn parse_section_xml(
    xml: &str,
    chars: &HashMap<u32, CharStyle>,
    named_styles: &HashMap<u32, String>,
) -> Result<Section, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_t = false;
    let mut in_table = false;
    let mut in_run = false;
    let mut run_text = String::new();
    let mut run_style = CharStyle::default();
    let mut para_runs: Vec<Run> = Vec::new();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_text_buf = String::new();
    let mut cell_width: i32 = 10000;
    let mut cell_height: Option<i32> = None;
    let mut row_height: Option<i32> = None;
    let mut tbl_repeat_header = false;
    let mut row_header = false;
    let mut hints = Vec::new();
    let mut in_lineseg = false;
    let mut pending_href: Option<String> = None;
    let mut in_string_param = false;
    let mut string_param_name = String::new();
    let mut param_text = String::new();
    let mut para_kind: Option<StyleKind> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "t" => in_t = true,
                    "p" if !in_table => {
                        para_kind = attr_u32(&e, "styleIDRef")
                            .and_then(|id| named_styles.get(&id).cloned())
                            .as_deref()
                            .and_then(style_kind);
                    }
                    "run" if !in_table => {
                        in_run = true;
                        run_text.clear();
                        run_style = CharStyle::default();
                        if let Some(id) = attr_u32(&e, "charPrIDRef")
                            && let Some(st) = chars.get(&id)
                        {
                            run_style = st.clone();
                        }
                    }
                    "bold" if in_run => run_style.bold = true,
                    "italic" if in_run => run_style.italic = true,
                    "underline" if in_run => {
                        let ty = attr_string(&e, "type").unwrap_or_default();
                        if !ty.is_empty() && !ty.eq_ignore_ascii_case("NONE") {
                            run_style.underline = Underline::Single;
                        }
                    }
                    "strikeout" if in_run => {
                        let shape = attr_string(&e, "shape").unwrap_or_default();
                        if !shape.eq_ignore_ascii_case("NONE") {
                            run_style.strike = true;
                        }
                    }
                    "supscript" if in_run => run_style.superscript = true,
                    "subscript" if in_run => run_style.subscript = true,
                    "fieldBegin" => {
                        let ty = attr_string(&e, "type").unwrap_or_default();
                        if ty.eq_ignore_ascii_case("HYPERLINK") {
                            pending_href = Some(String::new());
                        }
                    }
                    "stringParam" => {
                        in_string_param = true;
                        string_param_name = attr_string(&e, "name").unwrap_or_default();
                        param_text.clear();
                    }
                    "fieldEnd" => pending_href = None,
                    "tbl" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        flush_para(&mut body, &mut para_runs, &mut hints, &mut para_kind);
                        in_table = true;
                        tbl_repeat_header = attr_on(&e, "repeatHeader");
                    }
                    "tr" if in_table => {
                        cur_row.clear();
                        row_height = attr_i32(&e, "height").filter(|h| *h > 0);
                        row_header = attr_on(&e, "header");
                    }
                    "tc" if in_table => {
                        cell_text_buf.clear();
                        cell_width = attr_i32(&e, "width").unwrap_or(10000);
                        cell_height = attr_i32(&e, "height").filter(|h| *h > 0 && *h < 20_000);
                        if attr_on(&e, "header") {
                            row_header = true;
                        }
                    }
                    "lineseg" => {
                        in_lineseg = true;
                        hints.push(LayoutHint {
                            text_start: attr_u32(&e, "textpos").unwrap_or(0),
                            y: attr_i32(&e, "vertpos").unwrap_or(0),
                            line_height: attr_i32(&e, "vertsize").unwrap_or(0),
                            text_height: attr_i32(&e, "textheight").unwrap_or(0),
                            baseline: attr_i32(&e, "baseline").unwrap_or(0),
                            spacing: attr_i32(&e, "spacing").unwrap_or(0),
                            x: attr_i32(&e, "horzpos").unwrap_or(0),
                            width: attr_i32(&e, "horzsize").unwrap_or(0),
                            flags: attr_u32(&e, "flags").unwrap_or(0),
                        });
                    }
                    "margin" => {
                        if let Some(v) = attr_i32(&e, "left") {
                            section.page.margin_left = v;
                        }
                        if let Some(v) = attr_i32(&e, "right") {
                            section.page.margin_right = v;
                        }
                        if let Some(v) = attr_i32(&e, "top") {
                            section.page.margin_top = v;
                        }
                        if let Some(v) = attr_i32(&e, "bottom") {
                            section.page.margin_bottom = v;
                        }
                    }
                    "pagePr" => {
                        if let Some(v) = attr_i32(&e, "width") {
                            section.page.width = v;
                        }
                        if let Some(v) = attr_i32(&e, "height") {
                            section.page.height = v;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "t" => in_t = false,
                    "stringParam" => {
                        if in_string_param
                            && string_param_name.eq_ignore_ascii_case("Command")
                            && pending_href.is_some()
                        {
                            pending_href = Some(std::mem::take(&mut param_text));
                        }
                        in_string_param = false;
                    }
                    "run" if !in_table => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        in_run = false;
                    }
                    "p" if !in_table => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        flush_para(&mut body, &mut para_runs, &mut hints, &mut para_kind);
                        pending_href = None;
                    }
                    "tc" if in_table => {
                        cur_row.push(TableCell {
                            width: cell_width,
                            blocks: vec![Block::Paragraph(Paragraph::from_text(
                                cell_text_buf.clone(),
                            ))],
                            ..TableCell::default()
                        });
                        cell_text_buf.clear();
                    }
                    "tr" if in_table => {
                        let h = row_height.or(cell_height);
                        table_rows.push(TableRow {
                            cells: std::mem::take(&mut cur_row),
                            height: h,
                            header: row_header,
                            cant_split: None,
                        });
                        row_height = None;
                        cell_height = None;
                        row_header = false;
                    }
                    "tbl" => {
                        in_table = false;
                        let mut rows = std::mem::take(&mut table_rows);
                        if tbl_repeat_header
                            && !rows.iter().any(|r| r.header)
                            && let Some(row) = rows.first_mut()
                        {
                            row.header = true;
                        }
                        let header_row_count =
                            rows.iter().take_while(|r| r.header).count() as u8;
                        body.push(Block::Table(Table {
                            rows,
                            alignment: Alignment::Start,
                            header_row_count,
                            ..Table::from_cells(Vec::new())
                        }));
                        tbl_repeat_header = false;
                    }
                    "lineseg" => in_lineseg = false,
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                if in_table {
                    cell_text_buf.push_str(&decoded);
                } else if in_string_param {
                    param_text.push_str(&decoded);
                } else if in_t && !in_lineseg {
                    run_text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    flush_run(
        &mut para_runs,
        &mut run_text,
        &run_style,
        pending_href.as_deref(),
    );
    flush_para(&mut body, &mut para_runs, &mut hints, &mut para_kind);
    section.body = body;
    Ok(section)
}

fn flush_run(runs: &mut Vec<Run>, text: &mut String, style: &CharStyle, href: Option<&str>) {
    if text.is_empty() {
        return;
    }
    let display = std::mem::take(text);
    let mut run = if let Some(url) = href.filter(|u| !u.is_empty()) {
        Run::hyperlink(display, url)
    } else {
        Run::text(display)
    };
    run.style = style.clone();
    if href.filter(|u| !u.is_empty()).is_some() {
        run.style.underline = Underline::Single;
    }
    runs.push(run);
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

fn flush_para(
    body: &mut Vec<Block>,
    runs: &mut Vec<Run>,
    hints: &mut Vec<LayoutHint>,
    kind: &mut Option<StyleKind>,
) {
    let kind = kind.take();
    if kind == Some(StyleKind::Thematic) {
        runs.clear();
        hints.clear();
        body.push(Block::Break(BreakKind::Thematic));
        return;
    }
    if runs.is_empty() && hints.is_empty() && kind.is_none() {
        return;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
    p.layout_hints = std::mem::take(hints);
    p.quote = kind == Some(StyleKind::Quote);
    p.code_block = kind == Some(StyleKind::CodeBlock);
    apply_task_paragraph(&mut p, kind == Some(StyleKind::Task));
    body.push(Block::Paragraph(p));
}

fn local_name(e: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn attr_i32(e: &BytesStart<'_>, key: &str) -> Option<i32> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
}

fn attr_u32(e: &BytesStart<'_>, key: &str) -> Option<u32> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .and_then(|a| String::from_utf8_lossy(&a.value).parse().ok())
}

fn attr_string(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn attr_on(e: &BytesStart<'_>, key: &str) -> bool {
    match attr_string(e, key).as_deref() {
        Some(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on")
        }
        None => false,
    }
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_model::Block;

    #[test]
    fn roundtrip_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("HWPX 안녕")));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains("HWPX 안녕"));
    }

    #[test]
    fn roundtrip_keeps_char_pr_marks() {
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
        let xml = header_xml(&collect_char_styles(&doc));
        assert!(xml.contains("<hh:bold/>"), "{xml}");
        assert!(xml.contains("strikeout"), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, RunContent::Text(t) if t.contains("guess"))
        }));
    }

    #[test]
    fn roundtrip_keeps_highlight() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hashes");
        p.runs[0].style.highlight = Some(Color::MARK);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = header_xml(&collect_char_styles(&doc));
        assert!(xml.contains(r##"shadeColor="#FDE68A""##), "{xml}");
        assert!(xml.contains(r##"<hh:shade color="#FDE68A"/>"##), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.highlight.is_some()
                && matches!(&r.content, RunContent::Text(t) if t == "hashes")
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
        let xml = para_xml(0, &p, &[CharStyle::default()]);
        assert!(xml.contains(r#"type="HYPERLINK""#), "{xml}");
        assert!(xml.contains("stringParam"), "{xml}");
        assert!(
            xml.contains("https://github.com/kevin9327/docagent"),
            "{xml}"
        );
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
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
    fn shipped_read_parses_hangul_hwpx_when_present() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir)
            .join("samples")
            .join("hwp3-sample-hwpx.hwpx");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let doc = read(&bytes).expect("parse HWPX");
        assert!(
            !doc.plain_text().trim().is_empty(),
            "empty HWPX body diagnostics={:?}",
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
        assert!(hints > 0, "hp:lineseg must become LayoutHint");
    }

    #[test]
    fn roundtrip_table() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section.body.push(Block::Table(Table::from_cells(vec![vec![
            "x".into(),
            "y".into(),
        ]])));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains('x'));
        assert_eq!(back.table_count(), 1);
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
        let xml = table_xml(&table);
        assert!(xml.contains(r#"repeatHeader="1""#), "{xml}");
        assert!(xml.contains(r#"header="1""#), "{xml}");
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        let Block::Table(t) = &back.sections[0].body[0] else {
            panic!("table");
        };
        assert!(t.rows[0].header);
        assert!(!t.rows[1].header);
        assert_eq!(t.header_row_count, 1);
    }

    #[test]
    fn roundtrip_keeps_quote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = para_xml(
            0,
            &Paragraph {
                quote: true,
                ..Paragraph::from_text("Replay is evidence.")
            },
            &[CharStyle::default()],
        );
        assert!(xml.contains(r#"styleIDRef="1""#), "{xml}");
        let header = header_xml(&collect_char_styles(&doc));
        assert!(header.contains(r#"engName="Quote""#), "{header}");
        let back = roundtrip(&doc).unwrap();
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
        let xml = para_xml(
            0,
            &Paragraph {
                code_block: true,
                ..Paragraph::from_text("let n = 1;")
            },
            &[CharStyle::default()],
        );
        assert!(xml.contains(r#"styleIDRef="2""#), "{xml}");
        let header = header_xml(&collect_char_styles(&doc));
        assert!(header.contains(r#"engName="CodeBlock""#), "{header}");
        let back = roundtrip(&doc).unwrap();
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
        let header = header_xml(&collect_char_styles(&doc));
        assert!(header.contains(r#"engName="HorizontalLine""#), "{header}");
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
        let xml = para_xml(0, &checked, &[CharStyle::default()]);
        assert!(xml.contains(r#"styleIDRef="4""#), "{xml}");
        assert!(xml.contains("[x] "), "{xml}");
        section.body.push(Block::Paragraph(checked));
        section.body.push(Block::Paragraph(unchecked));
        doc.sections.push(section);
        let header = header_xml(&collect_char_styles(&doc));
        assert!(header.contains(r#"engName="Task""#), "{header}");
        let back = roundtrip(&doc).unwrap();
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
