//! HML / HWPML XML codec.

#![forbid(unsafe_code)]

use docagent_model::{
    Alignment, Block, Document, Paragraph, Section, Table, TableCell, TableRow,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not HML/HWPML")]
    NotHml,
    #[error("xml: {0}")]
    Xml(String),
}

pub fn sniff(bytes: &[u8]) -> bool {
    let s = String::from_utf8_lossy(bytes);
    let t = s.trim_start();
    t.contains("<HWPML") || t.contains("<hml") || t.contains("<HML")
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    let xml = std::str::from_utf8(bytes).map_err(|e| Error::Xml(e.to_string()))?;
    if !sniff(bytes) && !xml.contains("<SECTION") && !xml.contains("<P") {
        return Err(Error::NotHml);
    }
    parse_hml(xml)
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut s = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><HWPML><BODY>"#);
    if doc.sections.is_empty() {
        s.push_str("<SECTION><P><TEXT></TEXT></P></SECTION>");
    } else {
        for section in &doc.sections {
            s.push_str(&format!(
                r#"<SECTION PageWidth="{}" PageHeight="{}" MarginLeft="{}" MarginRight="{}" MarginTop="{}" MarginBottom="{}">"#,
                section.page.width,
                section.page.height,
                section.page.margin_left,
                section.page.margin_right,
                section.page.margin_top,
                section.page.margin_bottom
            ));
            if section.body.is_empty() {
                s.push_str("<P><TEXT></TEXT></P>");
            } else {
                for block in &section.body {
                    match block {
                        Block::Paragraph(p) => {
                            s.push_str(&format!(
                                "<P><TEXT>{}</TEXT></P>",
                                xml_escape(&p.plain_text())
                            ));
                        }
                        Block::Table(t) => s.push_str(&table_xml(t)),
                        Block::Float(_) | Block::Break(_) => s.push_str("<P><TEXT></TEXT></P>"),
                    }
                }
            }
            s.push_str("</SECTION>");
        }
    }
    s.push_str("</BODY></HWPML>");
    Ok(s.into_bytes())
}

fn table_xml(table: &Table) -> String {
    let mut s = String::from("<TABLE>");
    for row in &table.rows {
        s.push_str("<TR>");
        for cell in &row.cells {
            s.push_str(&format!(
                r#"<TD Width="{}"><P><TEXT>{}</TEXT></P></TD>"#,
                cell.width,
                xml_escape(&cell_text(cell))
            ));
        }
        s.push_str("</TR>");
    }
    s.push_str("</TABLE>");
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
}

fn parse_hml(xml: &str) -> Result<Document, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut doc = Document::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_text = false;
    let mut in_table = false;
    let mut cur = String::new();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_buf = String::new();
    let mut cell_width = 10000i32;
    let mut have_section = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "SECTION" | "Section" => {
                        if have_section {
                            section.body = std::mem::take(&mut body);
                            doc.sections.push(std::mem::take(&mut section));
                            section = Section::default();
                        }
                        have_section = true;
                        if let Some(v) = attr_i32(&e, "PageWidth") {
                            section.page.width = v;
                        }
                        if let Some(v) = attr_i32(&e, "PageHeight") {
                            section.page.height = v;
                        }
                    }
                    "TEXT" | "CHAR" => in_text = true,
                    "TABLE" => {
                        flush_p(&mut body, &mut cur);
                        in_table = true;
                    }
                    "TR" if in_table => cur_row.clear(),
                    "TD" if in_table => {
                        cell_buf.clear();
                        cell_width = attr_i32(&e, "Width").unwrap_or(10000);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "TEXT" | "CHAR" => in_text = false,
                    "P" if !in_table => flush_p(&mut body, &mut cur),
                    "TD" if in_table => {
                        cur_row.push(TableCell {
                            width: cell_width,
                            blocks: vec![Block::Paragraph(Paragraph::from_text(cell_buf.clone()))],
                            ..TableCell::default()
                        });
                        cell_buf.clear();
                    }
                    "TR" if in_table => table_rows.push(TableRow {
                        cells: std::mem::take(&mut cur_row),
                        height: None,
                        header: false,
                        cant_split: None,
                    }),
                    "TABLE" => {
                        in_table = false;
                        body.push(Block::Table(Table {
                            rows: std::mem::take(&mut table_rows),
                            alignment: Alignment::Start,
                            ..Table::from_cells(Vec::new())
                        }));
                    }
                    "SECTION" | "Section" => {
                        section.body = std::mem::take(&mut body);
                        doc.sections.push(std::mem::take(&mut section));
                        section = Section::default();
                        have_section = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                if in_table {
                    cell_buf.push_str(&decoded);
                } else if in_text {
                    cur.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    flush_p(&mut body, &mut cur);
    if have_section || !body.is_empty() {
        section.body = body;
        doc.sections.push(section);
    }
    if doc.sections.is_empty() {
        doc.sections.push(Section::default());
    }
    Ok(doc)
}

fn flush_p(body: &mut Vec<Block>, text: &mut String) {
    if text.is_empty() {
        return;
    }
    body.push(Block::Paragraph(Paragraph::from_text(std::mem::take(text))));
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

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("HML 본문")));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains("HML 본문"));
    }
}
