//! DOCX WordprocessingML codec (ECMA-376).
//!
//! Paragraph/run/table map onto `w:p` / `w:r` / `w:t` / `w:tbl`. Twips in the
//! package are converted with `twips_to_hu` / `hu_to_twips` (factor 5).

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use docagent_model::{
    Alignment, Block, Diagnostic, DiagnosticCode, Document, InlineObject, NumberFormat,
    NumberingRef, Paragraph, Run, RunContent, Section, Severity, Table, TableCell, TableRow,
    DEFAULT_FONT_SIZE_HU, hu_to_twips, twips_to_hu,
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
    let mut rels = HashMap::new();
    if let Ok(mut f) = zip.by_name("word/_rels/document.xml.rels") {
        let mut rels_xml = String::new();
        f.read_to_string(&mut rels_xml)?;
        rels = parse_rels(&rels_xml);
    }
    let mut xml = String::new();
    {
        let mut f = zip
            .by_name("word/document.xml")
            .map_err(|_| Error::NotDocx)?;
        f.read_to_string(&mut xml)?;
    }
    parse_document_xml(&xml, &rels)
}

fn parse_rels(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                if local_name(&e) == "Relationship" {
                    let id = attr(&e, "Id").or_else(|| attr(&e, "id"));
                    let target = attr(&e, "Target").or_else(|| attr(&e, "target"));
                    let ty = attr(&e, "Type").or_else(|| attr(&e, "type")).unwrap_or_default();
                    if ty.contains("hyperlink")
                        && let (Some(id), Some(target)) = (id, target)
                    {
                        map.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("[Content_Types].xml", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        )?;
        zip.start_file("_rels/.rels", deflated)?;
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        )?;
        zip.start_file("word/_rels/document.xml.rels", deflated)?;
        zip.write_all(document_rels(doc).as_bytes())?;
        zip.start_file("word/styles.xml", deflated)?;
        zip.write_all(STYLES_XML.as_bytes())?;
        zip.start_file("word/numbering.xml", deflated)?;
        zip.write_all(NUMBERING_XML.as_bytes())?;
        zip.start_file("word/document.xml", deflated)?;
        zip.write_all(document_xml(doc).as_bytes())?;
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn document_rels(doc: &Document) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
    );
    let mut i = 3u32;
    for target in hyperlink_targets(doc) {
        s.push_str(&format!(
            r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
            xml_escape(&target)
        ));
        i += 1;
    }
    s.push_str("</Relationships>");
    s
}

fn hyperlink_targets(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for section in &doc.sections {
        collect_hyperlinks(&section.body, &mut out);
    }
    out
}

fn collect_hyperlinks(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for run in &p.runs {
                    if let RunContent::Inline(InlineObject::Hyperlink { target, .. }) = &run.content {
                        out.push(target.clone());
                    }
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        collect_hyperlinks(&cell.blocks, out);
                    }
                }
            }
            Block::Float(_) | Block::Break(_) => {}
        }
    }
}

fn document_xml(doc: &Document) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>"#,
    );
    let sections = if doc.sections.is_empty() {
        vec![Section::default()]
    } else {
        doc.sections.clone()
    };
    let mut link_i = 0u32;
    for (si, section) in sections.iter().enumerate() {
        if section.body.is_empty() {
            s.push_str("<w:p><w:r><w:t/></w:r></w:p>");
        } else {
            for block in &section.body {
                match block {
                    Block::Paragraph(p) => s.push_str(&p_xml(p, &mut link_i)),
                    Block::Table(t) => s.push_str(&tbl_xml(t, &mut link_i)),
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

const STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/><w:szCs w:val="36"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style></w:styles>"#;

fn hu_to_half_points(hu: i32) -> i32 {
    (hu / 50).max(1)
}

fn half_points_to_hu(hp: i32) -> i32 {
    hp.saturating_mul(50)
}

const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%3."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="2160" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num></w:numbering>"#;

fn p_xml(p: &Paragraph, link_i: &mut u32) -> String {
    let mut s = String::from("<w:p>");
    let heading = p.outline_level.filter(|l| *l > 0);
    let num = p.numbering.as_ref();
    if heading.is_some() || num.is_some() {
        s.push_str("<w:pPr>");
        if let Some(level) = heading {
            let outline = level.saturating_sub(1);
            s.push_str(&format!(
                r#"<w:pStyle w:val="Heading{level}"/><w:outlineLvl w:val="{outline}"/>"#
            ));
        }
        if let Some(n) = num {
            let num_id = match n.format {
                Some(NumberFormat::Decimal) => 2,
                _ => 1,
            };
            let ilvl = n.level;
            s.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/></w:numPr>"#
            ));
        }
        s.push_str("</w:pPr>");
    }
    let mut wrote = false;
    for run in &p.runs {
        match &run.content {
            RunContent::Inline(InlineObject::Hyperlink { display, .. }) => {
                if display.is_empty() {
                    continue;
                }
                *link_i += 1;
                let rid = *link_i + 2;
                s.push_str(&format!(r#"<w:hyperlink r:id="rId{rid}">"#));
                s.push_str(r#"<w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">"#);
                s.push_str(&xml_escape(display));
                s.push_str("</w:t></w:r></w:hyperlink>");
                wrote = true;
            }
            RunContent::Text(t) => {
                if t.is_empty() {
                    continue;
                }
                s.push_str("<w:r>");
                let sz = hu_to_half_points(run.style.size);
                let default_sz = hu_to_half_points(DEFAULT_FONT_SIZE_HU);
                if run.style.bold || run.style.italic || run.style.strike || sz != default_sz {
                    s.push_str("<w:rPr>");
                    if run.style.bold {
                        s.push_str("<w:b/>");
                    }
                    if run.style.italic {
                        s.push_str("<w:i/>");
                    }
                    if run.style.strike {
                        s.push_str("<w:strike/>");
                    }
                    if sz != default_sz {
                        s.push_str(&format!(r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#));
                    }
                    s.push_str("</w:rPr>");
                }
                s.push_str("<w:t xml:space=\"preserve\">");
                s.push_str(&xml_escape(t));
                s.push_str("</w:t></w:r>");
                wrote = true;
            }
            RunContent::Inline(_) => {}
        }
    }
    if !wrote {
        s.push_str("<w:r><w:t/></w:r>");
    }
    s.push_str("</w:p>");
    s
}

fn tbl_xml(table: &Table, link_i: &mut u32) -> String {
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
                r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="dxa"/></w:tcPr>"#,
                hu_to_twips(cell.width)
            ));
            let mut wrote_p = false;
            for b in &cell.blocks {
                if let Block::Paragraph(p) = b {
                    s.push_str(&p_xml(p, link_i));
                    wrote_p = true;
                }
            }
            if !wrote_p {
                s.push_str("<w:p><w:r><w:t/></w:r></w:p>");
            }
            s.push_str("</w:tc>");
        }
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl>");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_document_xml(xml: &str, rels: &HashMap<String, String>) -> Result<Document, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_t = false;
    let mut in_run = false;
    let mut in_rpr = false;
    let mut in_ppr = false;
    let mut in_tbl = false;
    let mut run_bold = false;
    let mut run_italic = false;
    let mut run_strike = false;
    let mut run_size: Option<i32> = None;
    let mut run_text = String::new();
    let mut para_runs: Vec<Run> = Vec::new();
    let mut para_outline: Option<u8> = None;
    let mut para_num_id: Option<u32> = None;
    let mut para_ilvl: u8 = 0;
    let mut decimal_seq: u32 = 1;
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_blocks: Vec<Block> = Vec::new();
    let mut cell_width = 10000i32;
    let mut hyperlink_target: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = local_name(&e);
                match name.as_str() {
                    "t" => in_t = true,
                    "pPr" => in_ppr = true,
                    "pStyle" if in_ppr => {
                        if let Some(v) = attr(&e, "val")
                            && let Some(level) = heading_level_from_style(&v)
                        {
                            para_outline = Some(level);
                        }
                    }
                    "outlineLvl" if in_ppr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok()) {
                            para_outline = Some(v.saturating_add(1));
                        }
                    }
                    "ilvl" if in_ppr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok()) {
                            para_ilvl = v;
                        }
                    }
                    "numId" if in_ppr => {
                        para_num_id = attr(&e, "val").and_then(|v| v.parse().ok());
                    }
                    "rPr" if in_run => in_rpr = true,
                    "b" if in_rpr => run_bold = ooxml_on(&e),
                    "i" if in_rpr => run_italic = ooxml_on(&e),
                    "strike" if in_rpr => run_strike = ooxml_on(&e),
                    "sz" if in_rpr => {
                        if let Some(v) = attr(&e, "val").and_then(|v| v.parse::<i32>().ok()) {
                            run_size = Some(half_points_to_hu(v));
                        }
                    }
                    "hyperlink" => {
                        let id = attr(&e, "id").unwrap_or_default();
                        hyperlink_target = rels.get(&id).cloned();
                    }
                    "r" => {
                        in_run = true;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_size = None;
                        run_text.clear();
                    }
                    "p" => {
                        para_runs.clear();
                        para_outline = None;
                        para_num_id = None;
                        para_ilvl = 0;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_size = None;
                        run_text.clear();
                    }
                    "tbl" => {
                        if let Some(p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            body.push(Block::Paragraph(p));
                        }
                        in_tbl = true;
                    }
                    "tr" if in_tbl => cur_row.clear(),
                    "tc" if in_tbl => {
                        cell_blocks.clear();
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
                    "pPr" => in_ppr = false,
                    "rPr" => in_rpr = false,
                    "r" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            run_bold,
                            run_italic,
                            run_strike,
                            run_size,
                            hyperlink_target.as_deref(),
                        );
                        in_run = false;
                        run_bold = false;
                        run_italic = false;
                        run_strike = false;
                        run_size = None;
                    }
                    "hyperlink" => {
                        hyperlink_target = None;
                    }
                    "p" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            run_bold,
                            run_italic,
                            run_strike,
                            run_size,
                            hyperlink_target.as_deref(),
                        );
                        if let Some(p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            if in_tbl {
                                cell_blocks.push(Block::Paragraph(p));
                            } else {
                                body.push(Block::Paragraph(p));
                            }
                        }
                    }
                    "tc" if in_tbl => {
                        if let Some(p) = take_para(
                            &mut para_runs,
                            &mut para_outline,
                            &mut para_num_id,
                            &mut para_ilvl,
                            &mut decimal_seq,
                        ) {
                            cell_blocks.push(Block::Paragraph(p));
                        }
                        cur_row.push(TableCell {
                            width: cell_width,
                            blocks: std::mem::take(&mut cell_blocks),
                            ..TableCell::default()
                        });
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
                if in_t {
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
    flush_run(
        &mut para_runs,
        &mut run_text,
        run_bold,
        run_italic,
        run_strike,
        run_size,
        hyperlink_target.as_deref(),
    );
    if let Some(p) = take_para(
        &mut para_runs,
        &mut para_outline,
        &mut para_num_id,
        &mut para_ilvl,
        &mut decimal_seq,
    ) {
        body.push(Block::Paragraph(p));
    }
    section.body = body;
    Ok(Document {
        sections: vec![section],
        ..Document::default()
    })
}

fn heading_level_from_style(name: &str) -> Option<u8> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let rest = compact
        .strip_prefix("Heading")
        .or_else(|| compact.strip_prefix("heading"))?;
    rest.parse::<u8>().ok().filter(|v| (1..=9).contains(v))
}

fn flush_run(
    runs: &mut Vec<Run>,
    text: &mut String,
    bold: bool,
    italic: bool,
    strike: bool,
    size: Option<i32>,
    href: Option<&str>,
) {
    if text.is_empty() {
        return;
    }
    let mut run = if let Some(url) = href.filter(|u| !u.is_empty()) {
        Run::hyperlink(std::mem::take(text), url)
    } else {
        Run::text(std::mem::take(text))
    };
    run.style.bold = bold;
    run.style.italic = italic;
    run.style.strike = strike;
    if let Some(sz) = size {
        run.style.size = sz;
    }
    runs.push(run);
}

fn take_para(
    runs: &mut Vec<Run>,
    outline: &mut Option<u8>,
    num_id: &mut Option<u32>,
    ilvl: &mut u8,
    decimal_seq: &mut u32,
) -> Option<Paragraph> {
    if runs.is_empty() {
        *outline = None;
        *num_id = None;
        *ilvl = 0;
        return None;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
    p.outline_level = outline.take();
    if let Some(id) = num_id.take() {
        let format = if id == 2 {
            NumberFormat::Decimal
        } else {
            NumberFormat::Bullet
        };
        let start = if format == NumberFormat::Decimal && *ilvl == 0 {
            let n = *decimal_seq;
            *decimal_seq = decimal_seq.saturating_add(1);
            Some(n)
        } else {
            None
        };
        if format == NumberFormat::Bullet && *ilvl == 0 {
            *decimal_seq = 1;
        }
        p.numbering = Some(NumberingRef {
            definition_id: id,
            level: *ilvl,
            start,
            format: Some(format),
        });
    } else {
        *decimal_seq = 1;
    }
    *ilvl = 0;
    Some(p)
}

fn ooxml_on(e: &BytesStart<'_>) -> bool {
    !matches!(
        attr(e, "val").as_deref(),
        Some("0") | Some("false") | Some("off")
    )
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
        .or_else(|| {
            e.try_get_attribute(format!("r:{key}"))
                .ok()
                .flatten()
                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
        })
        .or_else(|| {
            e.attributes()
                .filter_map(|a| a.ok())
                .find(|a| a.key.local_name().as_ref() == key.as_bytes())
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
    use docagent_model::{
        InlineObject, LayoutHint, NumberFormat, NumberingRef, Paragraph, Run, RunContent,
    };

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
    fn roundtrip_keeps_strikethrough() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("plain ");
        let mut strike = Run::text("guess");
        strike.style.strike = true;
        p.runs.push(strike);
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:strike/>"), "docx must emit w:strike: {xml}");
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
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:numPr>"), "{xml}");
        assert!(xml.contains(r#"w:val="1""#), "{xml}");
        assert!(xml.contains(r#"w:val="2""#), "{xml}");
        let bytes = write(&doc).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("word/numbering.xml").is_ok());
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
        let xml = document_xml(&doc);
        assert!(xml.contains("w:hyperlink"), "{xml}");
        assert!(xml.contains("w:u"), "{xml}");
        let rels = document_rels(&doc);
        assert!(rels.contains("hyperlink"), "{rels}");
        assert!(rels.contains("https://github.com/kevin9327/docagent"), "{rels}");
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
        let xml = document_xml(&doc);
        assert!(xml.contains(r#"w:val="Heading1""#), "{xml}");
        assert!(xml.contains(r#"w:val="Heading2""#), "{xml}");
        assert!(xml.contains(r#"w:outlineLvl w:val="0""#), "{xml}");
        assert!(xml.contains(r#"w:sz w:val="36""#), "{xml}");
        let bytes = write(&doc).unwrap();
        assert!(sniff(&bytes));
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("h1");
        };
        assert_eq!(p.outline_level, Some(1));
        assert!(p.runs[0].style.bold);
        assert_eq!(p.runs[0].style.size, 1800);
        let Block::Paragraph(p2) = &back.sections[0].body[1] else {
            panic!("h2");
        };
        assert_eq!(p2.outline_level, Some(2));
        assert_eq!(p2.runs[0].style.size, 1400);
    }

    #[test]
    fn roundtrip_keeps_cell_emphasis() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut table = Table::from_cells(vec![vec!["plain".into()]]);
        let mut p = Paragraph::from_text("");
        let mut bold = Run::text("GPU-free");
        bold.style.bold = true;
        p.runs = vec![bold];
        let mut italic = Run::text(" byte-for-byte");
        italic.style.italic = true;
        p.runs.push(italic);
        table.rows[0].cells[0].blocks = vec![Block::Paragraph(p)];
        section.body.push(Block::Table(table));
        doc.sections.push(section);
        let xml = document_xml(&doc);
        assert!(xml.contains("<w:b/>"), "{xml}");
        assert!(xml.contains("<w:i/>"), "{xml}");
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
