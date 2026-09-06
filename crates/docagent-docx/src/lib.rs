//! DOCX WordprocessingML codec (ECMA-376).
//!
//! Paragraph/run/table map onto `w:p` / `w:r` / `w:t` / `w:tbl`. Twips in the
//! package are converted with `twips_to_hu` / `hu_to_twips` (factor 5).

#![forbid(unsafe_code)]

use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, Diagnostic, DiagnosticCode, Document, Paragraph, Run, RunContent, Section,
    Severity, Table, TableCell, TableRow, hu_to_twips, twips_to_hu,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a DOCX package")]
    NotDocx,
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
        .map(|mut z| z.by_name("word/document.xml").is_ok())
        .unwrap_or(false)
}

pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    if bytes.len() < 4 || bytes[0..2] != [0x50, 0x4B] {
        return Err(Error::NotDocx);
    }
    let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec()))?;
    let mut xml = String::new();
    {
        let mut f = zip
            .by_name("word/document.xml")
            .map_err(|_| Error::NotDocx)?;
        f.read_to_string(&mut xml)?;
    }
    parse_document_xml(&xml)
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        )?;
        zip.start_file("_rels/.rels", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        )?;
        zip.start_file("word/_rels/document.xml.rels", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
        )?;
        zip.start_file("word/document.xml", deflated)?;
        zip.write_all(document_xml(doc).as_bytes())?;
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn document_xml(doc: &Document) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    let sections = if doc.sections.is_empty() {
        vec![Section::default()]
    } else {
        doc.sections.clone()
    };
    for (si, section) in sections.iter().enumerate() {
        if section.body.is_empty() {
            s.push_str("<w:p><w:r><w:t/></w:r></w:p>");
        } else {
            for block in &section.body {
                match block {
                    Block::Paragraph(p) => s.push_str(&p_xml(p)),
                    Block::Table(t) => s.push_str(&tbl_xml(t)),
                    Block::Float(_) | Block::Break(_) => s.push_str("<w:p/>"),
                }
            }
        }
        s.push_str("<w:sectPr>");
        s.push_str(&format!(
            r#"<w:pgSz w:w="{}" w:h="{}" w:orient="{}"/>"#,
            hu_to_twips(section.page.width),
            hu_to_twips(section.page.height),
            if section.page.landscape {
                "landscape"
            } else {
                "portrait"
            }
        ));
        s.push_str(&format!(
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="{}"/>"#,
            hu_to_twips(section.page.margin_top),
            hu_to_twips(section.page.margin_right),
            hu_to_twips(section.page.margin_bottom),
            hu_to_twips(section.page.margin_left),
            hu_to_twips(section.page.header_distance),
            hu_to_twips(section.page.footer_distance),
            hu_to_twips(section.page.gutter)
        ));
        s.push_str("</w:sectPr>");
        let _ = si;
    }
    s.push_str("</w:body></w:document>");
    s
}

fn p_xml(p: &Paragraph) -> String {
    let mut s = String::from("<w:p>");
    let mut wrote = false;
    for run in &p.runs {
        let RunContent::Text(t) = &run.content else {
            continue;
        };
        if t.is_empty() {
            continue;
        }
        s.push_str("<w:r>");
        if run.style.bold || run.style.italic {
            s.push_str("<w:rPr>");
            if run.style.bold {
                s.push_str("<w:b/>");
            }
            if run.style.italic {
                s.push_str("<w:i/>");
            }
            s.push_str("</w:rPr>");
        }
        s.push_str("<w:t xml:space=\"preserve\">");
        s.push_str(&xml_escape(t));
        s.push_str("</w:t></w:r>");
        wrote = true;
    }
    if !wrote {
        s.push_str("<w:r><w:t/></w:r>");
    }
    s.push_str("</w:p>");
    s
}

fn tbl_xml(table: &Table) -> String {
    let mut s = String::from("<w:tbl><w:tblPr/><w:tblGrid>");
    if let Some(row) = table.rows.first() {
        for cell in &row.cells {
            s.push_str(&format!(
                r#"<w:gridCol w:w="{}"/>"#,
                hu_to_twips(cell.width)
            ));
        }
    }
    s.push_str("</w:tblGrid>");
    for row in &table.rows {
        s.push_str("<w:tr>");
        for cell in &row.cells {
            s.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr><w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:tc>"#,
                hu_to_twips(cell.width),
                xml_escape(&cell_text(cell))
            ));
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
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

fn parse_document_xml(xml: &str) -> Result<Document, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_t = false;
    let mut in_rpr = false;
    let mut in_tbl = false;
    let mut run_bold = false;
    let mut run_italic = false;
    let mut run_text = String::new();
    let mut para_runs: Vec<Run> = Vec::new();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_buf = String::new();
    let mut cell_width = 10000i32;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "t" => in_t = true,
                    "rPr" if !in_tbl => in_rpr = true,
                    "b" if in_rpr => run_bold = ooxml_on(&e),
                    "i" if in_rpr => run_italic = ooxml_on(&e),
                    "r" if !in_tbl => {
                        run_bold = false;
                        run_italic = false;
                        run_text.clear();
                    }
                    "tbl" => {
                        flush_para(&mut body, &mut para_runs);
                        in_tbl = true;
                    }
                    "tr" if in_tbl => cur_row.clear(),
                    "tc" if in_tbl => {
                        cell_buf.clear();
                        cell_width = 10000;
                    }
                    "tcW" => {
                        if let Some(w) = attr(&e, "w")
                            && let Ok(tw) = w.parse::<i32>()
                        {
                            cell_width = twips_to_hu(tw);
                        }
                    }
                    "pgSz" => {
                        if let Some(w) = attr(&e, "w").and_then(|v| v.parse().ok()) {
                            section.page.width = twips_to_hu(w);
                        }
                        if let Some(h) = attr(&e, "h").and_then(|v| v.parse().ok()) {
                            section.page.height = twips_to_hu(h);
                        }
                    }
                    "pgMar" => {
                        if let Some(v) = attr(&e, "left").and_then(|v| v.parse().ok()) {
                            section.page.margin_left = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "right").and_then(|v| v.parse().ok()) {
                            section.page.margin_right = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "top").and_then(|v| v.parse().ok()) {
                            section.page.margin_top = twips_to_hu(v);
                        }
                        if let Some(v) = attr(&e, "bottom").and_then(|v| v.parse().ok()) {
                            section.page.margin_bottom = twips_to_hu(v);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                match name.as_str() {
                    "t" => in_t = false,
                    "rPr" => in_rpr = false,
                    "r" if !in_tbl => {
                        flush_run(&mut para_runs, &mut run_text, run_bold, run_italic);
                        run_bold = false;
                        run_italic = false;
                    }
                    "p" if !in_tbl => {
                        flush_run(&mut para_runs, &mut run_text, run_bold, run_italic);
                        flush_para(&mut body, &mut para_runs);
                    }
                    "tc" if in_tbl => {
                        cur_row.push(TableCell {
                            width: cell_width,
                            blocks: vec![Block::Paragraph(Paragraph::from_text(cell_buf.clone()))],
                            ..TableCell::default()
                        });
                        cell_buf.clear();
                    }
                    "tr" if in_tbl => table_rows.push(TableRow {
                        cells: std::mem::take(&mut cur_row),
                        height: None,
                        header: false,
                        cant_split: None,
                    }),
                    "tbl" => {
                        in_tbl = false;
                        body.push(Block::Table(Table {
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
                if in_tbl {
                    cell_buf.push_str(&decoded);
                } else if in_t {
                    run_text.push_str(&decoded);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }
    flush_run(&mut para_runs, &mut run_text, run_bold, run_italic);
    flush_para(&mut body, &mut para_runs);
    section.body = body;
    Ok(Document {
        sections: vec![section],
        ..Document::default()
    })
}

fn flush_run(runs: &mut Vec<Run>, text: &mut String, bold: bool, italic: bool) {
    if text.is_empty() {
        return;
    }
    let mut run = Run::text(std::mem::take(text));
    run.style.bold = bold;
    run.style.italic = italic;
    runs.push(run);
}

fn flush_para(body: &mut Vec<Block>, runs: &mut Vec<Run>) {
    if runs.is_empty() {
        return;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
    body.push(Block::Paragraph(p));
}

fn ooxml_on(e: &BytesStart<'_>) -> bool {
    match attr(e, "val").as_deref() {
        Some("0") | Some("false") | Some("off") => false,
        _ => true,
    }
}

fn local_name(e: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.try_get_attribute(key)
        .ok()
        .flatten()
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        .or_else(|| {
            e.try_get_attribute(format!("w:{key}"))
                .ok()
                .flatten()
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        })
}

/// Structured loss list for HWP ↔ DOCX. Both sides of a missing concept are
/// explicit `Option` fields on the IR; this function reports what the other
/// format cannot represent losslessly.
pub fn loss_diagnostics(from_hwp: &Document, from_docx: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if from_hwp.plain_text() != from_docx.plain_text() {
        out.push(Diagnostic::roundtrip_loss(format!(
            "plain text diverged (hwp {} chars, docx {} chars)",
            from_hwp.plain_text().chars().count(),
            from_docx.plain_text().chars().count()
        )));
    }
    let hwp_hints: usize = from_hwp
        .sections
        .iter()
        .flat_map(|s| s.body.iter())
        .map(|b| match b {
            Block::Paragraph(p) => p.layout_hints.len(),
            _ => 0,
        })
        .sum();
    if hwp_hints > 0 {
        out.push(Diagnostic {
            severity: Severity::Info,
            code: DiagnosticCode::RoundtripLoss,
            message: format!(
                "LineSeg layout hints ({hwp_hints}) have no OOXML equivalent and are dropped on DOCX export"
            ),
            location: Some("paragraph.layout_hints".into()),
        });
    }
    for (i, section) in from_hwp.sections.iter().enumerate() {
        if section.background.is_some() {
            out.push(Diagnostic {
                severity: Severity::Warning,
                code: DiagnosticCode::RoundtripLoss,
                message: format!("section {i} background/바탕쪽 is not represented in DOCX export"),
                location: Some(format!("sections[{i}].background")),
            });
        }
        if !section.endnotes.is_empty() {
            out.push(Diagnostic {
                severity: Severity::Info,
                code: DiagnosticCode::RoundtripLoss,
                message: format!(
                    "section {i} endnotes ({}) require w:endnote part not emitted in v1 writer",
                    section.endnotes.len()
                ),
                location: Some(format!("sections[{i}].endnotes")),
            });
        }
    }
    if from_hwp.table_count() != from_docx.table_count() {
        out.push(Diagnostic::roundtrip_loss(format!(
            "table count hwp={} docx={}",
            from_hwp.table_count(),
            from_docx.table_count()
        )));
    }
    out
}

pub fn hwp_to_docx_with_loss(hwp_ir: &Document) -> Result<(Vec<u8>, Vec<Diagnostic>), Error> {
    let bytes = write(hwp_ir)?;
    let back = read(&bytes)?;
    Ok((bytes, loss_diagnostics(hwp_ir, &back)))
}

pub fn roundtrip(doc: &Document) -> Result<Document, Error> {
    read(&write(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docagent_model::{LayoutHint, Paragraph, Run};

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
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:b/>"), "docx must emit w:b");
        assert!(xml.contains("<w:i/>"), "docx must emit w:i");
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
    fn roundtrip_text_and_page() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("DOCX hello")));
        doc.sections.push(section);
        let back = roundtrip(&doc).unwrap();
        assert!(back.plain_text().contains("DOCX hello"));
        let delta = (back.sections[0].page.width - doc.sections[0].page.width).abs();
        assert!(
            delta < docagent_model::HU_PER_TWIP,
            "page width quantized beyond one twip: {delta}"
        );
    }

    #[test]
    fn loss_diagnostics_report_lineseg() {
        let mut hwp = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("hinted");
        p.layout_hints.push(LayoutHint {
            text_start: 0,
            x: 0,
            y: 10,
            width: 100,
            line_height: 20,
            text_height: 10,
            baseline: 8,
            spacing: 4,
            flags: 0,
        });
        section.body.push(Block::Paragraph(p));
        hwp.sections.push(section);
        let (_bytes, loss) = hwp_to_docx_with_loss(&hwp).unwrap();
        assert!(loss.iter().any(|d| d.message.contains("LineSeg")));
    }
}
