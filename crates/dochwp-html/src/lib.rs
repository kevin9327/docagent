//! Semantic HTML from IR (not from the paint tree). Search/RAG/a11y.

#![forbid(unsafe_code)]

use dochwp_model::{Block, Document, Paragraph, Table};

pub fn to_html(doc: &Document) -> String {
    let mut s = String::from(
        r#"<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"/><title>DocHWP</title><style>article{max-width:48rem;margin:auto;font-family:sans-serif}table{border-collapse:collapse}td,th{border:1px solid #333;padding:0.25rem}</style></head><body>"#,
    );
    for (i, section) in doc.sections.iter().enumerate() {
        s.push_str(&format!(r#"<article data-section="{i}">"#));
        if let Some(h) = &section.header {
            s.push_str("<header>");
            for b in &h.blocks {
                push_block(&mut s, b);
            }
            s.push_str("</header>");
        }
        s.push_str("<section>");
        for b in &section.body {
            push_block(&mut s, b);
        }
        s.push_str("</section>");
        if let Some(f) = &section.footer {
            s.push_str("<footer>");
            for b in &f.blocks {
                push_block(&mut s, b);
            }
            s.push_str("</footer>");
        }
        s.push_str("</article>");
    }
    s.push_str("</body></html>");
    s
}

fn push_block(s: &mut String, block: &Block) {
    match block {
        Block::Paragraph(p) => push_para(s, p),
        Block::Table(t) => push_table(s, t),
        Block::Float(_) => s.push_str("<aside></aside>"),
        Block::Break(_) => s.push_str("<hr/>"),
    }
}

fn push_para(s: &mut String, p: &Paragraph) {
    s.push_str("<p>");
    s.push_str(&escape(&p.plain_text()));
    s.push_str("</p>");
}

fn push_table(s: &mut String, t: &Table) {
    s.push_str("<table>");
    for (ri, row) in t.rows.iter().enumerate() {
        s.push_str("<tr>");
        for cell in &row.cells {
            let tag = if row.header || ri < t.header_row_count as usize {
                "th"
            } else {
                "td"
            };
            s.push_str(&format!("<{tag}>"));
            for b in &cell.blocks {
                if let Block::Paragraph(p) = b {
                    s.push_str(&escape(&p.plain_text()));
                }
            }
            s.push_str(&format!("</{tag}>"));
        }
        s.push_str("</tr>");
    }
    s.push_str("</table>");
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dochwp_model::{Paragraph, Section};

    #[test]
    fn html_has_semantic_structure() {
        let mut doc = Document::new();
        let mut section = Section::default();
        section
            .body
            .push(Block::Paragraph(Paragraph::from_text("hello")));
        section
            .body
            .push(Block::Table(Table::from_cells(vec![vec!["a".into(), "b".into()]])));
        doc.sections.push(section);
        let html = to_html(&doc);
        assert!(html.contains("<article"));
        assert!(html.contains("<section>"));
        assert!(html.contains("<p>hello</p>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>a</td>"));
    }
}
