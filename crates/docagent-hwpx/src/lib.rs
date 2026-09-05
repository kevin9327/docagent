//! HWPX (OWPML) ZIP+XML codec.
//!
//! Element names follow Hancom OWPML 2011 (`hp:p`, `hp:run`, `hp:t`, `hs:sec`,
//! `hp:tbl`). Writer emits a deterministic subset; reader accepts prefixed and
//! unprefixed names.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, Document, LayoutHint, Paragraph, Section, Table, TableCell, TableRow,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
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
            sections.push(parse_section_xml(&xml)?);
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
        zip.write_all(header_xml().as_bytes())?;
        if doc.sections.is_empty() {
            zip.start_file("Contents/section0.xml", deflated)?;
            zip.write_all(section_xml(&Section::default()).as_bytes())?;
        } else {
            for (i, section) in doc.sections.iter().enumerate() {
                zip.start_file(format!("Contents/section{i}.xml"), deflated)?;
                zip.write_all(section_xml(section).as_bytes())?;
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
    s.push_str(r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#);
    let count = n.max(1);
    for i in 0..count {
        s.push_str(&format!(
            r#"<opf:item id="section{i}" href="Contents/section{i}.xml" media-type="application/xml"/>"#
        ));
    }
    s.push_str("</opf:manifest></opf:package>");
    s
}

fn header_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?><hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head"/>"#.into()
}

fn section_xml(section: &Section) -> String {
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
        s.push_str(r#"<hp:p id="0"><hp:run><hp:t></hp:t></hp:run></hp:p>"#);
    } else {
        for (i, block) in section.body.iter().enumerate() {
            match block {
                Block::Paragraph(p) => {
                    s.push_str(&para_xml(i, p));
                }
                Block::Table(t) => s.push_str(&table_xml(t)),
                Block::Float(_) | Block::Break(_) => {
                    s.push_str(&format!(r#"<hp:p id="{i}"><hp:run><hp:t></hp:t></hp:run></hp:p>"#));
                }
            }
        }
    }
    s.push_str("</hs:sec>");
    s
}

fn para_xml(id: usize, p: &Paragraph) -> String {
    let text = xml_escape(&p.plain_text());
    let mut s = format!(r#"<hp:p id="{id}"><hp:run><hp:t>{text}</hp:t></hp:run>"#);
    for h in &p.layout_hints {
        s.push_str(&format!(
            r#"<hp:linesegarray><hp:lineseg textpos="{}" vertpos="{}" vertsize="{}" textheight="{}" baseline="{}" spacing="{}" horzpos="{}" horzsize="{}" flags="{}"/></hp:linesegarray>"#,
            h.text_start, h.y, h.line_height, h.text_height, h.baseline, h.spacing, h.x, h.width, h.flags
        ));
    }
    s.push_str("</hp:p>");
    s
}

fn table_xml(table: &Table) -> String {
    let rows = table.rows.len();
    let cols = table.rows.first().map(|r| r.cells.len()).unwrap_or(0);
    let mut s = format!(r#"<hp:tbl rowCnt="{rows}" colCnt="{cols}"><hp:tbody>"#);
    for row in &table.rows {
        s.push_str("<hp:tr>");
        for cell in &row.cells {
            let text = xml_escape(&cell_text(cell));
            s.push_str(&format!(
                r#"<hp:tc width="{}"><hp:p><hp:run><hp:t>{text}</hp:t></hp:run></hp:p></hp:tc>"#,
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

fn parse_section_xml(xml: &str) -> Result<Section, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_t = false;
    let mut in_table = false;
    let mut cur_text = String::new();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_text_buf = String::new();
    let mut cell_width: i32 = 10000;
    let mut cell_height: Option<i32> = None;
    let mut row_height: Option<i32> = None;
    let mut hints = Vec::new();
    let mut in_lineseg = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "t" => in_t = true,
                    "tbl" => {
                        flush_para(&mut body, &mut cur_text, &mut hints);
                        in_table = true;
                    }
                    "tr" if in_table => {
                        cur_row.clear();
                        row_height = attr_i32(&e, "height").filter(|h| *h > 0);
                    }
                    "tc" if in_table => {
                        cell_text_buf.clear();
                        cell_width = attr_i32(&e, "width").unwrap_or(10000);
                        cell_height = attr_i32(&e, "height").filter(|h| *h > 0 && *h < 20_000);
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
                    "p" if !in_table => {
                        flush_para(&mut body, &mut cur_text, &mut hints);
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
                            header: false,
                            cant_split: None,
                        });
                        row_height = None;
                        cell_height = None;
                    }
                    "tbl" => {
                        in_table = false;
                        body.push(Block::Table(Table {
                            rows: std::mem::take(&mut table_rows),
                            alignment: Alignment::Start,
                            ..Table::from_cells(Vec::new())
                        }));
                    }
                    "lineseg" => in_lineseg = false,
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                if in_table {
                    cell_text_buf.push_str(&decoded);
                } else if in_t && !in_lineseg {
                    cur_text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    flush_para(&mut body, &mut cur_text, &mut hints);
    section.body = body;
    Ok(section)
}

fn flush_para(body: &mut Vec<Block>, text: &mut String, hints: &mut Vec<LayoutHint>) {
    if text.is_empty() && hints.is_empty() {
        return;
    }
    let mut p = Paragraph::from_text(std::mem::take(text));
    p.layout_hints = std::mem::take(hints);
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
    fn shipped_read_parses_hangul_hwpx_when_present() {
        let dir = std::env::var("RHWP_DIR").unwrap_or_else(|_| r"C:\Users\swsz9\rhwp".into());
        let path = std::path::PathBuf::from(dir).join("samples").join("hwp3-sample-hwpx.hwpx");
        let bytes = std::fs::read(&path).expect("sibling HWPX sample");
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
        section
            .body
            .push(Block::Table(Table::from_cells(vec![vec!["x".into(), "y".into()]])));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains('x'));
        assert_eq!(back.table_count(), 1);
    }
}
