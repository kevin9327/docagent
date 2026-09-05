//! OpenDocument Text (ODF) codec. Reads and writes `content.xml` paragraphs
//! and tables. Lengths stay in the IR as HWPUNIT.

#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};

use docagent_model::{Alignment, Block, Document, Paragraph, Section, Table, TableCell, TableRow};
use quick_xml::events::Event;
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
    let mut buf = Vec::new();
    let mut doc = Document::new();
    let mut section = Section::default();
    let mut in_p = false;
    let mut in_table = false;
    let mut text = String::new();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "p" | "h" => {
                        in_p = true;
                        text.clear();
                    }
                    "table" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "table-row" if in_table => cur_row.clear(),
                    "table-cell" if in_table => cell_text.clear(),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "p" | "h" => {
                        in_p = false;
                        if !in_table && !text.trim().is_empty() {
                            section
                                .body
                                .push(Block::Paragraph(Paragraph::from_text(text.clone())));
                        }
                        text.clear();
                    }
                    "table-cell" if in_table => {
                        cur_row.push(TableCell {
                            width: 10000,
                            blocks: vec![Block::Paragraph(Paragraph::from_text(cell_text.clone()))],
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
                let decoded = t.unescape().map_err(|e| Error::Xml(e.to_string()))?;
                if in_table {
                    cell_text.push_str(&decoded);
                } else if in_p {
                    text.push_str(&decoded);
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

fn content_xml(doc: &Document) -> String {
    let mut body = String::new();
    for section in &doc.sections {
        for block in &section.body {
            match block {
                Block::Paragraph(p) => {
                    body.push_str(&format!(
                        "<text:p>{}</text:p>",
                        xml_escape(&p.plain_text())
                    ));
                }
                Block::Table(table) => {
                    body.push_str("<table:table>");
                    for row in &table.rows {
                        body.push_str("<table:table-row>");
                        for cell in &row.cells {
                            let t: String = cell
                                .blocks
                                .iter()
                                .filter_map(|b| match b {
                                    Block::Paragraph(p) => Some(p.plain_text()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join(" ");
                            body.push_str(&format!(
                                "<table:table-cell><text:p>{}</text:p></table:table-cell>",
                                xml_escape(&t)
                            ));
                        }
                        body.push_str("</table:table-row>");
                    }
                    body.push_str("</table:table>");
                }
                Block::Float(_) | Block::Break(_) => {}
            }
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
    )
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
