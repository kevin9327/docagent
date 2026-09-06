//! HML / HWPML XML codec.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use docagent_model::{
    Alignment, Block, BreakKind, CharStyle, Document, InlineObject, NumberFormat, NumberingRef,
    Paragraph, Run, RunContent, Section, Table, TableCell, TableRow, Underline,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
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
        s.push_str(
            r#"<SECTION><P Style="0" ParaShape="0"><TEXT CharShape="0"></TEXT></P></SECTION>"#,
        );
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
                s.push_str(r#"<P Style="0" ParaShape="0"><TEXT CharShape="0"></TEXT></P>"#);
            } else {
                for block in &section.body {
                    match block {
                        Block::Paragraph(p) => s.push_str(&p_xml(p, &styles)),
                        Block::Table(t) => s.push_str(&table_xml(t)),
                        Block::Break(BreakKind::Thematic) => s.push_str(
                            r#"<P Style="3" ParaShape="3"><TEXT CharShape="0"></TEXT></P>"#,
                        ),
                        Block::Float(_) | Block::Break(_) => s.push_str(
                            r#"<P Style="0" ParaShape="0"><TEXT CharShape="0"></TEXT></P>"#,
                        ),
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
    s.push_str("</CHARSHAPELIST>");
    s.push_str(
        r#"<PARASHAPELIST Count="5"><PARASHAPE Id="0"/><PARASHAPE Id="1"><PARAMARGIN Left="1400"/><PARABORDER BorderFill="1" Left="1"/></PARASHAPE><PARASHAPE Id="2"><PARABORDER BorderFill="2" Fill="1"/></PARASHAPE><PARASHAPE Id="3"><PARABORDER BorderFill="3" Bottom="1"/></PARASHAPE><PARASHAPE Id="4"/></PARASHAPELIST>"#,
    );
    s.push_str(
        r#"<STYLELIST Count="5"><STYLE Id="0" Name="Normal" EngName="Normal" Type="Para" ParaShape="0"/><STYLE Id="1" Name="Quote" EngName="Quote" Type="Para" ParaShape="1"/><STYLE Id="2" Name="CodeBlock" EngName="CodeBlock" Type="Para" ParaShape="2"/><STYLE Id="3" Name="HorizontalLine" EngName="HorizontalLine" Type="Para" ParaShape="3"/><STYLE Id="4" Name="Task" EngName="Task" Type="Para" ParaShape="4"/></STYLELIST></MAPPINGTABLE></HEAD>"#,
    );
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

fn p_xml(p: &Paragraph, styles: &[CharStyle]) -> String {
    let sid = para_style_id(p);
    let mut s = format!(r#"<P Style="{sid}" ParaShape="{sid}">"#);
    let mut pending_marker = task_prefix(p);
    if p.runs.is_empty() {
        let inner = pending_marker.take().unwrap_or("");
        s.push_str(&format!(
            r#"<TEXT CharShape="0">{}</TEXT>"#,
            xml_escape(inner)
        ));
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
                    let displayed = run.display_text();
                    let body = match pending_marker.take() {
                        Some(m) => format!("{m}{displayed}"),
                        None => displayed.to_string(),
                    };
                    s.push_str(&format!(
                        r#"<TEXT CharShape="{sid}">{}</TEXT>"#,
                        xml_escape(&body)
                    ));
                }
            }
        }
        if let Some(m) = pending_marker {
            s.push_str(&format!(
                r#"<TEXT CharShape="0">{}</TEXT>"#,
                xml_escape(m)
            ));
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
    let mut named_styles: HashMap<u32, String> = HashMap::new();
    let mut para_kind: Option<StyleKind> = None;

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
                    "STYLE" => {
                        if let Some(id) = attr_u32(&e, "Id") {
                            let eng = attr_string(&e, "EngName").unwrap_or_default();
                            let name = attr_string(&e, "Name").unwrap_or_default();
                            named_styles.insert(id, if eng.is_empty() { name } else { eng });
                        }
                    }
                    "P" if !in_table => {
                        para_kind = attr_u32(&e, "Style")
                            .and_then(|id| named_styles.get(&id).cloned())
                            .as_deref()
                            .and_then(style_kind);
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
                        flush_p(&mut body, &mut para_runs, &mut para_kind);
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
                        flush_p(&mut body, &mut para_runs, &mut para_kind);
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
    flush_p(&mut body, &mut para_runs, &mut para_kind);
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

fn flush_p(body: &mut Vec<Block>, runs: &mut Vec<Run>, kind: &mut Option<StyleKind>) {
    let kind = kind.take();
    if kind == Some(StyleKind::Thematic) {
        runs.clear();
        body.push(Block::Break(BreakKind::Thematic));
        return;
    }
    if runs.is_empty() && kind.is_none() {
        return;
    }
    let mut p = Paragraph::from_text("");
    p.runs = std::mem::take(runs);
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
            r.style.strike
                && matches!(&r.content, docagent_model::RunContent::Text(t) if t.contains("guess"))
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
    fn roundtrip_keeps_quote() {
        let mut doc = Document::new();
        let mut section = Section::default();
        let mut p = Paragraph::from_text("Replay is evidence.");
        p.quote = true;
        section.body.push(Block::Paragraph(p));
        doc.sections.push(section);
        let xml = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(xml.contains(r#"EngName="Quote""#), "{xml}");
        assert!(xml.contains(r#"Style="1""#), "{xml}");
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
        let xml = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(xml.contains(r#"EngName="CodeBlock""#), "{xml}");
        assert!(xml.contains(r#"Style="2""#), "{xml}");
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
        let xml = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(xml.contains(r#"EngName="HorizontalLine""#), "{xml}");
        assert!(xml.contains(r#"Style="3""#), "{xml}");
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
        let xml = p_xml(&checked, &[CharStyle::default()]);
        assert!(xml.contains(r#"Style="4""#), "{xml}");
        assert!(xml.contains("[x] "), "{xml}");
        section.body.push(Block::Paragraph(checked));
        section.body.push(Block::Paragraph(unchecked));
        doc.sections.push(section);
        let xml = String::from_utf8(write(&doc).unwrap()).unwrap();
        assert!(xml.contains(r#"EngName="Task""#), "{xml}");
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
