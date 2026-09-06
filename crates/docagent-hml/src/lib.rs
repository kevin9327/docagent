//! HML / HWPML XML codec.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use docagent_model::{
    Alignment, Block, CharStyle, Document, InlineObject, Paragraph, Run, RunContent, Section,
    Table, TableCell, TableRow, Underline,
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
    let styles = collect_char_styles(doc);
    let mut s = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><HWPML>"#);
    s.push_str(&charshape_head(&styles));
    s.push_str("<BODY>");
    if doc.sections.is_empty() {
        s.push_str(r#"<SECTION><P><TEXT CharShape="0"></TEXT></P></SECTION>"#);
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
                s.push_str(r#"<P><TEXT CharShape="0"></TEXT></P>"#);
            } else {
                for block in &section.body {
                    match block {
                        Block::Paragraph(p) => s.push_str(&p_xml(p, &styles)),
                        Block::Table(t) => s.push_str(&table_xml(t)),
                        Block::Float(_) | Block::Break(_) => {
                            s.push_str(r#"<P><TEXT CharShape="0"></TEXT></P>"#)
                        }
                    }
                }
            }
            s.push_str("</SECTION>");
        }
    }
    s.push_str("</BODY></HWPML>");
    Ok(s.into_bytes())
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

fn charshape_head(styles: &[CharStyle]) -> String {
    let mut s = format!(
        r#"<HEAD><MAPPINGTABLE><CHARSHAPELIST Count="{}">"#,
        styles.len()
    );
    for (i, st) in styles.iter().enumerate() {
        s.push_str(&format!(
            r#"<CHARSHAPE Id="{i}" Height="{}">"#,
            st.size.max(1)
        ));
        if st.bold {
            s.push_str("<BOLD/>");
        }
        if st.italic {
            s.push_str("<ITALIC/>");
        }
        if st.underline != Underline::None {
            s.push_str(r#"<UNDERLINE Type="Bottom"/>"#);
        }
        if st.strike {
            s.push_str(r#"<STRIKEOUT Shape="Solid"/>"#);
        }
        if st.superscript {
            s.push_str("<SUPERSCRIPT/>");
        }
        if st.subscript {
            s.push_str("<SUBSCRIPT/>");
        }
        s.push_str("</CHARSHAPE>");
    }
    s.push_str("</CHARSHAPELIST></MAPPINGTABLE></HEAD>");
    s
}

fn p_xml(p: &Paragraph, styles: &[CharStyle]) -> String {
    let mut s = String::from("<P>");
    if p.runs.is_empty() {
        s.push_str(r#"<TEXT CharShape="0"></TEXT>"#);
    } else {
        for run in &p.runs {
            let sid = style_id(styles, &run.style);
            match &run.content {
                RunContent::Inline(InlineObject::Hyperlink { target, display }) => {
                    s.push_str(&format!(
                        r#"<FIELDBEGIN Type="Hyperlink"><PARAMETERS><STRINGPARAM Name="Command">{}</STRINGPARAM></PARAMETERS></FIELDBEGIN><TEXT CharShape="{sid}">{}</TEXT><FIELDEND/>"#,
                        xml_escape(target),
                        xml_escape(display)
                    ));
                }
                _ => {
                    s.push_str(&format!(
                        r#"<TEXT CharShape="{sid}">{}</TEXT>"#,
                        xml_escape(run.display_text())
                    ));
                }
            }
        }
    }
    s.push_str("</P>");
    s
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
    reader.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut doc = Document::new();
    let mut section = Section::default();
    let mut body = Vec::new();
    let mut in_text = false;
    let mut in_table = false;
    let mut in_charshape = false;
    let mut chars: HashMap<u32, CharStyle> = HashMap::new();
    let mut char_id: Option<u32> = None;
    let mut char_style = CharStyle::default();
    let mut para_runs: Vec<Run> = Vec::new();
    let mut run_text = String::new();
    let mut run_style = CharStyle::default();
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut cur_row: Vec<TableCell> = Vec::new();
    let mut cell_buf = String::new();
    let mut cell_width = 10000i32;
    let mut have_section = false;
    let mut pending_href: Option<String> = None;
    let mut in_string_param = false;
    let mut string_param_name = String::new();
    let mut param_text = String::new();

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
                    "CHARSHAPE" => {
                        in_charshape = true;
                        char_id = attr_u32(&e, "Id");
                        char_style = CharStyle::default();
                        if let Some(h) = attr_i32(&e, "Height").filter(|h| *h > 0) {
                            char_style.size = h;
                        }
                    }
                    "BOLD" if in_charshape => char_style.bold = true,
                    "ITALIC" if in_charshape => char_style.italic = true,
                    "UNDERLINE" if in_charshape => char_style.underline = Underline::Single,
                    "STRIKEOUT" if in_charshape => char_style.strike = true,
                    "SUPERSCRIPT" if in_charshape => char_style.superscript = true,
                    "SUBSCRIPT" if in_charshape => char_style.subscript = true,
                    "TEXT" | "CHAR" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        in_text = true;
                        run_style = CharStyle::default();
                        if let Some(id) = attr_u32(&e, "CharShape")
                            && let Some(st) = chars.get(&id)
                        {
                            run_style = st.clone();
                        }
                    }
                    "FIELDBEGIN" => {
                        let ty = attr_string(&e, "Type").unwrap_or_default();
                        if ty.eq_ignore_ascii_case("Hyperlink") {
                            pending_href = Some(String::new());
                        }
                    }
                    "STRINGPARAM" => {
                        in_string_param = true;
                        string_param_name = attr_string(&e, "Name").unwrap_or_default();
                        param_text.clear();
                    }
                    "FIELDEND" => pending_href = None,
                    "TABLE" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        flush_p(&mut body, &mut para_runs);
                        pending_href = None;
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
                    "CHARSHAPE" => {
                        if let Some(id) = char_id.take() {
                            chars.insert(id, char_style.clone());
                        }
                        in_charshape = false;
                    }
                    "TEXT" | "CHAR" => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        in_text = false;
                    }
                    "STRINGPARAM" => {
                        if in_string_param
                            && string_param_name.eq_ignore_ascii_case("Command")
                            && pending_href.is_some()
                        {
                            pending_href = Some(std::mem::take(&mut param_text));
                        }
                        in_string_param = false;
                    }
                    "P" if !in_table => {
                        flush_run(
                            &mut para_runs,
                            &mut run_text,
                            &run_style,
                            pending_href.as_deref(),
                        );
                        flush_p(&mut body, &mut para_runs);
                        pending_href = None;
                    }
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
                } else if in_string_param {
                    param_text.push_str(&decoded);
                } else if in_text {
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
    flush_p(&mut body, &mut para_runs);
    if have_section || !body.is_empty() {
        section.body = body;
        doc.sections.push(section);
    }
    if doc.sections.is_empty() {
        doc.sections.push(Section::default());
    }
    Ok(doc)
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

fn flush_p(body: &mut Vec<Block>, runs: &mut Vec<Run>) {
    if runs.is_empty() {
        return;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
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

    #[test]
    fn roundtrip_keeps_charshape_marks() {
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
        let xml = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(xml.contains("<BOLD/>"), "{xml}");
        assert!(xml.contains("<STRIKEOUT"), "{xml}");
        assert!(xml.contains(r#"CharShape=""#), "{xml}");
        let back = roundtrip(&doc).unwrap();
        let Block::Paragraph(p) = &back.sections[0].body[0] else {
            panic!("paragraph");
        };
        assert!(p.runs.iter().any(|r| {
            r.style.bold && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("GPU-free"))
        }));
        assert!(p.runs.iter().any(|r| {
            r.style.strike && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("guess"))
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
        let xml = p_xml(&p, &[CharStyle::default()]);
        assert!(xml.contains(r#"Type="Hyperlink""#), "{xml}");
        assert!(xml.contains("STRINGPARAM"), "{xml}");
        assert!(xml.contains("https://github.com/kevin9327/docagent"), "{xml}");
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
}
